//! Shared types: log level and the architecture-specific `struct stat` ABI.

/// Verbosity for the supervisor logger. SDKs default to `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    #[default]
    Off,
    Debug,
}

impl LogLevel {
    /// Parse the level strings accepted by the Node SDK (`"OFF"`/`"DEBUG"`).
    pub fn parse(s: &str) -> Option<LogLevel> {
        match s {
            "OFF" => Some(LogLevel::Off),
            "DEBUG" => Some(LogLevel::Debug),
            _ => None,
        }
    }
}

/// The kernel `struct stat` ABI written by fstat/stat/newfstatat, selected by
/// target architecture. This is **not** `statx` (256 bytes) — it is the older
/// per-arch layout the guest expects back from those syscalls.
#[cfg(target_arch = "aarch64")]
pub type Stat = AArch64Stat;
#[cfg(target_arch = "x86_64")]
pub type Stat = X86_64Stat;
// Fallback so the crate type-checks on dev hosts (e.g. arm64 macOS builds the
// aarch64 layout, which happens to match Linux/aarch64).
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub type Stat = AArch64Stat;

/// aarch64 layout from `asm-generic/stat.h` — 128 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
#[allow(non_snake_case)]
pub struct AArch64Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub __pad1: u64,
    pub st_size: i64,
    pub st_blksize: i32,
    pub __pad2: i32,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: u64,
    pub st_mtime: i64,
    pub st_mtime_nsec: u64,
    pub st_ctime: i64,
    pub st_ctime_nsec: u64,
    pub __unused4: u32,
    pub __unused5: u32,
}

/// x86_64 layout from `arch/x86/include/uapi/asm/stat.h` — 144 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
#[allow(non_snake_case)]
pub struct X86_64Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: u64,
    pub st_atime_nsec: u64,
    pub st_mtime: u64,
    pub st_mtime_nsec: u64,
    pub st_ctime: u64,
    pub st_ctime_nsec: u64,
    pub __unused: [i64; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_layouts_match_kernel_abi() {
        assert_eq!(std::mem::size_of::<AArch64Stat>(), 128);
        assert_eq!(std::mem::size_of::<X86_64Stat>(), 144);
    }

    #[test]
    fn parse_log_level() {
        assert_eq!(LogLevel::parse("OFF"), Some(LogLevel::Off));
        assert_eq!(LogLevel::parse("DEBUG"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("debug"), None);
        assert_eq!(LogLevel::parse(""), None);
    }
}
