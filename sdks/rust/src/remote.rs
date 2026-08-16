use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::client::{Client, Error, Result};

/// Captured result of a command run.
#[derive(Debug, Clone, Deserialize)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
}

/// One guest environment variable.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

/// A sandbox's resource limits as reported by the daemon (nulls = unset).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxLimits {
    pub memory_max: Option<i64>,
    pub pids_max: Option<i64>,
    pub cpu_percent: Option<i32>,
}

/// A sandbox's identity and configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxInfo {
    pub id: String,
    pub name: String,
    pub allow_network: bool,
    pub allow_listen: bool,
    pub env: Vec<EnvVar>,
    pub limits: SandboxLimits,
}

/// An entry in a cache backend or the snapshot list.
#[derive(Debug, Clone, Deserialize)]
pub struct CacheEntry {
    pub name: String,
    pub size: i64,
}

/// Daemon liveness/build info.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub version: String,
    pub ok: bool,
}

/// cgroup v2 resource caps to send to the daemon; a `None` field is left unset.
#[derive(Debug, Clone, Default)]
pub struct Limits {
    pub memory_max: Option<i64>,
    pub pids_max: Option<i64>,
    pub cpu_percent: Option<i32>,
}

impl Limits {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        if let Some(v) = self.memory_max {
            m.insert("memoryMax".into(), v.into());
        }
        if let Some(v) = self.pids_max {
            m.insert("pidsMax".into(), v.into());
        }
        if let Some(v) = self.cpu_percent {
            m.insert("cpuPercent".into(), v.into());
        }
        Value::Object(m)
    }
}

/// Optional per-run overrides for [`RemoteSandbox::run`].
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub timeout_ms: Option<i64>,
    pub allow_network: Option<bool>,
    pub limits: Option<Limits>,
    pub env: Vec<(String, String)>,
}

/// Fields [`RemoteSandbox::configure`] may update; `None`/empty = unchanged.
#[derive(Debug, Clone, Default)]
pub struct ConfigureOptions {
    pub allow_network: Option<bool>,
    pub allow_listen: Option<bool>,
    pub limits: Option<Limits>,
    pub env: Vec<(String, String)>,
}

fn env_to_value(env: &[(String, String)]) -> Value {
    Value::Array(
        env.iter()
            .map(|(k, v)| json!({ "key": k, "value": v }))
            .collect(),
    )
}

/// The GraphQL selection for a full [`SandboxInfo`].
const SANDBOX_FIELDS: &str =
    "id name allowNetwork allowListen env { key value } limits { memoryMax pidsMax cpuPercent }";

/// Pull one top-level field out of a `data` object and deserialize it.
fn field<T: DeserializeOwned>(mut data: Value, key: &str) -> Result<T> {
    let v = data.get_mut(key).map(Value::take).unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| Error::Decode(e.to_string()))
}

/// A high-level, typed client for the daemon that mirrors the GraphQL
/// operations. It optionally binds to a sandbox `id`; an empty id runs `run`
/// ephemerally.
#[derive(Clone)]
pub struct RemoteSandbox {
    client: Client,
    id: String,
}

impl RemoteSandbox {
    /// Build a handle talking to the daemon at `url`, with no bound sandbox id.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        RemoteSandbox {
            client: Client::new(url, token),
            id: String::new(),
        }
    }

    /// Build a handle from an existing [`Client`].
    pub fn with_client(client: Client) -> Self {
        RemoteSandbox {
            client,
            id: String::new(),
        }
    }

    /// Bind this handle to a sandbox id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// The bound sandbox id (`""` if unbound).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The underlying GraphQL client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Probe the daemon.
    pub fn health(&self) -> Result<Health> {
        let data = self
            .client
            .query("{ health { version ok } }", Value::Null)?;
        field(data, "health")
    }

    /// Create a sandbox (empty name gets a random one) and bind this handle to
    /// it.
    pub fn create_sandbox(&mut self, name: &str) -> Result<SandboxInfo> {
        let doc =
            format!("mutation($name:String){{ createSandbox(name:$name){{ {SANDBOX_FIELDS} }} }}");
        let vars = if name.is_empty() {
            json!({})
        } else {
            json!({ "name": name })
        };
        let info: SandboxInfo = field(self.client.mutate(&doc, vars)?, "createSandbox")?;
        self.id = info.id.clone();
        Ok(info)
    }

    /// List sandboxes, optionally full-text filtered and paginated. `limit < 0`
    /// returns all.
    pub fn list_sandboxes(
        &self,
        search: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SandboxInfo>> {
        let doc = format!(
            "query($search:String,$limit:Int,$offset:Int){{ sandboxes(search:$search,limit:$limit,offset:$offset){{ {SANDBOX_FIELDS} }} }}"
        );
        let mut vars = Map::new();
        if !search.is_empty() {
            vars.insert("search".into(), search.into());
        }
        if limit >= 0 {
            vars.insert("limit".into(), limit.into());
        }
        if offset > 0 {
            vars.insert("offset".into(), offset.into());
        }
        field(self.client.query(&doc, Value::Object(vars))?, "sandboxes")
    }

    /// Free the bound sandbox and clean up its overlay.
    pub fn free_sandbox(&self) -> Result<()> {
        self.client.mutate(
            "mutation($id:String!){ freeSandbox(id:$id) }",
            json!({ "id": self.id }),
        )?;
        Ok(())
    }

    /// Update the bound sandbox's network/listen flags, limits, and env.
    pub fn configure(&self, opts: ConfigureOptions) -> Result<SandboxInfo> {
        let doc = format!(
            "mutation($id:String!,$allowNetwork:Boolean,$allowListen:Boolean,$limits:LimitsInput,$env:[EnvVarInput!]){{ configure(id:$id,allowNetwork:$allowNetwork,allowListen:$allowListen,limits:$limits,env:$env){{ {SANDBOX_FIELDS} }} }}"
        );
        let mut vars = Map::new();
        vars.insert("id".into(), self.id.clone().into());
        if let Some(v) = opts.allow_network {
            vars.insert("allowNetwork".into(), v.into());
        }
        if let Some(v) = opts.allow_listen {
            vars.insert("allowListen".into(), v.into());
        }
        if let Some(l) = &opts.limits {
            vars.insert("limits".into(), l.to_value());
        }
        if !opts.env.is_empty() {
            vars.insert("env".into(), env_to_value(&opts.env));
        }
        field(self.client.mutate(&doc, Value::Object(vars))?, "configure")
    }

    /// Run `command` in the bound sandbox (or ephemerally when unbound).
    pub fn run(&self, command: &str, opts: RunOptions) -> Result<Output> {
        let doc = "mutation($id:String,$command:String!,$timeoutMs:Int,$allowNetwork:Boolean,$limits:LimitsInput,$env:[EnvVarInput!]){ run(id:$id,command:$command,timeoutMs:$timeoutMs,allowNetwork:$allowNetwork,limits:$limits,env:$env){ stdout stderr exitCode } }";
        let mut vars = Map::new();
        vars.insert("command".into(), command.into());
        if !self.id.is_empty() {
            vars.insert("id".into(), self.id.clone().into());
        }
        if let Some(t) = opts.timeout_ms {
            vars.insert("timeoutMs".into(), t.into());
        }
        if let Some(v) = opts.allow_network {
            vars.insert("allowNetwork".into(), v.into());
        }
        if let Some(l) = &opts.limits {
            vars.insert("limits".into(), l.to_value());
        }
        if !opts.env.is_empty() {
            vars.insert("env".into(), env_to_value(&opts.env));
        }
        field(self.client.mutate(doc, Value::Object(vars))?, "run")
    }

    /// Write `data` to `path` inside the bound sandbox's overlay.
    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let doc = "mutation($id:String!,$path:String!,$data:String!){ writeFile(id:$id,path:$path,dataBase64:$data) }";
        let vars = json!({ "id": self.id, "path": path, "data": B64.encode(data) });
        self.client.mutate(doc, vars)?;
        Ok(())
    }

    /// Read `path` from the bound sandbox, decoding the base64 payload.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let doc = "query($id:String!,$path:String!){ readFile(id:$id,path:$path) }";
        let data = self
            .client
            .query(doc, json!({ "id": self.id, "path": path }))?;
        let b64: String = field(data, "readFile")?;
        B64.decode(b64.as_bytes())
            .map_err(|e| Error::Decode(e.to_string()))
    }

    /// Snapshot the bound sandbox's overlay (empty id gets a generated one).
    pub fn snapshot(&self, snapshot_id: &str) -> Result<String> {
        let doc =
            "mutation($id:String!,$snapshotId:String){ snapshot(id:$id,snapshotId:$snapshotId) }";
        let mut vars = Map::new();
        vars.insert("id".into(), self.id.clone().into());
        if !snapshot_id.is_empty() {
            vars.insert("snapshotId".into(), snapshot_id.into());
        }
        field(self.client.mutate(doc, Value::Object(vars))?, "snapshot")
    }

    /// Replace the bound sandbox's overlay with a snapshot.
    pub fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let doc =
            "mutation($id:String!,$snapshotId:String!){ rollback(id:$id,snapshotId:$snapshotId) }";
        self.client
            .mutate(doc, json!({ "id": self.id, "snapshotId": snapshot_id }))?;
        Ok(())
    }

    /// Create a new sandbox from a snapshot.
    pub fn branch(&self, snapshot_id: &str, name: &str) -> Result<SandboxInfo> {
        let doc = format!(
            "mutation($snapshotId:String!,$name:String){{ branch(snapshotId:$snapshotId,name:$name){{ {SANDBOX_FIELDS} }} }}"
        );
        let mut vars = Map::new();
        vars.insert("snapshotId".into(), snapshot_id.into());
        if !name.is_empty() {
            vars.insert("name".into(), name.into());
        }
        field(self.client.mutate(&doc, Value::Object(vars))?, "branch")
    }

    /// Fork the bound sandbox's current overlay into a new sandbox.
    pub fn fork(&self, name: &str) -> Result<SandboxInfo> {
        let doc = format!(
            "mutation($id:String!,$name:String){{ fork(id:$id,name:$name){{ {SANDBOX_FIELDS} }} }}"
        );
        let mut vars = Map::new();
        vars.insert("id".into(), self.id.clone().into());
        if !name.is_empty() {
            vars.insert("name".into(), name.into());
        }
        field(self.client.mutate(&doc, Value::Object(vars))?, "fork")
    }

    /// List saved overlay snapshots.
    pub fn snapshots(&self) -> Result<Vec<CacheEntry>> {
        let data = self
            .client
            .query("{ snapshots { name size } }", Value::Null)?;
        field(data, "snapshots")
    }

    /// Delete a snapshot; returns whether it existed.
    pub fn delete_snapshot(&self, id: &str) -> Result<bool> {
        let data = self.client.mutate(
            "mutation($id:String!){ deleteSnapshot(id:$id) }",
            json!({ "id": id }),
        )?;
        field(data, "deleteSnapshot")
    }

    /// Archive a sandbox path under `key`. Empty `backend`/`format` use the
    /// daemon defaults (disk backend, gzip).
    pub fn cache_save(&self, path: &str, key: &str, backend: &str, format: &str) -> Result<()> {
        self.cache_op(true, path, key, backend, format)
    }

    /// Restore a cached entry into a sandbox path.
    pub fn cache_restore(&self, path: &str, key: &str, backend: &str, format: &str) -> Result<()> {
        self.cache_op(false, path, key, backend, format)
    }

    fn cache_op(
        &self,
        save: bool,
        path: &str,
        key: &str,
        backend: &str,
        format: &str,
    ) -> Result<()> {
        let op = if save { "cacheSave" } else { "cacheRestore" };
        let doc = format!(
            "mutation($id:String!,$path:String!,$key:String!,$backend:String,$format:String){{ {op}(id:$id,path:$path,key:$key,backend:$backend,format:$format) }}"
        );
        let mut vars = Map::new();
        vars.insert("id".into(), self.id.clone().into());
        vars.insert("path".into(), path.into());
        vars.insert("key".into(), key.into());
        if !backend.is_empty() {
            vars.insert("backend".into(), backend.into());
        }
        if !format.is_empty() {
            vars.insert("format".into(), format.into());
        }
        self.client.mutate(&doc, Value::Object(vars))?;
        Ok(())
    }

    /// List entries in a cache backend (empty backend = default).
    pub fn cache_list(&self, backend: &str) -> Result<Vec<CacheEntry>> {
        let doc = "query($backend:String){ cacheList(backend:$backend){ name size } }";
        let mut vars = Map::new();
        if !backend.is_empty() {
            vars.insert("backend".into(), backend.into());
        }
        field(self.client.query(doc, Value::Object(vars))?, "cacheList")
    }
}
