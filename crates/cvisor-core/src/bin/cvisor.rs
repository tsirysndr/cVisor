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
        cache, copy_into, copy_out_of, exec_argv, generate_uid, run_argv, spawn_session,
        uid_from_name, ExecOpts, Format, LogBuffer, LogLevel, PtyMode,
    };

    const USAGE: &str = "\
cvisor — in-process Linux sandbox

USAGE:
    cvisor [OPTIONS]                     open an interactive sandboxed shell
    cvisor [OPTIONS] -- <cmd> [args]     run a command in the sandbox
    cvisor [OPTIONS] cp <SRC> <DST>      copy a file/dir in/out of the sandbox
    cvisor [OPTIONS] cache save    <KEY> <sb:PATH>   back up a sandbox dir
    cvisor [OPTIONS] cache restore <KEY> <sb:PATH>   restore into a sandbox dir
    cvisor [OPTIONS] cache ls                        list cached archives
    cvisor [OPTIONS] cache rm <KEY> | --all          delete cache entries
    cvisor doctor                        check the host can run the sandbox

    In `cp`, prefix the sandbox side with `sb:` — e.g.
        cvisor --sandbox dev cp ./src sb:/app       (host -> sandbox, recursive)
        cvisor --sandbox dev cp sb:/app/dist ./dist (sandbox -> host, recursive)
    Directory copies and `cache save` skip paths matched by .gitignore /
    .dockerignore.

OPTIONS:
    --sandbox <name>    use a named, persistent sandbox (files survive across
                        invocations). Without it, runs are ephemeral; cp/cache
                        default to the sandbox named \"default\".
    --no-network        deny outbound INET/INET6 networking
    --allow-listen      permit inbound TCP servers (bind fixed port, listen)
    --timeout <ms>      SIGKILL the guest after <ms> milliseconds
    --format <fmt>      cache archive format: gzip (default), zstd, estargz, none
    --cache-backend <b> cache store: disk (default), disk:/path, or s3://bkt/prefix
    -h, --help          show this help
";

    enum Action {
        Run(Vec<String>),
        Interactive,
        Cp { src: String, dst: String },
        Cache(CacheCmd),
        Doctor,
    }

    enum CacheCmd {
        Save { key: String, path: String },
        Restore { key: String, path: String },
        Ls,
        Rm { key: Option<String> }, // None = --all
    }

    struct Opts {
        exec: ExecOpts,
        sandbox: Option<String>,
        action: Action,
        format: Format,
        cache_backend: String,
    }

    impl Opts {
        /// The overlay uid: the named sandbox, else ephemeral (random) for a
        /// run/shell, or the "default" named sandbox for file/cache operations.
        fn uid(&self) -> [u8; 16] {
            match (&self.sandbox, &self.action) {
                (Some(name), _) => uid_from_name(name),
                (None, Action::Run(_) | Action::Interactive | Action::Doctor) => generate_uid(),
                (None, _) => uid_from_name("default"),
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
        let mut format = Format::Gzip;
        let mut cache_backend = String::new();
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
                "cache" => {
                    let op = args.next().ok_or("cache needs save|restore|ls|rm")?;
                    let cmd = match op.as_str() {
                        "save" | "restore" => {
                            let key = args.next().ok_or("cache needs <KEY> <sb:PATH>")?;
                            let path = args.next().ok_or("cache needs <sb:PATH>")?;
                            if op == "save" {
                                CacheCmd::Save { key, path }
                            } else {
                                CacheCmd::Restore { key, path }
                            }
                        }
                        "ls" => CacheCmd::Ls,
                        "rm" => {
                            let arg = args.next().ok_or("cache rm needs <KEY> or --all")?;
                            if arg == "--all" {
                                CacheCmd::Rm { key: None }
                            } else {
                                CacheCmd::Rm { key: Some(arg) }
                            }
                        }
                        other => return Err(format!("unknown cache op: {other}")),
                    };
                    action = Action::Cache(cmd);
                    break;
                }
                "doctor" => {
                    action = Action::Doctor;
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
                "--format" => {
                    let f = args.next().ok_or("--format needs a value")?;
                    format = Format::parse(&f).ok_or(format!("unknown --format: {f}"))?;
                }
                "--cache-backend" => {
                    cache_backend = args.next().ok_or("--cache-backend needs a value")?;
                }
                "-h" | "--help" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
            }
        }
        Ok(Opts {
            exec,
            sandbox,
            action,
            format,
            cache_backend,
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
            Action::Cache(cmd) => cmd_cache(uid, cmd, opts.format, &opts.cache_backend),
            Action::Doctor => cmd_doctor(),
            _ => run_interactive(uid, opts.exec),
        }
    }

    /// `cvisor cache …`: manage the directory cache in the configured backend.
    fn cmd_cache(uid: [u8; 16], cmd: &CacheCmd, format: Format, backend_spec: &str) -> i32 {
        let backend = match cache::Backend::parse(backend_spec) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("cvisor: bad --cache-backend: {e}");
                return 2;
            }
        };
        let sandbox_path = |path: &str| -> Result<String, i32> {
            path.strip_prefix("sb:").map(str::to_string).ok_or_else(|| {
                eprintln!("cvisor: cache <sb:PATH> must be a sandbox path (prefix sb:)");
                2
            })
        };
        let report = |verb: &str, r: cvisor_core::SysResult<()>| match r {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("cvisor: cache {verb} failed: {e}");
                1
            }
        };
        match cmd {
            CacheCmd::Save { key, path } => match sandbox_path(path) {
                Ok(p) => report("save", cache::save(uid, &p, key, &backend, format)),
                Err(c) => c,
            },
            CacheCmd::Restore { key, path } => match sandbox_path(path) {
                Ok(p) => report("restore", cache::restore(uid, &p, key, &backend, format)),
                Err(c) => c,
            },
            CacheCmd::Ls => match cache::list(&backend) {
                Ok(entries) => {
                    for e in entries {
                        println!("{:>12}  {}", human_size(e.size), e.name);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("cvisor: cache ls failed: {e}");
                    1
                }
            },
            CacheCmd::Rm { key: Some(key) } => match cache::remove(key, &backend, format) {
                Ok(true) => 0,
                Ok(false) => {
                    eprintln!("cvisor: no cache entry for key '{key}'");
                    1
                }
                Err(e) => {
                    eprintln!("cvisor: cache rm failed: {e}");
                    1
                }
            },
            CacheCmd::Rm { key: None } => match cache::clear(&backend) {
                Ok(n) => {
                    println!("removed {n} cache entr{}", if n == 1 { "y" } else { "ies" });
                    0
                }
                Err(e) => {
                    eprintln!("cvisor: cache rm --all failed: {e}");
                    1
                }
            },
        }
    }

    fn human_size(n: u64) -> String {
        const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
        let mut v = n as f64;
        let mut i = 0;
        while v >= 1024.0 && i < U.len() - 1 {
            v /= 1024.0;
            i += 1;
        }
        if i == 0 {
            format!("{n} B")
        } else {
            format!("{v:.1} {}", U[i])
        }
    }

    /// `cvisor cp`: copy a file in or out of the sandbox overlay. Exactly one of
    /// SRC/DST must be an `sb:`-prefixed sandbox path.
    fn cmd_cp(uid: [u8; 16], src: &str, dst: &str) -> i32 {
        use std::path::Path;
        let src_sb = src.strip_prefix("sb:");
        let dst_sb = dst.strip_prefix("sb:");
        let result = match (src_sb, dst_sb) {
            (Some(_), Some(_)) | (None, None) => {
                eprintln!("cvisor: cp needs exactly one `sb:` path (host <-> sandbox)");
                return 2;
            }
            // host -> sandbox (file or directory, recursive)
            (None, Some(spath)) => copy_into(uid, Path::new(src), spath)
                .map_err(|e| format!("cp into sb:{spath}: {e}")),
            // sandbox -> host (file or directory, recursive)
            (Some(spath), None) => copy_out_of(uid, spath, Path::new(dst))
                .map_err(|e| format!("cp out of sb:{spath}: {e}")),
        };
        match result {
            Ok(()) => 0,
            Err(msg) => {
                eprintln!("cvisor: {msg}");
                1
            }
        }
    }

    /// `cvisor doctor`: check that this host can run the sandbox, printing a
    /// pass/fail line per prerequisite. Exit 0 only if all pass.
    fn cmd_doctor() -> i32 {
        println!("cvisor doctor\n");
        let mut ok = true;
        let mut check = |name: &str, pass: bool, detail: &str| {
            let mark = if pass { "✔" } else { "✘" };
            println!("  {mark} {name:<26} {detail}");
            ok &= pass;
        };

        #[cfg(target_os = "linux")]
        {
            // Kernel + arch.
            let uname = std::process::Command::new("uname").arg("-sr").output().ok();
            let kernel = uname
                .as_ref()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            check("linux kernel", !kernel.is_empty(), &kernel);

            // seccomp user-notifier: try installing an all-trap filter in a child.
            let (seccomp_ok, detail) = probe_seccomp_notif();
            check("seccomp user-notifier", seccomp_ok, detail);

            // pidfd_getfd (used to steal the guest's notify fd).
            let pidfd =
                unsafe { libc::syscall(libc::SYS_pidfd_open, std::process::id() as i32, 0) };
            let pidfd_ok = pidfd >= 0;
            if pidfd_ok {
                unsafe { libc::close(pidfd as i32) };
            }
            check(
                "pidfd_open",
                pidfd_ok,
                if pidfd_ok {
                    "available"
                } else {
                    "missing (kernel < 5.3?)"
                },
            );

            // /proc mounted (lazy thread discovery).
            let proc_ok = std::path::Path::new("/proc/self/status").exists();
            check("/proc mounted", proc_ok, if proc_ok { "yes" } else { "no" });

            // A real end-to-end run: `echo` in the sandbox.
            let run_ok = smoke_run();
            check(
                "sandbox smoke run",
                run_ok,
                if run_ok {
                    "ok"
                } else {
                    "failed — is seccomp=unconfined set? (docker: --security-opt seccomp=unconfined)"
                },
            );
        }
        #[cfg(not(target_os = "linux"))]
        check("linux", false, "cvisor runs on Linux only");

        println!();
        if ok {
            println!("All checks passed.");
            0
        } else {
            println!("Some checks failed — see above.");
            1
        }
    }

    #[cfg(target_os = "linux")]
    fn probe_seccomp_notif() -> (bool, &'static str) {
        // A child installs the notifier filter; if the syscall returns a fd the
        // feature is present. The child exits immediately either way.
        // SAFETY: fork; child only calls async-signal-safe syscalls then _exit.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            unsafe {
                libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
                // SECCOMP_SET_MODE_FILTER=1, FLAG_NEW_LISTENER=1<<3.
                let mut insns = [libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: 0x7fff_0000,
                }];
                let prog = libc::sock_fprog {
                    len: 1,
                    filter: insns.as_mut_ptr(),
                };
                let fd = libc::syscall(libc::SYS_seccomp, 1, 1u64 << 3, &prog as *const _);
                libc::_exit(if fd >= 0 { 0 } else { 1 });
            }
        }
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        let ok = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
        (
            ok,
            if ok {
                "available"
            } else {
                "unavailable (need kernel ≥ 5.0 and seccomp=unconfined)"
            },
        )
    }

    #[cfg(target_os = "linux")]
    fn smoke_run() -> bool {
        let out = Arc::new(LogBuffer::new());
        let err = Arc::new(LogBuffer::new());
        match run_argv(
            generate_uid(),
            LogLevel::Off,
            &exec_argv(&["true".to_string()]),
            Arc::clone(&out),
            err,
            ExecOpts::default(),
        ) {
            Ok(code) => code == 0,
            Err(_) => false,
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

    /// The interactive shell to launch: `bash` if present in the sandbox,
    /// otherwise `/bin/sh`. cow paths read through to the host, so a host
    /// `bash` is what the guest would exec.
    fn interactive_shell() -> String {
        for candidate in ["/bin/bash", "/usr/bin/bash"] {
            if std::path::Path::new(candidate).exists() {
                return candidate.to_string();
            }
        }
        "/bin/sh".to_string()
    }

    /// `cvisor` (no command): an interactive shell on a PTY (bash if available,
    /// else /bin/sh). Falls back to a plain sandboxed shell if stdin is not a
    /// terminal.
    fn run_interactive(uid: [u8; 16], exec: ExecOpts) -> i32 {
        let shell = interactive_shell();
        if !isatty(libc::STDIN_FILENO) {
            // Not a TTY (piped/redirected): run a shell reading the inherited fd 0.
            let out = Arc::new(LogBuffer::new());
            let err = Arc::new(LogBuffer::new());
            return run_argv(uid, LogLevel::Off, &[shell], out, err, exec).unwrap_or(1);
        }

        let session = match spawn_session(
            uid,
            LogLevel::Off,
            &[shell, "-i".to_string()],
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
