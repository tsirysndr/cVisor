//! Sandbox snapshots: capture, branch, and roll back a sandbox's overlay.
//!
//! A sandbox's whole filesystem overlay lives at `/tmp/.cvisor/sb/{uid}`. A
//! snapshot is a copy of that tree under `/tmp/.cvisor/snapshots/{id}`:
//!
//! - [`snapshot`] copies a sandbox's overlay to a snapshot id.
//! - [`rollback`] replaces a sandbox's overlay with a snapshot (discarding since).
//! - [`branch`] copies a snapshot into a fresh uid (a new sandbox from a snapshot).
//! - [`fork`] copies a live sandbox's overlay into a fresh uid (branch the current
//!   state without an explicit snapshot).

use std::path::{Path, PathBuf};

use crate::error::{Errno, SysError, SysResult};

const SB_ROOT: &str = "/tmp/.cvisor/sb";
const SNAP_ROOT: &str = "/tmp/.cvisor/snapshots";

/// A stored snapshot: its id and total byte size on disk.
pub struct SnapshotInfo {
    pub id: String,
    pub size: u64,
}

fn io_errno(e: &std::io::Error) -> SysError {
    SysError(
        e.raw_os_error()
            .and_then(Errno::from_raw)
            .unwrap_or(Errno::IO),
    )
}

fn sb_dir(uid: &[u8; 16]) -> PathBuf {
    Path::new(SB_ROOT).join(String::from_utf8_lossy(uid).as_ref())
}

/// Validate a snapshot id — a path segment, so reject anything with separators
/// or traversal.
fn snap_dir(id: &str) -> SysResult<PathBuf> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && id != ".."
        && id != ".";
    if ok {
        Ok(Path::new(SNAP_ROOT).join(id))
    } else {
        Err(SysError(Errno::INVAL))
    }
}

/// Snapshot a sandbox's overlay under `id`, replacing any existing snapshot with
/// that id. A sandbox with no writes yet yields an empty snapshot.
pub fn snapshot(uid: [u8; 16], id: &str) -> SysResult<()> {
    let dst = snap_dir(id)?;
    let src = sb_dir(&uid);
    let _ = std::fs::remove_dir_all(&dst);
    if !src.exists() {
        return std::fs::create_dir_all(&dst).map_err(|e| io_errno(&e));
    }
    copy_tree(&src, &dst).map_err(|e| io_errno(&e))
}

/// Replace a sandbox's overlay with snapshot `id` (discarding changes since it
/// was taken).
pub fn rollback(uid: [u8; 16], id: &str) -> SysResult<()> {
    let src = snap_dir(id)?;
    if !src.exists() {
        return Err(SysError(Errno::NOENT));
    }
    let dst = sb_dir(&uid);
    let _ = std::fs::remove_dir_all(&dst);
    copy_tree(&src, &dst).map_err(|e| io_errno(&e))
}

/// Populate `new_uid`'s overlay from snapshot `id` (a new sandbox off a snapshot).
pub fn branch(id: &str, new_uid: [u8; 16]) -> SysResult<()> {
    let src = snap_dir(id)?;
    if !src.exists() {
        return Err(SysError(Errno::NOENT));
    }
    let dst = sb_dir(&new_uid);
    let _ = std::fs::remove_dir_all(&dst);
    copy_tree(&src, &dst).map_err(|e| io_errno(&e))
}

/// Populate `new_uid`'s overlay from a live sandbox's current overlay (fork).
pub fn fork(src_uid: [u8; 16], new_uid: [u8; 16]) -> SysResult<()> {
    let src = sb_dir(&src_uid);
    let dst = sb_dir(&new_uid);
    let _ = std::fs::remove_dir_all(&dst);
    if !src.exists() {
        return Ok(()); // nothing written yet -> empty branch
    }
    copy_tree(&src, &dst).map_err(|e| io_errno(&e))
}

/// List stored snapshots (id + on-disk size), sorted by id.
pub fn list() -> SysResult<Vec<SnapshotInfo>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(SNAP_ROOT) else {
        return Ok(out); // no snapshots dir yet
    };
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            out.push(SnapshotInfo {
                id: e.file_name().to_string_lossy().into_owned(),
                size: dir_size(&e.path()),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Delete snapshot `id`. Returns whether it existed.
pub fn delete(id: &str) -> SysResult<bool> {
    let dir = snap_dir(id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| io_errno(&e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Recursively copy `src` to `dst`, preserving symlinks (the overlay uses them).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = std::fs::read_link(src)?;
        let _ = std::fs::remove_file(dst);
        std::os::unix::fs::symlink(target, dst)?;
    } else if ft.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                total += dir_size(&e.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_snapshot_ids() {
        assert!(snap_dir("").is_err());
        assert!(snap_dir("..").is_err());
        assert!(snap_dir("a/b").is_err());
        assert!(snap_dir("../etc").is_err());
        assert!(snap_dir("ok-id_1.2").is_ok());
    }

    #[test]
    fn copy_tree_preserves_files_and_symlinks() {
        let base = std::env::temp_dir().join(format!("cvsnap-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::write(src.join("sub/b.txt"), b"world").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("link")).unwrap();

        copy_tree(&src, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dst.join("sub/b.txt")).unwrap(), b"world");
        assert!(std::fs::symlink_metadata(dst.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_link(dst.join("link")).unwrap().to_str(),
            Some("a.txt")
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
