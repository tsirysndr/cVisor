//// Optional GraphQL client for the cVisor daemon's HTTP API.
////
//// Uses `gleam_httpc` for HTTP and `gleam_json` for JSON — no native library —
//// so it works on any platform (macOS included), unlike the NIF-backed
//// `gleam_cvisor` runtime.
////
//// ```gleam
//// import cvisor/graphql
//// import gleam/json
////
//// pub fn main() {
////   let client = graphql.client("http://127.0.0.1:8080/graphql", token)
////   let assert Ok(data) =
////     graphql.query(client, "{ health { version ok } }", json.object([]))
//// }
//// ```

import gleam/dynamic.{type Dynamic}
import gleam/dynamic/decode
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/int
import gleam/json.{type Json}
import gleam/string

/// A GraphQL client: an endpoint URL and a bearer token.
pub type Client {
  Client(url: String, token: String)
}

/// Why a GraphQL call failed.
pub type GraphqlError {
  /// The HTTP transport failed or returned a non-2xx status.
  HttpError(String)
  /// The response carried a non-empty `errors` array (messages joined by `; `).
  GraphqlErrors(String)
  /// The response body was not the expected `{ data, errors }` JSON.
  DecodeError(String)
}

/// Build a client for `url` authenticating with bearer `token`.
pub fn client(url: String, token: String) -> Client {
  Client(url:, token:)
}

/// Run a GraphQL query `document` with `variables` (a `gleam/json` value, e.g.
/// `json.object([])` for none). Returns the `data` object as a `Dynamic`.
pub fn query(
  client: Client,
  document: String,
  variables: Json,
) -> Result(Dynamic, GraphqlError) {
  execute(client, document, variables)
}

/// Run a GraphQL mutation `document`; see `query`.
pub fn mutate(
  client: Client,
  document: String,
  variables: Json,
) -> Result(Dynamic, GraphqlError) {
  execute(client, document, variables)
}

fn execute(
  client: Client,
  document: String,
  variables: Json,
) -> Result(Dynamic, GraphqlError) {
  let body =
    json.to_string(
      json.object([#("query", json.string(document)), #("variables", variables)]),
    )
  case request.to(client.url) {
    Error(_) -> Error(HttpError("invalid url: " <> client.url))
    Ok(base) -> {
      let req =
        base
        |> request.set_method(http.Post)
        |> request.set_header("content-type", "application/json")
        |> request.set_header("authorization", "Bearer " <> client.token)
        |> request.set_body(body)
      case httpc.send(req) {
        Error(_) -> Error(HttpError("request to " <> client.url <> " failed"))
        Ok(resp) ->
          case resp.status >= 200 && resp.status < 300 {
            False -> Error(HttpError("HTTP " <> int.to_string(resp.status)))
            True -> parse(resp.body)
          }
      }
    }
  }
}

fn parse(body: String) -> Result(Dynamic, GraphqlError) {
  case json.parse(body, decode.dynamic) {
    Error(_) -> Error(DecodeError("invalid JSON response"))
    Ok(value) ->
      case decode.run(value, error_messages_decoder()) {
        Ok([_, ..] as messages) ->
          Error(GraphqlErrors(string.join(messages, "; ")))
        _ ->
          case decode.run(value, decode.at(["data"], decode.dynamic)) {
            Ok(data) -> Ok(data)
            Error(_) -> Error(DecodeError("response had no data field"))
          }
      }
  }
}

fn error_messages_decoder() -> decode.Decoder(List(String)) {
  decode.at(["errors"], decode.list(decode.at(["message"], decode.string)))
}
