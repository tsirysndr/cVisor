//! Repo automation replacing the old `zig build` steps.
//!
//!   cargo xtask test [--arch aarch64|x86_64]   cross-compile + run tests in Alpine
//!   cargo xtask run  [--arch aarch64|x86_64]    run the sandbox binary in Alpine (native arch only)
//!   cargo xtask run-node [--script F] [...]      build .node/.so + run the bun test image
//!   cargo xtask node-artifacts                   build libcvisor.node + libcvisor.so for all 4 platforms
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
    // Build the CLI supervisor (cvisor-cli) and the in-sandbox scorecard (smoke).
    if !run(Command::new("cargo").args([
        "build",
        "-p",
        "cvisor-cli",
        "--bin",
        "cvisor",
        "--target",
        &target,
        "--release",
    ])) {
        return false;
    }
    if !run(Command::new("cargo").args([
        "build",
        "-p",
        "cvisor-core",
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
    build_platform_cdylib(target, "cvisor-node", "libcvisor_node.so", |so| {
        copy_artifact(
            so,
            &format!("sdks/node/platforms/{platform_dir}/libcvisor.node"),
        )
    })
}

/// Build the C-ABI cdylib for one target and copy it as libcvisor.so into the
/// matching platform package (loaded by the Bun/Deno FFI entries).
fn build_ffi_one(target: &str, platform_dir: &str) -> bool {
    build_platform_cdylib(target, "cvisor-ffi", "libcvisor.so", |so| {
        if target.contains("musl") && !patch_musl_needed(so, target) {
            eprintln!("warning: patchelf step failed; .so may not load on minimal musl images");
        }
        copy_artifact(
            so,
            &format!("sdks/node/platforms/{platform_dir}/libcvisor.so"),
        )
    })
}

/// zigbuild `package` for `target` and hand the built artifact to `dispatch`.
fn build_platform_cdylib(
    target: &str,
    package: &str,
    artifact: &str,
    dispatch: impl FnOnce(&str) -> bool,
) -> bool {
    let mut cmd = Command::new("cargo");
    cmd.args(["zigbuild", "-p", package, "--target", target, "--release"]);
    if target.contains("musl") {
        // Dynamic musl cdylib.
        cmd.env("RUSTFLAGS", "-C target-feature=-crt-static");
    }
    if !run(&mut cmd) {
        return false;
    }
    // zigbuild strips the .2.17 suffix from the output directory.
    let out_target = target.split_once(".2.").map(|(t, _)| t).unwrap_or(target);
    let so = format!("target/{out_target}/release/{artifact}");
    if !std::path::Path::new(&so).exists() {
        eprintln!("expected {so} to exist");
        return false;
    }
    dispatch(&so)
}

fn copy_artifact(src: &str, dest: &str) -> bool {
    match std::fs::copy(src, dest) {
        Ok(_) => {
            eprintln!("+ copied {src} -> {dest}");
            true
        }
        Err(e) => {
            eprintln!("could not copy to {dest}: {e}");
            false
        }
    }
}

/// The dynamic musl cdylib NEEDs the bare `libc.so`, which only exists with
/// musl-dev. Repoint it at the musl runtime soname present on every musl
/// image so the .so loads on minimal runtimes (e.g. deno:alpine). Run
/// patchelf inside an Alpine container so it isn't a host dependency.
/// Build the all-features libcvisor.so natively inside `rust:alpine` for `arch`
/// (via `--platform` so it works cross-arch under emulation), writing it to the
/// host target dir. Uses gcc as the linker so the s3 feature's host proc-macros
/// find libgcc_s, matching the Dockerfile.
///
/// Note: the resulting .so has a runtime `NEEDED libgcc_s.so.1` (ring's unwinder
/// pulls it on musl+gcc; `-static-libgcc` doesn't fully drop it). It loads on
/// any system that has libgcc_s — present on glibc, and `apk add libgcc` on
/// Alpine. The default (pure-Rust) `xtask ffi` build stays fully self-contained.
fn build_ffi_alpine(arch: &str, target: &str) -> bool {
    let platform = if arch == "aarch64" {
        "linux/arm64"
    } else {
        "linux/amd64"
    };
    let cwd = std::env::current_dir().unwrap();
    let script = format!(
        "set -e; apk add --no-cache musl-dev gcc make perl >/dev/null; \
         export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
                CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
                RUSTFLAGS='-C target-feature=-crt-static -C link-arg=-static-libgcc'; \
         cargo build -p cvisor-ffi --target {target} --release --features all"
    );
    run(Command::new("docker").args([
        "run",
        "--rm",
        "--platform",
        platform,
        "-v",
        &format!("{}:/src", cwd.display()),
        "-w",
        "/src",
        "rust:alpine",
        "sh",
        "-c",
        &script,
    ]))
}

fn patch_musl_needed(so: &str, target: &str) -> bool {
    let arch = target.split('-').next().unwrap_or("aarch64");
    let soname = format!("libc.musl-{arch}.so.1");
    let so_abs = abs(so);
    let so_dir = std::path::Path::new(&so_abs)
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let so_name = std::path::Path::new(&so_abs)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    run(Command::new("docker").args([
        "run",
        "--rm",
        "-v",
        &format!("{so_dir}:/t"),
        "alpine",
        "sh",
        "-c",
        &format!(
            "apk add --no-cache patchelf >/dev/null 2>&1 && \
             patchelf --replace-needed libc.so {soname} /t/{so_name}"
        ),
    ]))
}

/// Build libcvisor.node and libcvisor.so for all four Node platform packages.
fn cmd_node_artifacts(_args: &[String]) -> bool {
    NODE_TARGETS
        .iter()
        .all(|(target, dir)| build_node_one(target, dir) && build_ffi_one(target, dir))
}

/// Build the native-arch .node and run the bun test image against it.
fn cmd_run_node(args: &[String]) -> bool {
    let arch = host_arch();
    if arch != host_arch() {
        eprintln!("run-node requires native arch");
        return false;
    }
    // The bun image is Debian/glibc; build the matching gnu artifacts. The
    // .so is needed too: a bare "cvisor" import under Bun resolves the "bun"
    // export condition to the FFI entry (test.ts pins the napi entry).
    let (target, dir) = if arch == "aarch64" {
        ("aarch64-unknown-linux-gnu.2.17", "linux-arm64-gnu")
    } else {
        ("x86_64-unknown-linux-gnu.2.17", "linux-x64-gnu")
    };
    if !build_node_one(target, dir) || !build_ffi_one(target, dir) {
        return false;
    }
    if !run(Command::new("docker").args(["build", "-t", "cvisor-node-test", "./sdks/node"])) {
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
        "./sdks/node:/app",
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
///
/// Default: cross-compile a pure-Rust cdylib with cargo-zigbuild (portable,
/// works from any host). `--all-features`: build natively inside `rust:alpine`
/// for the arch, so the C deps (zstd, and ring via s3) compile — the resulting
/// libcvisor.so carries every archive format and the S3 cache backend.
fn cmd_ffi(args: &[String]) -> bool {
    let arch = parse_arch(args);
    let target = format!("{arch}-unknown-linux-musl");
    let all_features = args.iter().any(|a| a == "--all-features");

    let so = format!("target/{target}/release/libcvisor.so");
    let built = if all_features {
        build_ffi_alpine(&arch, &target)
    } else {
        // Dynamic musl cdylib: disable crt-static and link via zig.
        run(Command::new("cargo")
            .args([
                "zigbuild",
                "-p",
                "cvisor-ffi",
                "--target",
                &target,
                "--release",
            ])
            .env("RUSTFLAGS", "-C target-feature=-crt-static"))
    };
    if !built {
        return false;
    }
    if !std::path::Path::new(&so).exists() {
        eprintln!("expected {so} to exist");
        return false;
    }
    // The Alpine build already links the musl soname; only the zigbuild output
    // needs its NEEDED patched.
    if !all_features && !patch_musl_needed(&so, &target) {
        eprintln!("warning: patchelf step failed; .so may not load on minimal musl images");
    }
    // Distribute the .so to each FFI SDK's native directory. The Bun/Deno
    // entries live in the Node package and load from the platform packages.
    let npm_arch = if arch == "aarch64" { "arm64" } else { "x64" };
    let dests = [
        format!("sdks/python/cvisor/_native/libcvisor-{arch}.so"),
        format!("sdks/node/platforms/linux-{npm_arch}-musl/libcvisor.so"),
        format!("sdks/ruby/native/libcvisor-{arch}.so"),
        format!("sdks/erlang/priv/libcvisor-{arch}.so"),
        format!("sdks/clojure/resources/cvisor/native/libcvisor-{arch}.so"),
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
