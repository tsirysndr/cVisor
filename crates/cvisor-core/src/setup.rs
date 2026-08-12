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

/// Run `cmd` inside the sandbox, blocking until the guest exits. Captured
/// stdout/stderr accumulate in the provided buffers.
pub fn execute(
    uid: [u8; 16],
    _log_level: LogLevel,
    cmd: &str,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
) -> SysResult<()> {
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
        // Child: install the filter (leaking the fd so the supervisor can steal
        // it via pidfd), then execve. No heap allocation on this path.
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
    ));
    supervisor.run();
    // Keep the notify fd alive for the whole run.
    drop(notify_fd);

    // Reap the guest.
    // SAFETY: valid pid; status pointer is optional (null).
    unsafe {
        libc::waitpid(child_pid, std::ptr::null_mut(), 0);
    }
    Ok(())
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
