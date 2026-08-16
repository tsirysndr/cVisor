//! Remote-GraphQL e2e smoke for the Rust SDK's pure-HTTP client
//! (`cvisor::RemoteSandbox` over ureq — no libcvisor). Run against a running
//! cvisord:
//!
//! ```sh
//! CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... \
//!   cargo run --example remote_smoke
//! ```

use std::env;

use cvisor::RemoteSandbox;

fn main() {
    let url = env::var("CVISOR_GRAPHQL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080/graphql".to_string());
    let token = env::var("CVISOR_TOKEN").unwrap_or_default();

    let mut remote = RemoteSandbox::new(url, token);

    let health = remote.health().expect("health");
    assert!(health.ok, "health not ok: {health:?}");

    let out = remote.run("echo hello", Default::default()).expect("run");
    assert_eq!(out.stdout, "hello\n", "run stdout: {out:?}");
    assert_eq!(out.exit_code, 0, "run exit code: {out:?}");

    let info = remote.create_sandbox("").expect("create_sandbox");
    assert!(!info.id.is_empty(), "create_sandbox returned no id");
    remote
        .write_file("/tmp/data.txt", b"round-trip\n")
        .expect("write_file");
    let data = remote.read_file("/tmp/data.txt").expect("read_file");
    assert_eq!(data, b"round-trip\n", "read_file round-trip");
    remote.free_sandbox().expect("free_sandbox");

    println!("RUST_GRAPHQL_OK");
}
