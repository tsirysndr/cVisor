//! Thin libc wrappers returning `SysResult`, shared by the fd-backed backends.

use crate::error::{Errno, SysError, SysResult};
use std::ffi::CString;
use std::os::fd::RawFd;

/// statx mask requesting the basic stat fields (`STATX_BASIC_STATS`).
pub const STATX_BASIC_STATS: u32 = 0x0000_07ff;

/// statx result-mask bits (`linux/stat.h`) — libc doesn't expose them all.
pub mod statx_mask {
    pub const TYPE: u64 = 0x0001;
    pub const MODE: u64 = 0x0002;
    pub const NLINK: u64 = 0x0004;
    pub const UID: u64 = 0x0008;
    pub const GID: u64 = 0x0010;
    pub const ATIME: u64 = 0x0020;
    pub const MTIME: u64 = 0x0040;
    pub const CTIME: u64 = 0x0080;
    pub const INO: u64 = 0x0100;
    pub const SIZE: u64 = 0x0200;
    pub const BLOCKS: u64 = 0x0400;
}

/// `struct statx_timestamp` (`linux/stat.h`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct StatxTimestamp {
    pub tv_sec: i64,
    pub tv_nsec: u32,
    pub __reserved: i32,
}

/// `struct statx` (256 bytes). Declared here because libc omits it on musl.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Statx {
    pub stx_mask: u32,
    pub stx_blksize: u32,
    pub stx_attributes: u64,
    pub stx_nlink: u32,
    pub stx_uid: u32,
    pub stx_gid: u32,
    pub stx_mode: u16,
    pub __spare0: [u16; 1],
    pub stx_ino: u64,
    pub stx_size: u64,
    pub stx_blocks: u64,
    pub stx_attributes_mask: u64,
    pub stx_atime: StatxTimestamp,
    pub stx_btime: StatxTimestamp,
    pub stx_ctime: StatxTimestamp,
    pub stx_mtime: StatxTimestamp,
    pub stx_rdev_major: u32,
    pub stx_rdev_minor: u32,
    pub stx_dev_major: u32,
    pub stx_dev_minor: u32,
    pub stx_mnt_id: u64,
    pub stx_dio_mem_align: u32,
    pub stx_dio_offset_align: u32,
    pub __spare3: [u64; 12],
}

impl Statx {
    fn zeroed() -> Statx {
        // SAFETY: all-zero is a valid bit pattern for this POD struct.
        unsafe { std::mem::zeroed() }
    }
}

const _: () = assert!(std::mem::size_of::<Statx>() == 256);

/// `stx_mask` field width note: the kernel field is u64 but only the low 32
/// bits carry the basic-stats flags used above.
fn raw_statx(dirfd: RawFd, path: *const libc::c_char, flags: i32) -> SysResult<Statx> {
    let mut out = Statx::zeroed();
    // SAFETY: valid path pointer; out is a live 256-byte buffer.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_statx,
            dirfd as libc::c_long,
            path as libc::c_long,
            flags as libc::c_long,
            STATX_BASIC_STATS as libc::c_long,
            &mut out as *mut Statx as libc::c_long,
        )
    };
    if rc < 0 {
        return Err(last_err());
    }
    Ok(out)
}

fn last_err() -> SysError {
    let e = nix::errno::Errno::last();
    SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO))
}

fn cpath(path: &str) -> SysResult<CString> {
    if path.len() > 512 {
        return Err(SysError(Errno::NAMETOOLONG));
    }
    CString::new(path).map_err(|_| SysError(Errno::INVAL))
}

pub fn openat(path: &str, flags: i32, mode: u32) -> SysResult<RawFd> {
    let c = cpath(path)?;
    // SAFETY: valid NUL-terminated path.
    let fd = unsafe { libc::openat(libc::AT_FDCWD, c.as_ptr(), flags, mode as libc::c_uint) };
    if fd < 0 {
        return Err(last_err());
    }
    Ok(fd)
}

pub fn read(fd: RawFd, buf: &mut [u8]) -> SysResult<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    // SAFETY: buf is a valid writable region of buf.len() bytes.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        return Err(last_err());
    }
    Ok(n as usize)
}

pub fn write(fd: RawFd, data: &[u8]) -> SysResult<usize> {
    // SAFETY: data is a valid readable region of data.len() bytes.
    let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
    if n < 0 {
        return Err(last_err());
    }
    Ok(n as usize)
}

pub fn lseek(fd: RawFd, offset: i64, whence: u32) -> SysResult<i64> {
    // SAFETY: plain lseek on an owned fd.
    let pos = unsafe { libc::lseek(fd, offset as libc::off_t, whence as libc::c_int) };
    if pos < 0 {
        return Err(last_err());
    }
    Ok(pos as i64)
}

pub fn close(fd: RawFd) {
    // SAFETY: closing an owned fd; EBADF is ignored (matches Zig test fds).
    unsafe {
        libc::close(fd);
    }
}

/// `statx` on an open fd (AT_EMPTY_PATH).
pub fn statx_fd(fd: RawFd) -> SysResult<Statx> {
    let empty = c"";
    raw_statx(fd, empty.as_ptr(), libc::AT_EMPTY_PATH)
}

/// `statx` on a path (opens O_PATH first, works on any file type).
pub fn statx_path(path: &str) -> SysResult<Statx> {
    let fd = openat(path, libc::O_PATH, 0)?;
    let out = statx_fd(fd);
    close(fd);
    out
}
