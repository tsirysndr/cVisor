//! In-sandbox smoke scorecard. This binary runs **as the guest** inside the
//! bVisor sandbox (the `bvisor` binary execs it). Each check exercises a syscall
//! from the guest's perspective — under seccomp interception — and prints
//! PASS/FAIL, ending with an `N/M passing` summary.
//!
//! Port of the categories in the original `smoke_test.zig`.
//!
//! Linux-only (it makes raw Linux syscalls); a stub `main` elsewhere keeps the
//! workspace building on other hosts.

#![allow(
    clippy::needless_return,
    clippy::unnecessary_cast,
    clippy::type_complexity
)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("smoke scorecard runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    imp::run();
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;

    fn ok(rc: i64) -> bool {
        rc >= 0
    }

    fn errno() -> i32 {
        // SAFETY: thread-local errno pointer.
        unsafe { *libc::__errno_location() }
    }

    // --- Process identity ---

    fn t_getpid() -> bool {
        // SAFETY: argless syscall.
        unsafe { libc::getpid() > 0 }
    }

    fn t_getppid_is_init() -> bool {
        // The sandbox root is namespace init → ppid virtualized to 0.
        unsafe { libc::getppid() == 0 }
    }

    fn t_gettid_eq_getpid() -> bool {
        // SAFETY: argless syscalls.
        unsafe { libc::syscall(libc::SYS_gettid) as i32 == libc::getpid() }
    }

    fn t_getuid() -> bool {
        // Passthrough; just must not error.
        unsafe {
            let _ = libc::getuid();
            true
        }
    }

    // --- File I/O (in the private /tmp overlay) ---

    fn t_file_roundtrip() -> bool {
        let path = CString::new("/tmp/smoke_rt.txt").unwrap();
        // SAFETY: valid path/buffers.
        unsafe {
            let fd = libc::open(
                path.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                0o644,
            );
            if fd < 0 {
                return false;
            }
            let data = b"hello smoke";
            let w = libc::write(fd, data.as_ptr() as *const _, data.len());
            libc::close(fd);
            if w != data.len() as isize {
                return false;
            }
            let fd = libc::open(path.as_ptr(), libc::O_RDONLY);
            if fd < 0 {
                return false;
            }
            let mut buf = [0u8; 32];
            let n = libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len());
            libc::close(fd);
            n == data.len() as isize && &buf[..data.len()] == data
        }
    }

    fn t_double_close_ebadf() -> bool {
        let path = CString::new("/tmp/smoke_dc.txt").unwrap();
        // SAFETY: valid path.
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CREAT, 0o644);
            if fd < 0 {
                return false;
            }
            libc::close(fd);
            // Second close must fail with EBADF.
            libc::close(fd) < 0 && errno() == libc::EBADF
        }
    }

    fn t_lseek() -> bool {
        let path = CString::new("/tmp/smoke_ls.txt").unwrap();
        // SAFETY: valid path/buffers.
        unsafe {
            let fd = libc::open(
                path.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC,
                0o644,
            );
            if fd < 0 {
                return false;
            }
            libc::write(fd, b"0123456789".as_ptr() as *const _, 10);
            let pos = libc::lseek(fd, 5, libc::SEEK_SET);
            let mut buf = [0u8; 5];
            let n = libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len());
            libc::close(fd);
            pos == 5 && n == 5 && &buf == b"56789"
        }
    }

    fn t_mkdir_rmdir() -> bool {
        let d = CString::new("/tmp/smoke_dir").unwrap();
        // SAFETY: valid path.
        unsafe {
            let _ = libc::rmdir(d.as_ptr());
            let m = libc::mkdir(d.as_ptr(), 0o755);
            let r = libc::rmdir(d.as_ptr());
            m == 0 && r == 0
        }
    }

    // --- /proc virtualization ---

    fn t_proc_self_status() -> bool {
        let path = CString::new("/proc/self/status").unwrap();
        // SAFETY: valid path/buffer.
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_RDONLY);
            if fd < 0 {
                return false;
            }
            let mut buf = [0u8; 128];
            let n = libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len());
            libc::close(fd);
            n > 0
                && std::str::from_utf8(&buf[..n as usize])
                    .map(|s| s.contains("bvisor-guest"))
                    .unwrap_or(false)
        }
    }

    // --- Memory (process-local, passthrough) ---

    fn t_mmap_munmap() -> bool {
        // SAFETY: standard anonymous mapping.
        unsafe {
            let len = 4096;
            let p = libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
                return false;
            }
            *(p as *mut u8) = 42;
            let val = *(p as *const u8);
            let u = libc::munmap(p, len);
            val == 42 && u == 0
        }
    }

    fn t_brk() -> bool {
        // SAFETY: brk(0) returns the current break.
        unsafe { libc::syscall(libc::SYS_brk, 0) != 0 }
    }

    // --- Time ---

    fn t_clock_gettime() -> bool {
        // SAFETY: valid timespec out-param.
        unsafe {
            let mut ts: libc::timespec = std::mem::zeroed();
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) == 0
        }
    }

    fn t_nanosleep() -> bool {
        // SAFETY: valid timespec.
        unsafe {
            let req = libc::timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            };
            libc::nanosleep(&req, std::ptr::null_mut()) == 0
        }
    }

    // --- Signals ---

    fn t_kill_self_zero() -> bool {
        // SAFETY: signal 0 just checks permission/existence.
        unsafe { libc::kill(libc::getpid(), 0) == 0 }
    }

    // --- Runtime ---

    fn t_uname_nodename() -> bool {
        // SAFETY: valid utsname out-param.
        unsafe {
            let mut u: libc::utsname = std::mem::zeroed();
            if libc::uname(&mut u) != 0 {
                return false;
            }
            let node: Vec<u8> = u
                .nodename
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            node == b"bvisor"
        }
    }

    fn t_getrandom() -> bool {
        // SAFETY: valid buffer.
        unsafe {
            let mut buf = [0u8; 16];
            libc::syscall(libc::SYS_getrandom, buf.as_mut_ptr(), buf.len(), 0) == buf.len() as i64
        }
    }

    // --- Blocked syscalls (must be denied) ---

    fn t_ptrace_blocked() -> bool {
        // PTRACE_TRACEME → ENOSYS in the sandbox.
        // SAFETY: ptrace with no memory args.
        unsafe { !ok(libc::syscall(libc::SYS_ptrace, 0, 0, 0, 0)) && errno() == libc::ENOSYS }
    }

    fn t_chroot_blocked() -> bool {
        let root = CString::new("/tmp").unwrap();
        // SAFETY: valid path.
        unsafe { libc::chroot(root.as_ptr()) < 0 && errno() == libc::ENOSYS }
    }

    fn t_mount_blocked() -> bool {
        let empty = CString::new("").unwrap();
        // SAFETY: mount with empty strings; must be denied before doing anything.
        unsafe {
            libc::mount(
                empty.as_ptr(),
                empty.as_ptr(),
                empty.as_ptr(),
                0,
                std::ptr::null(),
            ) < 0
                && errno() == libc::ENOSYS
        }
    }

    const CHECKS: &[(&str, fn() -> bool)] = &[
        ("getpid", t_getpid),
        ("getppid_is_init", t_getppid_is_init),
        ("gettid_eq_getpid", t_gettid_eq_getpid),
        ("getuid", t_getuid),
        ("file_roundtrip", t_file_roundtrip),
        ("double_close_ebadf", t_double_close_ebadf),
        ("lseek", t_lseek),
        ("mkdir_rmdir", t_mkdir_rmdir),
        ("proc_self_status", t_proc_self_status),
        ("mmap_munmap", t_mmap_munmap),
        ("brk", t_brk),
        ("clock_gettime", t_clock_gettime),
        ("nanosleep", t_nanosleep),
        ("kill_self_zero", t_kill_self_zero),
        ("uname_nodename", t_uname_nodename),
        ("getrandom", t_getrandom),
        ("ptrace_blocked", t_ptrace_blocked),
        ("chroot_blocked", t_chroot_blocked),
        ("mount_blocked", t_mount_blocked),
    ];

    pub fn run() {
        let mut passed = 0;
        for (name, check) in CHECKS {
            if check() {
                passed += 1;
                println!("PASS: {name}");
            } else {
                println!("FAIL: {name}");
            }
        }
        println!("{passed}/{} passing", CHECKS.len());
        if passed != CHECKS.len() {
            std::process::exit(1);
        }
    }
} // mod imp
