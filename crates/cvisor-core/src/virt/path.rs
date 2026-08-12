//! Path routing: normalize (resolve `..`/`.`) then match against a prefix tree
//! to decide which backend handles a path, or whether it is blocked.
//!
//! Rules (first match wins, exact- or `/`-boundary prefix):
//!   /sys, /run, /.b, most of /dev, /tmp/.cvisor      -> block (EPERM/EACCES)
//!   /dev/{null,zero,random,urandom}                  -> passthrough
//!   /proc                                            -> proc backend
//!   /tmp/*                                           -> tmp backend
//!   everything else (incl. /)                        -> cow backend

use crate::error::{Errno, SysResult};

/// Which file backend serves a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Passthrough,
    Cow,
    Tmp,
    Proc,
    Event,
}

/// Routing outcome for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteResult {
    /// Deny access.
    Block,
    /// Serve via the given backend.
    Handle(BackendType),
}

/// A resolved route: the backend plus the normalized guest path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedRoute {
    Block,
    Handle {
        backend: BackendType,
        normalized: String,
    },
}

/// Max normalized path length (matches the Zig 512-byte resolution buffer).
const PATH_MAX: usize = 512;

/// Normalize a POSIX path (resolving `.`/`..`, collapsing slashes). If `path`
/// is absolute, `cwd` is ignored; otherwise the two are joined. Over-long
/// results return `NOMEM` (mirroring the fixed-buffer failure in Zig).
fn normalize(cwd: &str, path: &str) -> SysResult<String> {
    fn feed<'a>(stack: &mut Vec<&'a str>, s: &'a str) {
        for seg in s.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    stack.pop();
                }
                other => stack.push(other),
            }
        }
    }

    let mut stack: Vec<&str> = Vec::new();
    if path.starts_with('/') {
        feed(&mut stack, path);
    } else {
        feed(&mut stack, cwd);
        feed(&mut stack, path);
    }

    let normalized = if stack.is_empty() {
        "/".to_string()
    } else {
        let mut s = String::with_capacity(stack.iter().map(|p| p.len() + 1).sum());
        for part in &stack {
            s.push('/');
            s.push_str(part);
        }
        s
    };

    if normalized.len() > PATH_MAX {
        return Err(Errno::NOMEM.into());
    }
    Ok(normalized)
}

/// Return the remainder after `prefix` if `path` matches it at a `/` boundary
/// (or exactly), else `None`. So `/tmpfoo` does not match `/tmp`.
fn matches_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() {
        Some("")
    } else if let Some(stripped) = rest.strip_prefix('/') {
        Some(stripped)
    } else {
        None
    }
}

/// Route an already-provided path, normalizing it first.
pub fn route(path: &str) -> SysResult<RouteResult> {
    let normalized = normalize("", path)?;
    Ok(route_normalized(&normalized))
}

/// Resolve `path` against `cwd`, then route. Returns the normalized path with
/// the backend so callers do not have to normalize twice.
pub fn resolve_and_route(cwd: &str, path: &str) -> SysResult<ResolvedRoute> {
    let normalized = normalize(cwd, path)?;
    Ok(match route_normalized(&normalized) {
        RouteResult::Block => ResolvedRoute::Block,
        RouteResult::Handle(backend) => ResolvedRoute::Handle {
            backend,
            normalized,
        },
    })
}

/// Route a path that is already normalized.
fn route_normalized(path: &str) -> RouteResult {
    use BackendType::*;
    use RouteResult::*;

    // Hard blocks.
    for prefix in ["/sys", "/run", "/.b"] {
        if matches_prefix(path, prefix).is_some() {
            return Block;
        }
    }

    // /dev: only the four safe devices pass through; everything else blocks.
    if let Some(rest) = matches_prefix(path, "/dev") {
        return match rest {
            "null" | "zero" | "random" | "urandom" => Handle(Passthrough),
            _ => Block,
        };
    }

    // /proc is fully virtualized.
    if matches_prefix(path, "/proc").is_some() {
        return Handle(Proc);
    }

    // /tmp: block the sandbox's own data dir, redirect the rest to the tmp overlay.
    if let Some(rest) = matches_prefix(path, "/tmp") {
        if rest == ".cvisor" || rest.starts_with(".cvisor/") {
            return Block;
        }
        return Handle(Tmp);
    }

    // Global default.
    Handle(Cow)
}

#[cfg(test)]
mod tests {
    use super::BackendType::*;
    use super::RouteResult::*;
    use super::*;

    fn r(p: &str) -> RouteResult {
        route(p).unwrap()
    }

    #[test]
    fn defaults_to_cow() {
        assert_eq!(r("/etc/passwd"), Handle(Cow));
        assert_eq!(r("/usr/bin/ls"), Handle(Cow));
        assert_eq!(r("/home/user/file.txt"), Handle(Cow));
        assert_eq!(r("/"), Handle(Cow));
    }

    #[test]
    fn proc_routes_to_proc() {
        assert_eq!(r("/proc"), Handle(Proc));
        assert_eq!(r("/proc/self"), Handle(Proc));
        assert_eq!(r("/proc/123/status"), Handle(Proc));
    }

    #[test]
    fn tmp_routes_to_tmp() {
        assert_eq!(r("/tmp/foo.txt"), Handle(Tmp));
        assert_eq!(r("/tmp/subdir/nested/file"), Handle(Tmp));
    }

    #[test]
    fn cvisor_dir_blocked() {
        assert_eq!(r("/tmp/.cvisor"), Block);
        assert_eq!(r("/tmp/.cvisor/secret"), Block);
        assert_eq!(r("/tmp/.cvisor/sb/uid/cow/etc/passwd"), Block);
    }

    #[test]
    fn hard_blocks() {
        assert_eq!(r("/sys"), Block);
        assert_eq!(r("/sys/class/net"), Block);
        assert_eq!(r("/run"), Block);
        assert_eq!(r("/run/lock"), Block);
        assert_eq!(r("/.b"), Block);
        assert_eq!(r("/.b/000"), Block);
    }

    #[test]
    fn dev_allowlist() {
        assert_eq!(r("/dev/null"), Handle(Passthrough));
        assert_eq!(r("/dev/zero"), Handle(Passthrough));
        assert_eq!(r("/dev/random"), Handle(Passthrough));
        assert_eq!(r("/dev/urandom"), Handle(Passthrough));
        assert_eq!(r("/dev/sda"), Block);
        assert_eq!(r("/dev/sdb"), Block);
        assert_eq!(r("/dev/tty"), Block);
        assert_eq!(r("/dev/mem"), Block);
    }

    #[test]
    fn prefix_boundaries() {
        assert_eq!(r("/tmpfoo"), Handle(Cow));
        assert_eq!(r("/system/file"), Handle(Cow));
        assert_eq!(r("/devnull"), Handle(Cow));
        assert_eq!(r("/running/file"), Handle(Cow));
    }

    #[test]
    fn path_traversal_normalized_before_routing() {
        assert_eq!(r("/../etc/passwd"), Handle(Cow));
        assert_eq!(r("/tmp/../etc/passwd"), Handle(Cow));
        assert_eq!(r("/proc/../sys/class/net"), Block);
        assert_eq!(r("/dev/null/../zero"), Handle(Passthrough));
        assert_eq!(r("/dev/null/../../etc/passwd"), Handle(Cow));
        assert_eq!(r("/tmp/.cvisor/../foo.txt"), Handle(Tmp));
        assert_eq!(r("/a/b/c/../../d/../e"), Handle(Cow));
    }

    #[test]
    fn long_path_within_buffer_ok() {
        let mut p = String::from("/");
        p.push_str(&"a".repeat(249));
        assert_eq!(route(&p).unwrap(), Handle(Cow));
    }

    #[test]
    fn over_long_path_errors() {
        let mut p = String::from("/");
        p.push_str(&"a".repeat(599));
        assert_eq!(route(&p).unwrap_err().errno(), Errno::NOMEM);
    }

    #[test]
    fn resolve_and_route_joins_cwd() {
        match resolve_and_route("/tmp", "foo.txt").unwrap() {
            ResolvedRoute::Handle {
                backend,
                normalized,
            } => {
                assert_eq!(backend, Tmp);
                assert_eq!(normalized, "/tmp/foo.txt");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn resolve_and_route_absolute_ignores_cwd() {
        match resolve_and_route("/tmp", "/etc/passwd").unwrap() {
            ResolvedRoute::Handle {
                backend,
                normalized,
            } => {
                assert_eq!(backend, Cow);
                assert_eq!(normalized, "/etc/passwd");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
