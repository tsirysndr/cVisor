//! A virtual open-file description: a backend plus the path/flags/dir-offset
//! the guest opened it with. Shared via `Arc` so dup'd fds alias one file.
//! Port of `File.zig` (fd-backed backends for this milestone).

use crate::error::SysResult;
use crate::types::Stat;
use crate::virt::fs::backend::sys::Statx;
use crate::virt::fs::backend::Backend;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct File {
    pub backend: Backend,
    pub opened_path: Option<String>,
    pub open_flags: i32,
    dirents_offset: AtomicUsize,
}

impl File {
    pub fn new(backend: Backend) -> File {
        File {
            backend,
            opened_path: None,
            open_flags: 0,
            dirents_offset: AtomicUsize::new(0),
        }
    }

    pub fn with_path(backend: Backend, opened_path: Option<String>, open_flags: i32) -> File {
        File {
            backend,
            opened_path,
            open_flags,
            dirents_offset: AtomicUsize::new(0),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> SysResult<usize> {
        self.backend.read(buf)
    }

    pub fn write(&self, data: &[u8]) -> SysResult<usize> {
        self.backend.write(data)
    }

    pub fn lseek(&self, offset: i64, whence: u32) -> SysResult<i64> {
        let pos = self.backend.lseek(offset, whence)?;
        if pos == 0 {
            self.dirents_offset.store(0, Ordering::Relaxed);
        }
        Ok(pos)
    }

    pub fn statx(&self) -> SysResult<Statx> {
        self.backend.statx()
    }

    pub fn stat(&self) -> SysResult<Stat> {
        Ok(statx_to_stat(&self.statx()?))
    }

    pub fn backing_fd(&self) -> Option<RawFd> {
        self.backend.backing_fd()
    }

    pub fn connect(&self, addr: &[u8]) -> SysResult<()> {
        self.backend.connect(addr)
    }

    pub fn bind(&self, addr: &[u8]) -> SysResult<()> {
        self.backend.bind(addr)
    }

    pub fn read_can_block(&self) -> bool {
        self.backend.read_can_block()
    }

    pub fn poll_readable(&self, timeout_ms: i32) -> SysResult<bool> {
        self.backend.poll_readable(timeout_ms)
    }

    pub fn socket_is_nonblocking(&self) -> SysResult<bool> {
        self.backend.socket_is_nonblocking()
    }

    pub fn shutdown(&self, how: i32) -> SysResult<()> {
        self.backend.shutdown(how)
    }

    pub fn send_to(&self, data: &[u8], flags: i32, dest: Option<&[u8]>) -> SysResult<usize> {
        self.backend.send_to(data, flags, dest)
    }

    pub fn recv_from(
        &self,
        buf: &mut [u8],
        flags: i32,
        src: Option<&mut [u8]>,
    ) -> SysResult<(usize, u32)> {
        self.backend.recv_from(buf, flags, src)
    }

    /// Duplicate this file (fd and metadata) for a forked fd-table copy.
    pub fn try_duplicate(&self) -> Option<File> {
        let backend = self.backend.duplicate()?;
        Some(File::with_path(
            backend,
            self.opened_path.clone(),
            self.open_flags,
        ))
    }

    pub fn dirents_offset(&self) -> usize {
        self.dirents_offset.load(Ordering::Relaxed)
    }

    pub fn set_dirents_offset(&self, v: usize) {
        self.dirents_offset.store(v, Ordering::Relaxed);
    }
}

impl Drop for File {
    fn drop(&mut self) {
        self.backend.close();
    }
}

/// Full Linux `new_encode_dev` (linux/kdev_t.h).
fn makedev(major: u32, minor: u32) -> u64 {
    ((minor & 0xff) as u64)
        | (((major & 0xfff) as u64) << 8)
        | (((minor & !0xffu32) as u64) << 12)
        | (((major & !0xfffu32) as u64) << 32)
}

/// Convert a kernel `statx` into the arch-specific `struct stat`, honoring the
/// populated mask bits. Port of `File.statxToStat`.
pub fn statx_to_stat(sx: &Statx) -> Stat {
    use crate::virt::fs::backend::sys::statx_mask as m;
    let mut st: Stat = unsafe { std::mem::zeroed() };
    let mask = sx.stx_mask as u64;

    if mask & m::MODE != 0 {
        st.st_mode = sx.stx_mode as _;
    }
    if mask & m::NLINK != 0 {
        st.st_nlink = sx.stx_nlink as _;
    }
    if mask & m::SIZE != 0 {
        st.st_size = sx.stx_size as _;
    }
    if mask & m::INO != 0 {
        st.st_ino = sx.stx_ino as _;
    }
    if mask & m::UID != 0 {
        st.st_uid = sx.stx_uid as _;
    }
    if mask & m::GID != 0 {
        st.st_gid = sx.stx_gid as _;
    }
    if mask & m::ATIME != 0 {
        st.st_atime = sx.stx_atime.tv_sec as _;
        st.st_atime_nsec = sx.stx_atime.tv_nsec as _;
    }
    if mask & m::MTIME != 0 {
        st.st_mtime = sx.stx_mtime.tv_sec as _;
        st.st_mtime_nsec = sx.stx_mtime.tv_nsec as _;
    }
    if mask & m::CTIME != 0 {
        st.st_ctime = sx.stx_ctime.tv_sec as _;
        st.st_ctime_nsec = sx.stx_ctime.tv_nsec as _;
    }
    if mask & m::BLOCKS != 0 {
        st.st_blocks = sx.stx_blocks as _;
    }
    // The kernel always populates blksize regardless of mask.
    st.st_blksize = sx.stx_blksize as _;
    st.st_dev = makedev(sx.stx_dev_major, sx.stx_dev_minor);
    st.st_rdev = makedev(sx.stx_rdev_major, sx.stx_rdev_minor);
    st
}
