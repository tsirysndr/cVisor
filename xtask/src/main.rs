//! Repo automation replacing the old `zig build` steps.
//!
//!   cargo xtask test [--arch aarch64|x86_64]   cross-compile + run tests in Alpine
//!   cargo xtask run  [--arch aarch64|x86_64]    run the sandbox binary in Alpine (native arch only)
//!   cargo xtask run-node [--script F] [...]      build .node + run the bun test image
//!   cargo xtask node-artifacts                   build libcvisor.node for all 4 platforms
//!
//! Test running itself is wired through `.cargo/config.toml` runner scripts, so
//! `xtask test` is a thin wrapper over `cargo test --target …-musl`.

use std::process::{Command, ExitCode};

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => panic!("unsupported host arch: {other}"),
    }
}

fn parse_arch(args: &[String]) -> String {
    if let Some(i) = args.iter().position(|a| a == "--arch") {
        return args
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| host_arch().to_string());
    }
    host_arch().to_string()
}

fn run(cmd: &mut Command) -> bool {
    eprintln!("+ {cmd:?}");
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

fn cmd_test(args: &[String]) -> bool {
    let arch = parse_arch(args);
    let target = format!("{arch}-unknown-linux-musl");
    run(Command::new("cargo").args(["test", "-p", "cvisor-core", "--target", &target]))
}

fn cmd_run(args: &[String]) -> bool {
    let arch = parse_arch(args);
    if arch != host_arch() {
        eprintln!("xtask run requires native arch (seccomp does not work under emulation)");
        return false;
    }
    let target = format!("{arch}-unknown-linux-musl");
    // Build both the supervisor and the in-sandbox scorecard.
    if !run(Command::new("cargo").args([
        "build",
        "-p",
        "cvisor-core",
        "--bin",
        "cvisor",
        "--bin",
        "smoke",
        "--target",
        &target,
        "--release",
    ])) {
        return false;
    }
    let cvisor = abs(&format!("target/{target}/release/cvisor"));
    let smoke = abs(&format!("target/{target}/release/smoke"));
    // Run the supervisor in Alpine; it execs /smoke as the guest command.
    run(Command::new("docker").args([
        "run",
        "--rm",
        "--security-opt",
        "seccomp=unconfined",
        "-v",
        &format!("{cvisor}:/bin/cvisor"),
        "-v",
        &format!("{smoke}:/smoke"),
        "alpine",
        "/bin/cvisor",
    ]))
}

fn abs(rel: &str) -> String {
    std::env::current_dir()
        .unwrap()
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

/// The four Node platform packages: (zigbuild target, platform dir).
const NODE_TARGETS: &[(&str, &str)] = &[
    ("aarch64-unknown-linux-gnu.2.17", "linux-arm64-gnu"),
    ("x86_64-unknown-linux-gnu.2.17", "linux-x64-gnu"),
    ("aarch64-unknown-linux-musl", "linux-arm64-musl"),
    ("x86_64-unknown-linux-musl", "linux-x64-musl"),
];

/// Build the napi cdylib for one target and copy it as libcvisor.node into the
/// matching platform package. `target` may carry a `.2.17` glibc suffix.
fn build_node_one(target: &str, platform_dir: &str) -> bool {
    let is_musl = target.contains("musl");
    let mut cmd = Command::new("cargo");
    cmd.args([
        "zigbuild",
        "-p",
        "cvisor-node",
        "--target",
        target,
        "--release",
    ]);
    if is_musl {
        // Dynamic musl cdylib.
        cmd.env("RUSTFLAGS", "-C target-feature=-crt-static");
    }
    if !run(&mut cmd) {
        return false;
    }
    // zigbuild strips the .2.17 suffix from the output directory.
    let out_target = target.split_once(".2.").map(|(t, _)| t).unwrap_or(target);
    let so = format!("target/{out_target}/release/libcvisor_node.so");
    if !std::path::Path::new(&so).exists() {
        eprintln!("expected {so} to exist");
        return false;
    }
    let dest = format!("src/sdks/node/platforms/{platform_dir}/libcvisor.node");
    match std::fs::copy(&so, &dest) {
        Ok(_) => {
            eprintln!("+ copied {so} -> {dest}");
            true
        }
        Err(e) => {
            eprintln!("could not copy to {dest}: {e}");
            false
        }
    }
}

/// Build libcvisor.node for all four Node platform packages.
fn cmd_node_artifacts(_args: &[String]) -> bool {
    NODE_TARGETS
        .iter()
        .all(|(target, dir)| build_node_one(target, dir))
}

/// Build the native-arch .node and run the bun test image against it.
fn cmd_run_node(args: &[String]) -> bool {
    let arch = host_arch();
    if arch != host_arch() {
        eprintln!("run-node requires native arch");
        return false;
    }
    // The bun image is Debian/glibc; build the matching gnu .node.
    let (target, dir) = if arch == "aarch64" {
        ("aarch64-unknown-linux-gnu.2.17", "linux-arm64-gnu")
    } else {
        ("x86_64-unknown-linux-gnu.2.17", "linux-x64-gnu")
    };
    if !build_node_one(target, dir) {
        return false;
    }
    if !run(Command::new("docker").args(["build", "-t", "cvisor-node-test", "./src/sdks/node"])) {
        return false;
    }
    let script = args
        .iter()
        .position(|a| a == "--script")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "test.ts".to_string());
    run(Command::new("docker").args([
        "run",
        "--rm",
        "--security-opt",
        "seccomp=unconfined",
        "-v",
        "./src/sdks/node:/app",
        "-w",
        "/app",
        "cvisor-node-test",
        "sh",
        "-c",
        &format!("bun install && bun {script} --log-level OFF"),
    ]))
}

/// Build the FFI cdylib (libcvisor.so) for the given arch and copy it into the
/// FFI SDK native dirs so they can load it.
fn cmd_ffi(args: &[String]) -> bool {
    let arch = parse_arch(args);
    let target = format!("{arch}-unknown-linux-musl");
    // Dynamic musl cdylib: disable crt-static and link via zig.
    let ok = run(Command::new("cargo")
        .args([
            "zigbuild",
            "-p",
            "cvisor-ffi",
            "--target",
            &target,
            "--release",
        ])
        .env("RUSTFLAGS", "-C target-feature=-crt-static"));
    if !ok {
        return false;
    }
    let so = format!("target/{target}/release/libcvisor.so");
    if !std::path::Path::new(&so).exists() {
        eprintln!("expected {so} to exist");
        return false;
    }
    // The dynamic musl cdylib NEEDs the bare `libc.so`, which only exists with
    // musl-dev. Repoint it at the musl runtime soname present on every musl
    // image so the .so loads on minimal runtimes (e.g. deno:alpine). Run
    // patchelf inside an Alpine container so it isn't a host dependency.
    let soname = format!("libc.musl-{arch}.so.1");
    let so_dir = format!("{}/target/{target}/release", abs("."));
    let patched = run(Command::new("docker").args([
        "run",
        "--rm",
        "-v",
        &format!("{so_dir}:/t"),
        "alpine",
        "sh",
        "-c",
        &format!(
            "apk add --no-cache patchelf >/dev/null 2>&1 && \
             patchelf --replace-needed libc.so {soname} /t/libcvisor.so"
        ),
    ]));
    if !patched {
        eprintln!("warning: patchelf step failed; .so may not load on minimal musl images");
    }
    // Distribute the .so to each FFI SDK's native directory.
    let dests = [
        format!("src/sdks/python/cvisor/_native/libcvisor-{arch}.so"),
        format!("src/sdks/bun/native/libcvisor-{arch}.so"),
        format!("src/sdks/deno/native/libcvisor-{arch}.so"),
        format!("src/sdks/ruby/native/libcvisor-{arch}.so"),
        format!("src/sdks/erlang/priv/libcvisor-{arch}.so"),
    ];
    for dest in dests {
        let p = std::path::Path::new(&dest);
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::copy(&so, &dest) {
            eprintln!("warning: could not copy to {dest}: {e}");
        } else {
            eprintln!("+ copied {so} -> {dest}");
        }
    }
    true
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (sub, rest) = match args.split_first() {
        Some((s, r)) => (s.as_str(), r),
        None => {
            eprintln!("usage: cargo xtask <test|run|ffi|run-node|node-artifacts> [args]");
            return ExitCode::FAILURE;
        }
    };

    let ok = match sub {
        "test" => cmd_test(rest),
        "run" => cmd_run(rest),
        "ffi" => cmd_ffi(rest),
        "run-node" => cmd_run_node(rest),
        "node-artifacts" => cmd_node_artifacts(rest),
        other => {
            eprintln!("unknown xtask subcommand: {other}");
            false
        }
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
