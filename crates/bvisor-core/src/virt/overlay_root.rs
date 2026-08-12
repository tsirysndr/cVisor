//! Per-sandbox overlay root at `/tmp/.bvisor/sb/{uid}` with `cow/` and `tmp/`
//! subdirectories. Path resolution here is pure string work (portable/testable);
//! the fd-backed backends that open these paths are Linux-only.

use crate::error::{Errno, SysError, SysResult};
use std::path::Path;

pub struct OverlayRoot {
    uid: [u8; 16],
    root_path: String,
}

impl OverlayRoot {
    /// Create the overlay root and its `cow`/`tmp` subdirectories.
    pub fn new(uid: [u8; 16]) -> std::io::Result<OverlayRoot> {
        let uid_str = String::from_utf8_lossy(&uid).into_owned();
        let root_path = format!("/tmp/.bvisor/sb/{uid_str}");
        std::fs::create_dir_all(format!("{root_path}/cow"))?;
        std::fs::create_dir_all(format!("{root_path}/tmp"))?;
        Ok(OverlayRoot { uid, root_path })
    }

    pub fn root_path(&self) -> &str {
        &self.root_path
    }

    pub fn uid(&self) -> &[u8; 16] {
        &self.uid
    }

    /// `/usr/bin/ls` -> `{root}/cow/usr/bin/ls`.
    pub fn resolve_cow(&self, path: &str) -> String {
        format!("{}/cow{}", self.root_path, path)
    }

    /// `/tmp/foo` -> `{root}/tmp/foo` (the `/tmp` prefix is stripped).
    pub fn resolve_tmp(&self, path: &str) -> SysResult<String> {
        let suffix = path.strip_prefix("/tmp").ok_or(SysError(Errno::INVAL))?;
        Ok(format!("{}/tmp{}", self.root_path, suffix))
    }

    /// Create the parent-directory chain for a path inside `cow/`.
    pub fn create_cow_parent_dirs(&self, path: &str) -> std::io::Result<()> {
        let parent = match Path::new(path).parent().and_then(|p| p.to_str()) {
            Some(p) if !p.is_empty() && p != "/" => p,
            _ => return Ok(()),
        };
        let rel = parent.strip_prefix('/').unwrap_or(parent);
        if rel.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(format!("{}/cow/{}", self.root_path, rel))
    }

    /// Create the parent-directory chain for a resolved overlay path.
    pub fn create_parent_dirs(resolved: &str) -> std::io::Result<()> {
        if let Some(parent) = Path::new(resolved).parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Exists on the real FS (no symlink follow, so dangling links count).
    pub fn path_exists_on_real_fs(path: &str) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// Is a directory on the real FS (follows symlinks).
    pub fn is_real_dir(path: &str) -> bool {
        std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
    }

    pub fn cow_exists(&self, path: &str) -> bool {
        Self::path_exists_on_real_fs(&self.resolve_cow(path))
    }

    pub fn is_cow_dir(&self, path: &str) -> bool {
        Self::is_real_dir(&self.resolve_cow(path))
    }

    pub fn tmp_exists(&self, path: &str) -> bool {
        match self.resolve_tmp(path) {
            Ok(p) => Self::path_exists_on_real_fs(&p),
            Err(_) => false,
        }
    }

    /// Is a path a directory in the tmp overlay?
    pub fn is_tmp_dir(&self, path: &str) -> bool {
        match self.resolve_tmp(path) {
            Ok(p) => Self::is_real_dir(&p),
            Err(_) => false,
        }
    }

    /// Exists from the guest's view: a cow copy or the real file.
    pub fn guest_path_exists(&self, path: &str) -> bool {
        self.cow_exists(path) || Self::path_exists_on_real_fs(path)
    }

    /// Is a directory from the guest's view (cow first, then real).
    pub fn is_guest_dir(&self, path: &str) -> bool {
        if self.cow_exists(path) {
            self.is_cow_dir(path)
        } else {
            Self::is_real_dir(path)
        }
    }

    /// Delete the overlay tree from disk.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.root_path);
    }
}

impl Drop for OverlayRoot {
    fn drop(&mut self) {
        // The session owns cleanup via cleanup(); Drop leaves the tree in place
        // so the SDK's explicit cleanup_overlay controls lifetime.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(uid: &str) -> OverlayRoot {
        let mut u = [0u8; 16];
        u[..uid.len()].copy_from_slice(uid.as_bytes());
        OverlayRoot::new(u).unwrap()
    }

    #[test]
    fn root_and_subdirs_created() {
        let o = mk("ovtestovtest0001");
        assert!(o.root_path().ends_with("/tmp/.bvisor/sb/ovtestovtest0001"));
        assert!(OverlayRoot::is_real_dir(&format!("{}/cow", o.root_path())));
        assert!(OverlayRoot::is_real_dir(&format!("{}/tmp", o.root_path())));
        o.cleanup();
    }

    #[test]
    fn resolve_cow_maps_under_cow() {
        let o = mk("ovtestovtest0003");
        let want = format!("{}/cow/etc/passwd", o.root_path());
        assert_eq!(o.resolve_cow("/etc/passwd"), want);
        o.cleanup();
    }

    #[test]
    fn resolve_tmp_strips_prefix() {
        let o = mk("ovtestovtest0004");
        let want = format!("{}/tmp/myfile", o.root_path());
        assert_eq!(o.resolve_tmp("/tmp/myfile").unwrap(), want);
        o.cleanup();
    }

    #[test]
    fn resolve_tmp_rejects_non_tmp() {
        let o = mk("ovtestovtest0005");
        assert_eq!(
            o.resolve_tmp("/etc/passwd").unwrap_err().errno(),
            Errno::INVAL
        );
        o.cleanup();
    }

    #[test]
    fn cow_exists_reflects_created_copy() {
        let o = mk("ovtestovtest0007");
        assert!(!o.cow_exists("/etc/passwd"));
        o.create_cow_parent_dirs("/etc/passwd").unwrap();
        std::fs::write(o.resolve_cow("/etc/passwd"), b"x").unwrap();
        assert!(o.cow_exists("/etc/passwd"));
        o.cleanup();
    }

    #[test]
    fn cleanup_removes_tree() {
        let o = mk("ovtestovtest0008");
        let root = o.root_path().to_string();
        o.cleanup();
        assert!(!OverlayRoot::path_exists_on_real_fs(&root));
    }
}
