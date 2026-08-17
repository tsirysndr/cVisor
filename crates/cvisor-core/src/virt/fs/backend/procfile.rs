//! Synthetic `/proc` files. Content is generated at open time (so we don't have
//! to track live variants) from the guest's namespaced view. Covers what `ps`,
//! `top`, and `free` read: the pid dirs with `status`/`stat`/`cmdline`, plus
//! `version`, `uptime`, `loadavg`, `meminfo`, and the global `stat`.

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
    SelfStat,
    PidStat(NsTgid),
    SelfCmdline,
    PidCmdline(NsTgid),
    Version,
    Uptime,
    Loadavg,
    Meminfo,
    StatGlobal,
}

/// The synthesized files inside a pid directory.
pub const PID_FILES: &[&str] = &["status", "stat", "cmdline"];

/// The synthesized top-level `/proc` files.
pub const TOP_FILES: &[&str] = &["version", "uptime", "loadavg", "meminfo", "stat"];

/// Parse a normalized `/proc...` path into a target, or NOENT.
pub fn parse_proc_path(path: &str) -> SysResult<ProcTarget> {
    if path == "/proc" {
        return Ok(ProcTarget::DirProc);
    }
    let rest = path.strip_prefix("/proc/").ok_or(SysError(Errno::NOENT))?;
    if rest.is_empty() {
        return Ok(ProcTarget::DirProc);
    }
    match rest {
        "version" => return Ok(ProcTarget::Version),
        "uptime" => return Ok(ProcTarget::Uptime),
        "loadavg" => return Ok(ProcTarget::Loadavg),
        "meminfo" => return Ok(ProcTarget::Meminfo),
        "stat" => return Ok(ProcTarget::StatGlobal),
        _ => {}
    }
    if let Some(after) = rest.strip_prefix("self") {
        return match after {
            "" => Ok(ProcTarget::DirSelf),
            "/status" => Ok(ProcTarget::SelfStatus),
            "/stat" => Ok(ProcTarget::SelfStat),
            "/cmdline" => Ok(ProcTarget::SelfCmdline),
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
        Some("stat") => Ok(ProcTarget::PidStat(nstgid)),
        Some("cmdline") => Ok(ProcTarget::PidCmdline(nstgid)),
        Some(_) => Err(SysError(Errno::NOENT)),
    }
}

/// Format a synthetic `status` file for a process with the given namespaced
/// pid/ppid. Fixed name and root ids; enough for `ps`'s user column.
pub fn format_status(ns_pid: NsTgid, ns_ppid: NsTgid) -> Vec<u8> {
    format!(
        "Name:\tcvisor-guest\n\
         State:\tS (sleeping)\n\
         Tgid:\t{ns_pid}\n\
         Pid:\t{ns_pid}\n\
         PPid:\t{ns_ppid}\n\
         Uid:\t0\t0\t0\t0\n\
         Gid:\t0\t0\t0\t0\n\
         Threads:\t1\n"
    )
    .into_bytes()
}

/// `/proc/version`: the check `ps` gates on (`stat("/proc/version")`).
pub fn format_version(release: &str) -> Vec<u8> {
    format!("Linux version {release} (cvisor) #1 SMP\n").into_bytes()
}

/// `/proc/uptime`: seconds since sandbox start (idle mirrors uptime).
pub fn format_uptime(secs: f64) -> Vec<u8> {
    format!("{secs:.2} {secs:.2}\n").into_bytes()
}

/// `/proc/loadavg`: zero load, the sandboxed process count, a plausible last pid.
pub fn format_loadavg(nprocs: usize, last_pid: i32) -> Vec<u8> {
    format!("0.00 0.00 0.00 1/{nprocs} {last_pid}\n").into_bytes()
}

/// `/proc/meminfo` (kB values): the fields `top`/`free` parse.
pub fn format_meminfo(
    total_kb: u64,
    free_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
) -> Vec<u8> {
    format!(
        "MemTotal:       {total_kb} kB\n\
         MemFree:        {free_kb} kB\n\
         MemAvailable:   {free_kb} kB\n\
         Buffers:        0 kB\n\
         Cached:         0 kB\n\
         SwapTotal:      {swap_total_kb} kB\n\
         SwapFree:       {swap_free_kb} kB\n\
         Shmem:          0 kB\n\
         SReclaimable:   0 kB\n"
    )
    .into_bytes()
}

/// Global `/proc/stat`: zeroed cpu counters plus the boot time `ps`/`top` use
/// to compute process start times.
pub fn format_stat_global(btime: u64, nprocs: usize) -> Vec<u8> {
    format!(
        "cpu  0 0 0 0 0 0 0 0 0 0\n\
         cpu0 0 0 0 0 0 0 0 0 0 0\n\
         btime {btime}\n\
         processes {nprocs}\n\
         procs_running 1\n\
         procs_blocked 0\n"
    )
    .into_bytes()
}

/// Rewrite a real `/proc/<pid>/stat` line into the guest's namespaced view:
/// pid, ppid, pgrp, and session are replaced (0 when unmapped), everything
/// else — comm, state, times, rss — passes through. Returns None if the line
/// doesn't have the expected shape.
pub fn rewrite_pid_stat(
    raw: &str,
    ns_pid: NsTgid,
    ns_ppid: NsTgid,
    ns_pgrp: Option<NsTgid>,
    ns_sid: Option<NsTgid>,
) -> Option<Vec<u8>> {
    // comm may contain spaces and ')': split at the LAST ')'.
    let open = raw.find('(')?;
    let close = raw.rfind(')')?;
    let comm = &raw[open..=close];
    let rest = raw[close + 1..].trim_start();
    let mut fields: Vec<&str> = rest.split_ascii_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let ns_ppid_s = ns_ppid.to_string();
    let ns_pgrp_s = ns_pgrp.unwrap_or(0).to_string();
    let ns_sid_s = ns_sid.unwrap_or(0).to_string();
    fields[1] = &ns_ppid_s; // after state: ppid pgrp session ...
    fields[2] = &ns_pgrp_s;
    fields[3] = &ns_sid_s;
    Some(format!("{ns_pid} {comm} {}\n", fields.join(" ")).into_bytes())
}

/// A fully synthetic `/proc/<pid>/stat` fallback (52 fields, mostly zero) for
/// when the real file can't be read.
pub fn synth_pid_stat(ns_pid: NsTgid, ns_ppid: NsTgid) -> Vec<u8> {
    let zeros = ["0"; 44].join(" ");
    format!("{ns_pid} (cvisor-guest) S {ns_ppid} {ns_pid} {ns_pid} 0 -1 {zeros}\n").into_bytes()
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
        assert_eq!(
            parse_proc_path("/proc/self/stat").unwrap(),
            ProcTarget::SelfStat
        );
        assert_eq!(
            parse_proc_path("/proc/self/cmdline").unwrap(),
            ProcTarget::SelfCmdline
        );
        assert_eq!(parse_proc_path("/proc/42").unwrap(), ProcTarget::DirPid(42));
        assert_eq!(
            parse_proc_path("/proc/42/status").unwrap(),
            ProcTarget::PidStatus(42)
        );
        assert_eq!(
            parse_proc_path("/proc/42/stat").unwrap(),
            ProcTarget::PidStat(42)
        );
        assert_eq!(
            parse_proc_path("/proc/42/cmdline").unwrap(),
            ProcTarget::PidCmdline(42)
        );
        assert_eq!(
            parse_proc_path("/proc/version").unwrap(),
            ProcTarget::Version
        );
        assert_eq!(parse_proc_path("/proc/uptime").unwrap(), ProcTarget::Uptime);
        assert_eq!(
            parse_proc_path("/proc/loadavg").unwrap(),
            ProcTarget::Loadavg
        );
        assert_eq!(
            parse_proc_path("/proc/meminfo").unwrap(),
            ProcTarget::Meminfo
        );
        assert_eq!(
            parse_proc_path("/proc/stat").unwrap(),
            ProcTarget::StatGlobal
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(
            parse_proc_path("/proc/self/maps").unwrap_err().errno(),
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
        let s = String::from_utf8(format_status(1, 0)).unwrap();
        assert!(s.contains("Name:\tcvisor-guest\n"));
        assert!(s.contains("Pid:\t1\n"));
        assert!(s.contains("PPid:\t0\n"));
        assert!(s.contains("Uid:\t0\t0\t0\t0\n"));
    }

    #[test]
    fn stat_rewrite_maps_ids_and_keeps_comm() {
        // comm with a space and a ')' inside, as the kernel allows.
        let raw = "4321 (weird) proc) R 100 200 300 0 -1 4194304 1 2 3 4 5 6 7 8 20 0 1 0 99 1000 10 18446744073709551615";
        let out = rewrite_pid_stat(raw, 7, 1, Some(7), Some(7)).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("7 (weird) proc) R 1 7 7 0 -1 "), "{s}");
        assert!(s.contains(" 99 1000 10 "), "times/rss preserved: {s}");
    }

    #[test]
    fn synth_stat_has_enough_fields() {
        let s = String::from_utf8(synth_pid_stat(3, 1)).unwrap();
        // procps reads up to field 52; count whitespace-split tokens (comm is
        // one token here since the synthetic name has no spaces).
        assert!(s.split_ascii_whitespace().count() >= 50, "{s}");
    }
}
