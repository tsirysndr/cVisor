//! Local host PTY sessions for the agent panel's "host" mode: run a command on
//! a pty on the machine running the desktop app (not inside a sandbox), so
//! agent CLIs can use locally-installed skills (e.g. the cvisor CLI) to drive
//! sandboxes. Emits the same `shell-output`/`shell-exit` events as the gRPC
//! shell, with `h<N>` session ids.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use tauri::{AppHandle, Emitter, State};

use crate::{ShellExitPayload, ShellOutputPayload};

#[derive(Default)]
pub struct HostShells {
    sessions: Mutex<HashMap<String, HostShell>>,
    counter: AtomicU64,
}

struct HostShell {
    master: std::os::fd::OwnedFd,
    child: std::process::Child,
}

#[cfg(unix)]
fn open_pty() -> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd), String> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty fills two fresh fds; null name/termios/winsize are valid.
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc < 0 {
        return Err("openpty failed".into());
    }
    // SAFETY: fresh owned fds from openpty.
    unsafe {
        Ok((
            std::os::fd::OwnedFd::from_raw_fd(master),
            std::os::fd::OwnedFd::from_raw_fd(slave),
        ))
    }
}

/// Open a host PTY running `command` under the user's shell (`$SHELL -lc`, so
/// PATH and profile-installed CLIs resolve). Output streams as `shell-output`
/// events; exit as `shell-exit`.
#[tauri::command]
pub fn host_shell_open(
    app: AppHandle,
    state: State<'_, HostShells>,
    command: String,
) -> Result<String, String> {
    #[cfg(not(unix))]
    {
        let _ = (app, state, command);
        return Err("host terminals are only supported on macOS/Linux".into());
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::process::CommandExt;

        let (master, slave) = open_pty()?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let cmd = if command.trim().is_empty() {
            shell.clone()
        } else {
            command
        };
        let slave_fd = slave.as_raw_fd();
        let mut c = std::process::Command::new(&shell);
        c.arg("-lc")
            .arg(&cmd)
            .env("TERM", "xterm-256color")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: pre_exec runs post-fork/pre-exec; only async-signal-safe calls.
        unsafe {
            c.pre_exec(move || {
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                Ok(())
            });
        }
        let child = c.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        drop(slave); // parent's copy; master sees HUP when the child exits

        let session_id = format!("h{}", state.counter.fetch_add(1, Ordering::Relaxed));
        // SAFETY: dup of an owned master fd for the reader thread.
        let reader_fd = unsafe {
            let fd = libc::dup(master.as_raw_fd());
            if fd < 0 {
                return Err("dup failed".into());
            }
            std::os::fd::OwnedFd::from_raw_fd(fd)
        };
        state
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), HostShell { master, child });

        let sid = session_id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                // SAFETY: read into a local buffer on an owned fd.
                let n = unsafe {
                    libc::read(
                        reader_fd.as_raw_fd(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                    )
                };
                if n > 0 {
                    let _ = app.emit(
                        "shell-output",
                        ShellOutputPayload {
                            session_id: sid.clone(),
                            base64: B64.encode(&buf[..n as usize]),
                        },
                    );
                } else {
                    break; // EOF or EIO: pty hung up (child exited)
                }
            }
            let _ = app.emit(
                "shell-exit",
                ShellExitPayload {
                    session_id: sid.clone(),
                    code: 0,
                },
            );
        });
        Ok(session_id)
    }
}

#[tauri::command]
pub fn host_shell_write(
    state: State<'_, HostShells>,
    session_id: String,
    base64: String,
) -> Result<(), String> {
    let data = B64.decode(&base64).map_err(|e| e.to_string())?;
    let sessions = state.sessions.lock().unwrap();
    if let Some(s) = sessions.get(&session_id) {
        use std::os::fd::AsRawFd;
        // SAFETY: write from a valid slice to the owned pty master fd.
        unsafe {
            libc::write(
                s.master.as_raw_fd(),
                data.as_ptr() as *const libc::c_void,
                data.len(),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub fn host_shell_resize(
    state: State<'_, HostShells>,
    session_id: String,
    rows: u32,
    cols: u32,
) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    if let Some(s) = sessions.get(&session_id) {
        use std::os::fd::AsRawFd;
        let ws = libc::winsize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: valid winsize pointer on the owned pty master fd.
        unsafe {
            libc::ioctl(s.master.as_raw_fd(), libc::TIOCSWINSZ, &ws);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn host_shell_close(state: State<'_, HostShells>, session_id: String) -> Result<(), String> {
    if let Some(mut s) = state.sessions.lock().unwrap().remove(&session_id) {
        let _ = s.child.kill();
        let _ = s.child.wait();
        // master drops here; the reader thread sees EOF/EIO and exits.
    }
    Ok(())
}
