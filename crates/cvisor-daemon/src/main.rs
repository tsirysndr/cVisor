//! `cvisord` — the cVisor daemon: a gRPC server and an actix/async-graphql
//! server that expose the full cVisor runtime, both guarded by a bearer token.

#[cfg(target_os = "linux")]
mod auth;
#[cfg(target_os = "linux")]
mod graphql;
#[cfg(target_os = "linux")]
mod grpc;
#[cfg(target_os = "linux")]
mod names;
#[cfg(target_os = "linux")]
mod registry;
#[cfg(target_os = "linux")]
mod server;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("cvisord runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
const BANNER: &str = r"
 ██████╗██╗   ██╗██╗███████╗ ██████╗ ██████╗
██╔════╝██║   ██║██║██╔════╝██╔═══██╗██╔══██╗
██║     ██║   ██║██║███████╗██║   ██║██████╔╝
██║     ╚██╗ ██╔╝██║╚════██║██║   ██║██╔══██╗
╚██████╗ ╚████╔╝ ██║███████║╚██████╔╝██║  ██║
 ╚═════╝  ╚═══╝  ╚═╝╚══════╝ ╚═════╝ ╚═╝  ╚═╝";

#[cfg(target_os = "linux")]
const USAGE: &str = "\
cvisord — the cVisor daemon (gRPC + GraphQL)

USAGE:
    cvisord [OPTIONS]

OPTIONS:
    --grpc-addr <ADDR>   gRPC listen address   (default 0.0.0.0:50051, env CVISOR_GRPC_ADDR)
    --http-addr <ADDR>   GraphQL listen address (default 0.0.0.0:8080, env CVISOR_HTTP_ADDR)
    -h, --help           show this help

AUTH:
    Both front-ends require a bearer token in the `authorization` header.
    Set CVISOR_TOKEN to choose it; otherwise one is generated and printed here.
";

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut grpc_addr =
        std::env::var("CVISOR_GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let mut http_addr =
        std::env::var("CVISOR_HTTP_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--grpc-addr" => grpc_addr = args.next().ok_or("--grpc-addr needs a value")?,
            "--http-addr" => http_addr = args.next().ok_or("--http-addr needs a value")?,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown option: {other}\n\n{USAGE}").into()),
        }
    }

    let grpc_addr: std::net::SocketAddr = grpc_addr
        .parse()
        .map_err(|e| format!("bad --grpc-addr: {e}"))?;
    let http_addr: std::net::SocketAddr = http_addr
        .parse()
        .map_err(|e| format!("bad --http-addr: {e}"))?;

    let auth = auth::Auth::resolve();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Electric-blue banner. Honor NO_COLOR (https://no-color.org).
    let color = std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, s: &str| {
        if color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    };
    const ELECTRIC: &str = "38;2;0;229;255"; // electric cyan-blue
    const BOLD_ELECTRIC: &str = "1;38;2;0;229;255";
    const AMBER: &str = "38;2;255;196;0";
    const DIM: &str = "2";

    eprintln!("{}", paint(ELECTRIC, BANNER));
    eprintln!(
        "  {}  {}",
        paint(BOLD_ELECTRIC, "cVisor daemon"),
        paint(DIM, &format!("cvisord v{}", env!("CARGO_PKG_VERSION"))),
    );
    eprintln!(
        "  {}",
        paint(
            DIM,
            "an in-process Linux sandbox, served over gRPC + GraphQL"
        )
    );
    eprintln!();
    eprintln!("  {}  {grpc_addr}", paint(ELECTRIC, "gRPC     "));
    eprintln!(
        "  {}  http://{http_addr}/graphql",
        paint(ELECTRIC, "GraphQL  ")
    );
    if auth.generated() {
        eprintln!(
            "  {}  {}",
            paint(ELECTRIC, "token    "),
            paint(AMBER, auth.token())
        );
        eprintln!(
            "  {}",
            paint(
                DIM,
                "an access token was generated; set CVISOR_TOKEN to choose your own"
            )
        );
    } else {
        eprintln!(
            "  {}  {}",
            paint(ELECTRIC, "token    "),
            paint(DIM, "(from CVISOR_TOKEN)")
        );
    }
    eprintln!();
    eprintln!(
        "  {}",
        paint(
            DIM,
            "gRPC reflection is open; all RPCs require the bearer token · Ctrl-C to stop"
        )
    );

    // Open the SQLite store and load persisted sandboxes so they survive a
    // restart (path from CVISOR_DB, default /tmp/.cvisor/cvisor.db).
    let db_path =
        std::env::var("CVISOR_DB").unwrap_or_else(|_| "/tmp/.cvisor/cvisor.db".to_string());
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    type Opened = (
        std::sync::Arc<cvisor_core::store::Store>,
        Vec<cvisor_core::store::PersistedSandbox>,
    );
    let opened: Result<Opened, Box<dyn std::error::Error>> = rt.block_on(async {
        let store = std::sync::Arc::new(cvisor_core::store::Store::open(&db_path).await?);
        let persisted = store.list_sandboxes(-1, 0).await?;
        Ok((store, persisted))
    });
    let reg = match opened {
        Ok((store, persisted)) => {
            eprintln!(
                "  {}  {db_path} ({} sandboxes)",
                paint(ELECTRIC, "db       "),
                persisted.len()
            );
            registry::Registry::with_store(store, rt.handle().clone(), persisted)
        }
        Err(e) => {
            eprintln!(
                "  {}  {}",
                paint(ELECTRIC, "db       "),
                paint(DIM, &format!("disabled ({e})"))
            );
            registry::Registry::new()
        }
    };

    // GraphQL/actix on its own runtime thread; gRPC/tonic on the main tokio one.
    let http = std::thread::spawn({
        let reg = reg.clone();
        let auth = auth.clone();
        move || server::run_http(http_addr, reg, auth)
    });

    let grpc_result = rt.block_on(server::run_grpc(grpc_addr, reg, auth));

    match http.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("http server: {e}").into()),
        Err(_) => return Err("http server thread panicked".into()),
    }
    grpc_result
}
