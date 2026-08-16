//! cVisor core: an in-process Linux sandbox built on the seccomp user notifier.
//!
//! Module layout mirrors the original Zig tree. Pure-logic modules (path
//! routing, tombstones, dirent serialization, error/stat definitions, symlink
//! naming, /proc status parsing) compile on any host so they can be unit-tested
//! natively. Everything that touches kernel APIs is gated behind
//! `cfg(target_os = "linux")` and is the source of truth only under Docker.

pub mod archive;
pub mod cache;
pub mod cgroup;
pub mod error;
pub mod fileio;
pub mod log_buffer;
pub mod snapshot;
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
pub use setup::{
    cleanup_overlay, exec_argv, execute, execute_with, run_argv, shell_argv, spawn_session,
    ExecOpts, PtyMode, Session,
};

pub use archive::Format;
pub use error::{Errno, SysError, SysResult};
pub use fileio::{copy_into, copy_out_of, read_file, write_file};
pub use log_buffer::LogBuffer;
pub use types::{LogLevel, Stat};

/// Generate a 16-char lowercase-hex sandbox uid from 8 random bytes.
pub fn generate_uid() -> [u8; 16] {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("getrandom failed");
    hex_uid(&bytes)
}

/// Derive a stable 16-hex-char sandbox uid from a name, so a named sandbox maps
/// to the same overlay across invocations (used by the CLI's `--sandbox`).
pub fn uid_from_name(name: &str) -> [u8; 16] {
    // FNV-1a over the name → 8 bytes, then the same hex encoding as generate_uid.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hex_uid(&h.to_le_bytes())
}

/// Encode 8 bytes as 16 lowercase-hex ASCII chars (the uid wire form).
fn hex_uid(bytes: &[u8; 8]) -> [u8; 16] {
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
