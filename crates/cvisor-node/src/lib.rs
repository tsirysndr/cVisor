//! N-API bindings for the Node SDK.
//!
//! Matches the frozen FFI contract in `src/sdks/node/src/native.ts` exactly, so
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
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    use cvisor_core::{cleanup_overlay, execute, generate_uid, LogBuffer, LogLevel};
    use napi::bindgen_prelude::{External, ExternalRef, Object, Uint8Array};
    use napi::Env;
    use napi_derive::napi;

    /// A sandbox handle. Cleanup of the overlay tree happens when the JS handle
    /// is garbage-collected (Drop), matching the original finalizer behavior.
    pub struct Sandbox {
        uid: [u8; 16],
        log_level: AtomicU8,
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
        })
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
    ) -> napi::Result<Object> {
        let stdout = Arc::new(LogBuffer::new());
        let stderr = Arc::new(LogBuffer::new());
        // Blocks until the guest command exits (forks inside the Node process;
        // the supervisor loop runs on this thread). Behavior preserved from Zig.
        execute(
            sandbox.uid,
            sandbox.level(),
            &command,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
        )
        .map_err(|e| napi::Error::from_reason(format!("sandbox execute failed: {e}")))?;

        // Return { stdout: External<Stream>, stderr: External<Stream> }.
        let mut obj = Object::new(env)?;
        obj.set("stdout", External::new(JsStream { buffer: stdout }))?;
        obj.set("stderr", External::new(JsStream { buffer: stderr }))?;
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
}
