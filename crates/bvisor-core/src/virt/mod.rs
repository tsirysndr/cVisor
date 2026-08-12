//! Virtualization layer: path routing, overlay filesystem, tombstones, the
//! virtual FD table, the process/namespace model, and syscall handlers.

pub mod fs;
pub mod overlay_root;
pub mod path;
#[cfg(target_os = "linux")]
pub mod proc;
pub mod symlinks;
pub mod tombstones;

pub use overlay_root::OverlayRoot;
pub use path::{route, BackendType, RouteResult};
pub use tombstones::Tombstones;
