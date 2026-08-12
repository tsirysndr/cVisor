//! Whiteout set for deletes over the read-only lower layer.
//!
//! Records guest-visible absolute paths that have been deleted, so a later
//! open/stat/getdents sees them as gone even though the real file still exists
//! underneath. Direct port of `Tombstones.zig`.

use std::collections::HashSet;

#[derive(Default)]
pub struct Tombstones {
    set: HashSet<String>,
}

impl Tombstones {
    pub fn new() -> Tombstones {
        Tombstones {
            set: HashSet::new(),
        }
    }

    /// Record a path as deleted.
    pub fn add(&mut self, path: &str) {
        self.set.insert(path.to_string());
    }

    /// Remove a tombstone (e.g. when a file is recreated via O_CREAT).
    pub fn remove(&mut self, path: &str) {
        self.set.remove(path);
    }

    /// Remove all tombstones that are strict children of `dir_path`. Called on
    /// rmdir so child tombstones don't outlive their parent directory.
    pub fn remove_children(&mut self, dir_path: &str) {
        let prefix = if dir_path == "/" {
            "/".to_string()
        } else {
            format!("{dir_path}/")
        };
        self.set
            .retain(|k| !(k.starts_with(&prefix) && k.len() > prefix.len()));
    }

    pub fn is_tombstoned(&self, path: &str) -> bool {
        self.set.contains(path)
    }

    /// Whether any ancestor directory of `path` is tombstoned.
    pub fn is_ancestor_tombstoned(&self, path: &str) -> bool {
        let mut current = path;
        while let Some(parent) = dirname(current) {
            if self.set.contains(parent) {
                return true;
            }
            if parent == "/" {
                break;
            }
            current = parent;
        }
        false
    }

    /// Whether a direct child `child_name` of `dir_path` is tombstoned.
    pub fn is_child_tombstoned(&self, dir_path: &str, child_name: &str) -> bool {
        let child = if dir_path == "/" {
            format!("/{child_name}")
        } else {
            format!("{dir_path}/{child_name}")
        };
        self.is_tombstoned(&child)
    }
}

/// POSIX dirname on an absolute path, matching Zig `std.fs.path.dirname`:
/// returns `None` when there is no parent to walk to.
fn dirname(path: &str) -> Option<&str> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let idx = trimmed.rfind('/')?;
    if idx == 0 {
        Some("/")
    } else {
        Some(&trimmed[..idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_nothing_tombstoned() {
        let ts = Tombstones::new();
        assert!(!ts.is_tombstoned("/etc/passwd"));
        assert!(!ts.is_tombstoned("/tmp/foo"));
    }

    #[test]
    fn add_exact_path() {
        let mut ts = Tombstones::new();
        ts.add("/etc/passwd");
        assert!(ts.is_tombstoned("/etc/passwd"));
        assert!(!ts.is_tombstoned("/etc/shadow"));
        assert!(!ts.is_tombstoned("/etc"));
    }

    #[test]
    fn does_not_affect_children_or_parents() {
        let mut ts = Tombstones::new();
        ts.add("/usr/local");
        assert!(ts.is_tombstoned("/usr/local"));
        assert!(!ts.is_tombstoned("/usr/local/bin"));
        assert!(!ts.is_tombstoned("/usr"));
    }

    #[test]
    fn remove_and_idempotent_add() {
        let mut ts = Tombstones::new();
        ts.add("/etc/passwd");
        ts.add("/etc/passwd");
        assert!(ts.is_tombstoned("/etc/passwd"));
        ts.remove("/etc/passwd");
        assert!(!ts.is_tombstoned("/etc/passwd"));
        ts.remove("/nonexistent");
    }

    #[test]
    fn child_tombstoned() {
        let mut ts = Tombstones::new();
        ts.add("/etc/passwd");
        assert!(ts.is_child_tombstoned("/etc", "passwd"));
        assert!(!ts.is_child_tombstoned("/etc", "shadow"));
    }

    #[test]
    fn remove_children_keeps_parent() {
        let mut ts = Tombstones::new();
        ts.add("/home/user");
        ts.add("/home/user/file.txt");
        ts.add("/home/user/docs/readme.md");
        ts.add("/home/other");
        ts.remove_children("/home/user");
        assert!(ts.is_tombstoned("/home/user"));
        assert!(!ts.is_tombstoned("/home/user/file.txt"));
        assert!(!ts.is_tombstoned("/home/user/docs/readme.md"));
        assert!(ts.is_tombstoned("/home/other"));
    }

    #[test]
    fn remove_children_no_children_noop() {
        let mut ts = Tombstones::new();
        ts.add("/etc/passwd");
        ts.remove_children("/etc/passwd");
        assert!(ts.is_tombstoned("/etc/passwd"));
    }

    #[test]
    fn ancestor_tombstoned() {
        let mut ts = Tombstones::new();
        ts.add("/home/user");
        assert!(ts.is_ancestor_tombstoned("/home/user/file.txt"));
        assert!(ts.is_ancestor_tombstoned("/home/user/docs/readme.md"));
        assert!(!ts.is_ancestor_tombstoned("/home/user"));
        assert!(!ts.is_ancestor_tombstoned("/home/other"));
        assert!(!ts.is_ancestor_tombstoned("/etc/passwd"));
    }

    #[test]
    fn ancestor_tombstoned_grandparent() {
        let mut ts = Tombstones::new();
        ts.add("/home");
        assert!(ts.is_ancestor_tombstoned("/home/user/file.txt"));
        assert!(ts.is_ancestor_tombstoned("/home/user"));
        assert!(!ts.is_ancestor_tombstoned("/home"));
    }

    #[test]
    fn ancestor_tombstoned_root() {
        let mut ts = Tombstones::new();
        ts.add("/");
        assert!(ts.is_ancestor_tombstoned("/anything"));
        assert!(ts.is_ancestor_tombstoned("/deep/nested/path"));
    }
}
