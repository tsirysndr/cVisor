//! Abstraction over the ADDFD side effect so handlers can be unit-tested with
//! a no-op notifier (no real guest) via the injection strategy.

use crate::error::{Errno, SysError, SysResult};
use std::os::fd::RawFd;

pub trait Notifier: Send + Sync {
    /// Inject supervisor fd `srcfd` into the guest at slot `newfd`.
    fn addfd(&self, id: u64, srcfd: RawFd, newfd: RawFd, cloexec: bool) -> SysResult<()>;
}

/// Production notifier issuing the real ADDFD ioctl.
pub struct IoctlNotifier {
    pub notify_fd: RawFd,
}

impl Notifier for IoctlNotifier {
    fn addfd(&self, id: u64, srcfd: RawFd, newfd: RawFd, cloexec: bool) -> SysResult<()> {
        super::notif::addfd(self.notify_fd, id, srcfd, newfd, cloexec).map_err(|e| {
            SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO))
        })
    }
}

/// Test notifier: records nothing, always succeeds.
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn addfd(&self, _id: u64, _srcfd: RawFd, _newfd: RawFd, _cloexec: bool) -> SysResult<()> {
        Ok(())
    }
}
