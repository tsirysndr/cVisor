//! File-descriptor virtualization: dirent serialization (portable), and — on
//! Linux — the FD table, virtual `File`, and its kernel-backed backends.

pub mod dirent;

#[cfg(target_os = "linux")]
pub mod backend;
#[cfg(target_os = "linux")]
pub mod fd_table;
#[cfg(target_os = "linux")]
pub mod file;
#[cfg(target_os = "linux")]
pub mod fs_info;
