//! `/proc`-backed `ProcInfo`: parses task status, scans for tasks, and infers
//! clone flags via `kcmp` + namespace-inode comparison. Port of the non-test
//! paths of `proc_info.zig`.

use super::{parse_status, AbsTid, CloneFlags, ProcInfo, ThreadStatus};

const KCMP_FILES: i32 = 2;

pub struct RealProcInfo;

impl ProcInfo for RealProcInfo {
    fn status(&self, tid: AbsTid) -> Option<ThreadStatus> {
        let text = std::fs::read_to_string(format!("/proc/{tid}/status")).ok()?;
        parse_status(tid, &text).ok()
    }

    fn list_tids(&self) -> Vec<AbsTid> {
        let mut tids = Vec::new();
        let Ok(procs) = std::fs::read_dir("/proc") else {
            return tids;
        };
        for proc in procs.flatten() {
            let Some(tgid) = proc
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let task_dir = format!("/proc/{tgid}/task");
            let Ok(tasks) = std::fs::read_dir(&task_dir) else {
                continue;
            };
            for task in tasks.flatten() {
                if let Some(tid) = task
                    .file_name()
                    .to_str()
                    .and_then(|s| s.parse::<i32>().ok())
                {
                    tids.push(tid);
                }
            }
        }
        tids
    }

    fn detect_clone_flags(&self, parent: AbsTid, child: AbsTid) -> CloneFlags {
        let mut flags = 0u64;
        if !same_pid_namespace(parent, child) {
            flags |= super::clone::NEWPID;
        }
        if shares_fd_table(parent, child) {
            flags |= super::clone::FILES;
        }
        CloneFlags(flags)
    }
}

/// Compare `/proc/<tid>/ns/pid` inodes; unknown → assume same namespace.
fn same_pid_namespace(a: AbsTid, b: AbsTid) -> bool {
    match (ns_inode(a, "pid"), ns_inode(b, "pid")) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

fn ns_inode(tid: AbsTid, kind: &str) -> Option<u64> {
    std::fs::metadata(format!("/proc/{tid}/ns/{kind}"))
        .ok()
        .map(|m| {
            use std::os::linux::fs::MetadataExt;
            m.st_ino()
        })
}

/// `kcmp(tid1, tid2, KCMP_FILES)` == 0 means they share an fd table.
fn shares_fd_table(a: AbsTid, b: AbsTid) -> bool {
    // SAFETY: kcmp with two tids and the FILES comparison type.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_kcmp,
            a as libc::c_long,
            b as libc::c_long,
            KCMP_FILES as libc::c_long,
            0,
            0,
        )
    };
    rc == 0
}
