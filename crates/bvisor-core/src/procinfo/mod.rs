//! Kernel-truth queries about tasks, reconstructed from `/proc` (clone is
//! passthrough, so the process tree is discovered lazily).
//!
//! Milestone 1 ports the pure `/proc/[tid]/status` text parsing. The Linux-side
//! directory scans, `kcmp`, and namespace-inode comparison land with the
//! process model (behind `cfg(target_os = "linux")`).

/// Absolute (kernel) thread id.
pub type AbsTid = i32;
/// Absolute (kernel) thread-group id (a PID).
pub type AbsTgid = i32;
/// Namespaced thread id (what the guest sees).
pub type NsTid = i32;
/// Namespaced thread-group id.
pub type NsTgid = i32;

/// Parsed subset of `/proc/[tid]/status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadStatus {
    pub tid: AbsTid,
    pub tgid: AbsTgid,
    pub ptid: AbsTid,
    /// NStgid chain, outermost first.
    pub nstgids: Vec<NsTgid>,
    /// NSpid chain, outermost first.
    pub nstids: Vec<NsTid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusParseError {
    MissingTgid,
    MissingPpid,
    MissingNStgid,
    MissingNSpid,
    BadNumber,
}

/// Parse the Tgid/PPid/NStgid/NSpid fields from `/proc/[tid]/status` text.
pub fn parse_status(tid: AbsTid, text: &str) -> Result<ThreadStatus, StatusParseError> {
    let mut tgid: Option<AbsTgid> = None;
    let mut ptid: Option<AbsTid> = None;
    let mut nstgids: Vec<NsTgid> = Vec::new();
    let mut nstids: Vec<NsTid> = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Tgid:") {
            tgid = Some(
                rest.trim()
                    .parse()
                    .map_err(|_| StatusParseError::BadNumber)?,
            );
        } else if let Some(rest) = line.strip_prefix("PPid:") {
            ptid = Some(
                rest.trim()
                    .parse()
                    .map_err(|_| StatusParseError::BadNumber)?,
            );
        } else if let Some(rest) = line.strip_prefix("NStgid:") {
            nstgids = parse_id_list(rest)?;
        } else if let Some(rest) = line.strip_prefix("NSpid:") {
            nstids = parse_id_list(rest)?;
        }

        if tgid.is_some() && ptid.is_some() && !nstgids.is_empty() && !nstids.is_empty() {
            break;
        }
    }

    Ok(ThreadStatus {
        tid,
        tgid: tgid.ok_or(StatusParseError::MissingTgid)?,
        ptid: ptid.ok_or(StatusParseError::MissingPpid)?,
        nstgids: {
            if nstgids.is_empty() {
                return Err(StatusParseError::MissingNStgid);
            }
            nstgids
        },
        nstids: {
            if nstids.is_empty() {
                return Err(StatusParseError::MissingNSpid);
            }
            nstids
        },
    })
}

/// Parse whitespace-separated namespace id fields.
pub fn parse_id_list(field: &str) -> Result<Vec<i32>, StatusParseError> {
    field
        .split_whitespace()
        .map(|s| s.parse::<i32>().map_err(|_| StatusParseError::BadNumber))
        .collect()
}

/// Linux `CLONE_*` flags relevant to the process model.
pub mod clone {
    pub const FS: u64 = 0x0000_0200;
    pub const FILES: u64 = 0x0000_0400;
    pub const THREAD: u64 = 0x0001_0000;
    pub const NEWNS: u64 = 0x0002_0000;
    pub const NEWPID: u64 = 0x2000_0000;
    pub const NEWUSER: u64 = 0x1000_0000;
    pub const NEWNET: u64 = 0x4000_0000;
}

/// Wrapper over raw clone flags with the predicates the model needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloneFlags(pub u64);

impl CloneFlags {
    /// Reject namespace kinds bVisor does not virtualize.
    pub fn check_supported(&self) -> Result<(), UnsupportedClone> {
        if self.0 & (clone::NEWUSER | clone::NEWNET | clone::NEWNS) != 0 {
            Err(UnsupportedClone)
        } else {
            Ok(())
        }
    }
    pub fn creates_pid_namespace(&self) -> bool {
        self.0 & clone::NEWPID != 0
    }
    pub fn is_thread(&self) -> bool {
        self.0 & clone::THREAD != 0
    }
    pub fn shares_files(&self) -> bool {
        self.0 & clone::FILES != 0
    }
    pub fn shares_fs(&self) -> bool {
        self.0 & clone::FS != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedClone;

/// Kernel-truth queries about tasks. `RealProcInfo` reads `/proc`; tests inject
/// a `MockProcInfo`.
pub trait ProcInfo: Send + Sync {
    /// Parsed `/proc/<tid>/status`, or None if the task is gone.
    fn status(&self, tid: AbsTid) -> Option<ThreadStatus>;
    /// All sandboxed task ids currently visible.
    fn list_tids(&self) -> Vec<AbsTid>;
    /// Infer the clone flags a child was created with (post-hoc, since clone is
    /// passthrough).
    fn detect_clone_flags(&self, parent: AbsTid, child: AbsTid) -> CloneFlags;
}

/// In-memory `ProcInfo` for tests (no `/proc`). Populate before use.
#[derive(Default)]
pub struct MockProcInfo {
    pub ppid: std::collections::HashMap<AbsTid, AbsTid>,
    pub tgid: std::collections::HashMap<AbsTid, AbsTgid>,
    pub flags: std::collections::HashMap<AbsTid, u64>,
}

impl MockProcInfo {
    pub fn new() -> MockProcInfo {
        MockProcInfo::default()
    }

    /// Register a task with its parent tid, group id, and clone flags.
    pub fn add(&mut self, tid: AbsTid, ppid: AbsTid, tgid: AbsTgid, flags: u64) {
        self.ppid.insert(tid, ppid);
        self.tgid.insert(tid, tgid);
        self.flags.insert(tid, flags);
    }
}

impl ProcInfo for MockProcInfo {
    fn status(&self, tid: AbsTid) -> Option<ThreadStatus> {
        let ptid = *self.ppid.get(&tid)?;
        let tgid = *self.tgid.get(&tid).unwrap_or(&tid);
        Some(ThreadStatus {
            tid,
            tgid,
            ptid,
            nstgids: vec![tgid],
            nstids: vec![tid],
        })
    }

    fn list_tids(&self) -> Vec<AbsTid> {
        self.ppid.keys().copied().collect()
    }

    fn detect_clone_flags(&self, _parent: AbsTid, child: AbsTid) -> CloneFlags {
        CloneFlags(self.flags.get(&child).copied().unwrap_or(0))
    }
}

#[cfg(target_os = "linux")]
pub mod real;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_namespace_status() {
        let text = "Name:\tsh\nTgid:\t100\nPid:\t100\nPPid:\t42\nNStgid:\t100\nNSpid:\t100\n";
        let s = parse_status(100, text).unwrap();
        assert_eq!(s.tid, 100);
        assert_eq!(s.tgid, 100);
        assert_eq!(s.ptid, 42);
        assert_eq!(s.nstgids, vec![100]);
        assert_eq!(s.nstids, vec![100]);
    }

    #[test]
    fn parses_nested_namespace_chain() {
        let text = "Tgid:\t500\nPPid:\t7\nNStgid:\t500\t3\nNSpid:\t500\t3\n";
        let s = parse_status(500, text).unwrap();
        assert_eq!(s.nstgids, vec![500, 3]);
        assert_eq!(s.nstids, vec![500, 3]);
    }

    #[test]
    fn missing_field_errors() {
        let text = "Tgid:\t100\nPPid:\t1\nNStgid:\t100\n"; // no NSpid
        assert_eq!(parse_status(100, text), Err(StatusParseError::MissingNSpid));
    }

    #[test]
    fn bad_number_errors() {
        let text = "Tgid:\tnope\n";
        assert_eq!(parse_status(1, text), Err(StatusParseError::BadNumber));
    }
}
