//! In-process native sandbox backed by the internal `cvisor-core` runtime.
//! Linux-only (the whole module is `#[cfg(target_os = "linux")]`).

use std::sync::Arc;
use std::time::Duration;

use cvisor_core::{
    cgroup::Limits as CoreLimits, cleanup_overlay, execute_with, generate_uid,
    read_file as core_read_file, write_file as core_write_file, ExecOpts, LogBuffer, LogLevel,
};

use crate::client::{Error, Result};
use crate::remote::Output;

/// A native, in-process sandbox. Configure it, then `run` shell commands or
/// read/write files in its persistent overlay. The overlay is cleaned up on
/// drop. Linux-only.
pub struct Sandbox {
    uid: [u8; 16],
    log_level: LogLevel,
    allow_network: bool,
    allow_listen: bool,
    env: Vec<(String, String)>,
    limits: CoreLimits,
}

impl Sandbox {
    /// Create a fresh sandbox with a random overlay id (networking on).
    pub fn new() -> Self {
        Sandbox {
            uid: generate_uid(),
            log_level: LogLevel::Off,
            allow_network: true,
            allow_listen: false,
            env: Vec::new(),
            limits: CoreLimits::default(),
        }
    }

    /// Toggle outbound INET/INET6 networking (default on).
    pub fn set_allow_network(&mut self, allow: bool) -> &mut Self {
        self.allow_network = allow;
        self
    }

    /// Toggle inbound TCP servers (default off).
    pub fn set_allow_listen(&mut self, allow: bool) -> &mut Self {
        self.allow_listen = allow;
        self
    }

    /// Enable debug logging (default off).
    pub fn set_log_debug(&mut self, debug: bool) -> &mut Self {
        self.log_level = if debug {
            LogLevel::Debug
        } else {
            LogLevel::Off
        };
        self
    }

    /// Set a guest environment variable (layered over PATH/HOME) for later runs.
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let (key, value) = (key.into(), value.into());
        if let Some(e) = self.env.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            self.env.push((key, value));
        }
        self
    }

    /// Cap guest cgroup v2 resources; `None` leaves that limit unset.
    pub fn set_limits(
        &mut self,
        memory_max: Option<u64>,
        pids_max: Option<u64>,
        cpu_percent: Option<u32>,
    ) -> &mut Self {
        self.limits = CoreLimits {
            memory_max,
            pids_max,
            cpu_percent,
        };
        self
    }

    /// Seed a file into the overlay at `path`; visible to later runs.
    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        core_write_file(self.uid, path, data).map_err(|e| Error::Runtime(e.to_string()))
    }

    /// Read the guest's view of `path` from the overlay.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        core_read_file(self.uid, path).map_err(|e| Error::Runtime(e.to_string()))
    }

    /// Run `cmd` (`/bin/sh -c cmd`), blocking until it exits.
    pub fn run(&self, cmd: &str) -> Result<Output> {
        self.run_inner(cmd, None)
    }

    /// Like [`Sandbox::run`], SIGKILLing the guest after `timeout` (exit 137).
    pub fn run_timeout(&self, cmd: &str, timeout: Duration) -> Result<Output> {
        self.run_inner(cmd, Some(timeout))
    }

    fn run_inner(&self, cmd: &str, timeout: Option<Duration>) -> Result<Output> {
        let opts = ExecOpts {
            allow_network: self.allow_network,
            allow_listen: self.allow_listen,
            env: self.env.clone(),
            limits: self.limits.clone(),
            timeout,
            ..ExecOpts::default()
        };
        let stdout = Arc::new(LogBuffer::new());
        let stderr = Arc::new(LogBuffer::new());
        let exit_code = execute_with(
            self.uid,
            self.log_level,
            cmd,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            opts,
        )
        .map_err(|e| Error::Runtime(e.to_string()))?;
        Ok(Output {
            stdout: String::from_utf8_lossy(&stdout.read()).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.read()).into_owned(),
            exit_code,
        })
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        cleanup_overlay(&self.uid);
    }
}
