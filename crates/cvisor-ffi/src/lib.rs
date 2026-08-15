//! C ABI for cVisor, consumed by the Bun / Deno / Python / Ruby FFI SDKs.
//!
//! A single shared library (`libcvisor.so`) exposes a small set of `extern "C"`
//! functions. The runtime is Linux-only; on other hosts this compiles to an
//! empty cdylib so the workspace still builds.
//!
//! Contract (all handles are opaque pointers; NULL on failure):
//!   CvisorSandbox* cvisor_sandbox_new(void);
//!   void           cvisor_sandbox_free(CvisorSandbox*);
//!   void           cvisor_sandbox_set_log_level(CvisorSandbox*, int level);      // 0=off 1=debug
//!   void           cvisor_sandbox_set_allow_network(CvisorSandbox*, int allow);  // 0=deny else allow
//!   void           cvisor_sandbox_set_allow_listen(CvisorSandbox*, int allow);   // 0=deny else allow inbound TCP servers
//!   int            cvisor_sandbox_write_file(CvisorSandbox*, const char* path, const uint8_t*, size_t); // 0 ok, -errno
//!   uint8_t*       cvisor_sandbox_read_file(CvisorSandbox*, const char* path, size_t* out_len);         // NULL on error
//!   CvisorOutput*  cvisor_run(CvisorSandbox*, const char* cmd);                  // blocks
//!   CvisorOutput*  cvisor_run_timeout(CvisorSandbox*, const char* cmd, uint64_t timeout_ms);
//!   int            cvisor_output_exit_code(CvisorOutput*);                       // status, or 128+signo
//!   void           cvisor_output_free(CvisorOutput*);
//!   uint8_t*       cvisor_output_stdout(CvisorOutput*, size_t* out_len);
//!   uint8_t*       cvisor_output_stderr(CvisorOutput*, size_t* out_len);
//!   void           cvisor_bytes_free(uint8_t* ptr, size_t len);
//!
//! Streaming sessions run the guest in the background so the caller can drain
//! output as it arrives and (for a PTY) feed stdin. Each SDK builds its own
//! stdout/stderr callbacks on top of the drain functions.
//!   CvisorSession* cvisor_session_start(CvisorSandbox*, const char* cmd, int pty); // pty: 0=none 1=shell
//!   uint8_t*       cvisor_session_read_stdout(CvisorSession*, size_t* out_len);    // drains; PTY = merged
//!   uint8_t*       cvisor_session_read_stderr(CvisorSession*, size_t* out_len);
//!   long           cvisor_session_write_stdin(CvisorSession*, const uint8_t*, size_t); // PTY only
//!   void           cvisor_session_resize(CvisorSession*, uint16_t rows, uint16_t cols);
//!   int            cvisor_session_try_wait(CvisorSession*, int* done);             // done=0 while running
//!   int            cvisor_session_wait(CvisorSession*);                            // blocks -> exit code
//!   void           cvisor_session_kill(CvisorSession*);
//!   void           cvisor_session_free(CvisorSession*);
//!
//! `cvisor_run` blocks until the guest command exits, then the captured
//! stdout/stderr are available in full via the `*_stdout`/`*_stderr` accessors
//! (each returns a freshly allocated copy the caller must release with
//! `cvisor_bytes_free`), and the exit code via `cvisor_output_exit_code`.

#![allow(clippy::missing_safety_doc)]

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int};
    use std::sync::Arc;

    use std::time::Duration;

    use cvisor_core::{
        cleanup_overlay, execute_with, generate_uid, read_file, shell_argv, spawn_session,
        write_file, ExecOpts, LogBuffer, LogLevel, PtyMode, Session,
    };

    /// Opaque sandbox handle.
    pub struct Sandbox {
        uid: [u8; 16],
        log_level: LogLevel,
        allow_network: bool,
        allow_listen: bool,
    }

    /// Opaque result handle holding the fully-captured output of one run.
    pub struct Output {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
    }

    #[no_mangle]
    pub extern "C" fn cvisor_sandbox_new() -> *mut Sandbox {
        Box::into_raw(Box::new(Sandbox {
            uid: generate_uid(),
            log_level: LogLevel::Off,
            allow_network: true,
            allow_listen: false,
        }))
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_free(sb: *mut Sandbox) {
        if sb.is_null() {
            return;
        }
        // SAFETY: `sb` was produced by cvisor_sandbox_new (Box::into_raw).
        let sb = Box::from_raw(sb);
        cleanup_overlay(&sb.uid);
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_set_log_level(sb: *mut Sandbox, level: c_int) {
        if let Some(sb) = sb.as_mut() {
            sb.log_level = if level == 1 {
                LogLevel::Debug
            } else {
                LogLevel::Off
            };
        }
    }

    /// Run `cmd` in the sandbox, blocking until it exits. Returns a heap Output
    /// holding the captured stdout/stderr, or NULL on error.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_run(sb: *mut Sandbox, cmd: *const c_char) -> *mut Output {
        run_impl(sb, cmd, 0)
    }

    /// Like cvisor_run, but SIGKILLs the guest after `timeout_ms` (0 = no limit).
    /// A timed-out run reports exit code 137 (128 + SIGKILL).
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_run_timeout(
        sb: *mut Sandbox,
        cmd: *const c_char,
        timeout_ms: u64,
    ) -> *mut Output {
        run_impl(sb, cmd, timeout_ms)
    }

    unsafe fn run_impl(sb: *mut Sandbox, cmd: *const c_char, timeout_ms: u64) -> *mut Output {
        let Some(sb) = sb.as_ref() else {
            return std::ptr::null_mut();
        };
        if cmd.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: caller guarantees `cmd` is a valid NUL-terminated C string.
        let Ok(cmd) = CStr::from_ptr(cmd).to_str() else {
            return std::ptr::null_mut();
        };

        let opts = ExecOpts {
            allow_network: sb.allow_network,
            allow_listen: sb.allow_listen,
            timeout: (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms)),
            ..ExecOpts::default()
        };
        let stdout = Arc::new(LogBuffer::new());
        let stderr = Arc::new(LogBuffer::new());
        let exit_code = match execute_with(
            sb.uid,
            sb.log_level,
            cmd,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            opts,
        ) {
            Ok(code) => code,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(Output {
            stdout: stdout.read(),
            stderr: stderr.read(),
            exit_code,
        }))
    }

    /// The guest's exit code (shell convention: status, or 128 + signal).
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_output_exit_code(out: *mut Output) -> c_int {
        out.as_ref().map(|o| o.exit_code).unwrap_or(-1)
    }

    /// Enable (nonzero) or disable (0) outbound INET/INET6 networking. Default on.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_set_allow_network(sb: *mut Sandbox, allow: c_int) {
        if let Some(sb) = sb.as_mut() {
            sb.allow_network = allow != 0;
        }
    }

    /// Enable (nonzero) or disable (0) inbound TCP servers (bind fixed port,
    /// listen, accept). Default off.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_set_allow_listen(sb: *mut Sandbox, allow: c_int) {
        if let Some(sb) = sb.as_mut() {
            sb.allow_listen = allow != 0;
        }
    }

    /// Write `data` to `path` inside the sandbox overlay (visible to later runs).
    /// Returns 0 on success or a negative errno.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_write_file(
        sb: *mut Sandbox,
        path: *const c_char,
        data: *const u8,
        len: usize,
    ) -> c_int {
        let Some(sb) = sb.as_ref() else {
            return -(cvisor_core::Errno::INVAL.code());
        };
        if path.is_null() || (data.is_null() && len != 0) {
            return -(cvisor_core::Errno::INVAL.code());
        }
        // SAFETY: caller guarantees `path` is NUL-terminated and `data`/`len`
        // describe a valid buffer.
        let Ok(path) = CStr::from_ptr(path).to_str() else {
            return -(cvisor_core::Errno::INVAL.code());
        };
        let bytes = if len == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(data, len)
        };
        match write_file(sb.uid, path, bytes) {
            Ok(()) => 0,
            Err(e) => -(e.errno().code()),
        }
    }

    /// Read `path` from the sandbox overlay (the guest's view). Returns a freshly
    /// allocated buffer the caller frees with `cvisor_bytes_free`, or NULL on
    /// error. A successful read of an empty file also returns NULL with
    /// `*out_len == 0`, so treat NULL with len 0 as "empty" when the file exists.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_sandbox_read_file(
        sb: *mut Sandbox,
        path: *const c_char,
        out_len: *mut usize,
    ) -> *mut u8 {
        if let Some(l) = out_len.as_mut() {
            *l = 0;
        }
        let Some(sb) = sb.as_ref() else {
            return std::ptr::null_mut();
        };
        if path.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: caller guarantees `path` is a valid NUL-terminated C string.
        let Ok(path) = CStr::from_ptr(path).to_str() else {
            return std::ptr::null_mut();
        };
        match read_file(sb.uid, path) {
            Ok(data) => copy_out(Some(&data), out_len),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_output_free(out: *mut Output) {
        if !out.is_null() {
            // SAFETY: `out` came from cvisor_run (Box::into_raw).
            drop(Box::from_raw(out));
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_output_stdout(
        out: *mut Output,
        out_len: *mut usize,
    ) -> *mut u8 {
        copy_out(out.as_ref().map(|o| &o.stdout), out_len)
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_output_stderr(
        out: *mut Output,
        out_len: *mut usize,
    ) -> *mut u8 {
        copy_out(out.as_ref().map(|o| &o.stderr), out_len)
    }

    /// Allocate a copy of `data` for the caller. Sets `*out_len` and returns a
    /// pointer to free with cvisor_bytes_free (NULL/0 when empty).
    unsafe fn copy_out(data: Option<&Vec<u8>>, out_len: *mut usize) -> *mut u8 {
        let Some(data) = data else {
            if let Some(l) = out_len.as_mut() {
                *l = 0;
            }
            return std::ptr::null_mut();
        };
        if let Some(l) = out_len.as_mut() {
            *l = data.len();
        }
        if data.is_empty() {
            return std::ptr::null_mut();
        }
        // Hand ownership of a boxed slice's buffer to the caller.
        let boxed = data.clone().into_boxed_slice();
        Box::into_raw(boxed) as *mut u8
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_bytes_free(ptr: *mut u8, len: usize) {
        if ptr.is_null() || len == 0 {
            return;
        }
        // SAFETY: `ptr`/`len` came from copy_out (Box<[u8]> into_raw).
        let slice = std::slice::from_raw_parts_mut(ptr, len);
        drop(Box::from_raw(slice as *mut [u8]));
    }

    /// Start a background session. `pty`: 0 = plain (stdout/stderr captured for
    /// drain), 1 = interactive shell on a PTY (merged output; stdin writable).
    /// For pty=1 `cmd` is ignored and `/bin/sh -i` is run. NULL on error.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_start(
        sb: *mut Sandbox,
        cmd: *const c_char,
        pty: c_int,
    ) -> *mut Session {
        let Some(sb) = sb.as_ref() else {
            return std::ptr::null_mut();
        };
        let opts = ExecOpts {
            allow_network: sb.allow_network,
            allow_listen: sb.allow_listen,
            ..ExecOpts::default()
        };
        let (argv, mode) = if pty != 0 {
            (
                vec!["/bin/sh".to_string(), "-i".to_string()],
                PtyMode::Buffered,
            )
        } else {
            if cmd.is_null() {
                return std::ptr::null_mut();
            }
            // SAFETY: caller guarantees `cmd` is a valid NUL-terminated C string.
            let Ok(cmd) = CStr::from_ptr(cmd).to_str() else {
                return std::ptr::null_mut();
            };
            (shell_argv(cmd), PtyMode::None)
        };
        match spawn_session(sb.uid, sb.log_level, &argv, opts, mode) {
            Ok(s) => Box::into_raw(Box::new(s)),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_read_stdout(
        sess: *mut Session,
        out_len: *mut usize,
    ) -> *mut u8 {
        let data = sess.as_ref().map(|s| s.read_stdout());
        copy_out(data.as_ref(), out_len)
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_read_stderr(
        sess: *mut Session,
        out_len: *mut usize,
    ) -> *mut u8 {
        let data = sess.as_ref().map(|s| s.read_stderr());
        copy_out(data.as_ref(), out_len)
    }

    /// Write to the guest's stdin (PTY sessions only). Returns bytes written, or
    /// -1 on error.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_write_stdin(
        sess: *mut Session,
        data: *const u8,
        len: usize,
    ) -> isize {
        let Some(sess) = sess.as_ref() else {
            return -1;
        };
        if data.is_null() {
            return -1;
        }
        // SAFETY: caller guarantees `data`/`len` describe a valid buffer.
        let bytes = std::slice::from_raw_parts(data, len);
        match sess.write_stdin(bytes) {
            Ok(n) => n as isize,
            Err(_) => -1,
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_resize(sess: *mut Session, rows: u16, cols: u16) {
        if let Some(sess) = sess.as_ref() {
            sess.resize(rows, cols);
        }
    }

    /// Non-blocking exit-code poll. Sets `*done` to 1 and returns the exit code
    /// once the guest has finished; otherwise sets `*done` to 0 and returns 0.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_try_wait(
        sess: *mut Session,
        done: *mut c_int,
    ) -> c_int {
        let code = sess.as_ref().and_then(|s| s.try_wait());
        if let Some(d) = done.as_mut() {
            *d = code.is_some() as c_int;
        }
        code.unwrap_or(0)
    }

    /// Block until the guest exits and return its exit code.
    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_wait(sess: *mut Session) -> c_int {
        sess.as_ref().map(|s| s.wait()).unwrap_or(-1)
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_kill(sess: *mut Session) {
        if let Some(sess) = sess.as_ref() {
            sess.kill();
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn cvisor_session_free(sess: *mut Session) {
        if !sess.is_null() {
            // SAFETY: `sess` came from cvisor_session_start (Box::into_raw).
            drop(Box::from_raw(sess));
        }
    }
}
