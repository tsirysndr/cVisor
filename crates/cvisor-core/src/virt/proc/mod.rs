//! Process/thread virtualization.
//!
//! Follows the Linux model where threads are the basic unit. Each `Thread` has
//! a tid, a thread-group id (tgid == pid), a parent, and references — by id into
//! per-`Threads` arenas — to a shared FD table and fs-info. `clone` is
//! passthrough, so children are discovered lazily by scanning `/proc`; clone
//! flags are inferred after the fact.
//!
//! This milestone models a single PID namespace (NsTid == AbsTid). Nested PID
//! namespaces (CLONE_NEWPID translation) are a later milestone.

use crate::procinfo::{AbsTgid, AbsTid, CloneFlags, NsTgid, NsTid, ProcInfo};
use crate::virt::fs::fd_table::FdTable;
use crate::virt::fs::fs_info::FsInfo;
use std::collections::{HashMap, VecDeque};

pub type FdTableId = u64;
pub type FsInfoId = u64;

pub struct Thread {
    pub tid: AbsTid,
    pub tgid: AbsTgid,
    /// Parent thread's tid, or None for the sandbox init thread.
    pub parent: Option<AbsTid>,
    pub fd_table_id: FdTableId,
    pub fs_info_id: FsInfoId,
}

struct Slot<T> {
    value: T,
    refs: usize,
}

/// A parent's fd-table + fs-info state captured at `clone` time, so a lazily
/// discovered child inherits the fork-time snapshot rather than the parent's
/// (possibly since-mutated) live state.
struct ForkSnapshot {
    fd_table: FdTable,
    fs_info: FsInfo,
}

pub struct Threads {
    map: HashMap<AbsTid, Thread>,
    fd_tables: HashMap<FdTableId, Slot<FdTable>>,
    fs_infos: HashMap<FsInfoId, Slot<FsInfo>>,
    fork_snapshots: HashMap<AbsTid, VecDeque<ForkSnapshot>>,
    next_fdt: FdTableId,
    next_fsi: FsInfoId,
    init_tid: AbsTid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadError {
    /// No such thread and it could not be discovered.
    NotFound,
    /// A thread's ancestor is outside the sandbox.
    NotInSandbox,
    /// An unsupported clone flag (a namespace kind we don't virtualize).
    Unsupported,
}

impl Threads {
    /// Create the registry with the initial guest thread registered.
    pub fn new(init_tid: AbsTid) -> Threads {
        let mut t = Threads {
            map: HashMap::new(),
            fd_tables: HashMap::new(),
            fs_infos: HashMap::new(),
            fork_snapshots: HashMap::new(),
            next_fdt: 1,
            next_fsi: 1,
            init_tid,
        };
        let fdt = t.new_fd_table(FdTable::new());
        let fsi = t.new_fs_info(FsInfo::new());
        t.map.insert(
            init_tid,
            Thread {
                tid: init_tid,
                tgid: init_tid,
                parent: None,
                fd_table_id: fdt,
                fs_info_id: fsi,
            },
        );
        t
    }

    pub fn init_tid(&self) -> AbsTid {
        self.init_tid
    }

    pub fn contains(&self, tid: AbsTid) -> bool {
        self.map.contains_key(&tid)
    }

    pub fn get(&self, tid: AbsTid) -> Option<&Thread> {
        self.map.get(&tid)
    }

    /// Look up a thread, discovering it (and any missing ancestors) from `/proc`
    /// on a miss. The tid comes from a seccomp notification, so it is known to
    /// be a sandbox task — orphan adoption applies.
    pub fn get_or_sync(&mut self, tid: AbsTid, proc: &dyn ProcInfo) -> Option<&Thread> {
        if !self.map.contains_key(&tid) {
            let _ = self.ensure_registered(tid, proc, true);
        }
        self.map.get(&tid)
    }

    /// The FD table backing `tid` (mutable).
    pub fn fd_table_mut(&mut self, tid: AbsTid) -> Option<&mut FdTable> {
        let id = self.map.get(&tid)?.fd_table_id;
        self.fd_tables.get_mut(&id).map(|s| &mut s.value)
    }

    /// The cwd of `tid`.
    pub fn cwd(&self, tid: AbsTid) -> Option<&str> {
        let id = self.map.get(&tid)?.fs_info_id;
        self.fs_infos.get(&id).map(|s| s.value.cwd.as_str())
    }

    /// Set the cwd of `tid` (and everything sharing its fs-info).
    pub fn set_cwd(&mut self, tid: AbsTid, cwd: &str) -> bool {
        let Some(id) = self.map.get(&tid).map(|t| t.fs_info_id) else {
            return false;
        };
        if let Some(slot) = self.fs_infos.get_mut(&id) {
            slot.value.set_cwd(cwd);
            true
        } else {
            false
        }
    }

    /// Namespaced pid (== tgid in a single namespace).
    pub fn ns_pid(&self, tid: AbsTid) -> Option<AbsTgid> {
        self.map.get(&tid).map(|t| t.tgid)
    }

    /// Translate a namespaced thread-group id to an absolute one. In the single
    /// PID namespace we model, the group leader's tgid equals its abs tid, so
    /// this looks up a live thread-group leader with that tgid.
    pub fn abs_tgid_for_ns(&self, ns_tgid: NsTgid) -> Option<AbsTgid> {
        self.map
            .values()
            .find(|t| t.tgid == ns_tgid && t.tid == t.tgid)
            .map(|t| t.tgid)
    }

    /// Translate a namespaced thread id to an absolute one (single namespace:
    /// identity if the thread is known).
    pub fn abs_tid_for_ns(&self, ns_tid: NsTid) -> Option<AbsTid> {
        self.map.get(&ns_tid).map(|t| t.tid)
    }

    /// Namespaced parent pid: the parent thread-group's id, or 0 for init / no
    /// visible parent.
    pub fn ns_ppid(&self, tid: AbsTid) -> Option<AbsTgid> {
        let t = self.map.get(&tid)?;
        Some(match t.parent {
            Some(ptid) => self.map.get(&ptid).map(|p| p.tgid).unwrap_or(0),
            None => 0,
        })
    }

    /// Snapshot the parent's fd table + fs-info at `clone` time. Because the
    /// parent blocks on the clone notification, this runs before any of the
    /// parent's subsequent fd mutations (e.g. closing pipe ends), so a lazily
    /// discovered child inherits the correct fork-time state.
    pub fn snapshot_fork(&mut self, parent_tid: AbsTid) {
        let Some(parent) = self.map.get(&parent_tid) else {
            return;
        };
        let fd_table = self.fd_tables[&parent.fd_table_id].value.deep_clone();
        let fs_info = self.fs_infos[&parent.fs_info_id].value.clone();
        let queue = self.fork_snapshots.entry(parent_tid).or_default();
        queue.push_back(ForkSnapshot { fd_table, fs_info });
        // Bound the queue: an unconsumed snapshot (a child that died before
        // its first syscall, or a clone kind that never claims one) would
        // otherwise pin its dup'd fds forever. Dropping the oldest closes them.
        while queue.len() > 8 {
            queue.pop_front();
        }
    }

    /// Register a child of `parent_tid` with the given clone flags.
    pub fn register_child(
        &mut self,
        parent_tid: AbsTid,
        child_tid: AbsTid,
        flags: CloneFlags,
    ) -> Result<(), ThreadError> {
        flags
            .check_supported()
            .map_err(|_| ThreadError::Unsupported)?;
        let parent = self.map.get(&parent_tid).ok_or(ThreadError::NotFound)?;
        let (p_fdt, p_fsi, p_tgid) = (parent.fd_table_id, parent.fs_info_id, parent.tgid);

        // A thread (CLONE_THREAD) shares its parent's group and, per Linux,
        // everything else too.
        let is_thread = flags.is_thread();
        let tgid = if is_thread { p_tgid } else { child_tid };
        let share_files = is_thread || flags.shares_files();
        let share_fs = is_thread || flags.shares_fs();

        // Consume one fork-time snapshot if either resource is being copied.
        let snapshot = if share_files && share_fs {
            None
        } else {
            self.fork_snapshots
                .get_mut(&parent_tid)
                .and_then(|q| q.pop_front())
        };
        let (snap_table, snap_fs) = match snapshot {
            Some(s) => (Some(s.fd_table), Some(s.fs_info)),
            None => (None, None),
        };

        let fd_table_id = if share_files {
            self.acquire_fd_table(p_fdt)
        } else {
            let table = snap_table.unwrap_or_else(|| self.fd_tables[&p_fdt].value.deep_clone());
            self.new_fd_table(table)
        };
        let fs_info_id = if share_fs {
            self.acquire_fs_info(p_fsi)
        } else {
            let info = snap_fs.unwrap_or_else(|| self.fs_infos[&p_fsi].value.clone());
            self.new_fs_info(info)
        };

        self.map.insert(
            child_tid,
            Thread {
                tid: child_tid,
                tgid,
                parent: Some(parent_tid),
                fd_table_id,
                fs_info_id,
            },
        );
        Ok(())
    }

    /// Scan `/proc` and register any not-yet-known tasks.
    pub fn sync_new_threads(&mut self, proc: &dyn ProcInfo) {
        for tid in proc.list_tids() {
            if !self.map.contains_key(&tid) {
                // These tids come from a full /proc scan, not from a seccomp
                // notification, so they include every process on the host (the
                // supervisor, other sessions' guests, kernel threads). No
                // orphan adoption: only tids whose ancestor chain reaches a
                // known sandbox thread may register, or the whole host process
                // tree leaks into the sandbox's /proc view.
                let _ = self.ensure_registered(tid, proc, false);
            }
        }
    }

    /// Register `tid`, registering ancestors first. Recursion stops at an
    /// already-known thread (e.g. the init thread). With `adopt`, a tid whose
    /// ancestry can't be established is adopted into the sandbox — valid only
    /// when the tid is known to carry our seccomp filter (it notified).
    fn ensure_registered(
        &mut self,
        tid: AbsTid,
        proc: &dyn ProcInfo,
        adopt: bool,
    ) -> Result<(), ThreadError> {
        if self.map.contains_key(&tid) {
            return Ok(());
        }
        let status = proc.status(tid).ok_or(ThreadError::NotFound)?;
        let parent_tid = status.ptid;
        let parent_known = self.map.contains_key(&parent_tid)
            || (parent_tid > 1 && self.ensure_registered(parent_tid, proc, false).is_ok());
        if !parent_known {
            if !adopt {
                return Err(ThreadError::NotInSandbox);
            }
            // The notifying tid carries our seccomp filter, so it IS a sandbox
            // task — its parent just exited before the tid's first syscall and
            // the host reaper adopted it. Refusing it would hand the orphan
            // ESRCH on every syscall forever (shared-library loads fail with
            // "No such process", bash job control breaks). Adopt it: as a
            // thread of its group if the leader is known, else under the init
            // guest with a fresh table (its inherited real fds keep working
            // untracked via reply_continue).
            if status.tgid != tid && self.map.contains_key(&status.tgid) {
                return self.register_child(
                    status.tgid,
                    tid,
                    CloneFlags(crate::procinfo::clone::THREAD),
                );
            }
            let init = self.init_tid;
            // Don't let adoption consume a fork snapshot queued for one of
            // init's real children.
            let saved = self.fork_snapshots.remove(&init);
            let r = self.register_child(init, tid, CloneFlags(0));
            if let Some(q) = saved {
                self.fork_snapshots.insert(init, q);
            }
            return r;
        }

        // Thread of an existing group?
        let mut flags = proc.detect_clone_flags(parent_tid, tid);
        if status.tgid != tid {
            flags = CloneFlags(flags.0 | crate::procinfo::clone::THREAD);
        }
        self.register_child(parent_tid, tid, flags)
    }

    /// Remove a thread on exit; reparent its children to the init thread and
    /// release its arena references.
    pub fn handle_thread_exit(&mut self, tid: AbsTid) {
        let Some(thread) = self.map.remove(&tid) else {
            return;
        };
        self.release_fd_table(thread.fd_table_id);
        self.release_fs_info(thread.fs_info_id);
        // Drop any unclaimed fork snapshots for this parent (closes their fds).
        self.fork_snapshots.remove(&tid);
        let init = self.init_tid;
        for t in self.map.values_mut() {
            if t.parent == Some(tid) {
                t.parent = Some(init);
            }
        }
    }

    /// Remove every thread in the caller's thread group (exit_group).
    pub fn handle_group_exit(&mut self, tid: AbsTid) {
        let Some(tgid) = self.map.get(&tid).map(|t| t.tgid) else {
            return;
        };
        let victims: Vec<AbsTid> = self
            .map
            .values()
            .filter(|t| t.tgid == tgid)
            .map(|t| t.tid)
            .collect();
        for v in victims {
            self.handle_thread_exit(v);
        }
    }

    /// Number of live threads (for sysinfo / tests).
    pub fn count(&self) -> usize {
        self.map.len()
    }

    /// All namespaced thread-group ids (process pids) currently visible — the
    /// group leaders. Used to synthesize `/proc` directory listings.
    pub fn ns_tgids(&self) -> Vec<NsTgid> {
        self.map
            .values()
            .filter(|t| t.tid == t.tgid)
            .map(|t| t.tgid)
            .collect()
    }

    fn new_fd_table(&mut self, table: FdTable) -> FdTableId {
        let id = self.next_fdt;
        self.next_fdt += 1;
        self.fd_tables.insert(
            id,
            Slot {
                value: table,
                refs: 1,
            },
        );
        id
    }

    fn new_fs_info(&mut self, info: FsInfo) -> FsInfoId {
        let id = self.next_fsi;
        self.next_fsi += 1;
        self.fs_infos.insert(
            id,
            Slot {
                value: info,
                refs: 1,
            },
        );
        id
    }

    fn acquire_fd_table(&mut self, id: FdTableId) -> FdTableId {
        if let Some(slot) = self.fd_tables.get_mut(&id) {
            slot.refs += 1;
        }
        id
    }

    fn acquire_fs_info(&mut self, id: FsInfoId) -> FsInfoId {
        if let Some(slot) = self.fs_infos.get_mut(&id) {
            slot.refs += 1;
        }
        id
    }

    fn release_fd_table(&mut self, id: FdTableId) {
        if let Some(slot) = self.fd_tables.get_mut(&id) {
            slot.refs -= 1;
            if slot.refs == 0 {
                self.fd_tables.remove(&id);
            }
        }
    }

    fn release_fs_info(&mut self, id: FsInfoId) {
        if let Some(slot) = self.fs_infos.get_mut(&id) {
            slot.refs -= 1;
            if slot.refs == 0 {
                self.fs_infos.remove(&id);
            }
        }
    }

    #[cfg(test)]
    fn fd_table_refs(&self, tid: AbsTid) -> usize {
        let id = self.map[&tid].fd_table_id;
        self.fd_tables[&id].refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procinfo::{clone, MockProcInfo};

    #[test]
    fn init_thread_registered() {
        let t = Threads::new(100);
        assert!(t.contains(100));
        assert_eq!(t.ns_pid(100), Some(100));
        assert_eq!(t.ns_ppid(100), Some(0)); // init has no parent
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn register_child_new_group() {
        let mut t = Threads::new(100);
        t.register_child(100, 200, CloneFlags(0)).unwrap();
        assert_eq!(t.ns_pid(200), Some(200));
        assert_eq!(t.ns_ppid(200), Some(100));
        // Fork without CLONE_FILES: independent fd tables.
        assert_ne!(
            t.get(100).unwrap().fd_table_id,
            t.get(200).unwrap().fd_table_id
        );
        assert_eq!(t.fd_table_refs(100), 1);
        assert_eq!(t.fd_table_refs(200), 1);
    }

    #[test]
    fn clone_files_shares_fd_table() {
        let mut t = Threads::new(100);
        t.register_child(100, 200, CloneFlags(clone::FILES))
            .unwrap();
        assert_eq!(
            t.get(100).unwrap().fd_table_id,
            t.get(200).unwrap().fd_table_id
        );
        assert_eq!(t.fd_table_refs(100), 2);
    }

    #[test]
    fn clone_thread_shares_group_and_tables() {
        let mut t = Threads::new(100);
        t.register_child(100, 201, CloneFlags(clone::THREAD))
            .unwrap();
        // Same thread group (tgid) as the leader.
        assert_eq!(t.ns_pid(201), Some(100));
        assert_eq!(
            t.get(100).unwrap().fd_table_id,
            t.get(201).unwrap().fd_table_id
        );
    }

    #[test]
    fn unsupported_clone_rejected() {
        let mut t = Threads::new(100);
        assert_eq!(
            t.register_child(100, 200, CloneFlags(clone::NEWUSER)),
            Err(ThreadError::Unsupported)
        );
    }

    #[test]
    fn lazy_discovery_registers_ancestors() {
        let mut t = Threads::new(100);
        let mut proc = MockProcInfo::new();
        // 100 (init) -> 200 -> 300, none of 200/300 known yet.
        proc.add(200, 100, 200, 0);
        proc.add(300, 200, 300, 0);
        let got = t.get_or_sync(300, &proc).map(|th| th.tid);
        assert_eq!(got, Some(300));
        assert!(t.contains(200)); // ancestor registered too
        assert_eq!(t.ns_ppid(300), Some(200));
    }

    #[test]
    fn thread_exit_reparents_children_to_init() {
        let mut t = Threads::new(100);
        t.register_child(100, 200, CloneFlags(0)).unwrap();
        t.register_child(200, 300, CloneFlags(0)).unwrap();
        t.handle_thread_exit(200);
        assert!(!t.contains(200));
        assert_eq!(t.ns_ppid(300), Some(100)); // reparented to init
    }

    #[test]
    fn group_exit_removes_whole_group() {
        let mut t = Threads::new(100);
        // Two threads in group 200.
        t.register_child(100, 200, CloneFlags(0)).unwrap();
        t.register_child(200, 201, CloneFlags(clone::THREAD))
            .unwrap();
        assert_eq!(t.count(), 3);
        t.handle_group_exit(200);
        assert!(!t.contains(200));
        assert!(!t.contains(201));
        assert!(t.contains(100));
    }

    #[test]
    fn fd_table_freed_on_last_exit() {
        let mut t = Threads::new(100);
        t.register_child(100, 200, CloneFlags(clone::FILES))
            .unwrap();
        assert_eq!(t.fd_table_refs(100), 2);
        t.handle_thread_exit(200);
        assert_eq!(t.fd_table_refs(100), 1);
    }
}
