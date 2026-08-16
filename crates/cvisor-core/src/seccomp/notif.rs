//! `struct seccomp_notif*` ABI structs, the RECV/SEND/ADDFD ioctls, and reply
//! constructors. Mirrors `linux/seccomp.h`; we declare the structs ourselves
//! rather than depend on libc exposing them.

use std::os::fd::RawFd;

/// `struct seccomp_data` — the trapped syscall's number, arch, IP and args.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeccompData {
    pub nr: i32,
    pub arch: u32,
    pub instruction_pointer: u64,
    pub args: [u64; 6],
}

/// `struct seccomp_notif` — one pending syscall notification (80 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeccompNotif {
    pub id: u64,
    pub pid: u32,
    pub flags: u32,
    pub data: SeccompData,
}

impl SeccompNotif {
    /// A zeroed notif (RECV requires the buffer be zeroed first).
    pub fn zeroed() -> SeccompNotif {
        // SAFETY: all-zeroes is a valid bit pattern for these POD fields.
        unsafe { std::mem::zeroed() }
    }
}

/// `struct seccomp_notif_resp` — the supervisor's reply.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeccompNotifResp {
    pub id: u64,
    pub val: i64,
    pub error: i32,
    pub flags: u32,
}

/// `struct seccomp_notif_addfd` — inject a supervisor fd into the guest.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeccompNotifAddfd {
    pub id: u64,
    pub flags: u32,
    pub srcfd: u32,
    pub newfd: u32,
    pub newfd_flags: u32,
}

/// Tell the kernel to let the trapped syscall proceed unchanged.
pub const USER_NOTIF_FLAG_CONTINUE: u32 = 1;
/// Inject the fd at the exact `newfd` slot requested.
pub const ADDFD_FLAG_SETFD: u32 = 1;

/// Reply: run the real syscall in the guest.
pub fn reply_continue(id: u64) -> SeccompNotifResp {
    SeccompNotifResp {
        id,
        val: 0,
        error: 0,
        flags: USER_NOTIF_FLAG_CONTINUE,
    }
}

/// Reply: syscall succeeded with return value `val`.
pub fn reply_success(id: u64, val: i64) -> SeccompNotifResp {
    SeccompNotifResp {
        id,
        val,
        error: 0,
        flags: 0,
    }
}

/// Reply: syscall failed with the given errno.
pub fn reply_error(id: u64, errno: i32) -> SeccompNotifResp {
    SeccompNotifResp {
        id,
        val: -1,
        error: -errno,
        flags: 0,
    }
}

// The seccomp notify ioctls use magic '!' (0x21).
// RECV/SEND are _IOWR; ID_VALID/ADDFD are _IOW.
nix::ioctl_readwrite!(ioctl_notif_recv, b'!', 0, SeccompNotif);
nix::ioctl_readwrite!(ioctl_notif_send, b'!', 1, SeccompNotifResp);
nix::ioctl_write_ptr!(ioctl_notif_id_valid, b'!', 2, u64);
nix::ioctl_write_ptr!(ioctl_notif_addfd, b'!', 3, SeccompNotifAddfd);

/// Receive the next pending notification. Returns `Ok(None)` when the guest has
/// exited (ENOENT).
pub fn recv(notify_fd: RawFd) -> nix::Result<Option<SeccompNotif>> {
    let mut notif = SeccompNotif::zeroed();
    // SAFETY: notify_fd is a valid seccomp listener; notif is a live, zeroed buffer.
    match unsafe { ioctl_notif_recv(notify_fd, &mut notif) } {
        Ok(_) => Ok(Some(notif)),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether notification `id` is still live — i.e. the guest is still blocked
/// waiting on this syscall. Returns false once the guest has been interrupted by
/// a signal or has exited (the ioctl fails with ENOENT). Used to abort a
/// long-running handler so a signal-paced guest is not left wedged.
pub fn id_valid(notify_fd: RawFd, id: u64) -> bool {
    // SAFETY: notify_fd is a valid seccomp listener; id is a live u64.
    unsafe { ioctl_notif_id_valid(notify_fd, &id) }.is_ok()
}

/// Send a reply. ENOENT (task already gone) is treated as success.
pub fn send(notify_fd: RawFd, resp: &SeccompNotifResp) -> nix::Result<()> {
    let mut resp = *resp;
    // SAFETY: notify_fd is a valid seccomp listener; resp is a live buffer.
    match unsafe { ioctl_notif_send(notify_fd, &mut resp) } {
        Ok(_) => Ok(()),
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Inject supervisor fd `srcfd` into the guest at slot `newfd`.
pub fn addfd(
    notify_fd: RawFd,
    id: u64,
    srcfd: RawFd,
    newfd: RawFd,
    cloexec: bool,
) -> nix::Result<()> {
    let req = SeccompNotifAddfd {
        id,
        flags: ADDFD_FLAG_SETFD,
        srcfd: srcfd as u32,
        newfd: newfd as u32,
        newfd_flags: if cloexec { libc::O_CLOEXEC as u32 } else { 0 },
    };
    // SAFETY: notify_fd valid; req is a live, fully-initialized struct.
    unsafe { ioctl_notif_addfd(notify_fd, &req) }.map(|_| ())
}

/// ADDFD letting the kernel pick the guest fd (lowest free slot in the guest's
/// own table — the single allocator). Returns the chosen fd number.
pub fn addfd_auto(notify_fd: RawFd, id: u64, srcfd: RawFd, cloexec: bool) -> nix::Result<RawFd> {
    let req = SeccompNotifAddfd {
        id,
        flags: 0,
        srcfd: srcfd as u32,
        newfd: 0,
        newfd_flags: if cloexec { libc::O_CLOEXEC as u32 } else { 0 },
    };
    // SAFETY: notify_fd valid; req is a live, fully-initialized struct.
    unsafe { ioctl_notif_addfd(notify_fd, &req) }.map(|fd| fd as RawFd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_kernel_abi() {
        assert_eq!(std::mem::size_of::<SeccompData>(), 64);
        assert_eq!(std::mem::size_of::<SeccompNotif>(), 80);
        assert_eq!(std::mem::size_of::<SeccompNotifResp>(), 24);
        assert_eq!(std::mem::size_of::<SeccompNotifAddfd>(), 24);
    }

    #[test]
    fn reply_shapes() {
        let c = reply_continue(5);
        assert_eq!(c.flags, USER_NOTIF_FLAG_CONTINUE);
        assert_eq!(c.error, 0);

        let s = reply_success(7, 42);
        assert_eq!(s.val, 42);
        assert_eq!(s.flags, 0);

        let e = reply_error(9, 13); // EACCES
        assert_eq!(e.val, -1);
        assert_eq!(e.error, -13);
    }
}
