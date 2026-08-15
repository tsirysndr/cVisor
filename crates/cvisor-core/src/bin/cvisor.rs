//! The `cvisor` CLI: run a command in the sandbox, or drop into an interactive
//! sandboxed shell.
//!
//!   cvisor -- uname -a          run a command, streaming its output
//!   cvisor                      interactive shell on a PTY
//!   cvisor --no-network -- ...  disable outbound networking
//!   cvisor --timeout 5000 -- .. SIGKILL the guest after N ms

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("cvisor runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    std::process::exit(imp::run());
}

#[cfg(target_os = "linux")]
mod imp {
    use std::os::fd::AsRawFd;
    use std::sync::Arc;
    use std::time::Duration;

    use cvisor_core::{
        exec_argv, generate_uid, read_file, run_argv, spawn_session, uid_from_name, write_file,
        ExecOpts, LogBuffer, LogLevel, PtyMode,
    };

    const USAGE: &str = "\
cvisor — in-process Linux sandbox

USAGE:
    cvisor [OPTIONS]                 open an interactive sandboxed shell
    cvisor [OPTIONS] -- <cmd> [args] run a command in the sandbox
    cvisor [OPTIONS] cp <SRC> <DST>  copy a file in/out of the sandbox

    In `cp`, prefix the sandbox side with `sb:` — e.g.
        cvisor --sandbox dev cp ./app.py sb:/app/app.py   (host -> sandbox)
        cvisor --sandbox dev cp sb:/app/out.txt ./out.txt (sandbox -> host)

OPTIONS:
    --sandbox <name>    use a named, persistent sandbox (files survive across
                        invocations). Without it, runs are ephemeral; `cp`
                        defaults to the sandbox named \"default\".
    --no-network        deny outbound INET/INET6 networking
    --allow-listen      permit inbound TCP servers (bind fixed port, listen)
    --timeout <ms>      SIGKILL the guest after <ms> milliseconds
    -h, --help          show this help
";

    enum Action {
        Run(Vec<String>),
        Interactive,
        Cp { src: String, dst: String },
    }

    struct Opts {
        exec: ExecOpts,
        sandbox: Option<String>,
        action: Action,
    }

    impl Opts {
        /// The overlay uid: the named sandbox, else ephemeral (random) for a
        /// run/shell, or the "default" named sandbox for `cp`.
        fn uid(&self) -> [u8; 16] {
            match (&self.sandbox, &self.action) {
                (Some(name), _) => uid_from_name(name),
                (None, Action::Cp { .. }) => uid_from_name("default"),
                (None, _) => generate_uid(),
            }
        }
    }

    fn parse(mut args: impl Iterator<Item = String>) -> Result<Opts, String> {
        let mut exec = ExecOpts {
            capture_stdio: false, // the CLI streams stdio to the terminal
            ..ExecOpts::default()
        };
        let mut sandbox = None;
        let mut action = Action::Interactive;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--" => {
                    action = Action::Run(args.by_ref().collect());
                    break;
                }
                "cp" => {
                    let src = args.next().ok_or("cp needs <SRC> <DST>")?;
                    let dst = args.next().ok_or("cp needs <DST>")?;
                    action = Action::Cp { src, dst };
                    break;
                }
                "--sandbox" => sandbox = Some(args.next().ok_or("--sandbox needs a name")?),
                "--no-network" => exec.allow_network = false,
                "--allow-listen" => exec.allow_listen = true,
                "--timeout" => {
                    let ms = args
                        .next()
                        .ok_or("--timeout needs a value (ms)")?
                        .parse::<u64>()
                        .map_err(|_| "--timeout value must be a number".to_string())?;
                    exec.timeout = Some(Duration::from_millis(ms));
                }
                "-h" | "--help" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
            }
        }
        Ok(Opts {
            exec,
            sandbox,
            action,
        })
    }

    pub fn run() -> i32 {
        let opts = match parse(std::env::args().skip(1)) {
            Ok(o) => o,
            Err(msg) => {
                // --help prints to stdout and exits 0; real errors to stderr.
                if msg.starts_with("cvisor —") {
                    print!("{msg}");
                    return 0;
                }
                eprintln!("{msg}");
                return 2;
            }
        };

        let uid = opts.uid();
        match &opts.action {
            Action::Run(cmd) if !cmd.is_empty() => run_command(uid, cmd, opts.exec),
            Action::Cp { src, dst } => cmd_cp(uid, src, dst),
            _ => run_interactive(uid, opts.exec),
        }
    }

    /// `cvisor cp`: copy a file in or out of the sandbox overlay. Exactly one of
    /// SRC/DST must be an `sb:`-prefixed sandbox path.
    fn cmd_cp(uid: [u8; 16], src: &str, dst: &str) -> i32 {
        let src_sb = src.strip_prefix("sb:");
        let dst_sb = dst.strip_prefix("sb:");
        let result = match (src_sb, dst_sb) {
            (Some(_), Some(_)) | (None, None) => {
                eprintln!("cvisor: cp needs exactly one `sb:` path (host <-> sandbox)");
                return 2;
            }
            // host -> sandbox
            (None, Some(spath)) => std::fs::read(src)
                .map_err(|e| format!("read {src}: {e}"))
                .and_then(|data| {
                    write_file(uid, spath, &data).map_err(|e| format!("write sb:{spath}: {e}"))
                }),
            // sandbox -> host
            (Some(spath), None) => read_file(uid, spath)
                .map_err(|e| format!("read sb:{spath}: {e}"))
                .and_then(|data| {
                    std::fs::write(dst, data).map_err(|e| format!("write {dst}: {e}"))
                }),
        };
        match result {
            Ok(()) => 0,
            Err(msg) => {
                eprintln!("cvisor: {msg}");
                1
            }
        }
    }

    /// `cvisor -- <cmd>`: run a command with its stdio wired straight to the
    /// terminal (or whatever the CLI's stdio is), returning its exit code.
    fn run_command(uid: [u8; 16], args: &[String], exec: ExecOpts) -> i32 {
        // stdout/stderr pass through (capture_stdio is false), so these buffers
        // stay empty; they satisfy the API.
        let out = Arc::new(LogBuffer::new());
        let err = Arc::new(LogBuffer::new());
        match run_argv(uid, LogLevel::Off, &exec_argv(args), out, err, exec) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("cvisor: {e}");
                1
            }
        }
    }

    /// `cvisor` (no command): an interactive `/bin/sh` on a PTY. Falls back to a
    /// plain sandboxed shell if stdin is not a terminal.
    fn run_interactive(uid: [u8; 16], exec: ExecOpts) -> i32 {
        if !isatty(libc::STDIN_FILENO) {
            // Not a TTY (piped/redirected): run a shell reading the inherited fd 0.
            let out = Arc::new(LogBuffer::new());
            let err = Arc::new(LogBuffer::new());
            return run_argv(uid, LogLevel::Off, &["/bin/sh".to_string()], out, err, exec)
                .unwrap_or(1);
        }

        let session = match spawn_session(
            uid,
            LogLevel::Off,
            &["/bin/sh".to_string(), "-i".to_string()],
            exec,
            PtyMode::Raw,
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cvisor: failed to start shell: {e}");
                return 1;
            }
        };
        let master = match session.take_master() {
            Some(m) => m,
            None => {
                eprintln!("cvisor: no pty");
                return 1;
            }
        };
        // Match the guest's window to ours, then pump until the shell exits.
        propagate_winsize(libc::STDIN_FILENO, master.as_raw_fd());
        let restore = RawMode::enable(libc::STDIN_FILENO);
        pump(master.as_raw_fd());
        drop(restore); // restore the terminal before the exit banner
        session.wait()
    }

    /// Bidirectional copy between the user's terminal (fd 0/1) and the PTY master
    /// until the master hangs up (the shell exited).
    fn pump(master: i32) {
        let mut fds = [
            libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: two valid pollfds, infinite timeout.
            let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if rc < 0 {
                let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if e == libc::EINTR {
                    continue;
                }
                break;
            }
            // User keystrokes -> guest stdin.
            if fds[0].revents & libc::POLLIN != 0 {
                match read_fd(libc::STDIN_FILENO, &mut buf) {
                    Some(n) if n > 0 => {
                        let _ = write_all(master, &buf[..n]);
                    }
                    _ => {} // stdin EOF: keep the session alive until the shell exits
                }
            }
            // Guest output -> user terminal.
            if fds[1].revents & libc::POLLIN != 0 {
                match read_fd(master, &mut buf) {
                    Some(n) if n > 0 => {
                        let _ = write_all(libc::STDOUT_FILENO, &buf[..n]);
                    }
                    _ => break, // master EOF/hangup: the shell exited
                }
            }
            if fds[1].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                // Drain any final output, then stop.
                while let Some(n) = read_fd(master, &mut buf) {
                    if n == 0 {
                        break;
                    }
                    let _ = write_all(libc::STDOUT_FILENO, &buf[..n]);
                }
                break;
            }
        }
        let _ = master; // OwnedFd master is dropped by the caller
    }

    fn read_fd(fd: i32, buf: &mut [u8]) -> Option<usize> {
        // SAFETY: read into a valid local buffer.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if e == libc::EINTR || e == libc::EAGAIN {
                return Some(0);
            }
            return None;
        }
        Some(n as usize)
    }

    fn write_all(fd: i32, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            // SAFETY: write from a valid slice.
            let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(err);
            }
            data = &data[n as usize..];
        }
        Ok(())
    }

    fn isatty(fd: i32) -> bool {
        // SAFETY: isatty on a raw fd.
        unsafe { libc::isatty(fd) == 1 }
    }

    /// Copy the terminal's window size onto the PTY master.
    fn propagate_winsize(tty: i32, master: i32) {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        // SAFETY: valid winsize out-param.
        if unsafe { libc::ioctl(tty, libc::TIOCGWINSZ, &mut ws) } == 0 {
            // SAFETY: valid winsize in-param on the master.
            unsafe {
                libc::ioctl(master, libc::TIOCSWINSZ, &ws);
            }
        }
    }

    /// Puts a terminal into raw mode and restores it on drop.
    struct RawMode {
        fd: i32,
        original: libc::termios,
    }

    impl RawMode {
        fn enable(fd: i32) -> Option<RawMode> {
            let mut original: libc::termios = unsafe { std::mem::zeroed() };
            // SAFETY: tcgetattr fills a valid termios.
            if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
                return None;
            }
            let mut raw = original;
            // SAFETY: cfmakeraw on a valid termios.
            unsafe {
                libc::cfmakeraw(&mut raw);
                libc::tcsetattr(fd, libc::TCSANOW, &raw);
            }
            Some(RawMode { fd, original })
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            // SAFETY: restore the saved termios.
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
            }
        }
    }
}
