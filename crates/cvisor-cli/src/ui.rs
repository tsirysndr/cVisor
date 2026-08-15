//! `cvisor ui`: serve the embedded web UI (built from `ui/`) and open a browser.
//! The daemon's GraphQL URL + token are injected into index.html as
//! `window.__CVISOR_CONFIG__`, which the app reads at runtime.

use rust_embed::RustEmbed;
use tiny_http::{Header, Response, Server};

/// The built web app (ui/dist), embedded at compile time. Until `ui/` is built
/// (`cd ui && npm run build`) this is a placeholder page.
#[derive(RustEmbed)]
#[folder = "../../ui/dist"]
struct Assets;

const USAGE: &str = "\
cvisor ui [OPTIONS]

Serve the cVisor web UI and open it in a browser.

OPTIONS:
    --daemon <URL>    daemon GraphQL base URL (default http://localhost:8080,
                      env CVISOR_DAEMON)
    --token <TOKEN>   bearer token for the daemon (or env CVISOR_TOKEN)
    --port <PORT>     local port to serve the UI on (default 4321)
    --no-open         don't open a browser
    -h, --help        show this help
";

struct Opts {
    daemon: String,
    token: String,
    port: u16,
    open: bool,
}

pub fn main(args: &[String]) -> i32 {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("{msg}");
            return 2;
        }
    };
    match serve(opts) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cvisor ui: {e}");
            1
        }
    }
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut daemon =
        std::env::var("CVISOR_DAEMON").unwrap_or_else(|_| "http://localhost:8080".into());
    let mut token = std::env::var("CVISOR_TOKEN").unwrap_or_default();
    let mut port = 4321u16;
    let mut open = true;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--daemon" => daemon = it.next().ok_or("--daemon needs a URL")?.clone(),
            "--token" => token = it.next().ok_or("--token needs a value")?.clone(),
            "--port" => {
                port = it
                    .next()
                    .ok_or("--port needs a value")?
                    .parse()
                    .map_err(|_| "--port must be a number".to_string())?;
            }
            "--no-open" => open = false,
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
        }
    }
    let daemon = daemon.trim_end_matches('/').to_string();
    Ok(Opts {
        daemon,
        token,
        port,
        open,
    })
}

fn serve(opts: Opts) -> Result<(), String> {
    let addr = ("127.0.0.1", opts.port);
    let server = Server::http(addr).map_err(|e| format!("bind :{}: {e}", opts.port))?;
    let url = format!("http://127.0.0.1:{}", opts.port);
    eprintln!("cVisor UI  {url}");
    eprintln!("  daemon   {}/graphql", opts.daemon);
    if opts.open {
        let _ = open::that(&url);
    }

    for req in server.incoming_requests() {
        let path = req
            .url()
            .split(['?', '#'])
            .next()
            .unwrap_or("/")
            .trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };

        let (bytes, ctype) = match Assets::get(path) {
            Some(file) => (file.data.into_owned(), content_type(path)),
            // SPA fallback: unknown routes serve index.html.
            None => match Assets::get("index.html") {
                Some(file) => (file.data.into_owned(), "text/html"),
                None => {
                    let _ = req.respond(Response::from_string("not found").with_status_code(404));
                    continue;
                }
            },
        };

        let body = if ctype == "text/html" {
            inject_config(&bytes, &opts).into_bytes()
        } else {
            bytes
        };

        let header = Header::from_bytes("Content-Type", ctype).unwrap();
        let _ = req.respond(Response::from_data(body).with_header(header));
    }
    Ok(())
}

/// Inject `window.__CVISOR_CONFIG__` into the served index.html.
fn inject_config(html: &[u8], opts: &Opts) -> String {
    let ws = if let Some(rest) = opts.daemon.strip_prefix("https") {
        format!("wss{rest}")
    } else if let Some(rest) = opts.daemon.strip_prefix("http") {
        format!("ws{rest}")
    } else {
        format!("ws://{}", opts.daemon)
    };
    let cfg = format!(
        "<script>window.__CVISOR_CONFIG__={{\"graphqlUrl\":\"{d}/graphql\",\"wsUrl\":\"{ws}/graphql/ws\",\"token\":\"{t}\"}};</script>",
        d = opts.daemon,
        t = opts.token.replace('"', ""),
    );
    let html = String::from_utf8_lossy(html);
    if html.contains("</head>") {
        html.replacen("</head>", &format!("{cfg}</head>"), 1)
    } else {
        format!("{cfg}{html}")
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
