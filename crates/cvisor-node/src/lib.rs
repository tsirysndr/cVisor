//! N-API bindings for the Node SDK.
//!
//! Matches the frozen FFI contract in `sdks/node/src/native.ts` exactly, so
//! the hand-written TypeScript loader and platform packages need no changes:
//!   createSandbox() -> External<Sandbox>
//!   sandboxSetLogLevel(sb, "OFF"|"DEBUG")
//!   sandboxRunCmd(sb, cmd) -> { stdout: External<Stream>, stderr: External<Stream> }
//!   streamNext(stream) -> Uint8Array | null
//!
//! Only compiled for Linux; on other hosts this is an empty cdylib so the
//! workspace still builds.

#![allow(clippy::all)]

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use cvisor_core::{
        cache, cleanup_overlay, copy_into, copy_out_of, execute_with, generate_uid, read_file,
        shell_argv, spawn_session, write_file, ExecOpts, Format, LogBuffer, LogLevel, PtyMode,
        Session,
    };
    use napi::bindgen_prelude::{External, ExternalRef, Object, Uint8Array};
    use napi::Env;
    use napi_derive::napi;
    use std::path::Path;

    /// A sandbox handle. Cleanup of the overlay tree happens when the JS handle
    /// is garbage-collected (Drop), matching the original finalizer behavior.
    pub struct Sandbox {
        uid: [u8; 16],
        log_level: AtomicU8,
        allow_network: AtomicBool,
        allow_listen: AtomicBool,
        env: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl Sandbox {
        fn level(&self) -> LogLevel {
            if self.log_level.load(Ordering::Relaxed) == 1 {
                LogLevel::Debug
            } else {
                LogLevel::Off
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            cleanup_overlay(&self.uid);
        }
    }

    /// An output stream backed by a captured LogBuffer.
    pub struct JsStream {
        buffer: Arc<LogBuffer>,
    }

    #[napi(js_name = "createSandbox")]
    pub fn create_sandbox() -> External<Sandbox> {
        External::new(Sandbox {
            uid: generate_uid(),
            log_level: AtomicU8::new(0),
            allow_network: AtomicBool::new(true),
            allow_listen: AtomicBool::new(false),
            env: std::sync::Mutex::new(Vec::new()),
        })
    }

    #[napi(js_name = "sandboxSetAllowNetwork")]
    pub fn sandbox_set_allow_network(sandbox: ExternalRef<Sandbox>, allow: bool) {
        sandbox.allow_network.store(allow, Ordering::Relaxed);
    }

    #[napi(js_name = "sandboxSetAllowListen")]
    pub fn sandbox_set_allow_listen(sandbox: ExternalRef<Sandbox>, allow: bool) {
        sandbox.allow_listen.store(allow, Ordering::Relaxed);
    }

    #[napi(js_name = "sandboxSetEnv")]
    pub fn sandbox_set_env(sandbox: ExternalRef<Sandbox>, key: String, value: String) {
        let mut env = sandbox.env.lock().unwrap();
        if let Some(e) = env.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            env.push((key, value));
        }
    }

    #[napi(js_name = "sandboxWriteFile")]
    pub fn sandbox_write_file(
        sandbox: ExternalRef<Sandbox>,
        path: String,
        data: Uint8Array,
    ) -> napi::Result<()> {
        write_file(sandbox.uid, &path, &data)
            .map_err(|e| napi::Error::from_reason(format!("write_file failed: {e}")))
    }

    #[napi(js_name = "sandboxReadFile")]
    pub fn sandbox_read_file(
        sandbox: ExternalRef<Sandbox>,
        path: String,
    ) -> napi::Result<Uint8Array> {
        read_file(sandbox.uid, &path)
            .map(Uint8Array::new)
            .map_err(|e| napi::Error::from_reason(format!("read_file failed: {e}")))
    }

    #[napi(js_name = "sandboxCopyInto")]
    pub fn sandbox_copy_into(
        sandbox: ExternalRef<Sandbox>,
        host_path: String,
        guest_path: String,
    ) -> napi::Result<()> {
        copy_into(sandbox.uid, Path::new(&host_path), &guest_path)
            .map_err(|e| napi::Error::from_reason(format!("copy_into failed: {e}")))
    }

    #[napi(js_name = "sandboxCopyOut")]
    pub fn sandbox_copy_out(
        sandbox: ExternalRef<Sandbox>,
        guest_path: String,
        host_path: String,
    ) -> napi::Result<()> {
        copy_out_of(sandbox.uid, &guest_path, Path::new(&host_path))
            .map_err(|e| napi::Error::from_reason(format!("copy_out failed: {e}")))
    }

    #[napi(js_name = "cacheSave")]
    pub fn cache_save(
        sandbox: ExternalRef<Sandbox>,
        sandbox_path: String,
        key: String,
        backend: String,
        format: String,
    ) -> napi::Result<()> {
        cache_op(&sandbox, &sandbox_path, &key, &backend, &format, false)
    }

    #[napi(js_name = "cacheRestore")]
    pub fn cache_restore(
        sandbox: ExternalRef<Sandbox>,
        sandbox_path: String,
        key: String,
        backend: String,
        format: String,
    ) -> napi::Result<()> {
        cache_op(&sandbox, &sandbox_path, &key, &backend, &format, true)
    }

    fn cache_op(
        sandbox: &Sandbox,
        sandbox_path: &str,
        key: &str,
        backend: &str,
        format: &str,
        restore: bool,
    ) -> napi::Result<()> {
        let fmt = if format.is_empty() {
            Format::Gzip
        } else {
            Format::parse(format)
                .ok_or_else(|| napi::Error::from_reason(format!("unknown format: {format}")))?
        };
        let backend = cache::Backend::parse(backend)
            .map_err(|e| napi::Error::from_reason(format!("bad backend: {e}")))?;
        let res = if restore {
            cache::restore(sandbox.uid, sandbox_path, key, &backend, fmt)
        } else {
            cache::save(sandbox.uid, sandbox_path, key, &backend, fmt)
        };
        res.map_err(|e| napi::Error::from_reason(format!("cache failed: {e}")))
    }

    #[napi(js_name = "sandboxSetLogLevel")]
    pub fn sandbox_set_log_level(sandbox: ExternalRef<Sandbox>, level: String) -> napi::Result<()> {
        let v = match LogLevel::parse(&level) {
            Some(LogLevel::Debug) => 1,
            Some(LogLevel::Off) => 0,
            None => {
                return Err(napi::Error::from_reason(format!(
                    "Invalid log level: {level}"
                )))
            }
        };
        sandbox.log_level.store(v, Ordering::Relaxed);
        Ok(())
    }

    #[napi(js_name = "sandboxRunCmd")]
    pub fn sandbox_run_cmd(
        env: &Env,
        sandbox: ExternalRef<Sandbox>,
        command: String,
        timeout_ms: Option<i64>,
    ) -> napi::Result<Object> {
        let stdout = Arc::new(LogBuffer::new());
        let stderr = Arc::new(LogBuffer::new());
        let opts = ExecOpts {
            allow_network: sandbox.allow_network.load(Ordering::Relaxed),
            allow_listen: sandbox.allow_listen.load(Ordering::Relaxed),
            env: sandbox.env.lock().unwrap().clone(),
            timeout: timeout_ms
                .filter(|ms| *ms > 0)
                .map(|ms| Duration::from_millis(ms as u64)),
            ..ExecOpts::default()
        };
        // Blocks until the guest command exits (forks inside the Node process;
        // the supervisor loop runs on this thread). Behavior preserved from Zig.
        let exit_code = execute_with(
            sandbox.uid,
            sandbox.level(),
            &command,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            opts,
        )
        .map_err(|e| napi::Error::from_reason(format!("sandbox execute failed: {e}")))?;

        // Return { stdout: External<Stream>, stderr: External<Stream>, exitCode }.
        let mut obj = Object::new(env)?;
        obj.set("stdout", External::new(JsStream { buffer: stdout }))?;
        obj.set("stderr", External::new(JsStream { buffer: stderr }))?;
        obj.set("exitCode", exit_code)?;
        Ok(obj)
    }

    #[napi(js_name = "streamNext")]
    pub fn stream_next(stream: ExternalRef<JsStream>) -> Option<Uint8Array> {
        let data = stream.buffer.read();
        if data.is_empty() {
            None
        } else {
            Some(Uint8Array::new(data))
        }
    }

    /// Start a background session. `pty` true runs an interactive `/bin/sh -i`
    /// on a pseudo-terminal (merged output; stdin writable); false runs `cmd`
    /// via `/bin/sh -c` with stdout/stderr captured for drain.
    #[napi(js_name = "sessionStart")]
    pub fn session_start(
        sandbox: ExternalRef<Sandbox>,
        cmd: Option<String>,
        pty: bool,
    ) -> napi::Result<External<Session>> {
        let opts = ExecOpts {
            allow_network: sandbox.allow_network.load(Ordering::Relaxed),
            allow_listen: sandbox.allow_listen.load(Ordering::Relaxed),
            env: sandbox.env.lock().unwrap().clone(),
            ..ExecOpts::default()
        };
        let (argv, mode) = if pty {
            (
                vec!["/bin/sh".to_string(), "-i".to_string()],
                PtyMode::Buffered,
            )
        } else {
            let c =
                cmd.ok_or_else(|| napi::Error::from_reason("cmd required for a non-pty session"))?;
            (shell_argv(&c), PtyMode::None)
        };
        spawn_session(sandbox.uid, sandbox.level(), &argv, opts, mode)
            .map(External::new)
            .map_err(|e| napi::Error::from_reason(format!("session start failed: {e}")))
    }

    #[napi(js_name = "sessionReadStdout")]
    pub fn session_read_stdout(session: ExternalRef<Session>) -> Option<Uint8Array> {
        let data = session.read_stdout();
        (!data.is_empty()).then(|| Uint8Array::new(data))
    }

    #[napi(js_name = "sessionReadStderr")]
    pub fn session_read_stderr(session: ExternalRef<Session>) -> Option<Uint8Array> {
        let data = session.read_stderr();
        (!data.is_empty()).then(|| Uint8Array::new(data))
    }

    #[napi(js_name = "sessionWriteStdin")]
    pub fn session_write_stdin(
        session: ExternalRef<Session>,
        data: Uint8Array,
    ) -> napi::Result<i64> {
        session
            .write_stdin(&data)
            .map(|n| n as i64)
            .map_err(|e| napi::Error::from_reason(format!("write_stdin failed: {e}")))
    }

    #[napi(js_name = "sessionResize")]
    pub fn session_resize(session: ExternalRef<Session>, rows: u32, cols: u32) {
        session.resize(rows as u16, cols as u16);
    }

    /// The guest's exit code once it has finished, else null.
    #[napi(js_name = "sessionTryWait")]
    pub fn session_try_wait(session: ExternalRef<Session>) -> Option<i32> {
        session.try_wait()
    }

    #[napi(js_name = "sessionKill")]
    pub fn session_kill(session: ExternalRef<Session>) {
        session.kill();
    }
}
