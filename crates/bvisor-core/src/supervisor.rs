//! The supervisor: receives seccomp notifications, dispatches them over shared
//! virtual state, and replies.
//!
//! One reader thread owns the RECV ioctl (a kernel quirk hangs multiple readers
//! after the filter dies); a bounded pool of workers handles notifications and
//! each issues its own SEND ioctl. All virtual state lives behind one mutex,
//! matching the original single global lock.
//!
//! Milestone 3 dispatches the core file/IO syscalls (openat, close, read,
//! write, writev, lseek, fstat, dup3) over an FD table + overlay; everything
//! else is continued. Per-process FD tables and the redirect save/restore dance
//! arrive with the process model in M4 (this uses one shared table + cwd).

use std::os::fd::RawFd;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::error::SysResult;
use crate::log_buffer::LogBuffer;
use crate::mem::{GuestMem, GuestMemExt};
use crate::procinfo::ProcInfo;
use crate::seccomp::notif::{self, SeccompNotif, SeccompNotifResp};
use crate::seccomp::notifier::Notifier;
use crate::virt::fs::fd_table::FdTable;
use crate::virt::fs::file::File;
use crate::virt::overlay_root::OverlayRoot;
use crate::virt::path::{resolve_and_route, BackendType, ResolvedRoute};
use crate::virt::proc::Threads;
use crate::virt::tombstones::Tombstones;
use crate::virt::fs::backend;

const MAX_INFLIGHT: usize = 8;
const IO_CHUNK: usize = 4096;
const MAX_IOV: usize = 16;

/// All virtual sandbox state, guarded by one mutex.
pub struct VirtState {
    pub overlay: OverlayRoot,
    pub tombstones: Tombstones,
    /// Virtual process tree: owns the per-thread FD tables and cwds, so a
    /// child's redirects (dup2 onto fd 1, etc.) don't corrupt the parent.
    pub threads: Threads,
    /// Short `/.b/XXX` symlinks for execve overlay redirection.
    pub symlinks: crate::virt::symlinks::manager::Symlinks,
}

impl VirtState {
    /// Ensure `tid` is registered (lazy /proc discovery) and return its FD table.
    fn fd_table(&mut self, tid: i32, procinfo: &dyn ProcInfo) -> Option<&mut FdTable> {
        self.threads.get_or_sync(tid, procinfo);
        self.threads.fd_table_mut(tid)
    }

    /// Resolve a guest path against the caller's cwd (or a dirfd's opened path
    /// for `*at` syscalls) and route it. Registers the caller first.
    fn resolve_path(
        &mut self,
        caller: i32,
        path: &str,
        dirfd: i32,
        procinfo: &dyn ProcInfo,
    ) -> SysResult<ResolvedRoute> {
        use crate::error::{Errno, SysError};
        self.threads.get_or_sync(caller, procinfo);
        let base: String = if path.starts_with('/') {
            "/".to_string()
        } else if dirfd != libc::AT_FDCWD {
            self.threads
                .fd_table_mut(caller)
                .and_then(|t| t.get(dirfd))
                .ok_or(SysError(Errno::BADF))?
                .opened_path
                .clone()
                .ok_or(SysError(Errno::NOTDIR))?
        } else {
            self.threads.cwd(caller).unwrap_or("/").to_string()
        };
        resolve_and_route(&base, path)
    }
}

pub struct Supervisor {
    notify_fd: RawFd,
    #[allow(dead_code)]
    init_guest_tid: i32,
    mem: Box<dyn GuestMem>,
    notifier: Box<dyn Notifier>,
    procinfo: Box<dyn ProcInfo>,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    start: std::time::Instant,
    state: Mutex<VirtState>,
}

impl Supervisor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notify_fd: RawFd,
        init_guest_tid: i32,
        mem: Box<dyn GuestMem>,
        notifier: Box<dyn Notifier>,
        procinfo: Box<dyn ProcInfo>,
        stdout: Arc<LogBuffer>,
        stderr: Arc<LogBuffer>,
        overlay: OverlayRoot,
    ) -> Supervisor {
        Supervisor {
            notify_fd,
            init_guest_tid,
            mem,
            notifier,
            procinfo,
            stdout,
            stderr,
            start: std::time::Instant::now(),
            state: Mutex::new(VirtState {
                overlay,
                tombstones: Tombstones::new(),
                threads: Threads::new(init_guest_tid),
                symlinks: crate::virt::symlinks::manager::Symlinks::new(),
            }),
        }
    }

    /// Run until the guest exits.
    pub fn run(self: Arc<Self>) {
        let (tx, rx) = crossbeam_channel::bounded::<SeccompNotif>(0);

        let workers: Vec<_> = (0..MAX_INFLIGHT)
            .map(|_| {
                let sup = Arc::clone(&self);
                let rx = rx.clone();
                thread::spawn(move || {
                    for notif in rx.iter() {
                        let resp = sup.handle(&notif);
                        let _ = notif::send(sup.notify_fd, &resp);
                    }
                })
            })
            .collect();
        drop(rx);

        while let Ok(Some(notif)) = self.recv() {
            if tx.send(notif).is_err() {
                break;
            }
        }
        drop(tx);
        for w in workers {
            let _ = w.join();
        }
    }

    fn recv(&self) -> nix::Result<Option<SeccompNotif>> {
        let mut pfd = libc::pollfd {
            fd: self.notify_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: single valid pollfd, infinite timeout.
        let rc = unsafe { libc::poll(&mut pfd, 1, -1) };
        if rc < 0 {
            return Err(nix::errno::Errno::last());
        }
        if pfd.revents & libc::POLLIN == 0 {
            return Ok(None); // POLLHUP: guest exited
        }
        notif::recv(self.notify_fd)
    }

    /// Dispatch one notification. Public so handler unit tests can drive it.
    pub fn handle(&self, notif: &SeccompNotif) -> SeccompNotifResp {
        let nr = notif.data.nr as i64;
        let result = if nr == libc::SYS_openat {
            self.sys_openat(notif)
        } else if nr == libc::SYS_close {
            self.sys_close(notif)
        } else if nr == libc::SYS_read {
            self.sys_read(notif)
        } else if nr == libc::SYS_write {
            self.sys_write(notif)
        } else if nr == libc::SYS_writev {
            self.sys_writev(notif)
        } else if nr == libc::SYS_lseek {
            self.sys_lseek(notif)
        } else if nr == FSTAT_NR {
            self.sys_fstat(notif)
        } else if nr == libc::SYS_dup {
            self.sys_dup(notif)
        } else if nr == libc::SYS_dup3 {
            self.sys_dup3(notif)
        } else if Some(nr) == DUP2_NR {
            self.sys_dup2(notif)
        } else if nr == libc::SYS_pipe2 {
            self.sys_pipe2(notif)
        } else if nr == libc::SYS_getcwd {
            self.sys_getcwd(notif)
        } else if nr == libc::SYS_chdir {
            self.sys_chdir(notif)
        } else if nr == libc::SYS_uname {
            self.sys_uname(notif)
        } else if nr == libc::SYS_sysinfo {
            self.sys_sysinfo(notif)
        } else if nr == NEWFSTATAT_NR {
            self.sys_fstatat(notif)
        } else if nr == libc::SYS_faccessat {
            self.sys_faccessat(notif)
        } else if nr == libc::SYS_getdents64 {
            self.sys_getdents64(notif)
        } else if nr == libc::SYS_mkdirat {
            self.sys_mkdirat(notif)
        } else if nr == libc::SYS_unlinkat {
            self.sys_unlinkat(notif)
        } else if nr == libc::SYS_readlinkat {
            self.sys_readlinkat(notif)
        } else if nr == libc::SYS_symlinkat {
            self.sys_symlinkat(notif)
        } else if nr == libc::SYS_fchdir {
            self.sys_fchdir(notif)
        } else if nr == libc::SYS_readv {
            self.sys_readv(notif)
        } else if nr == libc::SYS_socket {
            self.sys_socket(notif)
        } else if nr == libc::SYS_socketpair {
            self.sys_socketpair(notif)
        } else if nr == libc::SYS_connect {
            self.sys_connect(notif)
        } else if nr == libc::SYS_shutdown {
            self.sys_shutdown(notif)
        } else if nr == libc::SYS_sendto {
            self.sys_sendto(notif)
        } else if nr == libc::SYS_recvfrom {
            self.sys_recvfrom(notif)
        } else if nr == libc::SYS_sendmsg {
            self.sys_sendmsg(notif)
        } else if nr == libc::SYS_recvmsg {
            self.sys_recvmsg(notif)
        } else if nr == libc::SYS_fcntl {
            self.sys_fcntl(notif)
        } else if nr == libc::SYS_eventfd2 {
            self.sys_eventfd2(notif)
        } else if nr == libc::SYS_fchmodat {
            self.sys_fchmodat(notif)
        } else if nr == libc::SYS_utimensat {
            self.sys_utimensat(notif)
        } else if nr == libc::SYS_kill {
            self.sys_kill(notif)
        } else if nr == libc::SYS_tkill {
            self.sys_tkill(notif)
        } else if nr == libc::SYS_getpid {
            self.sys_getpid(notif)
        } else if nr == libc::SYS_getppid {
            self.sys_getppid(notif)
        } else if nr == libc::SYS_gettid {
            self.sys_gettid(notif)
        } else if nr == libc::SYS_exit {
            self.sys_exit(notif)
        } else if nr == libc::SYS_exit_group {
            self.sys_exit_group(notif)
        } else if nr == libc::SYS_execve {
            self.sys_execve(notif)
        } else if nr == libc::SYS_clone || CLONE3_NR == Some(nr) {
            self.sys_clone(notif)
        } else if is_blocked_eperm(nr) {
            // Inbound networking: outbound-only sandbox.
            Ok(notif::reply_error(notif.id, crate::error::Errno::PERM.code()))
        } else if is_blocked_enosys(nr) {
            // Escape/privilege/resource-control syscalls: report unavailable.
            Ok(notif::reply_error(notif.id, crate::error::Errno::NOSYS.code()))
        } else {
            // Default: let the kernel run it (process-local memory, signals,
            // time, futex, identity reads, etc.). Under the `fail-loudly`
            // feature, panic instead to surface unhandled syscalls in tests.
            #[cfg(feature = "fail-loudly")]
            panic!("unhandled syscall nr={nr}");
            #[cfg(not(feature = "fail-loudly"))]
            Ok(notif::reply_continue(notif.id))
        };
        result.unwrap_or_else(|e| notif::reply_error(notif.id, e.errno().code()))
    }

    fn sys_openat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let flags = notif.data.args[2] as i32;
        let mode = notif.data.args[3] as u32;

        let mut path_buf = [0u8; 256];
        let path = self.mem.read_string(caller, path_ptr, &mut path_buf)?;
        let path = std::str::from_utf8(path).map_err(|_| SysError(Errno::INVAL))?;
        if path.is_empty() {
            return Err(SysError(Errno::INVAL));
        }

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);

        // Resolve the base directory for relative paths.
        let base: String = if !path.starts_with('/') && dirfd != libc::AT_FDCWD {
            let dir_file = state
                .threads
                .fd_table_mut(caller)
                .and_then(|t| t.get(dirfd))
                .ok_or(SysError(Errno::BADF))?;
            dir_file
                .opened_path
                .clone()
                .ok_or(SysError(Errno::NOTDIR))?
        } else {
            state.threads.cwd(caller).unwrap_or("/").to_string()
        };

        let route = resolve_and_route(&base, path)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle {
                backend,
                normalized,
            } => (backend, normalized),
        };

        // Tombstone checks for cow/tmp.
        if matches!(btype, BackendType::Cow | BackendType::Tmp) {
            if state.tombstones.is_ancestor_tombstoned(&normalized) {
                return Err(SysError(Errno::NOENT));
            }
            if state.tombstones.is_tombstoned(&normalized) {
                if flags & libc::O_CREAT != 0 {
                    state.tombstones.remove(&normalized);
                } else {
                    return Err(SysError(Errno::NOENT));
                }
            }
        }

        let backend = match btype {
            BackendType::Passthrough => backend::passthrough_open(&normalized, flags, mode)?,
            BackendType::Cow => backend::cow_open(&state.overlay, &normalized, flags, mode)?,
            BackendType::Tmp => backend::tmp_open(&state.overlay, &normalized, flags, mode)?,
            BackendType::Proc => {
                state.threads.sync_new_threads(procinfo);
                let (content, is_dir) = self.build_proc(&mut state, caller, &normalized)?;
                backend::proc_open(content, is_dir)
            }
            // eventfd is created via eventfd2(), never path-opened.
            BackendType::Event => return Err(SysError(Errno::NOSYS)),
        };

        let backing_fd = backend.backing_fd();
        let cloexec = flags & libc::O_CLOEXEC != 0;
        let file = Arc::new(File::with_path(backend, Some(normalized), flags));
        let vfd = state
            .fd_table(caller, procinfo)
            .ok_or(SysError(Errno::SRCH))?
            .insert(file, cloexec);

        if let Some(bfd) = backing_fd {
            // Best-effort: with no real guest (tests) this is a no-op.
            let _ = self.notifier.addfd(notif.id, bfd, vfd, cloexec);
        }
        Ok(notif::reply_success(notif.id, vfd as i64))
    }

    /// Fetch the caller's virtual file at `fd`, if any (registers the caller).
    fn caller_fd(&self, caller: i32, fd: i32) -> Option<Arc<File>> {
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.fd_table(caller, procinfo)?.get(fd)
    }

    fn sys_close(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let Some(table) = state.fd_table(caller, procinfo) else {
            return Ok(notif::reply_continue(notif.id));
        };
        match table.get(fd) {
            Some(file) => {
                let has_backing = file.backing_fd().is_some();
                table.remove(fd);
                // Kernel-backed files: continue so the guest's addfd'd fd is
                // also closed; purely virtual files reply success.
                if has_backing {
                    Ok(notif::reply_continue(notif.id))
                } else {
                    Ok(notif::reply_success(notif.id, 0))
                }
            }
            // Not one of ours (stdio or an unknown fd): let the kernel close it.
            None => Ok(notif::reply_continue(notif.id)),
        }
    }

    fn sys_read(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let addr = notif.data.args[1];
        let count = notif.data.args[2] as usize;

        let Some(file) = self.caller_fd(caller, fd) else {
            // Unknown fd (e.g. inherited stdin): run it in the guest.
            return Ok(notif::reply_continue(notif.id));
        };

        let n = count.min(IO_CHUNK);
        let mut buf = vec![0u8; n];
        let got = file.read(&mut buf)?;
        self.mem.write_bytes(caller, addr, &buf[..got])?;
        Ok(notif::reply_success(notif.id, got as i64))
    }

    fn sys_write(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let addr = notif.data.args[1];
        let count = notif.data.args[2] as usize;

        let file = self.caller_fd(caller, fd);
        let n = count.min(IO_CHUNK);
        let mut buf = vec![0u8; n];
        self.mem.read_bytes(caller, addr, &mut buf)?;
        self.write_to_sink(notif, fd, file, &buf, n)
    }

    fn sys_writev(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let iov_addr = notif.data.args[1];
        let iovcnt = (notif.data.args[2] as usize).min(MAX_IOV);

        // Gather up to IO_CHUNK bytes across the iovecs into one buffer.
        let mut gathered: Vec<u8> = Vec::new();
        for i in 0..iovcnt {
            let base_addr = iov_addr + (i * 16) as u64;
            let base: u64 = self.mem.read_val(caller, base_addr)?;
            let len: u64 = self.mem.read_val(caller, base_addr + 8)?;
            let remaining = IO_CHUNK - gathered.len();
            if remaining == 0 {
                break;
            }
            let take = (len as usize).min(remaining);
            let mut chunk = vec![0u8; take];
            self.mem.read_bytes(caller, base, &mut chunk)?;
            gathered.extend_from_slice(&chunk);
        }

        let file = self.caller_fd(caller, fd);
        let n = gathered.len();
        self.write_to_sink(notif, fd, file, &gathered, n)
    }

    /// Common write path: to a virtual file if tracked, else capture fd 1/2 into
    /// the log buffers, else continue. `n` is the byte count to report.
    fn write_to_sink(
        &self,
        notif: &SeccompNotif,
        fd: i32,
        file: Option<Arc<File>>,
        buf: &[u8],
        n: usize,
    ) -> SysResult<SeccompNotifResp> {
        match file {
            Some(file) => {
                let written = file.write(buf)?;
                Ok(notif::reply_success(notif.id, written as i64))
            }
            None if fd == 1 => {
                self.stdout.write(buf);
                Ok(notif::reply_success(notif.id, n as i64))
            }
            None if fd == 2 => {
                self.stderr.write(buf);
                Ok(notif::reply_success(notif.id, n as i64))
            }
            None => Ok(notif::reply_continue(notif.id)),
        }
    }

    fn sys_lseek(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let offset = notif.data.args[1] as i64;
        let whence = notif.data.args[2] as u32;
        if fd == 0 || fd == 1 || fd == 2 {
            return Err(SysError(Errno::SPIPE));
        }
        let Some(file) = self.caller_fd(caller, fd) else {
            return Ok(notif::reply_continue(notif.id));
        };
        let pos = file.lseek(offset, whence)?;
        Ok(notif::reply_success(notif.id, pos))
    }

    fn sys_fstat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let addr = notif.data.args[1];
        if fd == 0 || fd == 1 || fd == 2 {
            return Ok(notif::reply_continue(notif.id));
        }
        let Some(file) = self.caller_fd(caller, fd) else {
            return Ok(notif::reply_continue(notif.id));
        };
        let st = file.stat()?;
        self.mem.write_val(caller, addr, &st)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_dup(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let oldfd = notif.data.args[0] as i32;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let table = state.fd_table(caller, procinfo).ok_or(SysError(Errno::SRCH))?;
        let Some(file) = table.get(oldfd) else {
            return Ok(notif::reply_continue(notif.id));
        };
        let backing = file.backing_fd();
        let newfd = table.dup(file);
        if let Some(bfd) = backing {
            let _ = self.notifier.addfd(notif.id, bfd, newfd, false);
        }
        Ok(notif::reply_success(notif.id, newfd as i64))
    }

    fn sys_dup3(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let flags = notif.data.args[2] as i32;
        if flags & !libc::O_CLOEXEC != 0 {
            return Err(SysError(Errno::INVAL));
        }
        let cloexec = flags & libc::O_CLOEXEC != 0;
        self.dup_onto(notif, cloexec, true)
    }

    fn sys_dup2(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        self.dup_onto(notif, false, false)
    }

    /// Shared dup2/dup3 core: alias `oldfd` onto `newfd` in the caller's table.
    fn dup_onto(
        &self,
        notif: &SeccompNotif,
        cloexec: bool,
        forbid_equal: bool,
    ) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let oldfd = notif.data.args[0] as i32;
        let newfd = notif.data.args[1] as i32;
        if forbid_equal && oldfd == newfd {
            return Err(SysError(Errno::INVAL));
        }
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let table = state.fd_table(caller, procinfo).ok_or(SysError(Errno::SRCH))?;
        let Some(file) = table.get(oldfd) else {
            // oldfd is a real (untracked) fd — e.g. a shell restoring saved
            // stdout. If newfd was one of ours, drop it so our table stops
            // shadowing the now-real fd; then let the kernel do the real dup.
            table.remove(newfd);
            return Ok(notif::reply_continue(notif.id));
        };
        if oldfd == newfd {
            return Ok(notif::reply_success(notif.id, newfd as i64));
        }
        let backing = file.backing_fd();
        table.remove(newfd);
        table.dup_at(file, newfd, cloexec);
        if let Some(bfd) = backing {
            let _ = self.notifier.addfd(notif.id, bfd, newfd, cloexec);
        }
        Ok(notif::reply_success(notif.id, newfd as i64))
    }

    fn sys_pipe2(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let pipefd_ptr = notif.data.args[0];
        let flags = notif.data.args[1] as i32;
        let cloexec = flags & libc::O_CLOEXEC != 0;

        // Create the real kernel pipe in the supervisor.
        let mut fds = [0i32; 2];
        // SAFETY: fds is a valid 2-int array; flags forwarded from the guest.
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), flags) };
        if rc < 0 {
            return Err(SysError(Errno::from_raw(errno_now()).unwrap_or(Errno::IO)));
        }

        let read_file = Arc::new(File::new(backend::Backend::Passthrough(fds[0])));
        let write_file = Arc::new(File::new(backend::Backend::Passthrough(fds[1])));

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let table = state.fd_table(caller, procinfo).ok_or(SysError(Errno::SRCH))?;
        let read_vfd = table.insert(read_file, cloexec);
        let write_vfd = table.insert(write_file, cloexec);
        drop(state);

        let _ = self.notifier.addfd(notif.id, fds[0], read_vfd, cloexec);
        let _ = self.notifier.addfd(notif.id, fds[1], write_vfd, cloexec);

        let vfds = [read_vfd, write_vfd];
        self.mem.write_val(caller, pipefd_ptr, &vfds)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_getcwd(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let buf_addr = notif.data.args[0];
        let buf_size = notif.data.args[1] as usize;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);
        let cwd = state.threads.cwd(caller).ok_or(SysError(Errno::SRCH))?.to_string();
        drop(state);
        if buf_size < cwd.len() + 1 {
            return Err(SysError(Errno::RANGE));
        }
        self.mem.write_string(caller, buf_addr, cwd.as_bytes())?;
        Ok(notif::reply_success(notif.id, (cwd.len() + 1) as i64))
    }

    fn sys_chdir(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let path_ptr = notif.data.args[0];
        let mut path_buf = [0u8; 256];
        let path = self.mem.read_string(caller, path_ptr, &mut path_buf)?;
        let path = std::str::from_utf8(path).map_err(|_| SysError(Errno::INVAL))?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);
        let base = state.threads.cwd(caller).unwrap_or("/").to_string();

        let route = resolve_and_route(&base, path)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        // Confirm the target exists and is a directory from the guest's view
        // (backend-aware, so tmp-overlay dirs are recognized).
        const S_IFMT: u32 = 0o170000;
        const S_IFDIR: u32 = 0o040000;
        let sx = Self::statx_routed(&state.overlay, btype, &normalized)?;
        if u32::from(sx.stx_mode) & S_IFMT != S_IFDIR {
            return Err(SysError(Errno::NOTDIR));
        }
        state.threads.set_cwd(caller, &normalized);
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_uname(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let addr = notif.data.args[0];
        // Read the kernel's real utsname, then overwrite the identifying fields.
        // SAFETY: buf is a live utsname; uname fills it.
        let mut buf: libc::utsname = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::uname(&mut buf) };
        if rc == 0 {
            set_charfield(&mut buf.nodename, b"bvisor");
            set_charfield(&mut buf.domainname, b"(none)");
        }
        self.mem.write_val(caller, addr, &buf)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_sysinfo(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let addr = notif.data.args[0];
        // Real sysinfo, then virtualize uptime (since sandbox start) and the
        // process count (sandboxed threads only).
        // SAFETY: buf is a live sysinfo struct; sysinfo fills it.
        let mut buf: libc::sysinfo = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::sysinfo(&mut buf) };
        if rc == 0 {
            buf.uptime = self.start.elapsed().as_secs() as _;
            let count = self.state.lock().unwrap().threads.count();
            buf.procs = count.min(u16::MAX as usize) as u16;
        }
        self.mem.write_val(caller, addr, &buf)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    /// Synthesize the directory listing for a `/proc` directory: `/proc` lists
    /// `self` + each visible pid; `/proc/<N>` and `/proc/self` list `status`.
    fn proc_dirents(state: &mut VirtState, dir_path: &str) -> crate::virt::fs::dirent::DirEntryMap {
        use crate::virt::fs::dirent::DirEntryMap;
        const DT_DIR: u8 = 4;
        const DT_REG: u8 = 8;
        const DT_LNK: u8 = 10;
        let mut map = DirEntryMap::new();
        map.insert(".", DT_DIR, false);
        map.insert("..", DT_DIR, false);
        if dir_path == "/proc" {
            map.insert("self", DT_LNK, false);
            for tgid in state.threads.ns_tgids() {
                map.insert(&tgid.to_string(), DT_DIR, false);
            }
        } else {
            map.insert("status", DT_REG, false);
        }
        map
    }

    /// Resolve a normalized `/proc` path into `(content, is_dir)` from the
    /// caller's namespaced view.
    fn build_proc(
        &self,
        state: &mut VirtState,
        caller: i32,
        normalized: &str,
    ) -> SysResult<(Vec<u8>, bool)> {
        use crate::error::{Errno, SysError};
        use crate::virt::fs::backend::procfile::{format_status, parse_proc_path, ProcTarget};
        let target = parse_proc_path(normalized)?;
        Ok(match target {
            ProcTarget::DirProc | ProcTarget::DirSelf => (Vec::new(), true),
            ProcTarget::DirPid(n) => {
                state.threads.abs_tgid_for_ns(n).ok_or(SysError(Errno::NOENT))?;
                (Vec::new(), true)
            }
            ProcTarget::SelfStatus => {
                let pid = state.threads.ns_pid(caller).ok_or(SysError(Errno::SRCH))?;
                let ppid = state.threads.ns_ppid(caller).ok_or(SysError(Errno::SRCH))?;
                (format_status(pid, ppid), false)
            }
            ProcTarget::PidStatus(n) => {
                let abs = state.threads.abs_tgid_for_ns(n).ok_or(SysError(Errno::NOENT))?;
                let ppid = state.threads.ns_ppid(abs).ok_or(SysError(Errno::NOENT))?;
                (format_status(n, ppid), false)
            }
        })
    }

    /// Read a path arg into a validated &str (max 256 bytes).
    fn read_path<'a>(&self, caller: i32, ptr: u64, buf: &'a mut [u8]) -> SysResult<&'a str> {
        use crate::error::{Errno, SysError};
        let bytes = self.mem.read_string(caller, ptr, buf)?;
        let s = std::str::from_utf8(bytes).map_err(|_| SysError(Errno::INVAL))?;
        if s.is_empty() {
            return Err(SysError(Errno::INVAL));
        }
        Ok(s)
    }

    /// statx-by-path for a routed path (cow/tmp/passthrough).
    fn statx_routed(
        overlay: &OverlayRoot,
        btype: BackendType,
        path: &str,
    ) -> SysResult<crate::virt::fs::backend::sys::Statx> {
        use crate::error::{Errno, SysError};
        match btype {
            BackendType::Passthrough => backend::passthrough_statx_path(path),
            BackendType::Cow => backend::cow_statx_path(overlay, path),
            BackendType::Tmp => backend::tmp_statx_path(overlay, path),
            BackendType::Proc | BackendType::Event => Err(SysError(Errno::NOSYS)),
        }
    }

    /// newfstatat: path-based stat (also handles AT_EMPTY_PATH → fstat).
    fn sys_fstatat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let stat_addr = notif.data.args[2];
        let flags = notif.data.args[3] as i32;

        // AT_EMPTY_PATH + empty path ≡ fstat(dirfd).
        let mut pbuf = [0u8; 256];
        let raw = self.mem.read_string(caller, path_ptr, &mut pbuf).unwrap_or(b"");
        if flags & libc::AT_EMPTY_PATH != 0 && raw.is_empty() {
            if dirfd == 0 || dirfd == 1 || dirfd == 2 {
                return Ok(notif::reply_continue(notif.id));
            }
            let Some(file) = self.caller_fd(caller, dirfd) else {
                return Ok(notif::reply_continue(notif.id));
            };
            let st = file.stat()?;
            self.mem.write_val(caller, stat_addr, &st)?;
            return Ok(notif::reply_success(notif.id, 0));
        }
        let path = std::str::from_utf8(raw).map_err(|_| SysError(Errno::INVAL))?;
        if path.is_empty() {
            return Err(SysError(Errno::INVAL));
        }

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::NOENT)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        if matches!(btype, BackendType::Cow | BackendType::Tmp)
            && (state.tombstones.is_tombstoned(&normalized)
                || state.tombstones.is_ancestor_tombstoned(&normalized))
        {
            return Err(SysError(Errno::NOENT));
        }
        let sx = if btype == BackendType::Proc {
            state.threads.sync_new_threads(procinfo);
            let (content, is_dir) = self.build_proc(&mut state, caller, &normalized)?;
            backend::proc_open(content, is_dir).statx()?
        } else {
            Self::statx_routed(&state.overlay, btype, &normalized)?
        };
        drop(state);
        let st = crate::virt::fs::file::statx_to_stat(&sx);
        self.mem.write_val(caller, stat_addr, &st)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    /// faccessat: existence/permission check over the overlay view.
    fn sys_faccessat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let mode = notif.data.args[2] as i32;

        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::ACCES)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        if matches!(btype, BackendType::Cow | BackendType::Tmp)
            && (state.tombstones.is_tombstoned(&normalized)
                || state.tombstones.is_ancestor_tombstoned(&normalized))
        {
            return Err(SysError(Errno::NOENT));
        }
        let exists = match btype {
            BackendType::Cow => state.overlay.guest_path_exists(&normalized),
            BackendType::Tmp => state.overlay.tmp_exists(&normalized),
            BackendType::Passthrough => OverlayRoot::path_exists_on_real_fs(&normalized),
            BackendType::Proc | BackendType::Event => true,
        };
        drop(state);
        // Only F_OK (existence) is meaningfully virtualized; forward real perms
        // for the lower layer when it exists.
        if mode == libc::F_OK {
            if exists {
                Ok(notif::reply_success(notif.id, 0))
            } else {
                Err(SysError(Errno::NOENT))
            }
        } else if exists {
            match btype {
                BackendType::Cow | BackendType::Passthrough => {
                    backend::real_access(&normalized, mode)?;
                    Ok(notif::reply_success(notif.id, 0))
                }
                _ => Ok(notif::reply_success(notif.id, 0)),
            }
        } else {
            Err(SysError(Errno::NOENT))
        }
    }

    /// getdents64: merged overlay+lower directory listing with tombstone filter.
    fn sys_getdents64(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let buf_addr = notif.data.args[1];
        let count = (notif.data.args[2] as usize).min(4096);

        if fd == 0 || fd == 1 || fd == 2 {
            return Ok(notif::reply_continue(notif.id));
        }
        let Some(file) = self.caller_fd(caller, fd) else {
            return Ok(notif::reply_continue(notif.id));
        };
        let Some(dir_path) = file.opened_path.clone() else {
            return Err(SysError(Errno::NOTDIR));
        };

        // Determine the backend from the path (routing is cheap and stable).
        let route = crate::virt::path::route(&dir_path)?;
        let btype = match route {
            crate::virt::path::RouteResult::Block => return Err(SysError(Errno::EXIST)),
            crate::virt::path::RouteResult::Handle(b) => b,
        };

        let mut out = vec![0u8; count];
        let n = {
            let mut state = self.state.lock().unwrap();
            let map = match btype {
                BackendType::Cow => backend::cow_merged_dirents(&state.overlay, &dir_path),
                BackendType::Tmp => backend::tmp_merged_dirents(&state.overlay, &dir_path)?,
                BackendType::Proc => Self::proc_dirents(&mut state, &dir_path),
                // Passthrough dirs (e.g. under /dev via a fd) fall back to raw.
                _ => return Ok(notif::reply_continue(notif.id)),
            };
            let mut offset = file.dirents_offset();
            let written = crate::virt::fs::dirent::serialize_entries(
                &map,
                &mut out,
                &dir_path,
                &mut offset,
                &state.tombstones,
            );
            file.set_dirents_offset(offset);
            written
        };
        self.mem.write_bytes(caller, buf_addr, &out[..n])?;
        Ok(notif::reply_success(notif.id, n as i64))
    }

    /// mkdirat: create a directory in the cow/tmp overlay.
    fn sys_mkdirat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let mode = notif.data.args[2] as u32;

        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        match btype {
            BackendType::Passthrough | BackendType::Proc | BackendType::Event => {
                Err(SysError(Errno::PERM))
            }
            BackendType::Cow => {
                if state.tombstones.is_tombstoned(&normalized) {
                    state.tombstones.remove(&normalized);
                } else if state.overlay.guest_path_exists(&normalized) {
                    return Err(SysError(Errno::EXIST));
                }
                backend::cow_mkdir(&state.overlay, &normalized, mode)?;
                Ok(notif::reply_success(notif.id, 0))
            }
            BackendType::Tmp => {
                if state.tombstones.is_tombstoned(&normalized) {
                    state.tombstones.remove(&normalized);
                } else if state.overlay.tmp_exists(&normalized) {
                    return Err(SysError(Errno::EXIST));
                }
                backend::tmp_mkdir(&state.overlay, &normalized, mode)?;
                Ok(notif::reply_success(notif.id, 0))
            }
        }
    }

    /// unlinkat: remove a file (or dir with AT_REMOVEDIR) via tombstone + overlay.
    fn sys_unlinkat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let flags = notif.data.args[2] as i32;
        let remove_dir = flags & libc::AT_REMOVEDIR != 0;
        if flags & !libc::AT_REMOVEDIR != 0 {
            return Err(SysError(Errno::INVAL));
        }

        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        match btype {
            BackendType::Passthrough | BackendType::Proc | BackendType::Event => {
                Err(SysError(Errno::PERM))
            }
            BackendType::Cow => {
                if state.tombstones.is_ancestor_tombstoned(&normalized)
                    || state.tombstones.is_tombstoned(&normalized)
                    || !state.overlay.guest_path_exists(&normalized)
                {
                    return Err(SysError(Errno::NOENT));
                }
                let is_dir = state.overlay.is_guest_dir(&normalized);
                if remove_dir && !is_dir {
                    return Err(SysError(Errno::NOTDIR));
                }
                if !remove_dir && is_dir {
                    return Err(SysError(Errno::ISDIR));
                }
                if remove_dir {
                    state.tombstones.remove_children(&normalized);
                }
                state.tombstones.add(&normalized);
                backend::cow_remove(&state.overlay, &normalized, remove_dir);
                Ok(notif::reply_success(notif.id, 0))
            }
            BackendType::Tmp => {
                if !state.overlay.tmp_exists(&normalized) {
                    return Err(SysError(Errno::NOENT));
                }
                let is_dir = state.overlay.is_tmp_dir(&normalized);
                if remove_dir && !is_dir {
                    return Err(SysError(Errno::NOTDIR));
                }
                if !remove_dir && is_dir {
                    return Err(SysError(Errno::ISDIR));
                }
                backend::tmp_remove(&state.overlay, &normalized, remove_dir)?;
                Ok(notif::reply_success(notif.id, 0))
            }
        }
    }

    /// readlinkat: resolve a symlink from the overlay view.
    fn sys_readlinkat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let buf_addr = notif.data.args[2];
        let buf_size = (notif.data.args[3] as usize).min(512);

        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        let (btype, normalized) = {
            let mut state = self.state.lock().unwrap();
            let procinfo = &*self.procinfo;
            match state.resolve_path(caller, path, dirfd, procinfo)? {
                ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
                ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
            }
        };
        let mut link = vec![0u8; buf_size.max(1)];
        let n = {
            let state = self.state.lock().unwrap();
            match btype {
                BackendType::Cow => backend::cow_readlink(&state.overlay, &normalized, &mut link)?,
                BackendType::Tmp => backend::tmp_readlink(&state.overlay, &normalized, &mut link)?,
                // /proc symlinks (e.g. self/exe) unimplemented; passthrough denied.
                _ => return Err(SysError(Errno::INVAL)),
            }
        };
        self.mem.write_bytes(caller, buf_addr, &link[..n])?;
        Ok(notif::reply_success(notif.id, n as i64))
    }

    /// symlinkat: create a symlink in the cow/tmp overlay.
    fn sys_symlinkat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let target_ptr = notif.data.args[0];
        let newdirfd = notif.data.args[1] as i64 as i32;
        let linkpath_ptr = notif.data.args[2];

        let mut tbuf = [0u8; 256];
        let target = self.read_path(caller, target_ptr, &mut tbuf)?.to_string();
        let mut lbuf = [0u8; 256];
        let linkpath = self.read_path(caller, linkpath_ptr, &mut lbuf)?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, linkpath, newdirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        match btype {
            BackendType::Passthrough | BackendType::Proc | BackendType::Event => {
                Err(SysError(Errno::PERM))
            }
            BackendType::Cow => {
                if state.tombstones.is_tombstoned(&normalized) {
                    state.tombstones.remove(&normalized);
                } else if state.overlay.guest_path_exists(&normalized) {
                    return Err(SysError(Errno::EXIST));
                }
                backend::cow_symlink(&state.overlay, &target, &normalized)?;
                Ok(notif::reply_success(notif.id, 0))
            }
            BackendType::Tmp => {
                if state.tombstones.is_tombstoned(&normalized) {
                    state.tombstones.remove(&normalized);
                } else if state.overlay.tmp_exists(&normalized) {
                    return Err(SysError(Errno::EXIST));
                }
                backend::tmp_symlink(&state.overlay, &target, &normalized)?;
                Ok(notif::reply_success(notif.id, 0))
            }
        }
    }

    /// fchdir: set cwd from a directory fd's opened path.
    fn sys_fchdir(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;
        let path = file.opened_path.clone().ok_or(SysError(Errno::NOTDIR))?;
        // Re-validate through routing (may now be blocked).
        match crate::virt::path::route(&path)? {
            crate::virt::path::RouteResult::Block => return Err(SysError(Errno::PERM)),
            crate::virt::path::RouteResult::Handle(_) => {}
        }
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);
        state.threads.set_cwd(caller, &path);
        Ok(notif::reply_success(notif.id, 0))
    }

    /// readv: scatter a single backend read across the guest's iovecs.
    fn sys_readv(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let iov_addr = notif.data.args[1];
        let iovcnt = (notif.data.args[2] as usize).min(MAX_IOV);

        let Some(file) = self.caller_fd(caller, fd) else {
            return Ok(notif::reply_continue(notif.id));
        };
        // One capped backend read, then scatter into the iovecs.
        let mut buf = vec![0u8; IO_CHUNK];
        let got = file.read(&mut buf)?;
        let mut remaining = &buf[..got];
        let mut total = 0usize;
        for i in 0..iovcnt {
            if remaining.is_empty() {
                break;
            }
            let base_addr = iov_addr + (i * 16) as u64;
            let base: u64 = self.mem.read_val(caller, base_addr)?;
            let len: u64 = self.mem.read_val(caller, base_addr + 8)?;
            let take = (len as usize).min(remaining.len());
            self.mem.write_bytes(caller, base, &remaining[..take])?;
            remaining = &remaining[take..];
            total += take;
        }
        Ok(notif::reply_success(notif.id, total as i64))
    }

    /// Insert a freshly created kernel fd as a passthrough File into the caller's
    /// table and addfd it into the guest. Returns the virtual fd.
    fn register_kernel_fd(
        &self,
        notif: &SeccompNotif,
        caller: i32,
        kernel_fd: RawFd,
        cloexec: bool,
    ) -> SysResult<i32> {
        use crate::error::{Errno, SysError};
        let file = Arc::new(File::new(backend::Backend::Passthrough(kernel_fd)));
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let vfd = state
            .fd_table(caller, procinfo)
            .ok_or(SysError(Errno::SRCH))?
            .insert(file, cloexec);
        drop(state);
        let _ = self.notifier.addfd(notif.id, kernel_fd, vfd, cloexec);
        Ok(vfd)
    }

    fn sys_socket(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let domain = notif.data.args[0] as i32;
        let sock_type = notif.data.args[1] as i32;
        let protocol = notif.data.args[2] as i32;
        let cloexec = sock_type & libc::SOCK_CLOEXEC != 0;
        // SAFETY: plain socket() with guest-provided args.
        let fd = unsafe { libc::socket(domain, sock_type, protocol) };
        if fd < 0 {
            return Err(crate::error::SysError(
                crate::error::Errno::from_raw(errno_now()).unwrap_or(crate::error::Errno::IO),
            ));
        }
        let vfd = self.register_kernel_fd(notif, caller, fd, cloexec)?;
        Ok(notif::reply_success(notif.id, vfd as i64))
    }

    fn sys_socketpair(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let domain = notif.data.args[0] as i32;
        let sock_type = notif.data.args[1] as i32;
        let protocol = notif.data.args[2] as i32;
        let sv_ptr = notif.data.args[3];
        let cloexec = sock_type & libc::SOCK_CLOEXEC != 0;
        let mut fds = [0i32; 2];
        // SAFETY: fds is a valid 2-int array.
        let rc = unsafe { libc::socketpair(domain, sock_type, protocol, fds.as_mut_ptr()) };
        if rc < 0 {
            return Err(crate::error::SysError(
                crate::error::Errno::from_raw(errno_now()).unwrap_or(crate::error::Errno::IO),
            ));
        }
        let v0 = self.register_kernel_fd(notif, caller, fds[0], cloexec)?;
        let v1 = self.register_kernel_fd(notif, caller, fds[1], cloexec)?;
        self.mem.write_val(caller, sv_ptr, &[v0, v1])?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_connect(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let addr_ptr = notif.data.args[1];
        let addrlen = notif.data.args[2] as usize;
        if addrlen == 0 || addrlen > 128 {
            return Err(SysError(Errno::INVAL));
        }
        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;
        let mut addr = vec![0u8; addrlen];
        self.mem.read_bytes(caller, addr_ptr, &mut addr)?;
        file.connect(&addr)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_shutdown(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let how = notif.data.args[1] as i32;
        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;
        file.shutdown(how)?;
        Ok(notif::reply_success(notif.id, 0))
    }

    fn sys_sendto(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let buf_addr = notif.data.args[1];
        let count = (notif.data.args[2] as usize).min(IO_CHUNK);
        let flags = notif.data.args[3] as i32;
        let dest_ptr = notif.data.args[4];
        let addrlen = notif.data.args[5] as usize;

        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;
        let mut data = vec![0u8; count];
        self.mem.read_bytes(caller, buf_addr, &mut data)?;
        let dest = if dest_ptr != 0 && (1..=128).contains(&addrlen) {
            let mut a = vec![0u8; addrlen];
            self.mem.read_bytes(caller, dest_ptr, &mut a)?;
            Some(a)
        } else {
            None
        };
        let n = file.send_to(&data, flags, dest.as_deref())?;
        Ok(notif::reply_success(notif.id, n as i64))
    }

    fn sys_recvfrom(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let buf_addr = notif.data.args[1];
        let count = (notif.data.args[2] as usize).min(IO_CHUNK);
        let flags = notif.data.args[3] as i32;
        let src_ptr = notif.data.args[4];
        let addrlen_ptr = notif.data.args[5];

        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;
        let mut buf = vec![0u8; count];
        let want_addr = src_ptr != 0 && addrlen_ptr != 0;
        let mut src = if want_addr { vec![0u8; 128] } else { Vec::new() };
        let (n, src_len) = file.recv_from(
            &mut buf,
            flags,
            if want_addr { Some(&mut src) } else { None },
        )?;
        self.mem.write_bytes(caller, buf_addr, &buf[..n])?;
        if want_addr {
            // Honor the guest's provided addrlen when copying back the address.
            let guest_len: u32 = self.mem.read_val(caller, addrlen_ptr)?;
            let copy = (src_len as usize).min(guest_len as usize).min(src.len());
            if copy > 0 {
                self.mem.write_bytes(caller, src_ptr, &src[..copy])?;
            }
            self.mem.write_val(caller, addrlen_ptr, &src_len)?;
        }
        Ok(notif::reply_success(notif.id, n as i64))
    }

    /// sendmsg: gather the msghdr's iovecs (with optional dest addr) and send.
    /// Control messages (SCM_RIGHTS fd passing) are ignored.
    fn sys_sendmsg(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let msg_addr = notif.data.args[1];
        let flags = notif.data.args[2] as i32;

        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;

        // struct msghdr layout (64-bit): name@0, namelen@8, iov@16, iovlen@24.
        let name_ptr: u64 = self.mem.read_val(caller, msg_addr)?;
        let name_len: u32 = self.mem.read_val(caller, msg_addr + 8)?;
        let iov_ptr: u64 = self.mem.read_val(caller, msg_addr + 16)?;
        let iovlen: u64 = self.mem.read_val(caller, msg_addr + 24)?;

        let gathered = self.gather_iovecs(caller, iov_ptr, iovlen as usize)?;
        let dest = if name_ptr != 0 && (1..=128).contains(&name_len) {
            let mut a = vec![0u8; name_len as usize];
            self.mem.read_bytes(caller, name_ptr, &mut a)?;
            Some(a)
        } else {
            None
        };
        let n = file.send_to(&gathered, flags, dest.as_deref())?;
        Ok(notif::reply_success(notif.id, n as i64))
    }

    /// recvmsg: receive into the msghdr's iovecs, write back the source address,
    /// and report no control data (controllen=0, flags=0).
    fn sys_recvmsg(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let msg_addr = notif.data.args[1];
        let flags = notif.data.args[2] as i32;

        let file = self.caller_fd(caller, fd).ok_or(SysError(Errno::BADF))?;

        let name_ptr: u64 = self.mem.read_val(caller, msg_addr)?;
        let name_len: u32 = self.mem.read_val(caller, msg_addr + 8)?;
        let iov_ptr: u64 = self.mem.read_val(caller, msg_addr + 16)?;
        let iovlen: u64 = self.mem.read_val(caller, msg_addr + 24)?;

        let want_addr = name_ptr != 0 && name_len > 0;
        let mut src = if want_addr { vec![0u8; 128] } else { Vec::new() };
        let mut buf = vec![0u8; IO_CHUNK];
        let (n, src_len) = file.recv_from(
            &mut buf,
            flags,
            if want_addr { Some(&mut src) } else { None },
        )?;

        // Scatter received bytes across the guest's iovecs.
        self.scatter_iovecs(caller, iov_ptr, iovlen as usize, &buf[..n])?;

        if want_addr {
            let copy = (src_len as usize).min(name_len as usize).min(src.len());
            if copy > 0 {
                self.mem.write_bytes(caller, name_ptr, &src[..copy])?;
            }
            // msg_namelen@8, msg_controllen@40, msg_flags@48.
            self.mem.write_val(caller, msg_addr + 8, &src_len)?;
        }
        self.mem.write_val(caller, msg_addr + 40, &0u64)?; // controllen = 0
        self.mem.write_val(caller, msg_addr + 48, &0i32)?; // flags = 0
        Ok(notif::reply_success(notif.id, n as i64))
    }

    /// Gather up to IO_CHUNK bytes across ≤MAX_IOV iovecs into one buffer.
    fn gather_iovecs(&self, caller: i32, iov_ptr: u64, iovcnt: usize) -> SysResult<Vec<u8>> {
        let mut out = Vec::new();
        for i in 0..iovcnt.min(MAX_IOV) {
            let base_addr = iov_ptr + (i * 16) as u64;
            let base: u64 = self.mem.read_val(caller, base_addr)?;
            let len: u64 = self.mem.read_val(caller, base_addr + 8)?;
            let remaining = IO_CHUNK - out.len();
            if remaining == 0 {
                break;
            }
            let take = (len as usize).min(remaining);
            let mut chunk = vec![0u8; take];
            self.mem.read_bytes(caller, base, &mut chunk)?;
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// Scatter `data` across ≤MAX_IOV guest iovecs.
    fn scatter_iovecs(
        &self,
        caller: i32,
        iov_ptr: u64,
        iovcnt: usize,
        data: &[u8],
    ) -> SysResult<()> {
        let mut rest = data;
        for i in 0..iovcnt.min(MAX_IOV) {
            if rest.is_empty() {
                break;
            }
            let base_addr = iov_ptr + (i * 16) as u64;
            let base: u64 = self.mem.read_val(caller, base_addr)?;
            let len: u64 = self.mem.read_val(caller, base_addr + 8)?;
            let take = (len as usize).min(rest.len());
            self.mem.write_bytes(caller, base, &rest[..take])?;
            rest = &rest[take..];
        }
        Ok(())
    }

    /// fcntl: cloexec management, flag get/set, and F_DUPFD for our fds. Locking
    /// and ownership commands are stubbed to 0 (matching the Zig behavior).
    fn sys_fcntl(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        const F_DUPFD_CLOEXEC: i32 = 1030;
        let caller = notif.pid as i32;
        let fd = notif.data.args[0] as i32;
        let cmd = notif.data.args[1] as i32;
        let arg = notif.data.args[2];

        if fd == 0 || fd == 1 || fd == 2 {
            return Ok(notif::reply_continue(notif.id));
        }

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let table = state.fd_table(caller, procinfo).ok_or(SysError(Errno::SRCH))?;
        // Not one of our fds: let the kernel handle it.
        let Some(file) = table.get(fd) else {
            return Ok(notif::reply_continue(notif.id));
        };

        match cmd {
            libc::F_DUPFD | F_DUPFD_CLOEXEC => {
                let cloexec = cmd == F_DUPFD_CLOEXEC;
                let backing = file.backing_fd();
                let newfd = table.dup(file);
                table.set_cloexec(newfd, cloexec);
                if let Some(bfd) = backing {
                    let _ = self.notifier.addfd(notif.id, bfd, newfd, cloexec);
                }
                Ok(notif::reply_success(notif.id, newfd as i64))
            }
            libc::F_GETFD => {
                let v = if table.get_cloexec(fd) { libc::FD_CLOEXEC } else { 0 };
                Ok(notif::reply_success(notif.id, v as i64))
            }
            libc::F_SETFD => {
                table.set_cloexec(fd, arg as i32 & libc::FD_CLOEXEC != 0);
                Ok(notif::reply_success(notif.id, 0))
            }
            libc::F_GETFL => Ok(notif::reply_success(notif.id, file.open_flags as i64)),
            libc::F_SETFL => {
                // Accept but only track; the mutable flag bits don't affect our
                // overlay semantics for now.
                Ok(notif::reply_success(notif.id, 0))
            }
            // Advisory locking / ownership / signal commands: stubbed to success.
            // F_SETSIG=10, F_GETSIG=11 (not exposed by libc on musl).
            libc::F_GETLK | libc::F_SETLK | libc::F_SETLKW | libc::F_GETOWN
            | libc::F_SETOWN | 10 | 11 => Ok(notif::reply_success(notif.id, 0)),
            _ => Err(SysError(Errno::INVAL)),
        }
    }

    /// eventfd2: create a kernel eventfd and track it as a passthrough fd.
    fn sys_eventfd2(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let count = notif.data.args[0] as libc::c_uint;
        let flags = notif.data.args[1] as i32;
        let cloexec = flags & libc::EFD_CLOEXEC != 0;
        // SAFETY: plain eventfd2 with guest-provided count/flags.
        let fd = unsafe { libc::eventfd(count, flags) };
        if fd < 0 {
            return Err(crate::error::SysError(
                crate::error::Errno::from_raw(errno_now()).unwrap_or(crate::error::Errno::IO),
            ));
        }
        let vfd = self.register_kernel_fd(notif, caller, fd, cloexec)?;
        Ok(notif::reply_success(notif.id, vfd as i64))
    }

    /// fchmodat: chmod a path in the overlay (AT_EMPTY_PATH handled minimally).
    fn sys_fchmodat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let mode = notif.data.args[2] as u32;
        let flags = notif.data.args[3] as i32;
        // Linux rejects AT_SYMLINK_NOFOLLOW for chmod.
        if flags & libc::AT_SYMLINK_NOFOLLOW != 0 {
            return Err(SysError(Errno::OPNOTSUPP));
        }
        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        if matches!(btype, BackendType::Cow | BackendType::Tmp)
            && state.tombstones.is_tombstoned(&normalized)
        {
            return Err(SysError(Errno::NOENT));
        }
        match btype {
            BackendType::Cow => backend::cow_fchmodat(&state.overlay, &normalized, mode)?,
            BackendType::Tmp => backend::tmp_fchmodat(&state.overlay, &normalized, mode)?,
            // passthrough/proc/event have no meaningful mode change.
            _ => {}
        }
        Ok(notif::reply_success(notif.id, 0))
    }

    /// utimensat: set timestamps on a path in the overlay.
    fn sys_utimensat(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let dirfd = notif.data.args[0] as i64 as i32;
        let path_ptr = notif.data.args[1];
        let times_ptr = notif.data.args[2];

        // AT_EMPTY_PATH / null path (fd-based) — accept as a no-op for now.
        if path_ptr == 0 {
            return Ok(notif::reply_success(notif.id, 0));
        }
        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;

        // Read the [timespec;2] (32 bytes) if provided.
        let times: Option<Vec<u8>> = if times_ptr != 0 {
            let mut t = vec![0u8; 32];
            self.mem.read_bytes(caller, times_ptr, &mut t)?;
            Some(t)
        } else {
            None
        };

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, dirfd, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };
        match btype {
            BackendType::Cow => {
                backend::cow_utimensat(&state.overlay, &normalized, times.as_deref())?
            }
            BackendType::Tmp => {
                backend::tmp_utimensat(&state.overlay, &normalized, times.as_deref())?
            }
            _ => {}
        }
        Ok(notif::reply_success(notif.id, 0))
    }

    /// kill: deliver a signal to a namespaced thread-group (translated to the
    /// absolute tgid). Process groups / -1 are not supported.
    fn sys_kill(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let ns_tgid = notif.data.args[0] as i64 as i32;
        let sig = notif.data.args[1] as i32;
        if ns_tgid <= 0 {
            return Err(SysError(Errno::INVAL));
        }
        let abs = {
            let mut state = self.state.lock().unwrap();
            let procinfo = &*self.procinfo;
            state.threads.get_or_sync(caller, procinfo);
            state.threads.abs_tgid_for_ns(ns_tgid).ok_or(SysError(Errno::SRCH))?
        };
        // SAFETY: real kill of a known guest tgid.
        let rc = unsafe { libc::kill(abs, sig) };
        if rc < 0 {
            return Err(SysError(Errno::from_raw(errno_now()).unwrap_or(Errno::PERM)));
        }
        Ok(notif::reply_success(notif.id, 0))
    }

    /// tkill: deliver a signal to a namespaced thread id (translated to abs tid).
    fn sys_tkill(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let ns_tid = notif.data.args[0] as i64 as i32;
        let sig = notif.data.args[1] as i32;
        if ns_tid <= 0 {
            return Err(SysError(Errno::INVAL));
        }
        let abs = {
            let mut state = self.state.lock().unwrap();
            let procinfo = &*self.procinfo;
            state.threads.get_or_sync(caller, procinfo);
            state.threads.abs_tid_for_ns(ns_tid).ok_or(SysError(Errno::SRCH))?
        };
        // SAFETY: tgkill(-1, tid, sig) delivers to a specific thread; use kill on
        // the tid which for a thread targets that task.
        let rc = unsafe { libc::syscall(libc::SYS_tkill, abs as libc::c_long, sig as libc::c_long) };
        if rc < 0 {
            return Err(SysError(Errno::from_raw(errno_now()).unwrap_or(Errno::PERM)));
        }
        Ok(notif::reply_success(notif.id, 0))
    }

    /// getpid → the caller's namespaced thread-group id.
    fn sys_getpid(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let mut state = self.state.lock().unwrap();
        let pid = {
            let procinfo = &*self.procinfo;
            state.threads.get_or_sync(caller, procinfo);
            state.threads.ns_pid(caller)
        };
        match pid {
            Some(p) => Ok(notif::reply_success(notif.id, p as i64)),
            None => Err(SysError(Errno::SRCH)),
        }
    }

    /// getppid → the caller's namespaced parent pid (0 for init).
    fn sys_getppid(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);
        match state.threads.ns_ppid(caller) {
            Some(p) => Ok(notif::reply_success(notif.id, p as i64)),
            None => Err(SysError(Errno::SRCH)),
        }
    }

    /// gettid → the caller's namespaced thread id.
    fn sys_gettid(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        // In a single PID namespace the namespaced tid == the kernel tid.
        match state.threads.get_or_sync(caller, procinfo).map(|t| t.tid) {
            Some(tid) => Ok(notif::reply_success(notif.id, tid as i64)),
            None => Err(SysError(Errno::SRCH)),
        }
    }

    /// execve → for overlay-backed binaries, redirect to the overlay path via a
    /// short `/.b/XXX` symlink written over the guest's original path buffer,
    /// then continue so the kernel performs the exec. Real/passthrough binaries
    /// just continue unchanged.
    fn sys_execve(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        use crate::error::{Errno, SysError};
        let caller = notif.pid as i32;
        let path_ptr = notif.data.args[0];

        let mut pbuf = [0u8; 256];
        let path = self.read_path(caller, path_ptr, &mut pbuf)?;
        let original_len = path.len();

        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        let route = state.resolve_path(caller, path, libc::AT_FDCWD, procinfo)?;
        let (btype, normalized) = match route {
            ResolvedRoute::Block => return Err(SysError(Errno::PERM)),
            ResolvedRoute::Handle { backend, normalized } => (backend, normalized),
        };

        // Determine the real kernel path to exec, if it needs redirection.
        let overlay_path: Option<String> = match btype {
            BackendType::Proc | BackendType::Event => return Err(SysError(Errno::INVAL)),
            BackendType::Passthrough => None, // guest path maps directly
            BackendType::Cow => {
                if state.overlay.cow_exists(&normalized) {
                    Some(state.overlay.resolve_cow(&normalized))
                } else {
                    None // no cow copy; original kernel path is correct
                }
            }
            BackendType::Tmp => Some(state.overlay.resolve_tmp(&normalized)?),
        };

        if let Some(target) = overlay_path {
            let short = state.symlinks.create(&target, original_len)?;
            drop(state);
            self.mem.write_string(caller, path_ptr, short.as_bytes())?;
        }
        Ok(notif::reply_continue(notif.id))
    }

    /// clone/clone3 → snapshot the caller's fd table + cwd at fork time so a
    /// lazily discovered child inherits fork-time state, then continue.
    fn sys_clone(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        let mut state = self.state.lock().unwrap();
        let procinfo = &*self.procinfo;
        state.threads.get_or_sync(caller, procinfo);
        state.threads.snapshot_fork(caller);
        Ok(notif::reply_continue(notif.id))
    }

    /// exit → prune the caller's virtual thread, then let the kernel exit it.
    fn sys_exit(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        self.state.lock().unwrap().threads.handle_thread_exit(caller);
        Ok(notif::reply_continue(notif.id))
    }

    /// exit_group → prune the caller's whole thread group, then let the kernel
    /// perform the real exit_group.
    fn sys_exit_group(&self, notif: &SeccompNotif) -> SysResult<SeccompNotifResp> {
        let caller = notif.pid as i32;
        self.state.lock().unwrap().threads.handle_group_exit(caller);
        Ok(notif::reply_continue(notif.id))
    }
}

// fstat exists on both arches we target.
const FSTAT_NR: i64 = libc::SYS_fstat;

// dup2 is a distinct syscall on x86_64 but does not exist on aarch64 (which
// only has dup3). None means "no dup2 syscall on this arch".
#[cfg(target_arch = "x86_64")]
const DUP2_NR: Option<i64> = Some(libc::SYS_dup2);
#[cfg(not(target_arch = "x86_64"))]
const DUP2_NR: Option<i64> = None;

// clone3 exists on modern kernels for both arches.
const CLONE3_NR: Option<i64> = Some(libc::SYS_clone3);

// The path-based stat syscall (fstatat64/newfstatat).
const NEWFSTATAT_NR: i64 = libc::SYS_newfstatat;

fn errno_now() -> i32 {
    nix::errno::Errno::last() as i32
}

/// Inbound-networking syscalls: blocked with EPERM (outbound-only sandbox).
fn is_blocked_eperm(nr: i64) -> bool {
    // accept exists only on x86_64; aarch64 uses accept4.
    #[cfg(target_arch = "x86_64")]
    {
        if nr == libc::SYS_accept {
            return true;
        }
    }
    nr == libc::SYS_bind || nr == libc::SYS_listen || nr == libc::SYS_accept4
}

/// Escape / privilege / resource-control syscalls: blocked with ENOSYS.
fn is_blocked_enosys(nr: i64) -> bool {
    const BLOCKED: &[i64] = &[
        libc::SYS_ptrace,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_chroot,
        libc::SYS_pivot_root,
        libc::SYS_reboot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_seccomp,
        libc::SYS_bpf,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_setrlimit,
        libc::SYS_prlimit64,
        libc::SYS_personality,
    ];
    BLOCKED.contains(&nr)
}

/// Write `val` (NUL-terminated) into a fixed C char array field, zero-padding.
fn set_charfield(field: &mut [libc::c_char], val: &[u8]) {
    let n = val.len().min(field.len().saturating_sub(1));
    for (dst, &src) in field.iter_mut().zip(&val[..n]) {
        *dst = src as libc::c_char;
    }
    for dst in &mut field[n..] {
        *dst = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::LocalMem;
    use crate::seccomp::notif::{SeccompData, USER_NOTIF_FLAG_CONTINUE};
    use crate::seccomp::notifier::NoopNotifier;

    fn test_supervisor() -> Arc<Supervisor> {
        test_supervisor_with_proc(crate::procinfo::MockProcInfo::new())
    }

    fn test_supervisor_with_proc(proc: crate::procinfo::MockProcInfo) -> Arc<Supervisor> {
        let uid = crate::generate_uid();
        let overlay = OverlayRoot::new(uid).unwrap();
        Arc::new(Supervisor::new(
            -1,
            100,
            Box::new(LocalMem),
            Box::new(NoopNotifier),
            Box::new(proc),
            Arc::new(LogBuffer::new()),
            Arc::new(LogBuffer::new()),
            overlay,
        ))
    }

    fn notif(nr: i64, args: [u64; 6]) -> SeccompNotif {
        SeccompNotif {
            id: 1,
            pid: 100,
            flags: 0,
            data: SeccompData {
                nr: nr as i32,
                arch: 0,
                instruction_pointer: 0,
                args,
            },
        }
    }

    fn addr_of(b: &[u8]) -> u64 {
        b.as_ptr() as u64
    }

    fn addr_of_mut(b: &mut [u8]) -> u64 {
        b.as_mut_ptr() as u64
    }

    fn open(sup: &Supervisor, path: &std::ffi::CStr, flags: i32, mode: u32) -> SeccompNotifResp {
        // read_string reads a full 256-byte window; back the path with a 256-byte
        // buffer so LocalMem never reads past a short static CStr.
        let mut pbuf = [0u8; 256];
        let bytes = path.to_bytes_with_nul();
        pbuf[..bytes.len()].copy_from_slice(bytes);
        sup.handle(&notif(
            libc::SYS_openat,
            [
                libc::AT_FDCWD as u64,
                pbuf.as_ptr() as u64,
                flags as u64,
                mode as u64,
                0,
                0,
            ],
        ))
    }

    #[test]
    fn openat_dev_null_returns_vfd() {
        let sup = test_supervisor();
        let resp = open(&sup, c"/dev/null", libc::O_RDWR, 0);
        assert!(resp.val >= 3);
        assert_eq!(resp.error, 0);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn openat_blocked_path_is_eperm() {
        let sup = test_supervisor();
        let resp = open(&sup, c"/sys/class/net", libc::O_RDONLY, 0);
        assert_eq!(resp.error, -(crate::error::Errno::PERM.code()));
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn openat_empty_path_is_einval() {
        let sup = test_supervisor();
        let resp = open(&sup, c"", libc::O_RDONLY, 0);
        assert_eq!(resp.error, -(crate::error::Errno::INVAL.code()));
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn tmp_write_then_read_roundtrip() {
        let sup = test_supervisor();

        // open /tmp/t.txt for write+create
        let w = open(
            &sup,
            c"/tmp/t.txt",
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
        );
        assert!(w.val >= 3, "open for write failed: {}", w.error);
        let wfd = w.val;

        // write "hello e2e" from a local buffer (LocalMem treats addr as a pointer)
        let data = b"hello e2e";
        let wr = sup.handle(&notif(
            libc::SYS_write,
            [wfd as u64, addr_of(data), data.len() as u64, 0, 0, 0],
        ));
        assert_eq!(wr.val, data.len() as i64);

        // close
        let _ = sup.handle(&notif(libc::SYS_close, [wfd as u64, 0, 0, 0, 0, 0]));

        // reopen read-only
        let r = open(&sup, c"/tmp/t.txt", libc::O_RDONLY, 0);
        assert!(r.val >= 3);
        let rfd = r.val;

        // read into a local buffer
        let mut buf = [0u8; 64];
        let rd = sup.handle(&notif(
            libc::SYS_read,
            [rfd as u64, addr_of_mut(&mut buf), buf.len() as u64, 0, 0, 0],
        ));
        assert_eq!(rd.val, data.len() as i64);
        assert_eq!(&buf[..data.len()], data);

        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn unknown_fd_write_to_stdout_is_captured() {
        let sup = test_supervisor();
        let data = b"captured";
        let resp = sup.handle(&notif(
            libc::SYS_write,
            [1, addr_of(data), data.len() as u64, 0, 0, 0],
        ));
        assert_eq!(resp.val, data.len() as i64);
        // The write went to the stdout LogBuffer, not a real fd.
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn unhandled_syscall_is_continued() {
        let sup = test_supervisor();
        // brk is not handled — should be continued.
        let resp = sup.handle(&notif(libc::SYS_brk, [0; 6]));
        assert_eq!(resp.flags, USER_NOTIF_FLAG_CONTINUE);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    fn notif_from(nr: i64, pid: u32) -> SeccompNotif {
        let mut n = notif(nr, [0; 6]);
        n.pid = pid;
        n
    }

    #[test]
    fn getpid_returns_ns_tgid() {
        let sup = test_supervisor(); // init tid 100
        let resp = sup.handle(&notif_from(libc::SYS_getpid, 100));
        assert_eq!(resp.val, 100);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn getppid_for_init_is_zero() {
        let sup = test_supervisor();
        let resp = sup.handle(&notif_from(libc::SYS_getppid, 100));
        assert_eq!(resp.val, 0);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn getpid_for_lazily_discovered_child() {
        let mut proc = crate::procinfo::MockProcInfo::new();
        // child 200 of init 100.
        proc.add(200, 100, 200, 0);
        let sup = test_supervisor_with_proc(proc);
        let pid = sup.handle(&notif_from(libc::SYS_getpid, 200));
        assert_eq!(pid.val, 200);
        let ppid = sup.handle(&notif_from(libc::SYS_getppid, 200));
        assert_eq!(ppid.val, 100);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn gettid_returns_caller_tid() {
        let mut proc = crate::procinfo::MockProcInfo::new();
        proc.add(201, 100, 100, crate::procinfo::clone::THREAD);
        let sup = test_supervisor_with_proc(proc);
        let resp = sup.handle(&notif_from(libc::SYS_gettid, 201));
        assert_eq!(resp.val, 201);
        // But its pid is the group leader's.
        let pid = sup.handle(&notif_from(libc::SYS_getpid, 201));
        assert_eq!(pid.val, 100);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn socketpair_send_recv_roundtrip() {
        let sup = test_supervisor();
        // socketpair(AF_UNIX, SOCK_STREAM, 0, &sv)
        let mut sv = [0i32; 2];
        let resp = sup.handle(&notif(
            libc::SYS_socketpair,
            [
                libc::AF_UNIX as u64,
                libc::SOCK_STREAM as u64,
                0,
                addr_of_mut(bytemuck_sv(&mut sv)),
                0,
                0,
            ],
        ));
        assert_eq!(resp.error, 0);
        assert!(sv[0] >= 3 && sv[1] >= 3 && sv[0] != sv[1]);

        // sendto(sv[0], "hello sock", len, 0, NULL, 0)
        let data = b"hello sock";
        let s = sup.handle(&notif(
            libc::SYS_sendto,
            [sv[0] as u64, addr_of(data), data.len() as u64, 0, 0, 0],
        ));
        assert_eq!(s.val, data.len() as i64);

        // recvfrom(sv[1], buf, 64, 0, NULL, NULL)
        let mut buf = [0u8; 64];
        let r = sup.handle(&notif(
            libc::SYS_recvfrom,
            [sv[1] as u64, addr_of_mut(&mut buf), buf.len() as u64, 0, 0, 0],
        ));
        assert_eq!(r.val, data.len() as i64);
        assert_eq!(&buf[..data.len()], data);

        sup.state.lock().unwrap().overlay.cleanup();
    }

    // Reinterpret the [i32;2] sv array as a byte address for the sockaddr write.
    fn bytemuck_sv(sv: &mut [i32; 2]) -> &mut [u8] {
        // SAFETY: [i32;2] is 8 bytes; reinterpret as bytes for the pointer arg.
        unsafe { std::slice::from_raw_parts_mut(sv.as_mut_ptr() as *mut u8, 8) }
    }

    #[test]
    fn socketpair_sendmsg_recvmsg_roundtrip() {
        let sup = test_supervisor();
        let mut sv = [0i32; 2];
        let resp = sup.handle(&notif(
            libc::SYS_socketpair,
            [
                libc::AF_UNIX as u64,
                libc::SOCK_STREAM as u64,
                0,
                addr_of_mut(bytemuck_sv(&mut sv)),
                0,
                0,
            ],
        ));
        assert_eq!(resp.error, 0);

        // sendmsg on sv[0] with one iovec.
        let data = b"hello msg";
        let mut siov = libc::iovec {
            iov_base: data.as_ptr() as *mut libc::c_void,
            iov_len: data.len(),
        };
        // SAFETY: zeroed msghdr, then fill iov fields.
        let mut smsg: libc::msghdr = unsafe { std::mem::zeroed() };
        smsg.msg_iov = &mut siov;
        smsg.msg_iovlen = 1;
        let s = sup.handle(&notif(
            libc::SYS_sendmsg,
            [sv[0] as u64, &smsg as *const _ as u64, 0, 0, 0, 0],
        ));
        assert_eq!(s.val, data.len() as i64);

        // recvmsg on sv[1] into one iovec.
        let mut rbuf = [0u8; 64];
        let mut riov = libc::iovec {
            iov_base: rbuf.as_mut_ptr() as *mut libc::c_void,
            iov_len: rbuf.len(),
        };
        let mut rmsg: libc::msghdr = unsafe { std::mem::zeroed() };
        rmsg.msg_iov = &mut riov;
        rmsg.msg_iovlen = 1;
        let r = sup.handle(&notif(
            libc::SYS_recvmsg,
            [sv[1] as u64, &mut rmsg as *mut _ as u64, 0, 0, 0, 0],
        ));
        assert_eq!(r.val, data.len() as i64);
        assert_eq!(&rbuf[..data.len()], data);

        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn blocked_syscalls_are_rejected() {
        let sup = test_supervisor();
        // ptrace/mount/chroot → ENOSYS.
        for nr in [libc::SYS_ptrace, libc::SYS_mount, libc::SYS_chroot] {
            let resp = sup.handle(&notif(nr, [0; 6]));
            assert_eq!(resp.error, -(crate::error::Errno::NOSYS.code()), "nr={nr}");
        }
        // bind/listen → EPERM (inbound networking).
        for nr in [libc::SYS_bind, libc::SYS_listen] {
            let resp = sup.handle(&notif(nr, [0; 6]));
            assert_eq!(resp.error, -(crate::error::Errno::PERM.code()), "nr={nr}");
        }
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn clone_snapshots_then_continues() {
        let sup = test_supervisor();
        let resp = sup.handle(&notif(libc::SYS_clone, [0; 6]));
        assert_eq!(resp.flags, USER_NOTIF_FLAG_CONTINUE);
        sup.state.lock().unwrap().overlay.cleanup();
    }

    #[test]
    fn exit_group_prunes_thread_and_continues() {
        let mut proc = crate::procinfo::MockProcInfo::new();
        proc.add(200, 100, 200, 0);
        let sup = test_supervisor_with_proc(proc);
        // Discover the child.
        sup.handle(&notif_from(libc::SYS_getpid, 200));
        assert!(sup.state.lock().unwrap().threads.contains(200));
        let resp = sup.handle(&notif_from(libc::SYS_exit_group, 200));
        assert_eq!(resp.flags, USER_NOTIF_FLAG_CONTINUE);
        assert!(!sup.state.lock().unwrap().threads.contains(200));
        sup.state.lock().unwrap().overlay.cleanup();
    }
}
