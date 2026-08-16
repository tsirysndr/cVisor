//! The daemon's GraphQL front-end: an async-graphql schema over the shared
//! [`Registry`], plus the actix-web handlers (query/mutation POST, subscription
//! websocket, GraphiQL playground) that the server wires up.
//!
//! Binary payloads (stdout/stderr, file contents, stdin) cross the GraphQL
//! boundary as base64 strings. Registry methods that fork + supervise are run on
//! `spawn_blocking`; cheap in-memory ones (listing, session polling) are called
//! directly. `registry::Error` maps to `async_graphql::Error`.

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use async_graphql::http::GraphiQLSource;
use async_graphql::{Context, Error, InputObject, Object, Schema, SimpleObject, Subscription};
use async_graphql_actix_web::{GraphQLRequest, GraphQLSubscription};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use futures_util::Stream;
use tokio::task::spawn_blocking;

use crate::auth::{self, Auth};
use crate::registry::{self, Overrides, Registry};

/// The schema type the server stores as `web::Data` and executes against.
pub type CvisorSchema = Schema<Query, Mutation, Subscription>;

/// Build the executable schema, injecting the shared registry as context data.
pub fn schema(registry: Arc<Registry>) -> CvisorSchema {
    Schema::build(Query, Mutation, Subscription)
        .data(registry)
        .finish()
}

/// A sandbox's identity and configuration.
#[derive(SimpleObject)]
pub struct Sandbox {
    id: String,
    name: String,
    allow_network: bool,
    allow_listen: bool,
    env: Vec<EnvVar>,
    limits: Limits,
}

/// One environment variable of a sandbox.
#[derive(SimpleObject)]
pub struct EnvVar {
    key: String,
    value: String,
}

/// Resource limits; `u64` cgroup values are widened to `i64` for GraphQL.
#[derive(SimpleObject)]
pub struct Limits {
    memory_max: Option<i64>,
    pids_max: Option<i64>,
    cpu_percent: Option<i32>,
}

/// The result of a one-shot `run`. Bytes are exposed both as base64 and as a
/// lossy-UTF-8 convenience string.
#[derive(SimpleObject)]
pub struct RunResult {
    stdout_base64: String,
    stderr_base64: String,
    exit_code: i32,
    /// stdout decoded as lossy UTF-8.
    stdout: String,
    /// stderr decoded as lossy UTF-8.
    stderr: String,
}

/// An entry in a cache backend.
#[derive(SimpleObject)]
pub struct CacheEntry {
    name: String,
    size: i64,
}

/// A started background session, referenced by id.
#[derive(SimpleObject)]
pub struct Session {
    id: String,
}

/// Liveness and build info.
#[derive(SimpleObject)]
pub struct Health {
    version: String,
    ok: bool,
}

/// Resource-limit overrides supplied by a client.
#[derive(InputObject)]
pub struct LimitsInput {
    memory_max: Option<i64>,
    pids_max: Option<i64>,
    cpu_percent: Option<i32>,
}

/// One environment variable supplied by a client.
#[derive(InputObject)]
pub struct EnvVarInput {
    key: String,
    value: String,
}

impl From<registry::SandboxInfo> for Sandbox {
    fn from(s: registry::SandboxInfo) -> Self {
        Sandbox {
            id: s.id,
            name: s.name,
            allow_network: s.allow_network,
            allow_listen: s.allow_listen,
            env: s
                .env
                .into_iter()
                .map(|(key, value)| EnvVar { key, value })
                .collect(),
            limits: s.limits.into(),
        }
    }
}

impl From<cvisor_core::cgroup::Limits> for Limits {
    fn from(l: cvisor_core::cgroup::Limits) -> Self {
        Limits {
            memory_max: l.memory_max.map(|v| v as i64),
            pids_max: l.pids_max.map(|v| v as i64),
            cpu_percent: l.cpu_percent.map(|v| v as i32),
        }
    }
}

impl From<LimitsInput> for cvisor_core::cgroup::Limits {
    fn from(l: LimitsInput) -> Self {
        cvisor_core::cgroup::Limits {
            memory_max: l.memory_max.map(|v| v as u64),
            pids_max: l.pids_max.map(|v| v as u64),
            cpu_percent: l.cpu_percent.map(|v| v as u32),
        }
    }
}

impl From<registry::RunResult> for RunResult {
    fn from(r: registry::RunResult) -> Self {
        RunResult {
            stdout_base64: B64.encode(&r.stdout),
            stderr_base64: B64.encode(&r.stderr),
            exit_code: r.exit_code,
            stdout: String::from_utf8_lossy(&r.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&r.stderr).into_owned(),
        }
    }
}

/// Flatten optional env-var inputs into the registry's `(key, value)` pairs.
fn env_pairs(env: Option<Vec<EnvVarInput>>) -> Vec<(String, String)> {
    env.unwrap_or_default()
        .into_iter()
        .map(|e| (e.key, e.value))
        .collect()
}

/// Decode a base64 payload, surfacing a decode failure as a GraphQL error.
fn decode_b64(s: &str) -> async_graphql::Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|e| Error::new(format!("invalid base64: {e}")))
}

/// Pull the shared registry out of the request context.
fn reg(ctx: &Context<'_>) -> Arc<Registry> {
    ctx.data_unchecked::<Arc<Registry>>().clone()
}

pub struct Query;

#[Object]
impl Query {
    /// Liveness probe with the daemon's crate version.
    async fn health(&self) -> Health {
        Health {
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: true,
        }
    }

    /// Sandboxes, optionally substring-filtered and paginated. `limit`
    /// null/absent returns all; `offset` skips from the front.
    async fn sandboxes(
        &self,
        ctx: &Context<'_>,
        search: Option<String>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<Sandbox> {
        let (items, _total) = reg(ctx).search_sandboxes(
            &search.unwrap_or_default(),
            limit.map(i64::from).unwrap_or(-1),
            offset.map(i64::from).unwrap_or(0),
        );
        items.into_iter().map(Sandbox::from).collect()
    }

    /// Total number of sandboxes (for pagination/count badges).
    async fn sandbox_count(&self, ctx: &Context<'_>) -> i32 {
        reg(ctx).search_sandboxes("", -1, 0).1 as i32
    }

    /// Read a file from a sandbox; the returned string is base64.
    async fn read_file(
        &self,
        ctx: &Context<'_>,
        id: String,
        path: String,
    ) -> async_graphql::Result<String> {
        let reg = reg(ctx);
        let data = spawn_blocking(move || reg.read_file(&id, &path))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(B64.encode(&data))
    }

    /// List entries in a cache backend (default backend when `backend` is null).
    async fn cache_list(
        &self,
        ctx: &Context<'_>,
        backend: Option<String>,
    ) -> async_graphql::Result<Vec<CacheEntry>> {
        let reg = reg(ctx);
        let backend = backend.unwrap_or_default();
        let entries = spawn_blocking(move || reg.cache_list(&backend))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(entries
            .into_iter()
            .map(|e| CacheEntry {
                name: e.name,
                size: e.size as i64,
            })
            .collect())
    }

    /// The exit code of a session, or null if it is still running / unknown.
    async fn session_exit(&self, ctx: &Context<'_>, id: String) -> Option<i32> {
        reg(ctx).session(&id).and_then(|s| s.try_wait())
    }

    /// List saved overlay snapshots.
    async fn snapshots(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<CacheEntry>> {
        let reg = reg(ctx);
        let entries = spawn_blocking(move || reg.list_snapshots())
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(entries
            .into_iter()
            .map(|e| CacheEntry {
                name: e.name,
                size: e.size as i64,
            })
            .collect())
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    /// Create a sandbox; a null/empty name gets a random docker-style name.
    async fn create_sandbox(&self, ctx: &Context<'_>, name: Option<String>) -> Sandbox {
        reg(ctx).create_sandbox(name.as_deref()).into()
    }

    /// Free a sandbox and clean up its overlay.
    async fn free_sandbox(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<bool> {
        reg(ctx)
            .free_sandbox(&id)
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Update a sandbox's network/listen flags, limits, and env.
    async fn configure(
        &self,
        ctx: &Context<'_>,
        id: String,
        allow_network: Option<bool>,
        allow_listen: Option<bool>,
        limits: Option<LimitsInput>,
        env: Option<Vec<EnvVarInput>>,
    ) -> async_graphql::Result<Sandbox> {
        let env = env_pairs(env);
        let info = reg(ctx)
            .configure(
                &id,
                allow_network,
                allow_listen,
                limits.map(Into::into),
                &env,
            )
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(info.into())
    }

    /// Run a command. A null/empty `id` runs in a fresh ephemeral sandbox built
    /// from the supplied overrides.
    #[allow(clippy::too_many_arguments)] // GraphQL resolver args map to fields
    async fn run(
        &self,
        ctx: &Context<'_>,
        id: Option<String>,
        command: String,
        timeout_ms: Option<i64>,
        allow_network: Option<bool>,
        limits: Option<LimitsInput>,
        env: Option<Vec<EnvVarInput>>,
    ) -> async_graphql::Result<RunResult> {
        let reg = reg(ctx);
        let r = id.unwrap_or_default();
        let timeout = timeout_ms.unwrap_or(0).max(0) as u64;
        let ov = Overrides {
            allow_network,
            limits: limits.map(Into::into),
            env: env_pairs(env),
        };
        let out = spawn_blocking(move || reg.run(&r, &command, timeout, ov))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(out.into())
    }

    /// Write a base64 payload to a file inside a sandbox.
    async fn write_file(
        &self,
        ctx: &Context<'_>,
        id: String,
        path: String,
        data_base64: String,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        let data = decode_b64(&data_base64)?;
        spawn_blocking(move || reg.write_file(&id, &path, &data))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Copy a host file into a sandbox.
    async fn copy_into(
        &self,
        ctx: &Context<'_>,
        id: String,
        host_path: String,
        guest_path: String,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        spawn_blocking(move || reg.copy_into(&id, &host_path, &guest_path))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Copy a file out of a sandbox to the host.
    async fn copy_out(
        &self,
        ctx: &Context<'_>,
        id: String,
        guest_path: String,
        host_path: String,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        spawn_blocking(move || reg.copy_out(&id, &guest_path, &host_path))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Save a sandbox path into the cache under `key`.
    async fn cache_save(
        &self,
        ctx: &Context<'_>,
        id: String,
        path: String,
        key: String,
        backend: Option<String>,
        format: Option<String>,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        let backend = backend.unwrap_or_default();
        let format = format.unwrap_or_default();
        spawn_blocking(move || reg.cache_save(&id, &path, &key, &backend, &format))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Restore a cached entry into a sandbox path.
    async fn cache_restore(
        &self,
        ctx: &Context<'_>,
        id: String,
        path: String,
        key: String,
        backend: Option<String>,
        format: Option<String>,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        let backend = backend.unwrap_or_default();
        let format = format.unwrap_or_default();
        spawn_blocking(move || reg.cache_restore(&id, &path, &key, &backend, &format))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Remove a cached entry; returns whether it existed.
    async fn cache_remove(
        &self,
        ctx: &Context<'_>,
        key: String,
        backend: Option<String>,
        format: Option<String>,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        let backend = backend.unwrap_or_default();
        let format = format.unwrap_or_default();
        spawn_blocking(move || reg.cache_remove(&key, &backend, &format))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))
    }

    /// Clear a cache backend; returns the number of entries removed.
    async fn cache_clear(
        &self,
        ctx: &Context<'_>,
        backend: Option<String>,
    ) -> async_graphql::Result<i32> {
        let reg = reg(ctx);
        let backend = backend.unwrap_or_default();
        let n = spawn_blocking(move || reg.cache_clear(&backend))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(n as i32)
    }

    /// Start a background session. Defaults: empty command, PTY on.
    async fn start_session(
        &self,
        ctx: &Context<'_>,
        id: Option<String>,
        command: Option<String>,
        pty: Option<bool>,
    ) -> async_graphql::Result<Session> {
        let reg = reg(ctx);
        let r = id.unwrap_or_default();
        let command = command.unwrap_or_default();
        let pty = pty.unwrap_or(true);
        let (sid, _session) = spawn_blocking(move || reg.start_session(&r, &command, pty))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(Session { id: sid })
    }

    /// Write a base64 payload to a session's stdin; returns bytes written.
    async fn write_session(
        &self,
        ctx: &Context<'_>,
        id: String,
        data_base64: String,
    ) -> async_graphql::Result<i32> {
        let data = decode_b64(&data_base64)?;
        let session = reg(ctx)
            .session(&id)
            .ok_or_else(|| Error::new("no such session"))?;
        let n = session
            .write_stdin(&data)
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(n as i32)
    }

    /// Resize a session's PTY.
    async fn resize_session(&self, ctx: &Context<'_>, id: String, rows: i32, cols: i32) -> bool {
        match reg(ctx).session(&id) {
            Some(session) => {
                session.resize(rows.max(0) as u16, cols.max(0) as u16);
                true
            }
            None => false,
        }
    }

    /// Kill and drop a session; returns whether it existed.
    async fn kill_session(&self, ctx: &Context<'_>, id: String) -> bool {
        let reg = reg(ctx);
        let existed = reg.session(&id).is_some();
        reg.end_session(&id);
        existed
    }

    /// Snapshot a sandbox's overlay; a null/empty `snapshot_id` gets a generated
    /// id. Returns the snapshot id.
    async fn snapshot(
        &self,
        ctx: &Context<'_>,
        id: String,
        snapshot_id: Option<String>,
    ) -> async_graphql::Result<String> {
        let reg = reg(ctx);
        let snapshot_id = snapshot_id.unwrap_or_default();
        spawn_blocking(move || reg.snapshot(&id, &snapshot_id))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))
    }

    /// Replace a sandbox's overlay with a snapshot (discard changes since).
    async fn rollback(
        &self,
        ctx: &Context<'_>,
        id: String,
        snapshot_id: String,
    ) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        spawn_blocking(move || reg.rollback(&id, &snapshot_id))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Create a new sandbox from a snapshot; a null/empty name gets a random one.
    async fn branch(
        &self,
        ctx: &Context<'_>,
        snapshot_id: String,
        name: Option<String>,
    ) -> async_graphql::Result<Sandbox> {
        let reg = reg(ctx);
        let name = name.unwrap_or_default();
        let info = spawn_blocking(move || reg.branch(&snapshot_id, &name))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(info.into())
    }

    /// Fork a live sandbox's current overlay into a new sandbox; a null/empty
    /// name gets a random one.
    async fn fork(
        &self,
        ctx: &Context<'_>,
        id: String,
        name: Option<String>,
    ) -> async_graphql::Result<Sandbox> {
        let reg = reg(ctx);
        let name = name.unwrap_or_default();
        let info = spawn_blocking(move || reg.fork(&id, &name))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(info.into())
    }

    /// Delete a snapshot; returns whether it existed.
    async fn delete_snapshot(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<bool> {
        let reg = reg(ctx);
        spawn_blocking(move || reg.delete_snapshot(&id))
            .await
            .map_err(|e| Error::new(e.to_string()))?
            .map_err(|e| Error::new(e.to_string()))
    }
}

pub struct Subscription;

#[Subscription]
impl Subscription {
    /// Stream a session's output as base64 chunks, polling every ~30ms. For a
    /// PTY session the merged output arrives on stdout. Ends after a final drain
    /// once the session exits (or immediately if there is no such session).
    async fn session_output(&self, ctx: &Context<'_>, id: String) -> impl Stream<Item = String> {
        let reg = reg(ctx);
        async_stream::stream! {
            let session = match reg.session(&id) {
                Some(s) => s,
                None => return,
            };
            loop {
                let chunk = session.read_stdout();
                if !chunk.is_empty() {
                    yield B64.encode(&chunk);
                }
                if session.try_wait().is_some() {
                    let tail = session.read_stdout();
                    if !tail.is_empty() {
                        yield B64.encode(&tail);
                    }
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        }
    }
}

/// POST endpoint for queries and mutations. Schema introspection is allowed
/// anonymously (like gRPC reflection) so tools can fetch the schema; every other
/// operation requires the bearer token in the `Authorization` header.
pub async fn graphql_handler(
    schema: web::Data<CvisorSchema>,
    auth: web::Data<Arc<Auth>>,
    http_req: HttpRequest,
    req: GraphQLRequest,
) -> HttpResponse {
    let header = http_req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let req = req.into_inner();
    if !auth.check_header(header) && !is_introspection_only(&req.query) {
        return HttpResponse::Unauthorized().finish();
    }
    let resp = schema.execute(req).await;
    HttpResponse::Ok().json(resp)
}

/// True if every top-level field of every operation is an introspection
/// meta-field (`__schema` / `__type` / `__typename`) — GraphQL reserves the
/// `__` prefix for introspection, so no data is reachable this way.
fn is_introspection_only(query: &str) -> bool {
    use async_graphql::parser::types::Selection;
    let Ok(doc) = async_graphql::parser::parse_query(query) else {
        return false;
    };
    let mut seen = false;
    for (_, op) in doc.operations.iter() {
        for item in &op.node.selection_set.node.items {
            seen = true;
            match &item.node {
                Selection::Field(f) if f.node.name.node.starts_with("__") => {}
                // A non-introspection field or a fragment could reach data.
                _ => return false,
            }
        }
    }
    seen
}

/// Websocket endpoint for subscriptions. The bearer token is read from the
/// graphql-ws `connection_init` payload (`Authorization`/`authorization` bearer
/// value, or a bare `token` field) and rejected if it does not match.
pub async fn graphql_ws(
    schema: web::Data<CvisorSchema>,
    auth: web::Data<Arc<Auth>>,
    req: HttpRequest,
    payload: web::Payload,
) -> actix_web::Result<HttpResponse> {
    let auth = auth.get_ref().clone();
    GraphQLSubscription::new(schema.get_ref().clone())
        .on_connection_init(move |params| async move {
            let token = params
                .get("Authorization")
                .or_else(|| params.get("authorization"))
                .and_then(|v| v.as_str())
                .and_then(auth::strip_bearer)
                .or_else(|| params.get("token").and_then(|v| v.as_str()));
            if auth.check(token) {
                Ok(async_graphql::Data::default())
            } else {
                Err(Error::new("unauthorized"))
            }
        })
        .start(&req, payload)
}

/// Serve the GraphiQL playground HTML.
pub async fn graphql_playground() -> HttpResponse {
    let html = GraphiQLSource::build()
        .endpoint("/graphql")
        .subscription_endpoint("/graphql/ws")
        .finish();
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
