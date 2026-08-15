//! Resource limits for the guest via cgroup v2.
//!
//! When limits are requested we create a leaf cgroup under the unified
//! hierarchy root (`/sys/fs/cgroup`), write `memory.max` / `pids.max` /
//! `cpu.max`, and move the guest process into it — its descendants inherit the
//! cgroup, so the whole guest tree is capped. On exit the cgroup is killed and
//! removed.
//!
//! cgroup v2 must be mounted and writable (typically a privileged or
//! cgroup-delegated environment). If it isn't, [`Cgroup::apply`] degrades
//! gracefully: it prints one warning and the run proceeds unlimited.

use std::path::{Path, PathBuf};

/// Per-run resource limits. All optional; `None` means "no limit".
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Hard memory cap in bytes (`memory.max`) — the guest is OOM-killed past it.
    pub memory_max: Option<u64>,
    /// Max number of processes/threads in the guest tree (`pids.max`).
    pub pids_max: Option<u64>,
    /// CPU cap as a percentage of one core (`cpu.max`): 50 = half a core,
    /// 200 = two cores.
    pub cpu_percent: Option<u32>,
}

impl Limits {
    pub fn is_empty(&self) -> bool {
        self.memory_max.is_none() && self.pids_max.is_none() && self.cpu_percent.is_none()
    }
}

/// Parse a human byte size like `512`, `256m`, `1g`, `2G`, `128k` into bytes.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last().unwrap().to_ascii_lowercase() {
        'k' => (&s[..s.len() - 1], 1024u64),
        'm' => (&s[..s.len() - 1], 1024 * 1024),
        'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        't' => (&s[..s.len() - 1], 1024u64.pow(4)),
        'b' => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().ok().map(|n| n * mult)
}

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const CPU_PERIOD_US: u64 = 100_000;

/// A leaf cgroup owning the guest; killed and removed on drop.
pub struct Cgroup {
    dir: PathBuf,
}

impl Cgroup {
    /// Create + configure a cgroup for `limits`, named by `key` (unique per
    /// run, e.g. the guest pid). Returns None when no limits are requested, or
    /// (after one warning) when cgroup v2 is unavailable/unwritable — the caller
    /// then runs unlimited.
    pub fn apply(key: &str, limits: &Limits) -> Option<Cgroup> {
        if limits.is_empty() {
            return None;
        }
        match Self::try_apply(key, limits) {
            Ok(cg) => Some(cg),
            Err(e) => {
                eprintln!("cvisor: resource limits not applied ({e}); running unlimited");
                None
            }
        }
    }

    fn try_apply(key: &str, limits: &Limits) -> Result<Cgroup, String> {
        let root = Path::new(CGROUP_ROOT);
        // cgroup v2 has a `cgroup.controllers` file at the root.
        let controllers = std::fs::read_to_string(root.join("cgroup.controllers"))
            .map_err(|_| "cgroup v2 not mounted".to_string())?;

        // Enable the controllers we need on the root's subtree so leaf cgroups
        // expose memory.max/pids.max/cpu.max. Root is exempt from the
        // no-internal-process rule, so this is allowed even with tasks in root.
        let want: Vec<&str> = ["memory", "pids", "cpu"]
            .into_iter()
            .filter(|c| controllers.split_whitespace().any(|h| h == *c))
            .collect();
        if !want.is_empty() {
            let enable: String = want.iter().map(|c| format!("+{c} ")).collect();
            // Best-effort: may already be enabled, or root may be delegated.
            let _ = std::fs::write(root.join("cgroup.subtree_control"), enable.trim());
        }

        let dir = root.join(format!("cvisor-{key}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        let cg = Cgroup { dir };

        if let Some(bytes) = limits.memory_max {
            cg.write("memory.max", &bytes.to_string())?;
            // Refuse to swap out to dodge the RAM cap.
            let _ = cg.write_soft("memory.swap.max", "0");
        }
        if let Some(pids) = limits.pids_max {
            cg.write("pids.max", &pids.to_string())?;
        }
        if let Some(pct) = limits.cpu_percent {
            let quota = (pct as u64) * CPU_PERIOD_US / 100;
            cg.write("cpu.max", &format!("{quota} {CPU_PERIOD_US}"))?;
        }
        Ok(cg)
    }

    fn write(&self, file: &str, val: &str) -> Result<(), String> {
        std::fs::write(self.dir.join(file), val).map_err(|e| format!("write {file}: {e}"))
    }

    fn write_soft(&self, file: &str, val: &str) -> std::io::Result<()> {
        std::fs::write(self.dir.join(file), val)
    }

    /// Move `pid` (and thus its future descendants) into the cgroup.
    pub fn attach(&self, pid: i32) {
        if let Err(e) = self.write_soft("cgroup.procs", &pid.to_string()) {
            eprintln!("cvisor: could not attach guest to cgroup: {e}");
        }
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // Kill any stragglers, then remove the (now empty) cgroup. A freshly
        // emptied cgroup may take a moment before rmdir succeeds.
        let _ = std::fs::write(self.dir.join("cgroup.kill"), "1");
        for _ in 0..50 {
            if std::fs::remove_dir(&self.dir).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sizes() {
        assert_eq!(parse_size("512"), Some(512));
        assert_eq!(parse_size("256m"), Some(256 * 1024 * 1024));
        assert_eq!(parse_size("1g"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("2G"), Some(2u64 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("128k"), Some(128 * 1024));
        assert_eq!(parse_size("100b"), Some(100));
        assert_eq!(parse_size("bad"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn limits_empty() {
        assert!(Limits::default().is_empty());
        assert!(!Limits {
            memory_max: Some(1),
            ..Default::default()
        }
        .is_empty());
    }
}
