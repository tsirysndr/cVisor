//! Synthetic `/proc` files. Content is generated at open time (so we don't have
//! to track live variants) from the guest's namespaced view. Port of the core
//! of `procfile.zig` — supports `/proc`, `/proc/self`, `/proc/<N>`, and their
//! `status` files.

use crate::error::{Errno, SysError, SysResult};
use crate::procinfo::NsTgid;

/// What a `/proc` path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcTarget {
    DirProc,
    DirSelf,
    DirPid(NsTgid),
    SelfStatus,
    PidStatus(NsTgid),
}

/// Parse a normalized `/proc...` path into a target, or NOENT.
pub fn parse_proc_path(path: &str) -> SysResult<ProcTarget> {
    if path == "/proc" {
        return Ok(ProcTarget::DirProc);
    }
    let rest = path.strip_prefix("/proc/").ok_or(SysError(Errno::NOENT))?;
    if rest.is_empty() {
        return Ok(ProcTarget::DirProc);
    }
    if let Some(after) = rest.strip_prefix("self") {
        return match after {
            "" => Ok(ProcTarget::DirSelf),
            "/status" => Ok(ProcTarget::SelfStatus),
            _ => Err(SysError(Errno::NOENT)),
        };
    }
    let (id_str, sub) = match rest.split_once('/') {
        Some((id, sub)) => (id, Some(sub)),
        None => (rest, None),
    };
    let nstgid: NsTgid = id_str.parse().map_err(|_| SysError(Errno::NOENT))?;
    if nstgid <= 0 {
        return Err(SysError(Errno::NOENT));
    }
    match sub {
        None => Ok(ProcTarget::DirPid(nstgid)),
        Some("status") => Ok(ProcTarget::PidStatus(nstgid)),
        Some(_) => Err(SysError(Errno::NOENT)),
    }
}

/// Format a synthetic `status` file for a process with the given namespaced
/// pid/ppid. Name is fixed (matches the Zig behavior).
pub fn format_status(ns_pid: NsTgid, ns_ppid: NsTgid) -> Vec<u8> {
    format!("Name:\tcvisor-guest\nPid:\t{ns_pid}\nPPid:\t{ns_ppid}\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_targets() {
        assert_eq!(parse_proc_path("/proc").unwrap(), ProcTarget::DirProc);
        assert_eq!(parse_proc_path("/proc/self").unwrap(), ProcTarget::DirSelf);
        assert_eq!(
            parse_proc_path("/proc/self/status").unwrap(),
            ProcTarget::SelfStatus
        );
        assert_eq!(parse_proc_path("/proc/42").unwrap(), ProcTarget::DirPid(42));
        assert_eq!(
            parse_proc_path("/proc/42/status").unwrap(),
            ProcTarget::PidStatus(42)
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(
            parse_proc_path("/proc/self/cmdline").unwrap_err().errno(),
            Errno::NOENT
        );
        assert_eq!(
            parse_proc_path("/proc/0").unwrap_err().errno(),
            Errno::NOENT
        );
        assert_eq!(
            parse_proc_path("/proc/abc").unwrap_err().errno(),
            Errno::NOENT
        );
    }

    #[test]
    fn status_content() {
        let s = format_status(1, 0);
        assert_eq!(s, b"Name:\tcvisor-guest\nPid:\t1\nPPid:\t0\n");
    }
}
