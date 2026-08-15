//! Installs the seccomp filter in the guest.
//!
//! The filter first checks the audit arch: a syscall issued on any ABI other
//! than the native one (x32 or i386 on an x86-64 kernel, for instance) carries
//! syscall numbers the supervisor would misinterpret, so those are killed
//! outright. Every native-ABI syscall returns `SECCOMP_RET_USER_NOTIF`, so all
//! interception is decided in userspace by the supervisor. (A future
//! optimization is to emit a real allow/deny/notify table so hot process-local
//! syscalls never round-trip; the arch guard already belongs at the front of
//! such a table.)

use std::os::fd::{FromRawFd, OwnedFd, RawFd};

const SECCOMP_SET_MODE_FILTER: libc::c_uint = 1;
const SECCOMP_FILTER_FLAG_NEW_LISTENER: libc::c_ulong = 1 << 3;
const SECCOMP_RET_USER_NOTIF: u32 = 0x7fc0_0000;
const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;

const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_RET: u16 = 0x06;
const BPF_K: u16 = 0x00;

/// Byte offset of `arch` within `struct seccomp_data` (after the u32 `nr`).
const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;

/// The audit arch value the guest must match; anything else is killed.
#[cfg(target_arch = "x86_64")]
const NATIVE_AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const NATIVE_AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

/// Install the arch-guarded USER_NOTIF filter and return the listener fd.
///
/// # Safety
/// Must be called in the guest after `fork` and before `execve`. Sets
/// `NO_NEW_PRIVS` first (required for an unprivileged seccomp filter).
pub fn install() -> Result<OwnedFd, i32> {
    let mut instructions = [
        // A = seccomp_data.arch
        libc::sock_filter {
            code: BPF_LD | BPF_W | BPF_ABS,
            jt: 0,
            jf: 0,
            k: SECCOMP_DATA_ARCH_OFFSET,
        },
        // if A == native arch, skip the kill; otherwise fall through to it.
        libc::sock_filter {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 1,
            jf: 0,
            k: NATIVE_AUDIT_ARCH,
        },
        // Wrong ABI: kill the whole process.
        libc::sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_KILL_PROCESS,
        },
        // Native ABI: hand off to the supervisor.
        libc::sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_USER_NOTIF,
        },
    ];
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
