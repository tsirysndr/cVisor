//! Remote mode: drive a `cvisord` daemon over gRPC. Portable (works from any
//! host), so `cvisor --remote <addr> --token <t> -- <cmd>` runs a command on a
//! remote sandbox, and `cvisor --remote <addr>` opens an interactive PTY shell.

use std::io::{Read, Write};

use cvisor_proto::cvisor::cvisor_client::CvisorClient;
use cvisor_proto::cvisor::{
    output_chunk, shell_input, shell_output, Resize, RunRequest, ShellInput, ShellOutput,
    ShellStart,
};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Request, Status};

const USAGE: &str = "\
cvisor --remote <ADDR> [OPTIONS] [<SANDBOX>] [-- <cmd> [args]]

Drive a remote cvisord daemon over gRPC.

    <SANDBOX>         optional sandbox ref (id or name) to target; else ephemeral

OPTIONS:
    --remote <ADDR>   daemon address, e.g. 127.0.0.1:50051 or http://host:50051
                      (or env CVISOR_REMOTE — its presence alone selects client mode)
    --token <TOKEN>   bearer token (or env CVISOR_TOKEN)
    --sandbox <REF>   alias for the positional sandbox ref
    --timeout <ms>    SIGKILL the guest after <ms> milliseconds
    -h, --help        show this help

With a trailing `-- <cmd>`, runs the command and streams its output. With no
command, opens an interactive PTY shell.
";

struct Opts {
    remote: String,
    token: String,
    sandbox: String,
    timeout_ms: u64,
    command: Option<String>,
}

/// Entry point when `--remote` is present. Returns the process exit code.
pub fn main(args: &[String]) -> i32 {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("{msg}");
            return 2;
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cvisor: runtime: {e}");
            return 1;
        }
    };
    match rt.block_on(run(opts)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("cvisor: {e}");
            1
        }
    }
}

fn parse(args: &[String]) -> Result<Opts, String> {
    // The daemon address may come from CVISOR_REMOTE; --remote overrides it.
    let mut remote = std::env::var("CVISOR_REMOTE")
        .ok()
        .filter(|s| !s.is_empty());
    let mut token = std::env::var("CVISOR_TOKEN").unwrap_or_default();
    let mut sandbox = String::new();
    let mut timeout_ms = 0u64;
    let mut command: Option<String> = None;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--remote" => remote = Some(it.next().ok_or("--remote needs an address")?.clone()),
            "--token" => token = it.next().ok_or("--token needs a value")?.clone(),
            "--sandbox" => sandbox = it.next().ok_or("--sandbox needs a value")?.clone(),
            "--timeout" => {
                timeout_ms = it
                    .next()
                    .ok_or("--timeout needs a value (ms)")?
                    .parse()
                    .map_err(|_| "--timeout must be a number".to_string())?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            "--" => {
                let rest: Vec<String> = it.by_ref().cloned().collect();
                command = Some(shell_join(&rest));
                break;
            }
            // A bare argument is the optional sandbox ref (id or name).
            other if !other.starts_with('-') && sandbox.is_empty() => sandbox = other.to_string(),
            other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
        }
    }

    let remote = remote.ok_or("a daemon address is required (--remote or CVISOR_REMOTE)")?;
    if token.is_empty() {
        return Err("a token is required (--token or CVISOR_TOKEN)".to_string());
    }
    Ok(Opts {
        remote,
        token,
        sandbox,
        timeout_ms,
        command,
    })
}

/// Join argv into a `/bin/sh -c` string, quoting each argument.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The auth interceptor stamps `authorization: Bearer <token>` on every request.
type Interceptor = Box<dyn FnMut(Request<()>) -> Result<Request<()>, Status> + Send>;
type Client = CvisorClient<tonic::service::interceptor::InterceptedService<Channel, Interceptor>>;

async fn connect(opts: &Opts) -> Result<Client, String> {
    let uri = if opts.remote.starts_with("http://") || opts.remote.starts_with("https://") {
        opts.remote.clone()
    } else {
        format!("http://{}", opts.remote)
    };
    let channel = Channel::from_shared(uri)
        .map_err(|e| format!("bad --remote address: {e}"))?
        .connect()
        .await
        .map_err(|e| format!("connect failed: {e}"))?;
    let bearer: MetadataValue<_> = format!("Bearer {}", opts.token)
        .parse()
        .map_err(|_| "invalid token".to_string())?;
    // The interceptor closure returns tonic's Result<_, Status>; Status is large.
    #[allow(clippy::result_large_err)]
    let interceptor: Interceptor = Box::new(move |mut req: Request<()>| {
        req.metadata_mut().insert("authorization", bearer.clone());
        Ok(req)
    });
    Ok(CvisorClient::with_interceptor(channel, interceptor))
}

async fn run(opts: Opts) -> Result<i32, String> {
    let mut client = connect(&opts).await?;
    match opts.command.clone() {
        Some(command) => run_command(&mut client, &opts, command).await,
        None => shell(&mut client, &opts).await,
    }
}

/// Stream a command's output to the local terminal; return its exit code.
async fn run_command(client: &mut Client, opts: &Opts, command: String) -> Result<i32, String> {
    let req = RunRequest {
        id: opts.sandbox.clone(),
        command,
        timeout_ms: opts.timeout_ms,
        ..Default::default()
    };
    let mut stream = client
        .run_stream(req)
        .await
        .map_err(|e| format!("run failed: {e}"))?
        .into_inner();

    let mut code = 0;
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    while let Some(chunk) = stream.message().await.map_err(|e| e.to_string())? {
        match chunk.kind {
            Some(output_chunk::Kind::Stdout(b)) => {
                let _ = out.write_all(&b);
                let _ = out.flush();
            }
            Some(output_chunk::Kind::Stderr(b)) => {
                let _ = err.write_all(&b);
                let _ = err.flush();
            }
            Some(output_chunk::Kind::ExitCode(c)) => code = c,
            None => {}
        }
    }
    Ok(code)
}

/// Open an interactive PTY shell: local terminal in raw mode, stdin forwarded to
/// the daemon, merged output written back.
async fn shell(client: &mut Client, opts: &Opts) -> Result<i32, String> {
    let (tx, rx) = tokio::sync::mpsc::channel::<ShellInput>(64);

    // First message: Start (PTY).
    let (rows, cols) = term_size();
    tx.send(ShellInput {
        kind: Some(shell_input::Kind::Start(ShellStart {
            id: opts.sandbox.clone(),
            command: String::new(),
            pty: true,
        })),
    })
    .await
    .ok();
    tx.send(ShellInput {
        kind: Some(shell_input::Kind::Resize(Resize { rows, cols })),
    })
    .await
    .ok();

    // Forward local stdin (raw) on a blocking thread.
    let _raw = RawMode::enable();
    let stdin_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stdin = std::io::stdin();
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stdin_tx
                        .blocking_send(ShellInput {
                            kind: Some(shell_input::Kind::Stdin(buf[..n].to_vec())),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
    let mut inbound = client
        .shell(outbound)
        .await
        .map_err(|e| format!("shell failed: {e}"))?
        .into_inner();

    let mut code = 0;
    let mut out = std::io::stdout();
    while let Some(msg) = inbound.message().await.map_err(|e: Status| e.to_string())? {
        let ShellOutput { kind } = msg;
        match kind {
            Some(shell_output::Kind::Output(b)) => {
                let _ = out.write_all(&b);
                let _ = out.flush();
            }
            Some(shell_output::Kind::ExitCode(c)) => {
                code = c;
                break;
            }
            None => {}
        }
    }
    drop(tx);
    Ok(code)
}

/// Local terminal window size (rows, cols), defaulting to 24x80.
fn term_size() -> (u32, u32) {
    // SAFETY: ioctl on stdin with a TIOCGWINSZ winsize out-param.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 {
            (ws.ws_row as u32, ws.ws_col as u32)
        } else {
            (24, 80)
        }
    }
}

/// Puts the terminal into raw mode for the duration; restores on drop.
struct RawMode(Option<libc::termios>);

impl RawMode {
    fn enable() -> RawMode {
        // SAFETY: standard termios get/set on stdin.
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut term) != 0 {
                return RawMode(None);
            }
            let original = term;
            libc::cfmakeraw(&mut term);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &term);
            RawMode(Some(original))
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(term) = self.0 {
            // SAFETY: restore the saved termios.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &term);
            }
        }
    }
}
