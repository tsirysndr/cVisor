//! Sandbox bootstrap: fork the guest, install the seccomp filter, and hand the
//! notify fd to the supervisor.
//!
//! Unlike the original Zig code — which guessed the guest's notify fd number via
//! a `dup(0)` probe (racy) — the supervisor discovers it race-free by scanning
//! `/proc/<child>/fd` for the seccomp-notify link, then obtains the fd itself
//! with `pidfd_getfd`. (We still use `pidfd_getfd` for the transfer, as
//! preferred; a socket + SCM_RIGHTS handshake cannot work here because this
//! filter traps *every* syscall, so the child's post-install `sendmsg` would
//! deadlock against the supervisor.)

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Errno, SysError, SysResult};
use crate::log_buffer::LogBuffer;
use crate::mem::RealGuestMem;
use crate::seccomp::filter;
use crate::seccomp::notifier::IoctlNotifier;
use crate::supervisor::Supervisor;
use crate::types::LogLevel;
use crate::virt::overlay_root::OverlayRoot;

const GUEST_ENVP: &[&str] = &[
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    "HOME=/",
];

/// Options controlling one sandboxed run.
#[derive(Clone, Copy)]
pub struct ExecOpts {
    /// Allow outbound INET/INET6 sockets (default true).
    pub allow_network: bool,
    /// Wall-clock limit; the guest process group is SIGKILLed when it elapses.
    pub timeout: Option<Duration>,
}

impl Default for ExecOpts {
    fn default() -> ExecOpts {
        ExecOpts {
            allow_network: true,
            timeout: None,
        }
    }
}

/// Run `cmd` inside the sandbox, blocking until the guest exits. Captured
/// stdout/stderr accumulate in the provided buffers. Returns the guest's exit
/// code in shell convention: the exit status for a normal exit, or `128 + signo`
/// if it was killed by a signal (e.g. `137` for a timeout SIGKILL).
pub fn execute(
    uid: [u8; 16],
    log_level: LogLevel,
    cmd: &str,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
) -> SysResult<i32> {
    execute_with(uid, log_level, cmd, stdout, stderr, ExecOpts::default())
}

/// Like [`execute`], with explicit per-run [`ExecOpts`].
pub fn execute_with(
    uid: [u8; 16],
    _log_level: LogLevel,
    cmd: &str,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    opts: ExecOpts,
) -> SysResult<i32> {
    // Build EVERYTHING that allocates before fork(). In a multithreaded process
    // (the Node SDK, the test harness) another thread may hold the malloc lock
    // at fork time; the child inherits it locked, so any allocation in the child
    // deadlocks. After fork the child only calls async-signal-safe syscalls.
    let sh = CString::new("/bin/sh").unwrap();
    let dash_c = CString::new("-c").unwrap();
    let cmd_c = CString::new(cmd).map_err(|_| SysError(Errno::INVAL))?;
    let argv: [*const libc::c_char; 4] = [
        sh.as_ptr(),
        dash_c.as_ptr(),
        cmd_c.as_ptr(),
        std::ptr::null(),
    ];
    let envp_owned: Vec<CString> = GUEST_ENVP
        .iter()
        .map(|s| CString::new(*s).unwrap())
        .collect();
    let mut envp: Vec<*const libc::c_char> = envp_owned.iter().map(|c| c.as_ptr()).collect();
    envp.push(std::ptr::null());

    let overlay = OverlayRoot::new(uid).map_err(|_| SysError(Errno::IO))?;

    // SAFETY: fork; the child branch below performs no allocation.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(last_errno());
    }

    if pid == 0 {
        // Child: become its own process-group leader so the supervisor can kill
        // the whole guest tree (e.g. on timeout) with kill(-pgid). Then install
        // the filter (leaking the fd so the supervisor can steal it via pidfd)
        // and execve. No heap allocation on this path.
        // SAFETY: setpgid(0, 0) on the calling process; async-signal-safe.
        unsafe {
            libc::setpgid(0, 0);
        }
        match filter::install() {
            Ok(fd) => std::mem::forget(fd),
            Err(_) => unsafe { libc::_exit(1) },
        }
        // SAFETY: valid NUL-terminated argv/envp built before the fork.
        unsafe {
            libc::execve(sh.as_ptr(), argv.as_ptr(), envp.as_ptr());
            libc::_exit(1); // only reached if execve fails
        }
    }

    let child_pid = pid;
    let notify_fd = steal_notify_fd(child_pid)?;

    let supervisor = Arc::new(Supervisor::new(
        notify_fd.as_raw_fd(),
        child_pid,
        Box::new(RealGuestMem),
        Box::new(IoctlNotifier {
            notify_fd: notify_fd.as_raw_fd(),
        }),
        Box::new(crate::procinfo::real::RealProcInfo),
        stdout,
        stderr,
        overlay,
        opts.allow_network,
    ));

    // Timeout watchdog: SIGKILL the guest process group if the deadline passes.
    // It exits early when `done_tx` is dropped after the guest finishes, so a
    // completed run never signals a recycled pid. `timed_out` records whether it
    // fired, so we report 137 even if the host reaps the guest before we can.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watchdog = opts.timeout.map(|timeout| {
        let timed_out = Arc::clone(&timed_out);
        std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(timeout) {
                timed_out.store(true, std::sync::atomic::Ordering::SeqCst);
                // SAFETY: signal the guest's process group (pgid == child_pid).
                unsafe {
                    libc::kill(-child_pid, libc::SIGKILL);
                }
            }
        })
    });

    // run() consumes an Arc; keep a handle to read the captured exit code after.
    Arc::clone(&supervisor).run();
    // Guest is gone; stop the watchdog before reaping so it can't signal a
    // recycled pid, then keep the notify fd alive until after run() returns.
    drop(done_tx);
    if let Some(w) = watchdog {
        let _ = w.join();
    }
    drop(notify_fd);

    // Reap the guest (best-effort: a host that reaps its own children — e.g. the
    // BEAM's SIGCHLD handler — may beat us to it, leaving waitpid with ECHILD).
    let mut status: libc::c_int = 0;
    // SAFETY: valid child pid; status points at a live int.
    let reaped = unsafe { libc::waitpid(child_pid, &mut status, 0) } == child_pid;

    // Prefer the watchdog (we know we SIGKILLed it), then the guest's own
    // exit_group status (host-independent), then a successful waitpid (the only
    // source that distinguishes a signal death we did not cause).
    let code = if timed_out.load(std::sync::atomic::Ordering::SeqCst) {
        128 + libc::SIGKILL
    } else if let Some(code) = supervisor.exit_code() {
        code
    } else if reaped {
        exit_code_from_status(status)
    } else {
        -1
    };
    Ok(code)
}

/// Translate a `waitpid` status into a shell-convention exit code.
fn exit_code_from_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        -1
    }
}

/// Obtain the guest's seccomp-notify fd race-free: open a pidfd, find the fd
/// number by scanning `/proc/<pid>/fd`, then `pidfd_getfd` it.
fn steal_notify_fd(child_pid: i32) -> SysResult<OwnedFd> {
    let pidfd = pidfd_open(child_pid)?;

    // The child installs the filter then immediately execve's (which blocks),
    // so the fd appears shortly after fork. Retry with a light backoff.
    for attempt in 0..2000 {
        if let Some(target) = find_seccomp_fd(child_pid) {
            let local = pidfd_getfd(pidfd.as_raw_fd(), target)?;
            return Ok(local);
        }
        let us = 50 + attempt.min(200);
        std::thread::sleep(Duration::from_micros(us as u64));
    }
    Err(SysError(Errno::IO))
}

/// Scan `/proc/<pid>/fd` for the fd whose link target names the seccomp notifier.
fn find_seccomp_fd(child_pid: i32) -> Option<RawFd> {
    let dir = format!("/proc/{child_pid}/fd");
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let num: RawFd = name.to_str()?.parse().ok()?;
        if let Ok(target) = std::fs::read_link(entry.path()) {
            if target.to_string_lossy().contains("seccomp") {
                return Some(num);
            }
        }
    }
    None
}

fn pidfd_open(pid: i32) -> SysResult<OwnedFd> {
    // SAFETY: pidfd_open with flags=0.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(last_errno());
    }
    // SAFETY: fd is a fresh owned pidfd.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn pidfd_getfd(pidfd: RawFd, target_fd: RawFd) -> SysResult<OwnedFd> {
    // SAFETY: valid pidfd and target fd number in the remote process.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_getfd, pidfd, target_fd, 0) };
    if fd < 0 {
        return Err(last_errno());
    }
    // SAFETY: fd is a fresh owned dup of the remote fd.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn last_errno() -> SysError {
    let e = nix::errno::Errno::last();
    SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO))
}

/// Delete the sandbox overlay tree for `uid` (called when the session ends).
pub fn cleanup_overlay(uid: &[u8; 16]) {
    let uid = String::from_utf8_lossy(uid);
    let path = format!("/tmp/.cvisor/sb/{uid}");
    let _ = std::fs::remove_dir_all(path);
}
