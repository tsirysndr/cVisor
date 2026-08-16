// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop backend: the `#[tauri::command]` handlers the frontend's Tauri
//! transport invokes. Each unary command opens a gRPC channel to the cvisord
//! daemon (bearer-token interceptor) and maps one RPC to a serde-serializable
//! reply. The interactive shell is a bidirectional stream whose server output is
//! forwarded to the frontend as `shell-output` events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use cvisor_proto::cvisor::cvisor_client::CvisorClient;
use cvisor_proto::cvisor::{
    shell_input, shell_output, BranchRequest, CacheRemoveRequest, CacheRequest, CacheScope,
    ConfigureRequest, CreateSandboxRequest, Empty, ForkRequest, ReadFileRequest, Resize,
    RollbackRequest, RunRequest, Sandbox, SandboxRef, ShellInput, ShellStart, SnapshotRef,
    SnapshotRequest, WriteFileRequest,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Request, Status};

mod tray;

/// The daemon gRPC address + bearer token, set from the frontend via `set_config`.
#[derive(Clone, Default)]
struct DaemonConfig {
    url: String,
    token: String,
}

/// Tauri-managed state: the connection config and the live shell senders.
struct AppState {
    config: Mutex<DaemonConfig>,
    shells: Arc<Mutex<HashMap<String, mpsc::Sender<ShellInput>>>>,
    counter: AtomicU64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: Mutex::new(DaemonConfig::default()),
            shells: Arc::new(Mutex::new(HashMap::new())),
            counter: AtomicU64::new(1),
        }
    }
}

type Interceptor = Box<dyn FnMut(Request<()>) -> Result<Request<()>, Status> + Send>;
type Client = CvisorClient<tonic::service::interceptor::InterceptedService<Channel, Interceptor>>;

/// Connect to the daemon, stamping `authorization: Bearer <token>` on every RPC.
async fn connect(cfg: &DaemonConfig) -> Result<Client, String> {
    if cfg.url.is_empty() {
        return Err("daemon not configured".into());
    }
    let uri = if cfg.url.starts_with("http://") || cfg.url.starts_with("https://") {
        cfg.url.clone()
    } else {
        format!("http://{}", cfg.url)
    };
    let channel = Channel::from_shared(uri)
        .map_err(|e| format!("bad daemon url: {e}"))?
        .connect()
        .await
        .map_err(|e| format!("connect failed: {e}"))?;
    let bearer: MetadataValue<_> = format!("Bearer {}", cfg.token)
        .parse()
        .map_err(|_| "invalid token".to_string())?;
    #[allow(clippy::result_large_err)]
    let interceptor: Interceptor = Box::new(move |mut req: Request<()>| {
        req.metadata_mut().insert("authorization", bearer.clone());
        Ok(req)
    });
    Ok(CvisorClient::with_interceptor(channel, interceptor))
}

async fn client(state: &AppState) -> Result<Client, String> {
    let cfg = state.config.lock().await.clone();
    connect(&cfg).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxDto {
    id: String,
    name: String,
    allow_network: bool,
    allow_listen: bool,
}

fn sandbox_dto(s: Sandbox) -> SandboxDto {
    SandboxDto {
        id: s.id,
        name: s.name,
        allow_network: s.allow_network,
        allow_listen: s.allow_listen,
    }
}

#[derive(Serialize)]
struct HealthDto {
    version: String,
    ok: bool,
}

#[derive(Serialize)]
struct CacheEntryDto {
    name: String,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunResultDto {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ShellOutputPayload {
    session_id: String,
    base64: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ShellExitPayload {
    session_id: String,
    code: i32,
}

/// Update the menu-bar tray status line (called by the frontend on probe).
#[tauri::command]
fn set_tray_status(app: AppHandle, ok: bool, detail: String) {
    tray::set_status(&app, ok, &detail);
}

#[tauri::command]
async fn set_config(state: State<'_, AppState>, url: String, token: String) -> Result<(), String> {
    let mut cfg = state.config.lock().await;
    cfg.url = url;
    cfg.token = token;
    Ok(())
}

#[tauri::command]
async fn health(state: State<'_, AppState>) -> Result<HealthDto, String> {
    let mut client = client(&state).await?;
    let r = client
        .health(Empty {})
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(HealthDto {
        version: r.version,
        ok: r.ok,
    })
}

#[tauri::command]
async fn list_sandboxes(state: State<'_, AppState>) -> Result<Vec<SandboxDto>, String> {
    let mut client = client(&state).await?;
    let list = client
        .list_sandboxes(Empty {})
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(list.sandboxes.into_iter().map(sandbox_dto).collect())
}

#[tauri::command]
async fn create_sandbox(
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<SandboxDto, String> {
    let mut client = client(&state).await?;
    let s = client
        .create_sandbox(CreateSandboxRequest {
            name: name.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(sandbox_dto(s))
}

#[tauri::command]
async fn free_sandbox(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut client = client(&state).await?;
    client
        .free_sandbox(SandboxRef { id })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn configure(
    state: State<'_, AppState>,
    id: String,
    allow_network: Option<bool>,
    allow_listen: Option<bool>,
) -> Result<SandboxDto, String> {
    let mut client = client(&state).await?;
    let s = client
        .configure(ConfigureRequest {
            id,
            allow_network,
            allow_listen,
            limits: None,
            env: vec![],
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(sandbox_dto(s))
}

#[tauri::command]
async fn run(
    state: State<'_, AppState>,
    id: Option<String>,
    command: String,
    timeout_ms: u64,
) -> Result<RunResultDto, String> {
    let mut client = client(&state).await?;
    let r = client
        .run(RunRequest {
            id: id.unwrap_or_default(),
            command,
            timeout_ms,
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(RunResultDto {
        stdout: String::from_utf8_lossy(&r.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&r.stderr).into_owned(),
        exit_code: r.exit_code,
    })
}

#[tauri::command]
async fn snapshot(
    state: State<'_, AppState>,
    id: String,
    snapshot_id: Option<String>,
) -> Result<String, String> {
    let mut client = client(&state).await?;
    let r = client
        .snapshot(SnapshotRequest {
            id,
            snapshot_id: snapshot_id.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(r.snapshot_id)
}

#[tauri::command]
async fn rollback(
    state: State<'_, AppState>,
    id: String,
    snapshot_id: String,
) -> Result<(), String> {
    let mut client = client(&state).await?;
    client
        .rollback(RollbackRequest { id, snapshot_id })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn branch(
    state: State<'_, AppState>,
    snapshot_id: String,
    name: Option<String>,
) -> Result<SandboxDto, String> {
    let mut client = client(&state).await?;
    let s = client
        .branch(BranchRequest {
            snapshot_id,
            name: name.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(sandbox_dto(s))
}

#[tauri::command]
async fn fork(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
) -> Result<SandboxDto, String> {
    let mut client = client(&state).await?;
    let s = client
        .fork(ForkRequest {
            id,
            name: name.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(sandbox_dto(s))
}

#[tauri::command]
async fn list_snapshots(state: State<'_, AppState>) -> Result<Vec<CacheEntryDto>, String> {
    let mut client = client(&state).await?;
    let entries = client
        .list_snapshots(Empty {})
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(entries
        .entries
        .into_iter()
        .map(|e| CacheEntryDto {
            name: e.name,
            size: e.size,
        })
        .collect())
}

#[tauri::command]
async fn delete_snapshot(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let mut client = client(&state).await?;
    let r = client
        .delete_snapshot(SnapshotRef { id })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(r.removed)
}

#[tauri::command]
async fn read_file(state: State<'_, AppState>, id: String, path: String) -> Result<String, String> {
    let mut client = client(&state).await?;
    let f = client
        .read_file(ReadFileRequest { id, path })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(B64.encode(&f.data))
}

#[tauri::command]
async fn write_file(
    state: State<'_, AppState>,
    id: String,
    path: String,
    data_base64: String,
) -> Result<(), String> {
    let mut client = client(&state).await?;
    let data = B64.decode(&data_base64).map_err(|e| e.to_string())?;
    client
        .write_file(WriteFileRequest { id, path, data })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn cache_list(
    state: State<'_, AppState>,
    backend: Option<String>,
) -> Result<Vec<CacheEntryDto>, String> {
    let mut client = client(&state).await?;
    let entries = client
        .cache_list(CacheScope {
            backend: backend.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(entries
        .entries
        .into_iter()
        .map(|e| CacheEntryDto {
            name: e.name,
            size: e.size,
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn cache_save(
    state: State<'_, AppState>,
    id: String,
    path: String,
    key: String,
    backend: Option<String>,
    format: Option<String>,
) -> Result<(), String> {
    let mut client = client(&state).await?;
    client
        .cache_save(CacheRequest {
            id,
            path,
            key,
            backend: backend.unwrap_or_default(),
            format: format.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn cache_restore(
    state: State<'_, AppState>,
    id: String,
    path: String,
    key: String,
    backend: Option<String>,
    format: Option<String>,
) -> Result<(), String> {
    let mut client = client(&state).await?;
    client
        .cache_restore(CacheRequest {
            id,
            path,
            key,
            backend: backend.unwrap_or_default(),
            format: format.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn cache_remove(
    state: State<'_, AppState>,
    key: String,
    backend: Option<String>,
    format: Option<String>,
) -> Result<bool, String> {
    let mut client = client(&state).await?;
    let r = client
        .cache_remove(CacheRemoveRequest {
            key,
            backend: backend.unwrap_or_default(),
            format: format.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(r.removed)
}

#[tauri::command]
async fn cache_clear(state: State<'_, AppState>, backend: Option<String>) -> Result<u32, String> {
    let mut client = client(&state).await?;
    let r = client
        .cache_clear(CacheScope {
            backend: backend.unwrap_or_default(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(r.count)
}

#[tauri::command]
async fn shell_open(
    app: AppHandle,
    state: State<'_, AppState>,
    sandbox_id: String,
) -> Result<String, String> {
    let mut client = client(&state).await?;
    let (tx, rx) = mpsc::channel::<ShellInput>(64);
    tx.send(ShellInput {
        kind: Some(shell_input::Kind::Start(ShellStart {
            id: sandbox_id,
            command: String::new(),
            pty: true,
        })),
    })
    .await
    .map_err(|e| e.to_string())?;

    let session_id = format!("s{}", state.counter.fetch_add(1, Ordering::Relaxed));
    state
        .shells
        .lock()
        .await
        .insert(session_id.clone(), tx);

    let outbound = ReceiverStream::new(rx);
    let shells = state.shells.clone();
    let sid = session_id.clone();

    tokio::spawn(async move {
        let mut inbound = match client.shell(outbound).await {
            Ok(r) => r.into_inner(),
            Err(_) => {
                let _ = app.emit(
                    "shell-exit",
                    ShellExitPayload {
                        session_id: sid.clone(),
                        code: -1,
                    },
                );
                shells.lock().await.remove(&sid);
                return;
            }
        };
        loop {
            match inbound.message().await {
                Ok(Some(msg)) => match msg.kind {
                    Some(shell_output::Kind::Output(b)) => {
                        let _ = app.emit(
                            "shell-output",
                            ShellOutputPayload {
                                session_id: sid.clone(),
                                base64: B64.encode(&b),
                            },
                        );
                    }
                    Some(shell_output::Kind::ExitCode(c)) => {
                        let _ = app.emit(
                            "shell-exit",
                            ShellExitPayload {
                                session_id: sid.clone(),
                                code: c,
                            },
                        );
                        break;
                    }
                    None => {}
                },
                Ok(None) => break,
                Err(_) => break,
            }
        }
        shells.lock().await.remove(&sid);
    });

    Ok(session_id)
}

#[tauri::command]
async fn shell_write(
    state: State<'_, AppState>,
    session_id: String,
    base64: String,
) -> Result<(), String> {
    let data = B64.decode(&base64).map_err(|e| e.to_string())?;
    let sender = state.shells.lock().await.get(&session_id).cloned();
    if let Some(tx) = sender {
        tx.send(ShellInput {
            kind: Some(shell_input::Kind::Stdin(data)),
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn shell_resize(
    state: State<'_, AppState>,
    session_id: String,
    rows: u32,
    cols: u32,
) -> Result<(), String> {
    let sender = state.shells.lock().await.get(&session_id).cloned();
    if let Some(tx) = sender {
        let _ = tx
            .send(ShellInput {
                kind: Some(shell_input::Kind::Resize(Resize { rows, cols })),
            })
            .await;
    }
    Ok(())
}

#[tauri::command]
async fn shell_close(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    // Dropping the sender ends the outbound stream; the daemon then closes it.
    state.shells.lock().await.remove(&session_id);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(tray::TrayStatus::default())
        .setup(|app| {
            // macOS-style system tray (Docker-Desktop-like).
            tray::install(app.handle())?;
            // Close the window to the tray instead of quitting (the app stays
            // alive in the menu bar; use the tray menu/Cmd-Q to actually quit).
            if let Some(win) = app.get_webview_window("main") {
                let w = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_tray_status,
            set_config,
            health,
            list_sandboxes,
            create_sandbox,
            free_sandbox,
            configure,
            run,
            snapshot,
            rollback,
            branch,
            fork,
            list_snapshots,
            delete_snapshot,
            read_file,
            write_file,
            cache_list,
            cache_save,
            cache_restore,
            cache_remove,
            cache_clear,
            shell_open,
            shell_write,
            shell_resize,
            shell_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
