//! macOS support.
//!
//! cVisor's local sandbox virtualizes Linux syscalls, so it needs a Linux
//! kernel. On macOS we provide one transparently: a single reusable **bsdkrun
//! microVM** named `cvisor-sandbox`, booted from the cVisor OCI image with
//! `cvisord` as its main process. The CLI then drives that daemon over gRPC
//! (remote mode), so `cvisor -- <cmd>` and `cvisor` (shell) "just work".
//!
//! The microVM is created once and reused for every run (started again if it was
//! stopped). The daemon's bearer token is generated on first boot and cached
//! under `~/.cvisor/` so later invocations reconnect to the same daemon.
//!
//! Requires the `bsdkrun` CLI to be installed (`cvisor doctor` checks this).
//!
//! Env overrides:
//!   - `CVISOR_SANDBOX_TAG`   — `ubuntu` (default), `trixie`, or `alpine`.
//!   - `CVISOR_SANDBOX_IMAGE` — a full OCI ref, overriding the tag shorthand,
//!     e.g. `ghcr.io/tsirysndr/cvisor:ubuntu-latest`,
//!     `ghcr.io/tsirysndr/cvisor:trixie-1.2.3`, or
//!     `registry.example.com/team/cvisor:custom`.
//!   - `CVISOR_SANDBOX_CPUS` / `CVISOR_SANDBOX_MEM` — vCPUs / RAM (MiB).
//!   - `CVISOR_SANDBOX_DISK`  — extra disk: a size (`8G`), a path, or `off`.

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bsdkrun_sdk::{Sandbox, SandboxInfo};

const HELP: &str = "\
cvisor — in-process Linux sandbox (macOS)

On macOS the sandbox runs inside a reusable bsdkrun microVM named
`cvisor-sandbox`, booted from the cVisor image with cvisord inside. The first
run provisions it (streaming each step); later runs reconnect instantly.

USAGE:
    cvisor                          open a sandboxed shell in the microVM
    cvisor -- <cmd> [args]          run a command in the microVM sandbox
    cvisor <sandbox> -- <cmd>       target a named sandbox on the daemon
    cvisor doctor                   check bsdkrun / VM support / the microVM
    cvisor ui [OPTIONS]             serve the web UI against a daemon
    cvisor --remote <ADDR> ...      drive a specific daemon directly (skip the VM)

OPTIONS (run/shell):
    --timeout <ms>      SIGKILL the guest after <ms> milliseconds
    -h, --help          show this help

REQUIREMENTS:
    The bsdkrun CLI must be installed (brew install tsirysndr/tap/bsdkrun).

ENV:
    CVISOR_SANDBOX_TAG    image tag: ubuntu (default), trixie, alpine
    CVISOR_SANDBOX_IMAGE  full OCI ref, overriding the tag — e.g.
                          ghcr.io/tsirysndr/cvisor:ubuntu-latest
                          ghcr.io/tsirysndr/cvisor:trixie-1.2.3
                          registry.example.com/team/cvisor:custom
    CVISOR_SANDBOX_CPUS   vCPUs   (default 2)
    CVISOR_SANDBOX_MEM    RAM MiB (default 2048)
    CVISOR_SANDBOX_DISK   extra disk: a size (8G), a path, or off
    CVISOR_REMOTE         point at an existing daemon instead of the microVM

    Unset CVISOR_SANDBOX_* values fall back to ~/.cvisor/sandbox.json, the
    settings written by the cVisor desktop app.
";

/// The single, reusable sandbox microVM name.
const VM_NAME: &str = "cvisor-sandbox";
/// Host-forwarded ports for the in-VM daemon (gRPC / GraphQL).
const GRPC_PORT: u16 = 50051;
const HTTP_PORT: u16 = 8080;

/// Entry point for macOS (`argv[1..]`). `doctor` runs locally; everything else
/// ensures the microVM is up, then delegates to remote mode against it.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("doctor") => return doctor(),
        Some("-h") | Some("--help") => {
            print!("{HELP}");
            return 0;
        }
        _ => {}
    }

    let (addr, token) = match ensure_daemon() {
        Ok(v) => v,
        Err(code) => return code,
    };

    // Drive the in-VM daemon over gRPC. Inject --remote/--token, keep the user's
    // args (a sandbox ref, `--timeout`, and/or a trailing `-- <cmd>`; no command
    // opens an interactive shell).
    let mut remote_args = vec!["--remote".to_string(), addr, "--token".to_string(), token];
    remote_args.extend(args.iter().cloned());
    crate::remote::main(&remote_args)
}

// -- provisioning -------------------------------------------------------------

/// Ensure the `cvisor-sandbox` microVM is running with `cvisord`, returning the
/// gRPC address and bearer token. Reuses a running VM, starts a stopped one, or
/// creates a fresh one — streaming each step.
fn ensure_daemon() -> Result<(String, String), i32> {
    if bsdkrun_sdk::resolve_binary().is_err() {
        eprintln!("cvisor: bsdkrun is required on macOS but was not found on PATH.");
        eprintln!("        Install it, then re-run:");
        eprintln!("            brew install tsirysndr/tap/bsdkrun");
        eprintln!("        See https://github.com/tsirysndr/bsdkrun");
        eprintln!("        Verify with: cvisor doctor");
        return Err(1);
    }

    let addr = format!("127.0.0.1:{GRPC_PORT}");
    match find_vm() {
        Some(info) if info.running => {
            let token = require_token()?;
            ok(&format!("reusing sandbox microVM '{VM_NAME}'"));
            ensure_daemon_running(&Sandbox::from_id(info.id.as_str()), &token)?;
            Ok((addr, token))
        }
        Some(info) => {
            let token = require_token()?;
            step(&format!("starting stopped microVM '{VM_NAME}'…"));
            let sb = Sandbox::from_id(info.id.as_str());
            sb.start().map_err(bsd_err)?;
            ensure_daemon_running(&sb, &token)?;
            ok("microVM started");
            Ok((addr, token))
        }
        None => create_vm(&addr),
    }
}

/// Boot a fresh `cvisor-sandbox` microVM and verify it.
fn create_vm(addr: &str) -> Result<(String, String), i32> {
    let image = sandbox_image();
    let token = load_or_create_token()?;
    let (cpus, mem) = resources();

    step(&format!("preparing sandbox microVM '{VM_NAME}'"));
    eprintln!("    image:   {image}");
    eprintln!("    daemon:  gRPC 127.0.0.1:{GRPC_PORT} · GraphQL 127.0.0.1:{HTTP_PORT}");
    eprintln!("    machine: {cpus} vCPU · {mem} MiB");

    // The bsdkrun-sdk builder assembles the argv; run it with `spawn` so the
    // image pull + boot stream live to the terminal (the builder's own
    // `create()` captures instead).
    let mut builder = Sandbox::linux(&image)
        .name(VM_NAME)
        .entrypoint("cvisord")
        .env("CVISOR_TOKEN", token.as_str())
        .env("CVISOR_GRPC_ADDR", format!("0.0.0.0:{GRPC_PORT}"))
        .env("CVISOR_HTTP_ADDR", format!("0.0.0.0:{HTTP_PORT}"))
        .forward(GRPC_PORT, GRPC_PORT)
        .forward(HTTP_PORT, HTTP_PORT)
        .cpus(cpus)
        .mem(mem)
        .log_level(1);
    if let Some(disk) = ensure_disk() {
        eprintln!("    disk:    {disk}");
        builder = builder.attach_disk(disk.as_str());
    }

    step("booting microVM (first run pulls the image — this can take a minute)…");
    let code = bsdkrun_sdk::spawn(builder.to_args()).map_err(bsd_err)?;
    if code != 0 {
        eprintln!("cvisor: bsdkrun failed to boot the microVM (exit {code})");
        return Err(1);
    }
    let info = find_vm().ok_or_else(|| {
        eprintln!("cvisor: the microVM booted but was not found in `bsdkrun ps`");
        1
    })?;
    let sb = Sandbox::from_id(info.id.as_str());
    ok(&format!("microVM booted (id {})", short_id(sb.id())));

    step("waiting for cvisord to accept connections…");
    wait_ready(&token)?;
    ok("cvisord is up");

    // "check with cvisor doctor": run the in-VM sandbox self-check, streamed.
    // The bsdkrun guest agent can lag the daemon on a cold boot, so retry a few
    // times until it produces output; the check is informational either way.
    step("verifying the in-VM sandbox (cvisor doctor)…");
    verify_sandbox(&sb);

    ok(&format!("sandbox '{VM_NAME}' ready"));
    Ok((addr.to_string(), token))
}

/// Make sure `cvisord` is serving inside a running microVM, then wait for it.
///
/// `bsdkrun start` re-boots a stopped machine from its own rootfs but does not
/// replay the `--entrypoint`/`-e` recorded at create time: the guest comes back
/// on the image's default command with no `CVISOR_*` env, so cvisord never runs
/// and the forwarded ports lead nowhere. Launch it through the guest agent when
/// the daemon isn't answering — this also recovers a VM restarted by hand or by
/// the desktop app.
fn ensure_daemon_running(sb: &Sandbox, token: &str) -> Result<(), i32> {
    if health_ok(token) {
        return Ok(());
    }
    step("starting cvisord in the microVM…");
    let script = daemon_launch_script(token);
    // The guest agent lags the boot, so a failed exec means "not up yet".
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if matches!(sb.command("sh").arg("-c").arg(&script).run(), Ok(res) if res.exit_code == 0) {
            break;
        }
        if Instant::now() >= deadline {
            eprintln!("cvisor: could not start cvisord in microVM '{VM_NAME}'");
            eprintln!("        Inspect boot logs with: bsdkrun logs {VM_NAME}");
            return Err(1);
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    wait_ready(token)
}

/// Shell run in the guest to (re)start `cvisord` detached, with the env the
/// original boot gave it. Idempotent: it exits early if a daemon is already
/// running, so retrying the exec never stacks up processes fighting for the
/// listen ports.
fn daemon_launch_script(token: &str) -> String {
    format!(
        r#"for p in /proc/[0-9]*; do
    [ -r "$p/comm" ] || continue
    read -r c < "$p/comm" || continue
    [ "$c" = cvisord ] && exit 0
done
export CVISOR_TOKEN='{token}'
export CVISOR_GRPC_ADDR='0.0.0.0:{GRPC_PORT}'
export CVISOR_HTTP_ADDR='0.0.0.0:{HTTP_PORT}'
mkdir -p /var/log
launch() {{
    if command -v setsid >/dev/null 2>&1; then setsid cvisord; else cvisord; fi
}}
launch >/var/log/cvisord.log 2>&1 </dev/null &
"#,
        token = sh_quoted(token),
    )
}

/// The body of a single-quoted shell word: `'` closes, escapes, and reopens.
fn sh_quoted(s: &str) -> String {
    s.replace('\'', r"'\''")
}

/// Run `cvisor doctor` inside the VM (streamed), retrying while the guest agent
/// is still warming up. Best-effort: a failure only warns.
fn verify_sandbox(sb: &Sandbox) {
    for attempt in 1..=5 {
        match sb
            .command("cvisor")
            .arg("doctor")
            .stdout(std::io::stderr())
            .stderr(std::io::stderr())
            .run()
        {
            Ok(res) if res.exit_code == 0 => {
                ok("sandbox healthy");
                return;
            }
            Ok(res) => {
                warn(&format!(
                    "cvisor doctor reported issues (exit {})",
                    res.exit_code
                ));
                return;
            }
            // Empty agent output on a cold boot surfaces as an error; retry.
            Err(_) if attempt < 5 => std::thread::sleep(Duration::from_millis(1000)),
            Err(e) => warn(&format!("could not run cvisor doctor in the VM: {e}")),
        }
    }
}

/// `cvisor doctor` on macOS: check bsdkrun, VM support, and the sandbox VM.
fn doctor() -> i32 {
    println!("cvisor doctor (macOS)\n");
    let mut all_ok = true;
    let mut check = |name: &str, pass: bool, detail: &str| {
        let mark = if pass { "✔" } else { "✘" };
        println!("  {mark} {name:<26} {detail}");
        all_ok &= pass;
    };

    match bsdkrun_sdk::resolve_binary() {
        Ok(path) => check("bsdkrun installed", true, &path),
        Err(_) => check(
            "bsdkrun installed",
            false,
            "not found — brew install tsirysndr/tap/bsdkrun",
        ),
    }

    match bsdkrun_sdk::system::probe() {
        Ok(true) => check("libkrun / VM support", true, "ok"),
        Ok(false) => check("libkrun / VM support", false, "probe reported failure"),
        Err(e) => check("libkrun / VM support", false, &format!("probe error: {e}")),
    }

    // VM presence is informational (created lazily), so it never fails the run.
    match find_vm() {
        Some(info) if info.running => check(VM_NAME, true, "running"),
        Some(_) => check(VM_NAME, true, "stopped (starts on next run)"),
        None => check(VM_NAME, true, "not created yet (created on first run)"),
    }

    println!();
    if all_ok {
        println!("All checks passed. Run `cvisor -- <cmd>` or `cvisor` for a sandboxed shell.");
        0
    } else {
        println!("Some checks failed — see above.");
        1
    }
}

// -- helpers ------------------------------------------------------------------

/// microVM settings persisted by the desktop app's Settings screen
/// (`~/.cvisor/sandbox.json`). Each `CVISOR_SANDBOX_*` env var still overrides
/// its field; the file sits between the env and the built-in defaults.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct FileSettings {
    image: Option<String>,
    tag: Option<String>,
    cpus: Option<u32>,
    mem_mib: Option<u32>,
    disk: Option<String>,
}

fn file_settings() -> &'static FileSettings {
    static SETTINGS: std::sync::OnceLock<FileSettings> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(|| {
        std::fs::read_to_string(state_dir().join("sandbox.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    })
}

/// A non-empty env var, if set.
fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// The OCI image for the sandbox VM. `CVISOR_SANDBOX_IMAGE` (env, then file)
/// overrides fully; otherwise `CVISOR_SANDBOX_TAG` (env, then file) selects
/// ubuntu (default) / trixie / alpine.
fn sandbox_image() -> String {
    if let Some(full) = env_str("CVISOR_SANDBOX_IMAGE").or_else(|| {
        file_settings()
            .image
            .clone()
            .filter(|v| !v.trim().is_empty())
    }) {
        return full;
    }
    let tag = env_str("CVISOR_SANDBOX_TAG")
        .or_else(|| file_settings().tag.clone())
        .unwrap_or_default();
    let suffix = match tag.trim() {
        "" | "ubuntu" => "ubuntu-latest",
        "trixie" | "debian" => "trixie-latest",
        "alpine" => "latest",
        // Any other value is treated as an explicit tag on the cVisor image.
        other => return format!("ghcr.io/tsirysndr/cvisor:{other}"),
    };
    format!("ghcr.io/tsirysndr/cvisor:{suffix}")
}

/// vCPU count and RAM (MiB): env, then the settings file, then 2 vCPU / 2048 MiB.
fn resources() -> (u32, u32) {
    let cpus = env_str("CVISOR_SANDBOX_CPUS")
        .and_then(|v| v.trim().parse().ok())
        .or(file_settings().cpus)
        .filter(|&n| n > 0)
        .unwrap_or(2);
    let mem = env_str("CVISOR_SANDBOX_MEM")
        .and_then(|v| v.trim().parse().ok())
        .or(file_settings().mem_mib)
        .filter(|&n| n >= 256)
        .unwrap_or(2048);
    (cpus, mem)
}

/// The extra disk to attach, if any. `CVISOR_SANDBOX_DISK`: a size (`8G`, the
/// default), an explicit path, or `off`/`none` to disable. A size-specced disk
/// is created sparsely under `~/.cvisor/` and reused.
fn ensure_disk() -> Option<String> {
    let spec = env_str("CVISOR_SANDBOX_DISK")
        .or_else(|| file_settings().disk.clone())
        .unwrap_or_default();
    let spec = spec.trim();
    if matches!(spec, "off" | "none" | "0" | "false") {
        return None;
    }
    // An explicit path is attached as-is.
    if spec.contains('/') || spec.ends_with(".raw") || spec.ends_with(".img") {
        return Some(spec.to_string());
    }
    // Otherwise it's a size; default 8 GiB.
    let bytes = parse_size(if spec.is_empty() { "8G" } else { spec })?;
    let path = disk_path();
    if !path.exists() {
        std::fs::create_dir_all(state_dir()).ok()?;
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .ok()?;
        f.set_len(bytes).ok()?;
    }
    Some(path.to_string_lossy().into_owned())
}

/// Parse a size like `8G`, `512M`, or a bare byte count.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let last = s.chars().last()?;
    let (digits, mult) = match last {
        'G' | 'g' => (&s[..s.len() - 1], 1u64 << 30),
        'M' | 'm' => (&s[..s.len() - 1], 1u64 << 20),
        '0'..='9' => (s, 1),
        _ => return None,
    };
    digits.trim().parse::<u64>().ok().map(|n| n * mult)
}

/// Find the `cvisor-sandbox` machine (running or stopped), if it exists.
fn find_vm() -> Option<SandboxInfo> {
    Sandbox::list(true)
        .ok()?
        .into_iter()
        .find(|info| info.name.as_deref() == Some(VM_NAME))
}

/// Block until the daemon actually serves a GraphQL `health` request (up to
/// ~120s), showing progress. A bare TCP accept isn't enough — on a cold boot the
/// listener is up before the server processes requests, which cancels the first
/// run; a real health round-trip proves it's ready. First boot also pulls the
/// image and boots a kernel, so be patient.
fn wait_ready(token: &str) -> Result<(), i32> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut dotted = false;
    loop {
        if health_ok(token) {
            if dotted {
                eprintln!();
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            if dotted {
                eprintln!();
            }
            eprintln!("cvisor: timed out waiting for cvisord (127.0.0.1:{GRPC_PORT})");
            eprintln!("        Inspect boot logs with: bsdkrun logs {VM_NAME}");
            eprintln!(
                "        Daemon log:             bsdkrun exec {VM_NAME} cat /var/log/cvisord.log"
            );
            return Err(1);
        }
        eprint!(".");
        let _ = std::io::stderr().flush();
        dotted = true;
        std::thread::sleep(Duration::from_millis(800));
    }
}

/// One GraphQL `{ health { ok } }` round-trip over the forwarded HTTP port, via a
/// minimal raw HTTP/1.1 POST (no HTTP-client dependency). True iff the daemon
/// answers `"ok":true`.
fn health_ok(token: &str) -> bool {
    use std::io::{Read, Write};
    let sockaddr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    let Ok(mut stream) = TcpStream::connect_timeout(&sockaddr, Duration::from_millis(700)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
    let body = br#"{"query":"{ health { ok } }"}"#;
    let req = format!(
        "POST /graphql HTTP/1.1\r\n\
         Host: 127.0.0.1:{HTTP_PORT}\r\n\
         Authorization: Bearer {token}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(req.as_bytes()).is_err() || stream.write_all(body).is_err() {
        return false;
    }
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp.contains("\"ok\":true")
}

// -- token + state ------------------------------------------------------------

fn state_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".cvisor")
}

fn token_path() -> PathBuf {
    state_dir().join("sandbox-token")
}

fn disk_path() -> PathBuf {
    state_dir().join("cvisor-sandbox.disk.raw")
}

fn read_token() -> Option<String> {
    std::fs::read_to_string(token_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The cached token, or an error telling the user how to recover if a running
/// VM's token is unknown (e.g. the state file was deleted).
fn require_token() -> Result<String, i32> {
    read_token().ok_or_else(|| {
        eprintln!("cvisor: microVM '{VM_NAME}' exists but its access token is unknown.");
        eprintln!("        Recreate it: bsdkrun rm -f {VM_NAME}   (then re-run)");
        1
    })
}

/// The cached token, or a freshly generated one persisted for reuse.
fn load_or_create_token() -> Result<String, i32> {
    if let Some(t) = read_token() {
        return Ok(t);
    }
    let token = generate_token();
    std::fs::create_dir_all(state_dir())
        .and_then(|_| std::fs::write(token_path(), &token))
        .map_err(|e| {
            eprintln!(
                "cvisor: could not write access token to {:?}: {e}",
                token_path()
            );
            1
        })?;
    Ok(token)
}

/// 16 random bytes from `/dev/urandom`, hex-encoded (32 hex chars).
fn generate_token() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    // A tiny fallback mix so a urandom read failure still yields a unique value.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    buf[0] ^= nanos as u8;
    buf[1] ^= (nanos >> 8) as u8;
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(12)]
}

fn bsd_err(e: bsdkrun_sdk::Error) -> i32 {
    eprintln!("cvisor: {e}");
    1
}

// -- step logging -------------------------------------------------------------

fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn step(msg: &str) {
    if use_color() {
        eprintln!("\x1b[36m▸\x1b[0m {msg}");
    } else {
        eprintln!("▸ {msg}");
    }
}

fn ok(msg: &str) {
    if use_color() {
        eprintln!("\x1b[32m✔\x1b[0m {msg}");
    } else {
        eprintln!("✔ {msg}");
    }
}

fn warn(msg: &str) {
    if use_color() {
        eprintln!("\x1b[33m!\x1b[0m {msg}");
    } else {
        eprintln!("! {msg}");
    }
}
