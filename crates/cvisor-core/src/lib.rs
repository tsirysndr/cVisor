//! cVisor core: an in-process Linux sandbox built on the seccomp user notifier.
//!
//! Module layout mirrors the original Zig tree. Pure-logic modules (path
//! routing, tombstones, dirent serialization, error/stat definitions, symlink
//! naming, /proc status parsing) compile on any host so they can be unit-tested
//! natively. Everything that touches kernel APIs is gated behind
//! `cfg(target_os = "linux")` and is the source of truth only under Docker.

pub mod error;
pub mod log_buffer;
pub mod types;

pub mod procinfo;
pub mod virt;

// Kernel-facing modules: Linux only. On other hosts the crate still builds with
// just the pure-logic modules above so they can be unit-tested natively.
#[cfg(target_os = "linux")]
pub mod mem;
#[cfg(target_os = "linux")]
pub mod seccomp;
#[cfg(target_os = "linux")]
pub mod setup;
#[cfg(target_os = "linux")]
pub mod supervisor;

#[cfg(target_os = "linux")]
pub use setup::{cleanup_overlay, execute, execute_with, ExecOpts};

pub use error::{Errno, SysError, SysResult};
pub use log_buffer::LogBuffer;
pub use types::{LogLevel, Stat};

/// Generate a 16-char lowercase-hex sandbox uid from 8 random bytes.
pub fn generate_uid() -> [u8; 16] {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("getrandom failed");
    let mut out = [0u8; 16];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0xf) as usize];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_is_16_lowercase_hex() {
        let uid = generate_uid();
        assert_eq!(uid.len(), 16);
        assert!(uid
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c)));
    }

    #[test]
    fn uids_differ() {
        assert_ne!(generate_uid(), generate_uid());
    }
}
