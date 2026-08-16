//// A GraphQL-backed high-level API mirroring the FFI `gleam_cvisor` runtime,
//// but driven over HTTP against a running cVisor daemon. Pure HTTP + JSON, so
//// it works on any platform (macOS included) with no native library.
////
//// ```gleam
//// import cvisor/remote
////
//// pub fn main() {
////   let client = remote.connect("http://127.0.0.1:8080/graphql", token)
////   let assert Ok(out) = remote.run(client, "echo hello", 0)
////   // out.stdout == "hello\n"
//// }
//// ```

import cvisor/graphql.{type Client, type GraphqlError, DecodeError}
import gleam/bit_array
import gleam/dynamic.{type Dynamic}
import gleam/dynamic/decode
import gleam/json.{type Json}
import gleam/result

/// A daemon-side sandbox's identity and network flags.
pub type Sandbox {
  Sandbox(id: String, name: String, allow_network: Bool, allow_listen: Bool)
}

/// The captured result of a `run`.
pub type RunResult {
  RunResult(stdout: String, stderr: String, exit_code: Int)
}

/// Daemon liveness and build info.
pub type Health {
  Health(version: String, ok: Bool)
}

/// Connect to a daemon's GraphQL endpoint (an alias for `graphql.client`).
pub fn connect(url: String, token: String) -> Client {
  graphql.client(url, token)
}

/// Daemon liveness probe.
pub fn health(client: Client) -> Result(Health, GraphqlError) {
  let doc = "{ health { version ok } }"
  use data <- result.try(graphql.query(client, doc, json.object([])))
  decode_at(data, ["health"], {
    use version <- decode.field("version", decode.string)
    use ok <- decode.field("ok", decode.bool)
    decode.success(Health(version:, ok:))
  })
}

/// Run a command in a fresh ephemeral sandbox; `timeout_ms` of 0 means no limit.
pub fn run(
  client: Client,
  command: String,
  timeout_ms: Int,
) -> Result(RunResult, GraphqlError) {
  let doc =
    "mutation($command: String!, $timeoutMs: Int) {
       run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
     }"
  let vars =
    json.object([
      #("command", json.string(command)),
      #("timeoutMs", json.int(timeout_ms)),
    ])
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, ["run"], run_decoder())
}

/// Create a sandbox; an empty `name` gets a random docker-style name.
pub fn create_sandbox(
  client: Client,
  name: String,
) -> Result(Sandbox, GraphqlError) {
  let doc =
    "mutation($name: String) { createSandbox(name: $name) "
    <> sandbox_fields
    <> " }"
  let vars = json.object([#("name", json.string(name))])
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, ["createSandbox"], sandbox_decoder())
}

/// List all live sandboxes.
pub fn list_sandboxes(client: Client) -> Result(List(Sandbox), GraphqlError) {
  let doc = "{ sandboxes " <> sandbox_fields <> " }"
  use data <- result.try(graphql.query(client, doc, json.object([])))
  decode_at(data, ["sandboxes"], decode.list(sandbox_decoder()))
}

/// Free a sandbox and clean up its overlay.
pub fn free_sandbox(client: Client, id: String) -> Result(Bool, GraphqlError) {
  let doc = "mutation($id: String!) { freeSandbox(id: $id) }"
  bool_field(client, doc, json.object([#("id", json.string(id))]), "freeSandbox")
}

/// Write bytes to a file inside a sandbox (base64-encoded over the wire).
pub fn write_file(
  client: Client,
  id: String,
  path: String,
  data: BitArray,
) -> Result(Bool, GraphqlError) {
  let doc =
    "mutation($id: String!, $path: String!, $data: String!) {
       writeFile(id: $id, path: $path, dataBase64: $data)
     }"
  let vars =
    json.object([
      #("id", json.string(id)),
      #("path", json.string(path)),
      #("data", json.string(bit_array.base64_encode(data, True))),
    ])
  bool_field(client, doc, vars, "writeFile")
}

/// Read a file from a sandbox (base64-decoded from the wire).
pub fn read_file(
  client: Client,
  id: String,
  path: String,
) -> Result(BitArray, GraphqlError) {
  let doc =
    "query($id: String!, $path: String!) { readFile(id: $id, path: $path) }"
  let vars =
    json.object([#("id", json.string(id)), #("path", json.string(path))])
  use data <- result.try(graphql.query(client, doc, vars))
  use encoded <- result.try(decode_at(data, ["readFile"], decode.string))
  bit_array.base64_decode(encoded)
  |> result.replace_error(DecodeError("invalid base64 in readFile"))
}

/// Snapshot a sandbox's overlay; an empty `snapshot_id` gets a generated id.
/// Returns the snapshot id.
pub fn snapshot(
  client: Client,
  id: String,
  snapshot_id: String,
) -> Result(String, GraphqlError) {
  let doc =
    "mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }"
  let vars =
    json.object([
      #("id", json.string(id)),
      #("snapshotId", json.string(snapshot_id)),
    ])
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, ["snapshot"], decode.string)
}

/// Replace a sandbox's overlay with a snapshot (discard changes since).
pub fn rollback(
  client: Client,
  id: String,
  snapshot_id: String,
) -> Result(Bool, GraphqlError) {
  let doc =
    "mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }"
  let vars =
    json.object([
      #("id", json.string(id)),
      #("snapshotId", json.string(snapshot_id)),
    ])
  bool_field(client, doc, vars, "rollback")
}

/// Create a new sandbox from a snapshot; an empty `name` gets a random one.
pub fn branch(
  client: Client,
  snapshot_id: String,
  name: String,
) -> Result(Sandbox, GraphqlError) {
  let doc =
    "mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) "
    <> sandbox_fields
    <> " }"
  let vars =
    json.object([
      #("snapshotId", json.string(snapshot_id)),
      #("name", json.string(name)),
    ])
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, ["branch"], sandbox_decoder())
}

/// Fork a live sandbox's overlay into a new sandbox; empty `name` gets random.
pub fn fork(
  client: Client,
  id: String,
  name: String,
) -> Result(Sandbox, GraphqlError) {
  let doc =
    "mutation($id: String!, $name: String) { fork(id: $id, name: $name) "
    <> sandbox_fields
    <> " }"
  let vars =
    json.object([#("id", json.string(id)), #("name", json.string(name))])
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, ["fork"], sandbox_decoder())
}

/// List saved overlay snapshots (their names).
pub fn snapshots(client: Client) -> Result(List(String), GraphqlError) {
  let doc = "{ snapshots { name size } }"
  use data <- result.try(graphql.query(client, doc, json.object([])))
  decode_at(data, ["snapshots"], decode.list(decode.at(["name"], decode.string)))
}

/// Delete a snapshot; returns whether it existed.
pub fn delete_snapshot(
  client: Client,
  id: String,
) -> Result(Bool, GraphqlError) {
  let doc = "mutation($id: String!) { deleteSnapshot(id: $id) }"
  bool_field(
    client,
    doc,
    json.object([#("id", json.string(id))]),
    "deleteSnapshot",
  )
}

/// Archive a sandbox path into the cache under `key`.
pub fn cache_save(
  client: Client,
  id: String,
  path: String,
  key: String,
) -> Result(Bool, GraphqlError) {
  let doc =
    "mutation($id: String!, $path: String!, $key: String!) { cacheSave(id: $id, path: $path, key: $key) }"
  let vars =
    json.object([
      #("id", json.string(id)),
      #("path", json.string(path)),
      #("key", json.string(key)),
    ])
  bool_field(client, doc, vars, "cacheSave")
}

/// Restore a cached entry into a sandbox path.
pub fn cache_restore(
  client: Client,
  id: String,
  path: String,
  key: String,
) -> Result(Bool, GraphqlError) {
  let doc =
    "mutation($id: String!, $path: String!, $key: String!) { cacheRestore(id: $id, path: $path, key: $key) }"
  let vars =
    json.object([
      #("id", json.string(id)),
      #("path", json.string(path)),
      #("key", json.string(key)),
    ])
  bool_field(client, doc, vars, "cacheRestore")
}

/// List entries in a cache backend (their names); empty `backend` = default.
pub fn cache_list(
  client: Client,
  backend: String,
) -> Result(List(String), GraphqlError) {
  let doc = "query($backend: String) { cacheList(backend: $backend) { name size } }"
  let vars = json.object([#("backend", json.string(backend))])
  use data <- result.try(graphql.query(client, doc, vars))
  decode_at(data, ["cacheList"], decode.list(decode.at(["name"], decode.string)))
}

const sandbox_fields = "{ id name allowNetwork allowListen }"

fn sandbox_decoder() -> decode.Decoder(Sandbox) {
  use id <- decode.field("id", decode.string)
  use name <- decode.field("name", decode.string)
  use allow_network <- decode.field("allowNetwork", decode.bool)
  use allow_listen <- decode.field("allowListen", decode.bool)
  decode.success(Sandbox(id:, name:, allow_network:, allow_listen:))
}

fn run_decoder() -> decode.Decoder(RunResult) {
  use stdout <- decode.field("stdout", decode.string)
  use stderr <- decode.field("stderr", decode.string)
  use exit_code <- decode.field("exitCode", decode.int)
  decode.success(RunResult(stdout:, stderr:, exit_code:))
}

fn bool_field(
  client: Client,
  doc: String,
  vars: Json,
  field: String,
) -> Result(Bool, GraphqlError) {
  use data <- result.try(graphql.mutate(client, doc, vars))
  decode_at(data, [field], decode.bool)
}

fn decode_at(
  data: Dynamic,
  path: List(String),
  decoder: decode.Decoder(a),
) -> Result(a, GraphqlError) {
  decode.run(data, decode.at(path, decoder))
  |> result.replace_error(DecodeError("unexpected shape at " <> path_str(path)))
}

fn path_str(path: List(String)) -> String {
  case path {
    [] -> ""
    [only] -> only
    [first, ..rest] -> first <> "." <> path_str(rest)
  }
}
