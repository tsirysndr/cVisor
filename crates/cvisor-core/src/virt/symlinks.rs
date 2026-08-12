//! Short `/.b/XXX` symlink naming used to redirect execve targets into the
//! overlay without overflowing the guest's original path buffer.
//!
//! Only the pure base-37 naming lives here for now; the Linux-side symlink
//! creation/cleanup (which issues real symlinkat/unlinkat syscalls) is added
//! alongside the execve handler.

/// Directory holding the short symlinks (blocked in the router).
pub const DIR: &str = "/.b";
const CHARSET: &[u8; 37] = b"0123456789abcdefghijklmnopqrstuvwxyz_";
const CODE_LEN: usize = 3;

/// Number of distinct slots: 37^3.
pub const MAX_ENTRIES: u32 = 37 * 37 * 37;
/// Length of a formatted path: "/.b/" + "XXX" = 7.
pub const PATH_LEN: usize = DIR.len() + 1 + CODE_LEN;

/// Encode `idx` as a fixed 3-char base-37 code.
pub fn encode_index(idx: u32) -> [u8; CODE_LEN] {
    let mut n = idx;
    let mut buf = [0u8; CODE_LEN];
    let mut i = CODE_LEN;
    while i > 0 {
        i -= 1;
        buf[i] = CHARSET[(n % 37) as usize];
        n /= 37;
    }
    buf
}

/// Format the full `/.b/XXX` path for slot `idx`.
pub fn format_path(idx: u32) -> String {
    let code = encode_index(idx);
    let mut s = String::with_capacity(PATH_LEN);
    s.push_str(DIR);
    s.push('/');
    s.push_str(std::str::from_utf8(&code).unwrap());
    s
}

/// Linux-side manager that creates the short `/.b/XXX` symlinks and cleans them
/// up. Each sandbox instance owns one so it can unlink its links on teardown.
#[cfg(target_os = "linux")]
pub mod manager {
    use super::{format_path, DIR, MAX_ENTRIES, PATH_LEN};
    use crate::error::{Errno, SysError, SysResult};
    use std::ffi::CString;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    pub struct Symlinks {
        counter: AtomicU32,
        created: Mutex<Vec<u32>>,
    }

    impl Default for Symlinks {
        fn default() -> Self {
            Symlinks::new()
        }
    }

    impl Symlinks {
        pub fn new() -> Symlinks {
            Symlinks {
                counter: AtomicU32::new(0),
                created: Mutex::new(Vec::new()),
            }
        }

        /// Create a symlink at `/.b/XXX` pointing at `target`. `original_len` is
        /// the guest's original path length; the short link must fit within it.
        /// Probes forward on EEXIST (another sandbox owns that slot).
        pub fn create(&self, target: &str, original_len: usize) -> SysResult<String> {
            if PATH_LEN > original_len {
                return Err(SysError(Errno::PERM));
            }
            if target.len() > 512 {
                return Err(SysError(Errno::NAMETOOLONG));
            }
            ensure_dir();
            let target_c = CString::new(target).map_err(|_| SysError(Errno::INVAL))?;

            for _ in 0..20 {
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) % MAX_ENTRIES;
                let path = format_path(idx);
                let path_c = CString::new(path.clone()).unwrap();
                // SAFETY: both are valid NUL-terminated paths.
                let rc = unsafe { libc::symlink(target_c.as_ptr(), path_c.as_ptr()) };
                if rc == 0 {
                    self.created.lock().unwrap().push(idx);
                    return Ok(path);
                }
                let err = nix::errno::Errno::last();
                if err == nix::errno::Errno::EEXIST {
                    continue; // slot taken by another sandbox; try the next
                }
                return Err(SysError(Errno::from_raw(err as i32).unwrap_or(Errno::PERM)));
            }
            Err(SysError(Errno::NOSPC))
        }
    }

    impl Drop for Symlinks {
        fn drop(&mut self) {
            let created = self.created.lock().unwrap();
            for &idx in created.iter() {
                if let Ok(c) = CString::new(format_path(idx)) {
                    // SAFETY: valid path; best-effort cleanup.
                    unsafe {
                        libc::unlink(c.as_ptr());
                    }
                }
            }
        }
    }

    fn ensure_dir() {
        if let Ok(c) = CString::new(DIR) {
            // SAFETY: valid path; best-effort mkdir (EEXIST is fine).
            unsafe {
                libc::mkdir(c.as_ptr(), 0o777);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base37_encoding() {
        assert_eq!(&encode_index(0), b"000");
        assert_eq!(&encode_index(1), b"001");
        assert_eq!(&encode_index(36), b"00_");
        assert_eq!(&encode_index(37), b"010");
    }

    #[test]
    fn format_produces_b_path() {
        assert_eq!(format_path(0), "/.b/000");
        assert_eq!(format_path(37), "/.b/010");
        assert_eq!(format_path(0).len(), PATH_LEN);
    }

    #[test]
    fn slot_count() {
        assert_eq!(MAX_ENTRIES, 50_653);
    }
}
