//! Per-process virtual file-descriptor table. Virtual fds start at 3 and are
//! never reused. `Arc<File>` gives true POSIX dup semantics (shared open-file
//! description). Port of `FdTable.zig` + `FdEntry.zig`.

use crate::virt::fs::file::File;
use std::collections::HashMap;
use std::sync::Arc;

pub type VirtualFd = i32;

#[derive(Clone)]
pub struct FdEntry {
    pub file: Arc<File>,
    pub cloexec: bool,
}

pub struct FdTable {
    open_files: HashMap<VirtualFd, FdEntry>,
    next_vfd: VirtualFd,
}

impl Default for FdTable {
    fn default() -> Self {
        FdTable::new()
    }
}

impl FdTable {
    pub fn new() -> FdTable {
        FdTable {
            open_files: HashMap::new(),
            next_vfd: 3, // after stdin/stdout/stderr
        }
    }

    /// Deep-clone for a fork without CLONE_FILES: each file gets its own dup'd
    /// kernel fd (the dup-on-clone fix), cloexec preserved, next_vfd inherited.
    pub fn deep_clone(&self) -> FdTable {
        let mut new = FdTable {
            open_files: HashMap::new(),
            next_vfd: self.next_vfd,
        };
        for (vfd, entry) in &self.open_files {
            if let Some(dup) = entry.file.try_duplicate() {
                new.open_files.insert(
                    *vfd,
                    FdEntry {
                        file: Arc::new(dup),
                        cloexec: entry.cloexec,
                    },
                );
            }
        }
        new
    }

    /// Insert a newly opened file, returning its virtual fd.
    pub fn insert(&mut self, file: Arc<File>, cloexec: bool) -> VirtualFd {
        let vfd = self.next_vfd;
        self.next_vfd += 1;
        self.open_files.insert(vfd, FdEntry { file, cloexec });
        vfd
    }

    /// Insert at a specific vfd (caller must have removed any existing entry).
    pub fn insert_at(&mut self, file: Arc<File>, vfd: VirtualFd, cloexec: bool) -> VirtualFd {
        if vfd >= self.next_vfd {
            self.next_vfd = vfd + 1;
        }
        self.open_files.insert(vfd, FdEntry { file, cloexec });
        vfd
    }

    /// Duplicate an existing file to the next vfd (POSIX dup: shared file).
    pub fn dup(&mut self, file: Arc<File>) -> VirtualFd {
        let vfd = self.next_vfd;
        self.next_vfd += 1;
        self.open_files.insert(
            vfd,
            FdEntry {
                file,
                cloexec: false,
            },
        );
        vfd
    }

    /// Duplicate to a specific vfd (dup2/dup3). Replaces any existing entry.
    pub fn dup_at(&mut self, file: Arc<File>, newfd: VirtualFd, cloexec: bool) -> VirtualFd {
        if newfd >= self.next_vfd {
            self.next_vfd = newfd + 1;
        }
        self.open_files.insert(newfd, FdEntry { file, cloexec });
        newfd
    }

    /// Get a cloned handle (Arc bump) to the file at `vfd`.
    pub fn get(&self, vfd: VirtualFd) -> Option<Arc<File>> {
        self.open_files.get(&vfd).map(|e| Arc::clone(&e.file))
    }

    pub fn get_entry(&self, vfd: VirtualFd) -> Option<FdEntry> {
        self.open_files.get(&vfd).cloned()
    }

    pub fn contains(&self, vfd: VirtualFd) -> bool {
        self.open_files.contains_key(&vfd)
    }

    /// Drop every tracked fd in `[first, last]` (close_range). Returns how many
    /// entries were removed.
    pub fn remove_range(&mut self, first: VirtualFd, last: VirtualFd) -> usize {
        let doomed: Vec<VirtualFd> = self
            .open_files
            .keys()
            .copied()
            .filter(|&fd| fd >= first && fd <= last)
            .collect();
        for fd in &doomed {
            self.open_files.remove(fd);
        }
        doomed.len()
    }

    /// Drop every CLOEXEC-marked entry (applied at execve). Dropping the
    /// entries releases the supervisor's dups of their backing fds, so e.g. a
    /// CLOEXEC pipe write-end actually reaches EOF for its reader once the
    /// kernel closes the guest's copy at exec.
    pub fn remove_cloexec(&mut self) -> usize {
        let before = self.open_files.len();
        self.open_files.retain(|_, entry| !entry.cloexec);
        before - self.open_files.len()
    }

    /// Set CLOEXEC on every tracked fd in `[first, last]`
    /// (close_range CLOSE_RANGE_CLOEXEC).
    pub fn set_cloexec_range(&mut self, first: VirtualFd, last: VirtualFd) {
        for (fd, entry) in self.open_files.iter_mut() {
            if *fd >= first && *fd <= last {
                entry.cloexec = true;
            }
        }
    }

    /// Remove the entry (drops the table's Arc; the file closes on last ref).
    pub fn remove(&mut self, vfd: VirtualFd) -> bool {
        self.open_files.remove(&vfd).is_some()
    }

    pub fn get_cloexec(&self, vfd: VirtualFd) -> bool {
        self.open_files
            .get(&vfd)
            .map(|e| e.cloexec)
            .unwrap_or(false)
    }

    pub fn set_cloexec(&mut self, vfd: VirtualFd, value: bool) -> bool {
        match self.open_files.get_mut(&vfd) {
            Some(e) => {
                e.cloexec = value;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virt::fs::backend::Backend;

    fn fake(fd: i32) -> Arc<File> {
        // Passthrough over a fake fd; never actually opened. Drop will call
        // close() on it, which harmlessly returns EBADF. Use high fd numbers.
        Arc::new(File::new(Backend::Passthrough(fd)))
    }

    #[test]
    fn insert_starts_at_3_and_increments() {
        let mut t = FdTable::new();
        for i in 0..5 {
            assert_eq!(t.insert(fake(1000 + i), false), 3 + i);
        }
    }

    #[test]
    fn get_and_remove() {
        let mut t = FdTable::new();
        let vfd = t.insert(fake(1042), false);
        assert!(t.get(vfd).is_some());
        assert!(t.remove(vfd));
        assert!(t.get(vfd).is_none());
        assert!(!t.remove(vfd));
    }

    #[test]
    fn vfd_never_reused() {
        let mut t = FdTable::new();
        let a = t.insert(fake(1100), false);
        t.remove(a);
        let b = t.insert(fake(1101), false);
        assert_eq!(a, 3);
        assert_eq!(b, 4);
    }

    #[test]
    fn dup_shares_file() {
        let mut t = FdTable::new();
        let vfd = t.insert(fake(1200), false);
        let file = t.get(vfd).unwrap();
        let dupd = t.dup(file);
        assert_eq!(dupd, 4);
        // Both point at the same Arc<File>.
        assert!(Arc::ptr_eq(&t.get(vfd).unwrap(), &t.get(dupd).unwrap()));
    }

    #[test]
    fn dup_at_replaces_slot_and_advances_next() {
        let mut t = FdTable::new();
        let vfd = t.insert(fake(1300), false);
        let file = t.get(vfd).unwrap();
        t.dup_at(file, 10, true);
        assert!(t.get(10).is_some());
        assert!(t.get_cloexec(10));
        // next_vfd advanced past 10.
        assert_eq!(t.insert(fake(1301), false), 11);
    }

    #[test]
    fn cloexec_get_set() {
        let mut t = FdTable::new();
        let vfd = t.insert(fake(1400), false);
        assert!(!t.get_cloexec(vfd));
        assert!(t.set_cloexec(vfd, true));
        assert!(t.get_cloexec(vfd));
        assert!(!t.set_cloexec(999, true));
    }
}
