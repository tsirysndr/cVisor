//! Installs the seccomp filter in the guest.
//!
//! The filter is a single BPF instruction: return `SECCOMP_RET_USER_NOTIF` for
//! every syscall, so all interception is decided in userspace by the
//! supervisor. (A future optimization is to emit a real allow/deny/notify
//! instruction table so hot syscalls never round-trip.)

use std::os::fd::{FromRawFd, OwnedFd, RawFd};

const SECCOMP_SET_MODE_FILTER: libc::c_uint = 1;
const SECCOMP_FILTER_FLAG_NEW_LISTENER: libc::c_ulong = 1 << 3;
const SECCOMP_RET_USER_NOTIF: u32 = 0x7fc0_0000;

const BPF_RET: u16 = 0x06;
const BPF_K: u16 = 0x00;

/// Install the all-trapping USER_NOTIF filter and return the listener fd.
///
/// # Safety
/// Must be called in the guest after `fork` and before `execve`. Sets
/// `NO_NEW_PRIVS` first (required for an unprivileged seccomp filter).
pub fn install() -> Result<OwnedFd, i32> {
    let mut instructions = [libc::sock_filter {
        code: BPF_RET | BPF_K,
        jt: 0,
        jf: 0,
        k: SECCOMP_RET_USER_NOTIF,
    }];
    let prog = libc::sock_fprog {
        len: instructions.len() as u16,
        filter: instructions.as_mut_ptr(),
    };

    // SAFETY: standard prctl/seccomp calls with valid arguments.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(errno());
        }
        let fd = libc::syscall(
            libc::SYS_seccomp,
            SECCOMP_SET_MODE_FILTER as libc::c_long,
            SECCOMP_FILTER_FLAG_NEW_LISTENER as libc::c_long,
            &prog as *const libc::sock_fprog as libc::c_long,
        );
        if fd < 0 {
            return Err(errno());
        }
        Ok(OwnedFd::from_raw_fd(fd as RawFd))
    }
}

fn errno() -> i32 {
    // SAFETY: __errno_location returns a valid pointer to thread-local errno.
    unsafe { *libc::__errno_location() }
}
