//// Remote-GraphQL e2e smoke for the Gleam SDK's pure-HTTP client
//// (`cvisor/remote` over `gleam_httpc` — no NIF). Run against a running cvisord:
////
//// ```sh
//// CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... \
////   gleam run -m remote_smoke
//// ```

import cvisor/remote
import gleam/io

type Charlist

// The OTP applications the HTTP client needs. Gleam compiles a no-arg
// constructor to the atom of its snake_cased name, so `Inets`/`Ssl` are the
// `inets`/`ssl` atoms. Launching via `gleam run -m` does not boot these, so the
// smoke starts them itself (as the Erlang/Elixir clients do).
type OtpApp {
  Inets
  Ssl
}

type StartResult

@external(erlang, "application", "ensure_all_started")
fn ensure_all_started(app: OtpApp) -> StartResult

@external(erlang, "os", "getenv")
fn os_getenv(name: Charlist, default: Charlist) -> Charlist

@external(erlang, "erlang", "binary_to_list")
fn to_charlist(s: String) -> Charlist

@external(erlang, "erlang", "list_to_binary")
fn to_string(cl: Charlist) -> String

fn env(name: String, default: String) -> String {
  to_string(os_getenv(to_charlist(name), to_charlist(default)))
}

pub fn main() {
  let _ = ensure_all_started(Inets)
  let _ = ensure_all_started(Ssl)

  let url = env("CVISOR_GRAPHQL_URL", "http://127.0.0.1:8080/graphql")
  let token = env("CVISOR_TOKEN", "")
  let client = remote.connect(url, token)

  let assert Ok(health) = remote.health(client)
  let assert True = health.ok

  let assert Ok(out) = remote.run(client, "echo hello", 0)
  let assert "hello\n" = out.stdout
  let assert 0 = out.exit_code

  let assert Ok(sb) = remote.create_sandbox(client, "")
  let assert Ok(True) =
    remote.write_file(client, sb.id, "/tmp/data.txt", <<"round-trip\n":utf8>>)
  let assert Ok(data) = remote.read_file(client, sb.id, "/tmp/data.txt")
  let assert True = data == <<"round-trip\n":utf8>>
  let assert Ok(True) = remote.free_sandbox(client, sb.id)

  io.println("GLEAM_GRAPHQL_OK")
}
