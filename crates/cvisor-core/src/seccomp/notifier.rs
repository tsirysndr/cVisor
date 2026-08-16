//! Abstraction over the ADDFD side effect so handlers can be unit-tested with
//! a no-op notifier (no real guest) via the injection strategy.

use crate::error::{Errno, SysError, SysResult};
use std::os::fd::RawFd;

pub trait Notifier: Send + Sync {
    /// Inject supervisor fd `srcfd` into the guest at slot `newfd`.
    fn addfd(&self, id: u64, srcfd: RawFd, newfd: RawFd, cloexec: bool) -> SysResult<()>;

    /// ADDFD with the kernel choosing the guest fd number; returns it. Keeps
    /// the guest's own fd table the single fd allocator, so supervisor-created
    /// fds can never clobber kernel-created ones (epoll, timerfd, ...).
    fn addfd_auto(&self, id: u64, srcfd: RawFd, cloexec: bool) -> SysResult<RawFd>;

    /// Whether notification `id` is still live (the guest is still blocked on
    /// this syscall). False once it was interrupted by a signal or exited.
    fn id_valid(&self, id: u64) -> bool;
}

/// Production notifier issuing the real ADDFD ioctl.
pub struct IoctlNotifier {
    pub notify_fd: RawFd,
}

impl Notifier for IoctlNotifier {
    fn addfd(&self, id: u64, srcfd: RawFd, newfd: RawFd, cloexec: bool) -> SysResult<()> {
        super::notif::addfd(self.notify_fd, id, srcfd, newfd, cloexec)
            .map_err(|e| SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO)))
    }

    fn addfd_auto(&self, id: u64, srcfd: RawFd, cloexec: bool) -> SysResult<RawFd> {
        super::notif::addfd_auto(self.notify_fd, id, srcfd, cloexec)
            .map_err(|e| SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO)))
    }

    fn id_valid(&self, id: u64) -> bool {
        super::notif::id_valid(self.notify_fd, id)
    }
}

/// Test notifier: records nothing, always succeeds. `addfd_auto` hands out
/// increasing fd numbers from 3, mimicking a fresh guest's kernel allocator.
#[derive(Default)]
pub struct NoopNotifier {
    next: std::sync::atomic::AtomicI32,
}

impl Notifier for NoopNotifier {
    fn addfd(&self, _id: u64, _srcfd: RawFd, _newfd: RawFd, _cloexec: bool) -> SysResult<()> {
        Ok(())
    }

    fn addfd_auto(&self, _id: u64, _srcfd: RawFd, _cloexec: bool) -> SysResult<RawFd> {
        use std::sync::atomic::Ordering;
        let _ = self
            .next
            .compare_exchange(0, 3, Ordering::Relaxed, Ordering::Relaxed);
        Ok(self.next.fetch_add(1, Ordering::Relaxed))
    }

    fn id_valid(&self, _id: u64) -> bool {
        true
    }
}
