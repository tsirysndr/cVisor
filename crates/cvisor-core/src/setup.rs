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

use std::ffi::{CStr, CString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
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

/// Options controlling one sandboxed run.
#[derive(Clone, Copy)]
pub struct ExecOpts {
    /// Allow outbound INET/INET6 sockets (default true).
    pub allow_network: bool,
    /// Wall-clock limit; the guest process group is SIGKILLed when it elapses.
    pub timeout: Option<Duration>,
    /// Capture writes to fd 1/2 into the log buffers (default true). When false,
    /// they pass through to the guest's inherited stdout/stderr (terminal, PTY
    /// slave, pipe) — used by the CLI and PTY sessions.
    pub capture_stdio: bool,
}

impl Default for ExecOpts {
    fn default() -> ExecOpts {
        ExecOpts {
            allow_network: true,
            timeout: None,
            capture_stdio: true,
        }
    }
}

/// Run `cmd` (via `/bin/sh -c`) inside the sandbox, blocking until the guest
/// exits. Captured stdout/stderr accumulate in the provided buffers. Returns the
/// guest's exit code in shell convention: the exit status for a normal exit, or
/// `128 + signo` if it was killed by a signal (e.g. `137` for a timeout SIGKILL).
pub fn execute(
    uid: [u8; 16],
    log_level: LogLevel,
    cmd: &str,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
) -> SysResult<i32> {
    execute_with(uid, log_level, cmd, stdout, stderr, ExecOpts::default())
}

/// Like [`execute`], with explicit per-run [`ExecOpts`].
pub fn execute_with(
    uid: [u8; 16],
    log_level: LogLevel,
    cmd: &str,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    opts: ExecOpts,
) -> SysResult<i32> {
    run_argv(uid, log_level, &shell_argv(cmd), stdout, stderr, opts)
}

/// `argv` for running a shell command string: `/bin/sh -c <cmd>`.
pub fn shell_argv(cmd: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), cmd.into()]
}

/// `argv` that execs a user command with its argument boundaries preserved:
/// `/bin/sh -c 'exec "$@"' cvisor <args...>`. PATH resolution is done by the
/// guest shell, so `cvisor -- uname -a` behaves like a login shell would.
pub fn exec_argv(args: &[String]) -> Vec<String> {
    let mut v = vec![
        "/bin/sh".into(),
        "-c".into(),
        "exec \"$@\"".into(),
        "cvisor".into(),
    ];
    v.extend(args.iter().cloned());
    v
}

/// Run an explicit `argv` (argv[0] is the program path) inside the sandbox,
/// blocking until the guest exits. Same return convention as [`execute`].
pub fn run_argv(
    uid: [u8; 16],
    _log_level: LogLevel,
    argv: &[String],
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    opts: ExecOpts,
) -> SysResult<i32> {
    let guest = fork_guest(uid, argv, None)?;
    Ok(supervise_blocking(guest, stdout, stderr, opts))
}

/// A forked, seccomp-filtered guest ready to be supervised.
struct Guest {
    child_pid: i32,
    notify_fd: OwnedFd,
    overlay: OverlayRoot,
}

/// Fork the guest, wire up its stdio, install the filter, and steal its notify
/// fd. When `tty_slave` is set the child becomes a session leader on that PTY
/// slave (its stdin/stdout/stderr and controlling terminal); otherwise it stays
/// its own process-group leader (so the timeout watchdog can `kill(-pgid)`) and
/// inherits the parent's stdio. The caller retains ownership of `tty_slave` and
/// must drop its copy after this returns so the master sees HUP on child exit.
fn fork_guest(uid: [u8; 16], argv: &[String], tty_slave: Option<RawFd>) -> SysResult<Guest> {
    // Build EVERYTHING that allocates before fork(). In a multithreaded process
    // (the Node SDK, the test harness) another thread may hold the malloc lock
    // at fork time; the child inherits it locked, so any allocation in the child
    // deadlocks. After fork the child only calls async-signal-safe syscalls.
    if argv.is_empty() {
        return Err(SysError(Errno::INVAL));
    }
    let argv_owned: Vec<CString> = argv
        .iter()
        .map(|s| CString::new(s.as_str()).map_err(|_| SysError(Errno::INVAL)))
        .collect::<SysResult<_>>()?;
    let mut argv_ptrs: Vec<*const c_char> = argv_owned.iter().map(|c| c.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());
    let prog: &CStr = &argv_owned[0];

    let envp_owned: Vec<CString> = GUEST_ENVP
        .iter()
        .map(|s| CString::new(*s).unwrap())
        .collect();
    let mut envp: Vec<*const c_char> = envp_owned.iter().map(|c| c.as_ptr()).collect();
    envp.push(std::ptr::null());

    let overlay = OverlayRoot::new(uid).map_err(|_| SysError(Errno::IO))?;

    // SAFETY: fork; the child branch below performs no allocation.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(last_errno());
    }

    if pid == 0 {
        // Child: set up stdio + process group, install the filter (leaking the
        // fd so the supervisor can steal it via pidfd), then execve. No heap
        // allocation on this path.
        // SAFETY: async-signal-safe syscalls only.
        unsafe {
            match tty_slave {
                Some(slave) => {
                    // New session on the PTY: line discipline, job control, and
                    // isatty() all work as on a real terminal.
                    libc::setsid();
                    libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
                    libc::dup2(slave, 0);
                    libc::dup2(slave, 1);
                    libc::dup2(slave, 2);
                    if slave > 2 {
                        libc::close(slave);
                    }
                }
                None => {
                    // Own process group so the watchdog can kill(-pgid) the tree.
                    libc::setpgid(0, 0);
                }
            }
        }
        match filter::install() {
            Ok(fd) => std::mem::forget(fd),
            Err(_) => unsafe { libc::_exit(1) },
        }
        // SAFETY: valid NUL-terminated argv/envp built before the fork.
        unsafe {
            libc::execve(prog.as_ptr(), argv_ptrs.as_ptr(), envp.as_ptr());
            libc::_exit(1); // only reached if execve fails
        }
    }

    let notify_fd = steal_notify_fd(pid)?;
    Ok(Guest {
        child_pid: pid,
        notify_fd,
        overlay,
    })
}

/// Supervise a forked guest to completion on the calling thread, returning its
/// exit code. Consumes the `Guest`, moving its overlay into the supervisor.
fn supervise_blocking(
    guest: Guest,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    opts: ExecOpts,
) -> i32 {
    let Guest {
        child_pid,
        notify_fd,
        overlay,
    } = guest;
    let supervisor = make_supervisor(
        notify_fd.as_raw_fd(),
        child_pid,
        stdout,
        stderr,
        overlay,
        opts,
    );
    run_and_reap(supervisor, child_pid, notify_fd, opts.timeout)
}

/// Assemble a `Supervisor` from a forked guest's parts.
fn make_supervisor(
    notify_raw: RawFd,
    child_pid: i32,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    overlay: OverlayRoot,
    opts: ExecOpts,
) -> Arc<Supervisor> {
    Arc::new(Supervisor::new(
        notify_raw,
        child_pid,
        Box::new(RealGuestMem),
        Box::new(IoctlNotifier {
            notify_fd: notify_raw,
        }),
        Box::new(crate::procinfo::real::RealProcInfo),
        stdout,
        stderr,
        overlay,
        opts.allow_network,
        opts.capture_stdio,
    ))
}

/// Run the supervisor to completion (with the optional timeout watchdog), reap
/// the guest, and return its exit code in shell convention. Keeps `notify_fd`
/// alive for the whole run, then drops it.
fn run_and_reap(
    supervisor: Arc<Supervisor>,
    child_pid: i32,
    notify_fd: OwnedFd,
    timeout: Option<Duration>,
) -> i32 {
    // Timeout watchdog: SIGKILL the guest process group if the deadline passes.
    // It exits early when `done_tx` is dropped after the guest finishes, so a
    // completed run never signals a recycled pid. `timed_out` records whether it
    // fired, so we report 137 even if the host reaps the guest before we can.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let timed_out = Arc::new(AtomicBool::new(false));
    let watchdog = timeout.map(|timeout| {
        let timed_out = Arc::clone(&timed_out);
        std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(timeout) {
                timed_out.store(true, Ordering::SeqCst);
                // SAFETY: signal the guest's process group (pgid == child_pid).
                unsafe {
                    libc::kill(-child_pid, libc::SIGKILL);
                }
            }
        })
    });

    Arc::clone(&supervisor).run();
    // Guest is gone; stop the watchdog before reaping so it can't signal a
    // recycled pid, then keep the notify fd alive until after run() returns.
    drop(done_tx);
    if let Some(w) = watchdog {
        let _ = w.join();
    }
    drop(notify_fd);

    // Reap the guest (best-effort: a host that reaps its own children — e.g. the
    // BEAM's SIGCHLD handler — may beat us to it, leaving waitpid with ECHILD).
    let mut status: libc::c_int = 0;
    // SAFETY: valid child pid; status points at a live int.
    let reaped = unsafe { libc::waitpid(child_pid, &mut status, 0) } == child_pid;

    // Prefer the watchdog (we know we SIGKILLed it), then the guest's own
    // exit_group status (host-independent), then a successful waitpid (the only
    // source that distinguishes a signal death we did not cause).
    if timed_out.load(Ordering::SeqCst) {
        128 + libc::SIGKILL
    } else if let Some(code) = supervisor.exit_code() {
        code
    } else if reaped {
        exit_code_from_status(status)
    } else {
        -1
    }
}

/// Translate a `waitpid` status into a shell-convention exit code.
fn exit_code_from_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        -1
    }
}

/// How a [`Session`]'s stdio is exposed.
#[derive(Clone, Copy)]
pub enum PtyMode {
    /// No PTY: stdout/stderr are captured into the buffers, drained via
    /// [`Session::read_stdout`]/[`Session::read_stderr`].
    None,
    /// PTY whose master the core reads into the (merged) stdout buffer;
    /// [`Session::write_stdin`] feeds it. For programmatic interactive shells.
    Buffered,
    /// PTY whose master fd the caller takes via [`Session::take_master`] and
    /// drives itself (the CLI's raw-terminal pump). The core does not read it.
    Raw,
}

enum MasterState {
    None,
    Raw(OwnedFd),
    Shared(Arc<OwnedFd>),
}

/// A sandboxed process running in the background: the supervisor loop runs on a
/// worker thread while the caller streams output, feeds stdin, and waits.
pub struct Session {
    child_pid: i32,
    stdout: Arc<LogBuffer>,
    stderr: Arc<LogBuffer>,
    master: Mutex<MasterState>,
    exit: Arc<Mutex<Option<i32>>>,
    supervisor_join: Mutex<Option<JoinHandle<()>>>,
    reader_join: Mutex<Option<JoinHandle<()>>>,
}

/// Start a sandboxed `argv` in the background. With a PTY mode, the guest runs
/// on a pseudo-terminal (interactive shells, job control, `isatty()`); without
/// one, stdout/stderr are captured for streaming drain.
pub fn spawn_session(
    uid: [u8; 16],
    _log_level: LogLevel,
    argv: &[String],
    opts: ExecOpts,
    pty: PtyMode,
) -> SysResult<Session> {
    // PTY stdio flows through the slave, so don't capture; without a PTY, do.
    let mut opts = opts;
    opts.capture_stdio = matches!(pty, PtyMode::None);

    let (master, slave) = match pty {
        PtyMode::None => (None, None),
        PtyMode::Buffered | PtyMode::Raw => {
            let (m, s) = open_pty()?;
            (Some(m), Some(s))
        }
    };

    let guest = fork_guest(uid, argv, slave.as_ref().map(|s| s.as_raw_fd()))?;
    // Parent closes its slave copy so the master sees HUP when the guest exits.
    drop(slave);

    let Guest {
        child_pid,
        notify_fd,
        overlay,
    } = guest;
    let stdout = Arc::new(LogBuffer::new());
    let stderr = Arc::new(LogBuffer::new());
    let supervisor = make_supervisor(
        notify_fd.as_raw_fd(),
        child_pid,
        Arc::clone(&stdout),
        Arc::clone(&stderr),
        overlay,
        opts,
    );

    let exit = Arc::new(Mutex::new(None));
    let exit_thread = Arc::clone(&exit);
    let timeout = opts.timeout;
    let supervisor_join = std::thread::spawn(move || {
        let code = run_and_reap(supervisor, child_pid, notify_fd, timeout);
        *exit_thread.lock().unwrap() = Some(code);
    });

    let (master_state, reader_join) = match (pty, master) {
        (PtyMode::Buffered, Some(m)) => {
            let m = Arc::new(m);
            let reader_m = Arc::clone(&m);
            let out = Arc::clone(&stdout);
            let j = std::thread::spawn(move || pump_master_to_buffer(&reader_m, &out));
            (MasterState::Shared(m), Some(j))
        }
        (PtyMode::Raw, Some(m)) => (MasterState::Raw(m), None),
        _ => (MasterState::None, None),
    };

    Ok(Session {
        child_pid,
        stdout,
        stderr,
        master: Mutex::new(master_state),
        exit,
        supervisor_join: Mutex::new(Some(supervisor_join)),
        reader_join: Mutex::new(reader_join),
    })
}

impl Session {
    /// Drain and return captured stdout (or, for a buffered PTY, the merged
    /// terminal output) accumulated since the last call.
    pub fn read_stdout(&self) -> Vec<u8> {
        self.stdout.read()
    }

    /// Drain and return captured stderr (empty for a PTY, which merges streams).
    pub fn read_stderr(&self) -> Vec<u8> {
        self.stderr.read()
    }

    fn master_raw(&self) -> Option<RawFd> {
        match &*self.master.lock().unwrap() {
            MasterState::Raw(m) => Some(m.as_raw_fd()),
            MasterState::Shared(m) => Some(m.as_raw_fd()),
            MasterState::None => None,
        }
    }

    /// Write bytes to the guest's stdin (the PTY master). Errors without a PTY.
    pub fn write_stdin(&self, data: &[u8]) -> SysResult<usize> {
        let fd = self.master_raw().ok_or(SysError(Errno::BADF))?;
        // SAFETY: write from a valid slice to an owned pty master fd.
        let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if n < 0 {
            return Err(last_errno());
        }
        Ok(n as usize)
    }

    /// Set the PTY window size (no-op without a PTY).
    pub fn resize(&self, rows: u16, cols: u16) {
        if let Some(fd) = self.master_raw() {
            let ws = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: valid winsize pointer on an owned pty master fd.
            unsafe {
                libc::ioctl(fd, libc::TIOCSWINSZ, &ws);
            }
        }
    }

    /// Take the PTY master fd (Raw mode only) so the caller can drive it. After
    /// this, `write_stdin`/`resize` no longer apply.
    pub fn take_master(&self) -> Option<OwnedFd> {
        let mut g = self.master.lock().unwrap();
        match std::mem::replace(&mut *g, MasterState::None) {
            MasterState::Raw(m) => Some(m),
            other => {
                *g = other;
                None
            }
        }
    }

    /// The guest's exit code if it has finished, else None.
    pub fn try_wait(&self) -> Option<i32> {
        *self.exit.lock().unwrap()
    }

    /// Block until the guest exits and return its exit code.
    pub fn wait(&self) -> i32 {
        if let Some(j) = self.supervisor_join.lock().unwrap().take() {
            let _ = j.join();
        }
        if let Some(j) = self.reader_join.lock().unwrap().take() {
            let _ = j.join();
        }
        self.try_wait().unwrap_or(-1)
    }

    /// SIGKILL the guest process group.
    pub fn kill(&self) {
        // SAFETY: signal the guest's own process group (pgid == child_pid).
        unsafe {
            libc::kill(-self.child_pid, libc::SIGKILL);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Don't leak a running guest or its threads.
        if self.try_wait().is_none() {
            self.kill();
        }
        if let Some(j) = self.supervisor_join.lock().unwrap().take() {
            let _ = j.join();
        }
        if let Some(j) = self.reader_join.lock().unwrap().take() {
            let _ = j.join();
        }
    }
}

/// Open a PTY master/slave pair.
fn open_pty() -> SysResult<(OwnedFd, OwnedFd)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    // SAFETY: openpty writes two fresh fds; null name/termios/winsize are valid.
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if rc < 0 {
        return Err(last_errno());
    }
    // SAFETY: master/slave are fresh owned fds from openpty.
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}

/// Copy PTY master output into `out` until EOF/hangup (the guest exited).
fn pump_master_to_buffer(master: &OwnedFd, out: &LogBuffer) {
    let fd = master.as_raw_fd();
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: read into a valid local buffer on an owned fd.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            out.write(&buf[..n as usize]);
        } else if n == 0 {
            break; // EOF: slave fully closed (guest exited)
        } else {
            let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if e == libc::EINTR {
                continue;
            }
            break; // EIO on PTY hangup, etc.
        }
    }
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
