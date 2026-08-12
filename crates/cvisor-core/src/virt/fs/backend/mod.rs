//! File backends. Each holds the kernel fd(s) backing a virtual file and knows
//! how to read/write/stat/seek it. Ported from `virtual/fs/backend/*.zig`
//! (passthrough, cow, tmp for this milestone; proc and event follow).

pub mod procfile;
pub mod sys;

use crate::error::{Errno, SysError, SysResult};
use crate::virt::overlay_root::OverlayRoot;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Synthetic `/proc` file: content generated at open time, served from memory.
#[derive(Debug)]
pub struct ProcData {
    pub content: Vec<u8>,
    pub is_dir: bool,
    /// Synthetic inode. Non-zero and distinct per rendered file so tools that
    /// guard against reading a file into itself (e.g. GNU grep's `same_file`
    /// check against stdout) don't mistake a /proc file for their output.
    ino: u64,
    offset: AtomicUsize,
}

impl ProcData {
    pub fn new(content: Vec<u8>, is_dir: bool) -> ProcData {
        ProcData {
            ino: synthetic_ino(&content, is_dir),
            content,
            is_dir,
            offset: AtomicUsize::new(0),
        }
    }
}

/// FNV-1a hash of the rendered content, forced non-zero, so distinct /proc
/// files get distinct inodes (dirs and files with equal bodies are mixed apart).
fn synthetic_ino(content: &[u8], is_dir: bool) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in content {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h = (h ^ if is_dir { 1 } else { 2 }).wrapping_mul(0x0000_0100_0000_01b3);
    h | 1
}

/// Which backend serves an open file, and the kernel fd(s) behind it.
#[derive(Debug)]
pub enum Backend {
    /// Direct kernel fd (safe devices, pipes, sockets).
    Passthrough(RawFd),
    /// Read-through to the real file (no copy-up yet).
    CowReadthrough(RawFd),
    /// Writable copy in the overlay `cow/` dir.
    CowWritecopy(RawFd),
    /// File in the private `/tmp` overlay.
    Tmp(RawFd),
    /// Synthetic `/proc` file (no kernel fd).
    Proc(ProcData),
}

impl Backend {
    /// The kernel fd behind this backend (for addfd / passthrough syscalls).
    /// `None` for synthetic backends.
    pub fn backing_fd(&self) -> Option<RawFd> {
        match self {
            Backend::Passthrough(fd)
            | Backend::CowReadthrough(fd)
            | Backend::CowWritecopy(fd)
            | Backend::Tmp(fd) => Some(*fd),
            Backend::Proc(_) => None,
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> SysResult<usize> {
        if let Backend::Proc(p) = self {
            let off = p.offset.load(Ordering::Relaxed).min(p.content.len());
            let n = (p.content.len() - off).min(buf.len());
            buf[..n].copy_from_slice(&p.content[off..off + n]);
            p.offset.store(off + n, Ordering::Relaxed);
            return Ok(n);
        }
        sys::read(self.backing_fd().unwrap(), buf)
    }

    pub fn write(&self, data: &[u8]) -> SysResult<usize> {
        match self {
            Backend::CowReadthrough(_) | Backend::Proc(_) => Err(SysError(Errno::ROFS)),
            _ => sys::write(self.backing_fd().unwrap(), data),
        }
    }

    pub fn lseek(&self, offset: i64, whence: u32) -> SysResult<i64> {
        if let Backend::Proc(p) = self {
            let len = p.content.len() as i64;
            let base = match whence as i32 {
                libc::SEEK_SET => 0,
                libc::SEEK_CUR => p.offset.load(Ordering::Relaxed) as i64,
                libc::SEEK_END => len,
                _ => return Err(SysError(Errno::INVAL)),
            };
            let pos = (base + offset).max(0);
            p.offset.store(pos as usize, Ordering::Relaxed);
            return Ok(pos);
        }
        sys::lseek(self.backing_fd().unwrap(), offset, whence)
    }

    pub fn statx(&self) -> SysResult<sys::Statx> {
        if let Backend::Proc(p) = self {
            return Ok(synthetic_statx(p.is_dir, p.content.len() as u64, p.ino));
        }
        sys::statx_fd(self.backing_fd().unwrap())
    }

    pub fn close(&self) {
        if let Some(fd) = self.backing_fd() {
            sys::close(fd);
        }
    }

    /// Whether this backend wraps a raw kernel fd that supports socket ops.
    /// Only the passthrough backend (used for real sockets) does.
    fn as_socket_fd(&self) -> SysResult<RawFd> {
        match self {
            Backend::Passthrough(fd) => Ok(*fd),
            _ => Err(SysError(Errno::NOTSOCK)),
        }
    }

    pub fn connect(&self, addr: &[u8]) -> SysResult<()> {
        let fd = self.as_socket_fd()?;
        // SAFETY: fd is a real socket; addr is a valid sockaddr of addr.len().
        let rc = unsafe {
            libc::connect(
                fd,
                addr.as_ptr() as *const libc::sockaddr,
                addr.len() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(last_errno());
        }
        Ok(())
    }

    pub fn shutdown(&self, how: i32) -> SysResult<()> {
        let fd = self.as_socket_fd()?;
        // SAFETY: fd is a real socket.
        let rc = unsafe { libc::shutdown(fd, how) };
        if rc < 0 {
            return Err(last_errno());
        }
        Ok(())
    }

    pub fn send_to(&self, data: &[u8], flags: i32, dest: Option<&[u8]>) -> SysResult<usize> {
        let fd = self.as_socket_fd()?;
        let (addr_ptr, addr_len) = match dest {
            Some(a) => (
                a.as_ptr() as *const libc::sockaddr,
                a.len() as libc::socklen_t,
            ),
            None => (std::ptr::null(), 0),
        };
        // SAFETY: fd is a real socket; data/addr are valid for their lengths.
        let n = unsafe {
            libc::sendto(
                fd,
                data.as_ptr() as *const libc::c_void,
                data.len(),
                flags,
                addr_ptr,
                addr_len,
            )
        };
        if n < 0 {
            return Err(last_errno());
        }
        Ok(n as usize)
    }

    /// recvfrom into `buf`; optionally fills `src` with the source address,
    /// returning (bytes, src_len).
    pub fn recv_from(
        &self,
        buf: &mut [u8],
        flags: i32,
        src: Option<&mut [u8]>,
    ) -> SysResult<(usize, u32)> {
        let fd = self.as_socket_fd()?;
        let (addr_ptr, mut addr_len, has_addr) = match &src {
            Some(a) => (
                a.as_ptr() as *mut libc::sockaddr,
                a.len() as libc::socklen_t,
                true,
            ),
            None => (std::ptr::null_mut(), 0, false),
        };
        let len_ptr = if has_addr {
            &mut addr_len as *mut libc::socklen_t
        } else {
            std::ptr::null_mut()
        };
        // SAFETY: fd is a real socket; buf/src are valid for their lengths.
        let n = unsafe {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                flags,
                addr_ptr,
                len_ptr,
            )
        };
        let _ = src;
        if n < 0 {
            return Err(last_errno());
        }
        Ok((n as usize, addr_len))
    }

    /// Duplicate the backing fd into a fresh backend of the same kind (for the
    /// fork fd-table copy — each cloned File owns its own kernel fd, avoiding
    /// the double-close bug in the original Zig code). None if the dup fails.
    pub fn duplicate(&self) -> Option<Backend> {
        if let Backend::Proc(p) = self {
            // Synthetic file: clone the content with a fresh offset.
            return Some(Backend::Proc(ProcData::new(p.content.clone(), p.is_dir)));
        }
        let fd = self.backing_fd()?;
        // SAFETY: F_DUPFD_CLOEXEC on an owned fd.
        let newfd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if newfd < 0 {
            return None;
        }
        Some(match self {
            Backend::Passthrough(_) => Backend::Passthrough(newfd),
            Backend::CowReadthrough(_) => Backend::CowReadthrough(newfd),
            Backend::CowWritecopy(_) => Backend::CowWritecopy(newfd),
            Backend::Tmp(_) => Backend::Tmp(newfd),
            Backend::Proc(_) => unreachable!(),
        })
    }
}

fn last_errno() -> SysError {
    let e = nix::errno::Errno::last();
    SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO))
}

/// Build a synthetic `statx` for a proc file/dir: dirs are `S_IFDIR|0555`,
/// files `S_IFREG|0444`, with mode/nlink/size masked in so statx_to_stat picks
/// them up.
fn synthetic_statx(is_dir: bool, size: u64, ino: u64) -> sys::Statx {
    use sys::statx_mask as m;
    // SAFETY: all-zero is a valid bit pattern for this POD struct.
    let mut sx: sys::Statx = unsafe { std::mem::zeroed() };
    sx.stx_mask = (m::MODE | m::NLINK | m::SIZE | m::INO) as u32;
    sx.stx_blksize = 4096;
    sx.stx_ino = ino;
    // A synthetic device id for the cvisor /proc view — non-zero and unlike any
    // real filesystem/pipe device, so (dev, ino) never collides with a real fd.
    sx.stx_dev_major = 0;
    sx.stx_dev_minor = 0x63; // 'c'
    if is_dir {
        sx.stx_mode = (libc::S_IFDIR | 0o555) as u16;
        sx.stx_nlink = 2;
        sx.stx_size = 0;
    } else {
        sx.stx_mode = (libc::S_IFREG | 0o444) as u16;
        sx.stx_nlink = 1;
        sx.stx_size = size;
    }
    sx
}

/// Open a synthetic `/proc` file/dir. `content` is the pre-rendered body (empty
/// for directories).
pub fn proc_open(content: Vec<u8>, is_dir: bool) -> Backend {
    Backend::Proc(ProcData::new(content, is_dir))
}

fn has_write_flags(flags: i32) -> bool {
    let acc = flags & libc::O_ACCMODE;
    acc == libc::O_WRONLY
        || acc == libc::O_RDWR
        || (flags & libc::O_CREAT) != 0
        || (flags & libc::O_TRUNC) != 0
}

/// Open a passthrough device.
pub fn passthrough_open(path: &str, flags: i32, mode: u32) -> SysResult<Backend> {
    Ok(Backend::Passthrough(sys::openat(path, flags, mode)?))
}

/// Open a tmp-overlay file (no lower layer; the guest's /tmp is fresh).
pub fn tmp_open(overlay: &OverlayRoot, path: &str, flags: i32, mode: u32) -> SysResult<Backend> {
    let resolved = overlay.resolve_tmp(path)?;
    Ok(Backend::Tmp(sys::openat(&resolved, flags, mode)?))
}

/// Open a copy-on-write file: read-through when read-only and no copy exists,
/// otherwise a writable copy in the overlay (copying up the real file first).
pub fn cow_open(overlay: &OverlayRoot, path: &str, flags: i32, mode: u32) -> SysResult<Backend> {
    let cow_exists = overlay.cow_exists(path);
    let cow_path = overlay.resolve_cow(path);

    if cow_exists {
        Ok(Backend::CowWritecopy(sys::openat(&cow_path, flags, mode)?))
    } else if has_write_flags(flags) {
        overlay
            .create_cow_parent_dirs(path)
            .map_err(|_| SysError(Errno::IO))?;
        if OverlayRoot::path_exists_on_real_fs(path) {
            copy_file(path, &cow_path)?;
        } else if (flags & libc::O_CREAT) == 0 {
            return Err(SysError(Errno::NOENT));
        }
        Ok(Backend::CowWritecopy(sys::openat(&cow_path, flags, mode)?))
    } else {
        Ok(Backend::CowReadthrough(sys::openat(path, flags, mode)?))
    }
}

/// statx a path from the guest's view: cow copy if present, else the real path.
pub fn cow_statx_path(overlay: &OverlayRoot, path: &str) -> SysResult<sys::Statx> {
    if overlay.cow_exists(path) {
        sys::statx_path(&overlay.resolve_cow(path))
    } else {
        sys::statx_path(path)
    }
}

/// statx a path in the tmp overlay.
pub fn tmp_statx_path(overlay: &OverlayRoot, path: &str) -> SysResult<sys::Statx> {
    sys::statx_path(&overlay.resolve_tmp(path)?)
}

/// statx a real path (passthrough).
pub fn passthrough_statx_path(path: &str) -> SysResult<sys::Statx> {
    sys::statx_path(path)
}

/// faccessat(F_OK-style) existence check on the guest path (real lower layer).
pub fn real_access(path: &str, mode: i32) -> SysResult<()> {
    let c = std::ffi::CString::new(path).map_err(|_| SysError(Errno::INVAL))?;
    // SAFETY: valid path; faccessat with AT_FDCWD.
    let rc = unsafe { libc::faccessat(libc::AT_FDCWD, c.as_ptr(), mode, 0) };
    if rc < 0 {
        return Err(last_errno());
    }
    Ok(())
}

/// readlink on a path from the guest's view (cow copy if present, else real).
pub fn cow_readlink(overlay: &OverlayRoot, path: &str, buf: &mut [u8]) -> SysResult<usize> {
    let target = if overlay.cow_exists(path) {
        overlay.resolve_cow(path)
    } else {
        path.to_string()
    };
    do_readlink(&target, buf)
}

/// readlink in the tmp overlay.
pub fn tmp_readlink(overlay: &OverlayRoot, path: &str, buf: &mut [u8]) -> SysResult<usize> {
    do_readlink(&overlay.resolve_tmp(path)?, buf)
}

fn do_readlink(path: &str, buf: &mut [u8]) -> SysResult<usize> {
    let c = std::ffi::CString::new(path).map_err(|_| SysError(Errno::INVAL))?;
    // SAFETY: valid path; buf is a writable region of buf.len() bytes.
    let n = unsafe { libc::readlink(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if n < 0 {
        return Err(last_errno());
    }
    Ok(n as usize)
}

/// mkdir into the cow overlay (creating parent dirs first).
pub fn cow_mkdir(overlay: &OverlayRoot, path: &str, mode: u32) -> SysResult<()> {
    overlay
        .create_cow_parent_dirs(path)
        .map_err(|_| SysError(Errno::IO))?;
    do_mkdir(&overlay.resolve_cow(path), mode)
}

/// mkdir into the tmp overlay (creating parent dirs first).
pub fn tmp_mkdir(overlay: &OverlayRoot, path: &str, mode: u32) -> SysResult<()> {
    let resolved = overlay.resolve_tmp(path)?;
    OverlayRoot::create_parent_dirs(&resolved).map_err(|_| SysError(Errno::NOENT))?;
    do_mkdir(&resolved, mode)
}

/// chmod a path in the cow overlay, copying up the real file first if needed.
pub fn cow_fchmodat(overlay: &OverlayRoot, path: &str, mode: u32) -> SysResult<()> {
    let target = if overlay.cow_exists(path) {
        overlay.resolve_cow(path)
    } else if OverlayRoot::path_exists_on_real_fs(path) {
        let cow = overlay.resolve_cow(path);
        overlay
            .create_cow_parent_dirs(path)
            .map_err(|_| SysError(Errno::IO))?;
        copy_file(path, &cow)?;
        cow
    } else {
        return Err(SysError(Errno::NOENT));
    };
    do_chmod(&target, mode)
}

/// chmod a path in the tmp overlay.
pub fn tmp_fchmodat(overlay: &OverlayRoot, path: &str, mode: u32) -> SysResult<()> {
    do_chmod(&overlay.resolve_tmp(path)?, mode)
}

fn do_chmod(path: &str, mode: u32) -> SysResult<()> {
    let c = std::ffi::CString::new(path).map_err(|_| SysError(Errno::INVAL))?;
    // SAFETY: valid path.
    let rc = unsafe { libc::chmod(c.as_ptr(), mode as libc::mode_t) };
    if rc < 0 {
        return Err(last_errno());
    }
    Ok(())
}

/// utimensat on a path in the cow overlay (copy-up first so the host file's
/// timestamps aren't mutated). `times` is the raw [timespec;2] or None (now).
pub fn cow_utimensat(overlay: &OverlayRoot, path: &str, times: Option<&[u8]>) -> SysResult<()> {
    let target = if overlay.cow_exists(path) {
        overlay.resolve_cow(path)
    } else if OverlayRoot::path_exists_on_real_fs(path) {
        let cow = overlay.resolve_cow(path);
        overlay
            .create_cow_parent_dirs(path)
            .map_err(|_| SysError(Errno::IO))?;
        copy_file(path, &cow)?;
        cow
    } else {
        return Err(SysError(Errno::NOENT));
    };
    do_utimensat(&target, times)
}

/// utimensat on a path in the tmp overlay.
pub fn tmp_utimensat(overlay: &OverlayRoot, path: &str, times: Option<&[u8]>) -> SysResult<()> {
    do_utimensat(&overlay.resolve_tmp(path)?, times)
}

fn do_utimensat(path: &str, times: Option<&[u8]>) -> SysResult<()> {
    let c = std::ffi::CString::new(path).map_err(|_| SysError(Errno::INVAL))?;
    let times_ptr = match times {
        // Two struct timespec = 32 bytes on 64-bit.
        Some(t) if t.len() >= 32 => t.as_ptr() as *const libc::timespec,
        _ => std::ptr::null(),
    };
    // SAFETY: valid path; times_ptr is null or a valid [timespec;2].
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times_ptr, 0) };
    if rc < 0 {
        return Err(last_errno());
    }
    Ok(())
}

/// Create a symlink in the cow overlay (target stored verbatim).
pub fn cow_symlink(overlay: &OverlayRoot, target: &str, linkpath: &str) -> SysResult<()> {
    overlay
        .create_cow_parent_dirs(linkpath)
        .map_err(|_| SysError(Errno::IO))?;
    do_symlink(target, &overlay.resolve_cow(linkpath))
}

/// Create a symlink in the tmp overlay.
pub fn tmp_symlink(overlay: &OverlayRoot, target: &str, linkpath: &str) -> SysResult<()> {
    let resolved = overlay.resolve_tmp(linkpath)?;
    OverlayRoot::create_parent_dirs(&resolved).map_err(|_| SysError(Errno::NOENT))?;
    do_symlink(target, &resolved)
}

fn do_symlink(target: &str, linkpath: &str) -> SysResult<()> {
    let t = std::ffi::CString::new(target).map_err(|_| SysError(Errno::INVAL))?;
    let l = std::ffi::CString::new(linkpath).map_err(|_| SysError(Errno::INVAL))?;
    // SAFETY: both are valid NUL-terminated paths.
    let rc = unsafe { libc::symlink(t.as_ptr(), l.as_ptr()) };
    if rc < 0 {
        return Err(last_errno());
    }
    Ok(())
}

fn do_mkdir(path: &str, mode: u32) -> SysResult<()> {
    let c = std::ffi::CString::new(path).map_err(|_| SysError(Errno::INVAL))?;
    // SAFETY: valid path.
    let rc = unsafe { libc::mkdir(c.as_ptr(), mode as libc::mode_t) };
    if rc < 0 {
        return Err(last_errno());
    }
    Ok(())
}

/// Physically remove a cow overlay file/dir (best-effort; tombstone is truth).
pub fn cow_remove(overlay: &OverlayRoot, path: &str, is_dir: bool) {
    remove_path(&overlay.resolve_cow(path), is_dir);
}

/// Physically remove a tmp overlay file/dir.
pub fn tmp_remove(overlay: &OverlayRoot, path: &str, is_dir: bool) -> SysResult<()> {
    let resolved = overlay.resolve_tmp(path)?;
    remove_path(&resolved, is_dir);
    Ok(())
}

fn remove_path(path: &str, is_dir: bool) {
    let Ok(c) = std::ffi::CString::new(path) else {
        return;
    };
    // SAFETY: valid path; unlink for files, rmdir for dirs.
    unsafe {
        if is_dir {
            libc::rmdir(c.as_ptr());
        } else {
            libc::unlink(c.as_ptr());
        }
    }
}

/// Build a merged directory listing for a cow directory: the real lower layer
/// plus the overlay (overlay names win on collision but real d_types are kept).
pub fn cow_merged_dirents(
    overlay: &OverlayRoot,
    path: &str,
) -> crate::virt::fs::dirent::DirEntryMap {
    use crate::virt::fs::dirent::{collect_dirents, DirEntryMap};
    let mut map = DirEntryMap::new();
    read_dir_into(path, &mut map, false);
    let cow = overlay.resolve_cow(path);
    read_dir_into(&cow, &mut map, true);
    let _ = collect_dirents; // (collect_dirents used inside read_dir_into)
    map
}

/// Directory listing for a tmp overlay directory (single layer).
pub fn tmp_merged_dirents(
    overlay: &OverlayRoot,
    path: &str,
) -> SysResult<crate::virt::fs::dirent::DirEntryMap> {
    use crate::virt::fs::dirent::DirEntryMap;
    let mut map = DirEntryMap::new();
    read_dir_into(&overlay.resolve_tmp(path)?, &mut map, false);
    Ok(map)
}

/// Open `dir_path`, getdents64 it fully, and collect into `map`.
fn read_dir_into(dir_path: &str, map: &mut crate::virt::fs::dirent::DirEntryMap, dedup: bool) {
    use crate::virt::fs::dirent::collect_dirents;
    let Ok(fd) = sys::openat(dir_path, libc::O_RDONLY | libc::O_DIRECTORY, 0) else {
        return;
    };
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: valid dir fd; buf is a writable 4096-byte region.
        let n = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                fd as libc::c_long,
                buf.as_mut_ptr() as libc::c_long,
                buf.len() as libc::c_long,
            )
        };
        if n <= 0 {
            break;
        }
        collect_dirents(&buf[..n as usize], map, dedup);
    }
    sys::close(fd);
}

/// Copy `src` -> `dst` (used for cow copy-up).
fn copy_file(src: &str, dst: &str) -> SysResult<()> {
    let in_fd = sys::openat(src, libc::O_RDONLY, 0)?;
    let out_fd = match sys::openat(dst, libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC, 0o644) {
        Ok(fd) => fd,
        Err(e) => {
            sys::close(in_fd);
            return Err(e);
        }
    };
    let mut buf = [0u8; 4096];
    let result = (|| loop {
        let n = sys::read(in_fd, &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let mut written = 0;
        while written < n {
            written += sys::write(out_fd, &buf[written..n])?;
        }
    })();
    sys::close(in_fd);
    sys::close(out_fd);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // A synthetic /proc file must report a non-zero, distinct (dev, ino).
    // Zero/zero collided with what GNU grep sees for its stdout, tripping its
    // "input file is also the output" self-loop guard so it refused to read.
    #[test]
    fn proc_statx_has_nonzero_distinct_identity() {
        let status = proc_open(b"Name:\tcvisor-guest\nPid:\t7\n".to_vec(), false)
            .statx()
            .unwrap();
        assert_ne!(status.stx_ino, 0);
        assert_ne!(status.stx_dev_minor, 0);
        assert_ne!(status.stx_mask & sys::statx_mask::INO as u32, 0);

        // Different content ⇒ different inode; a dir differs from a file.
        let other = proc_open(b"Name:\tcvisor-guest\nPid:\t8\n".to_vec(), false)
            .statx()
            .unwrap();
        let dir = proc_open(Vec::new(), true).statx().unwrap();
        assert_ne!(status.stx_ino, other.stx_ino);
        assert_ne!(status.stx_ino, dir.stx_ino);
    }
}
