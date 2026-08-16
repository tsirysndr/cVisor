//! Rust SDK for the [cVisor](https://github.com/butter-dot-dev/cVisor) sandbox
//! runtime.
//!
//! Two layers:
//!
//! - [`Client`] / [`RemoteSandbox`]: a portable, blocking client for the cVisor
//!   daemon's GraphQL HTTP API. It works on any OS (macOS included) and is the
//!   primary API. Binary payloads are base64-encoded/decoded for you.
//! - [`Sandbox`]: an in-process native sandbox that wraps the internal runtime
//!   (`cvisor-core`). It is **Linux-only** — the type and its dependency are
//!   gated behind `#[cfg(target_os = "linux")]`, so this crate still builds on
//!   macOS with only the GraphQL path available.
//!
//! ```no_run
//! use cvisor::RemoteSandbox;
//!
//! let sb = RemoteSandbox::new("http://127.0.0.1:8080/graphql", "my-token");
//! let out = sb.run("echo hi", Default::default())?;
//! print!("{}", out.stdout); // "hi\n"
//! # Ok::<(), cvisor::Error>(())
//! ```

mod client;
mod remote;

pub use client::{Client, Error, Result};
pub use remote::{
    CacheEntry, ConfigureOptions, EnvVar, Health, Limits, Output, RemoteSandbox, RunOptions,
    SandboxInfo, SandboxLimits,
};

#[cfg(target_os = "linux")]
mod native;
#[cfg(target_os = "linux")]
pub use native::Sandbox;
