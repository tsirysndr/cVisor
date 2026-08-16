//! The daemon's sandbox registry and the core-facing operations that both the
//! gRPC and GraphQL layers delegate to. All methods are blocking (the runtime
//! forks + supervises synchronously); callers run them on `spawn_blocking`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cvisor_core::cgroup::Limits;
use cvisor_core::{
    cache, cleanup_overlay, copy_into, copy_out_of, execute_with, generate_uid, read_file,
    shell_argv, spawn_session, write_file, ExecOpts, Format, LogBuffer, LogLevel, PtyMode, Session,
};

use crate::names::random_name;

/// A daemon-side error; the transports map it to a gRPC `Status` / GraphQL error.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn oops<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

/// Public view of a sandbox's identity and configuration.
#[derive(Clone)]
pub struct SandboxInfo {
    pub id: String,
    pub name: String,
    pub allow_network: bool,
    pub allow_listen: bool,
    pub env: Vec<(String, String)>,
    pub limits: Limits,
}

/// Per-run configuration overrides for an ephemeral (one-shot) run.
#[derive(Default)]
pub struct Overrides {
    pub allow_network: Option<bool>,
    pub limits: Option<Limits>,
    pub env: Vec<(String, String)>,
}

pub struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

pub struct CacheEntry {
    pub name: String,
    pub size: u64,
}

struct SandboxState {
    id: String,
    name: String,
    uid: [u8; 16],
    allow_network: bool,
    allow_listen: bool,
    env: Vec<(String, String)>,
    limits: Limits,
}

impl SandboxState {
    fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            allow_network: self.allow_network,
            allow_listen: self.allow_listen,
            env: self.env.clone(),
            limits: self.limits.clone(),
        }
    }
}

struct SessionEntry {
    session: Arc<Session>,
    uid: [u8; 16],
    ephemeral: bool,
}

pub struct Registry {
    sandboxes: Mutex<HashMap<String, SandboxState>>,
    sessions: Mutex<HashMap<String, SessionEntry>>,
}

fn short_id() -> String {
    let mut b = [0u8; 6];
    getrandom::fill(&mut b).expect("getrandom failed");
    hex::encode(b)
}

impl Registry {
    pub fn new() -> Arc<Registry> {
        Arc::new(Registry {
            sandboxes: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Create a sandbox. `name` empty → a random docker-style name; a name that
    /// collides gets a numeric suffix (`nervous_einstein2`, ...).
    pub fn create_sandbox(&self, name: Option<&str>) -> SandboxInfo {
        let mut map = self.sandboxes.lock().unwrap();
        let base = match name {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => random_name(),
        };
        let name_taken =
            |m: &HashMap<String, SandboxState>, n: &str| m.values().any(|s| s.name == n);
        let name = if name_taken(&map, &base) {
            let mut n = 2;
            loop {
                let cand = format!("{base}{n}");
                if !name_taken(&map, &cand) {
                    break cand;
                }
                n += 1;
            }
        } else {
            base
        };
        let mut id = short_id();
        while map.contains_key(&id) {
            id = short_id();
        }
        let st = SandboxState {
            id: id.clone(),
            name,
            uid: generate_uid(),
            allow_network: true,
            allow_listen: false,
            env: Vec::new(),
            limits: Limits::default(),
        };
        let info = st.info();
        map.insert(id, st);
        info
    }

    pub fn list_sandboxes(&self) -> Vec<SandboxInfo> {
        self.sandboxes
            .lock()
            .unwrap()
            .values()
            .map(|s| s.info())
            .collect()
    }

    /// Resolve a reference (id or name) to a sandbox id.
    fn resolve(&self, r: &str) -> Result<String> {
        let map = self.sandboxes.lock().unwrap();
        if map.contains_key(r) {
            return Ok(r.to_string());
        }
        map.values()
            .find(|s| s.name == r)
            .map(|s| s.id.clone())
            .ok_or_else(|| Error(format!("no such sandbox: {r}")))
    }

    fn uid_of(&self, r: &str) -> Result<[u8; 16]> {
        let id = self.resolve(r)?;
        Ok(self.sandboxes.lock().unwrap()[&id].uid)
    }

    pub fn free_sandbox(&self, r: &str) -> Result<()> {
        let id = self.resolve(r)?;
        if let Some(st) = self.sandboxes.lock().unwrap().remove(&id) {
            cleanup_overlay(&st.uid);
        }
        Ok(())
    }

    pub fn configure(
        &self,
        r: &str,
        allow_network: Option<bool>,
        allow_listen: Option<bool>,
        limits: Option<Limits>,
        env: &[(String, String)],
    ) -> Result<SandboxInfo> {
        let id = self.resolve(r)?;
        let mut map = self.sandboxes.lock().unwrap();
        let st = map
            .get_mut(&id)
            .ok_or_else(|| Error("sandbox gone".into()))?;
        if let Some(v) = allow_network {
            st.allow_network = v;
        }
        if let Some(v) = allow_listen {
            st.allow_listen = v;
        }
        if let Some(v) = limits {
            st.limits = v;
        }
        for (k, v) in env {
            if let Some(e) = st.env.iter_mut().find(|(ek, _)| ek == k) {
                e.1 = v.clone();
            } else {
                st.env.push((k.clone(), v.clone()));
            }
        }
        Ok(st.info())
    }

    /// Resolve (uid, ExecOpts) for a run. `r` empty → a fresh ephemeral sandbox
    /// using `ov`; otherwise the named sandbox's stored config. Returns whether
    /// the uid is ephemeral (its overlay should be cleaned after use).
    fn exec_target(
        &self,
        r: &str,
        timeout_ms: u64,
        ov: Overrides,
    ) -> Result<([u8; 16], ExecOpts, bool)> {
        let timeout = (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms));
        if r.is_empty() {
            let opts = ExecOpts {
                allow_network: ov.allow_network.unwrap_or(true),
                allow_listen: false,
                timeout,
                env: ov.env,
                limits: ov.limits.unwrap_or_default(),
                ..ExecOpts::default()
            };
            Ok((generate_uid(), opts, true))
        } else {
            let id = self.resolve(r)?;
            let map = self.sandboxes.lock().unwrap();
            let st = &map[&id];
            let opts = ExecOpts {
                allow_network: st.allow_network,
                allow_listen: st.allow_listen,
                timeout,
                env: st.env.clone(),
                limits: st.limits.clone(),
                ..ExecOpts::default()
            };
            Ok((st.uid, opts, false))
        }
    }

    pub fn run(&self, r: &str, command: &str, timeout_ms: u64, ov: Overrides) -> Result<RunResult> {
        let (uid, opts, ephemeral) = self.exec_target(r, timeout_ms, ov)?;
        let stdout = Arc::new(LogBuffer::new());
        let stderr = Arc::new(LogBuffer::new());
        let code = execute_with(
            uid,
            LogLevel::Off,
            command,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            opts,
        )
        .map_err(|e| Error(format!("run failed: {e}")))?;
        if ephemeral {
            cleanup_overlay(&uid);
        }
        Ok(RunResult {
            stdout: stdout.read(),
            stderr: stderr.read(),
            exit_code: code,
        })
    }

    /// Start a background session. `pty` → interactive `/bin/sh -i` on a PTY
    /// (merged output, stdin writable); otherwise `command` runs with streamed
    /// stdout/stderr. Returns the session id and a handle.
    pub fn start_session(
        &self,
        r: &str,
        command: &str,
        pty: bool,
    ) -> Result<(String, Arc<Session>)> {
        let (uid, opts, ephemeral) = self.exec_target(r, 0, Overrides::default())?;
        let (argv, mode) = if pty {
            (
                vec!["/bin/sh".to_string(), "-i".to_string()],
                PtyMode::Buffered,
            )
        } else if command.is_empty() {
            return oops("a command is required for a non-PTY session");
        } else {
            (shell_argv(command), PtyMode::None)
        };
        let session = spawn_session(uid, LogLevel::Off, &argv, opts, mode)
            .map_err(|e| Error(format!("session start failed: {e}")))?;
        let session = Arc::new(session);
        let sid = format!("{}{}", short_id(), short_id());
        self.sessions.lock().unwrap().insert(
            sid.clone(),
            SessionEntry {
                session: Arc::clone(&session),
                uid,
                ephemeral,
            },
        );
        Ok((sid, session))
    }

    pub fn session(&self, sid: &str) -> Option<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap()
            .get(sid)
            .map(|e| Arc::clone(&e.session))
    }

    /// Drop a finished session; cleans an ephemeral sandbox's overlay.
    pub fn end_session(&self, sid: &str) {
        if let Some(e) = self.sessions.lock().unwrap().remove(sid) {
            e.session.kill();
            if e.ephemeral {
                cleanup_overlay(&e.uid);
            }
        }
    }

    pub fn write_file(&self, r: &str, path: &str, data: &[u8]) -> Result<()> {
        let uid = self.uid_of(r)?;
        write_file(uid, path, data).map_err(|e| Error(format!("write_file failed: {e}")))
    }

    pub fn read_file(&self, r: &str, path: &str) -> Result<Vec<u8>> {
        let uid = self.uid_of(r)?;
        read_file(uid, path).map_err(|e| Error(format!("read_file failed: {e}")))
    }

    pub fn copy_into(&self, r: &str, host: &str, guest: &str) -> Result<()> {
        let uid = self.uid_of(r)?;
        copy_into(uid, Path::new(host), guest).map_err(|e| Error(format!("copy_into failed: {e}")))
    }

    pub fn copy_out(&self, r: &str, guest: &str, host: &str) -> Result<()> {
        let uid = self.uid_of(r)?;
        copy_out_of(uid, guest, Path::new(host)).map_err(|e| Error(format!("copy_out failed: {e}")))
    }

    /// Snapshot a sandbox's overlay under `id` (empty → a generated id). Returns
    /// the snapshot id.
    pub fn snapshot(&self, r: &str, id: &str) -> Result<String> {
        let uid = self.uid_of(r)?;
        let id = if id.is_empty() {
            short_id()
        } else {
            id.to_string()
        };
        cvisor_core::snapshot::snapshot(uid, &id)
            .map_err(|e| Error(format!("snapshot failed: {e}")))?;
        Ok(id)
    }

    /// Replace a sandbox's overlay with a snapshot (discard changes since).
    pub fn rollback(&self, r: &str, snapshot_id: &str) -> Result<()> {
        let uid = self.uid_of(r)?;
        cvisor_core::snapshot::rollback(uid, snapshot_id)
            .map_err(|e| Error(format!("rollback failed: {e}")))
    }

    /// Create a new sandbox whose overlay comes from a snapshot.
    pub fn branch(&self, snapshot_id: &str, name: &str) -> Result<SandboxInfo> {
        let info = self.create_sandbox((!name.is_empty()).then_some(name));
        let uid = self.uid_of(&info.id)?;
        if let Err(e) = cvisor_core::snapshot::branch(snapshot_id, uid) {
            let _ = self.free_sandbox(&info.id);
            return Err(Error(format!("branch failed: {e}")));
        }
        Ok(info)
    }

    /// Create a new sandbox forked from a live sandbox's current overlay.
    pub fn fork(&self, r: &str, name: &str) -> Result<SandboxInfo> {
        let src = self.uid_of(r)?;
        let info = self.create_sandbox((!name.is_empty()).then_some(name));
        let uid = self.uid_of(&info.id)?;
        if let Err(e) = cvisor_core::snapshot::fork(src, uid) {
            let _ = self.free_sandbox(&info.id);
            return Err(Error(format!("fork failed: {e}")));
        }
        Ok(info)
    }

    pub fn list_snapshots(&self) -> Result<Vec<CacheEntry>> {
        cvisor_core::snapshot::list()
            .map(|v| {
                v.into_iter()
                    .map(|s| CacheEntry {
                        name: s.id,
                        size: s.size,
                    })
                    .collect()
            })
            .map_err(|e| Error(format!("list snapshots failed: {e}")))
    }

    pub fn delete_snapshot(&self, id: &str) -> Result<bool> {
        cvisor_core::snapshot::delete(id).map_err(|e| Error(format!("delete snapshot failed: {e}")))
    }

    pub fn cache_save(
        &self,
        r: &str,
        path: &str,
        key: &str,
        backend: &str,
        format: &str,
    ) -> Result<()> {
        let uid = self.uid_of(r)?;
        let (backend, fmt) = parse_cache(backend, format)?;
        cache::save(uid, path, key, &backend, fmt)
            .map_err(|e| Error(format!("cache save failed: {e}")))
    }

    pub fn cache_restore(
        &self,
        r: &str,
        path: &str,
        key: &str,
        backend: &str,
        format: &str,
    ) -> Result<()> {
        let uid = self.uid_of(r)?;
        let (backend, fmt) = parse_cache(backend, format)?;
        cache::restore(uid, path, key, &backend, fmt)
            .map_err(|e| Error(format!("cache restore failed: {e}")))
    }

    pub fn cache_list(&self, backend: &str) -> Result<Vec<CacheEntry>> {
        let (backend, _) = parse_cache(backend, "")?;
        let entries =
            cache::list(&backend).map_err(|e| Error(format!("cache list failed: {e}")))?;
        Ok(entries
            .into_iter()
            .map(|e| CacheEntry {
                name: e.name,
                size: e.size,
            })
            .collect())
    }

    pub fn cache_remove(&self, key: &str, backend: &str, format: &str) -> Result<bool> {
        let (backend, fmt) = parse_cache(backend, format)?;
        cache::remove(key, &backend, fmt).map_err(|e| Error(format!("cache remove failed: {e}")))
    }

    pub fn cache_clear(&self, backend: &str) -> Result<u32> {
        let (backend, _) = parse_cache(backend, "")?;
        cache::clear(&backend)
            .map(|n| n as u32)
            .map_err(|e| Error(format!("cache clear failed: {e}")))
    }
}

fn parse_cache(backend: &str, format: &str) -> Result<(cache::Backend, Format)> {
    let backend = if backend.is_empty() {
        cache::Backend::default_disk()
    } else {
        cache::Backend::parse(backend).map_err(|e| Error(format!("bad backend: {e}")))?
    };
    let fmt = if format.is_empty() {
        Format::Gzip
    } else {
        Format::parse(format).ok_or_else(|| Error(format!("unknown format: {format}")))?
    };
    Ok((backend, fmt))
}
