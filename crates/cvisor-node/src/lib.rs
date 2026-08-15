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
        cleanup_overlay, execute_with, generate_uid, shell_argv, spawn_session, ExecOpts,
        LogBuffer, LogLevel, PtyMode, Session,
    };
    use napi::bindgen_prelude::{External, ExternalRef, Object, Uint8Array};
    use napi::Env;
    use napi_derive::napi;

    /// A sandbox handle. Cleanup of the overlay tree happens when the JS handle
    /// is garbage-collected (Drop), matching the original finalizer behavior.
    pub struct Sandbox {
        uid: [u8; 16],
        log_level: AtomicU8,
        allow_network: AtomicBool,
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
        })
    }

    #[napi(js_name = "sandboxSetAllowNetwork")]
    pub fn sandbox_set_allow_network(sandbox: ExternalRef<Sandbox>, allow: bool) {
        sandbox.allow_network.store(allow, Ordering::Relaxed);
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
            ..ExecOpts::default()
        };
        let (argv, mode) = if pty {
            (
                vec!["/bin/sh".to_string(), "-i".to_string()],
                PtyMode::Buffered,
            )
        } else {
            let c = cmd
                .ok_or_else(|| napi::Error::from_reason("cmd required for a non-pty session"))?;
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
