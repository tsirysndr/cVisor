//! The gRPC front-end: maps the `cvisor.proto` service onto the registry.
//!
//! Blocking/forking core calls run on `spawn_blocking`. `RunStream` streams a
//! command's output; `Shell` is a bidirectional stream driving an interactive
//! PTY session (client sends stdin/resize, server streams merged output).

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use cvisor_core::cgroup::Limits;
use cvisor_proto::cvisor as pb;
use pb::cvisor_server::Cvisor;

use crate::registry::{Overrides, Registry, SandboxInfo};

pub struct Grpc {
    reg: Arc<Registry>,
}

impl Grpc {
    pub fn new(reg: Arc<Registry>) -> Grpc {
        Grpc { reg }
    }
}

/// Run a blocking registry closure on the blocking pool, mapping both the join
/// error and the registry error to a gRPC `Status`.
async fn block<T, F>(f: F) -> Result<T, Status>
where
    T: Send + 'static,
    F: FnOnce() -> crate::registry::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Status::internal(format!("task join: {e}")))?
        .map_err(|e| Status::internal(e.to_string()))
}

fn limits_from(p: &pb::Limits) -> Limits {
    Limits {
        memory_max: (p.memory_max > 0).then_some(p.memory_max),
        pids_max: (p.pids_max > 0).then_some(p.pids_max),
        cpu_percent: (p.cpu_percent > 0).then_some(p.cpu_percent),
    }
}

fn limits_to(l: &Limits) -> pb::Limits {
    pb::Limits {
        memory_max: l.memory_max.unwrap_or(0),
        pids_max: l.pids_max.unwrap_or(0),
        cpu_percent: l.cpu_percent.unwrap_or(0),
    }
}

fn env_from(v: Vec<pb::EnvVar>) -> Vec<(String, String)> {
    v.into_iter().map(|e| (e.key, e.value)).collect()
}

fn sandbox_to(i: SandboxInfo) -> pb::Sandbox {
    pb::Sandbox {
        id: i.id,
        name: i.name,
        allow_network: i.allow_network,
        allow_listen: i.allow_listen,
        limits: Some(limits_to(&i.limits)),
        env: i
            .env
            .into_iter()
            .map(|(k, v)| pb::EnvVar { key: k, value: v })
            .collect(),
    }
}

type OutStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[tonic::async_trait]
impl Cvisor for Grpc {
    async fn create_sandbox(
        &self,
        req: Request<pb::CreateSandboxRequest>,
    ) -> Result<Response<pb::Sandbox>, Status> {
        let r = req.into_inner();
        let (name, repo_url) = (r.name, r.repo_url);
        let reg = self.reg.clone();
        let info = tokio::task::spawn_blocking(move || {
            reg.create_sandbox_with_repo(
                (!name.is_empty()).then_some(name.as_str()),
                (!repo_url.is_empty()).then_some(repo_url.as_str()),
            )
        })
        .await
        .map_err(|e| Status::internal(format!("task join: {e}")))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(sandbox_to(info)))
    }

    async fn list_sandboxes(
        &self,
        _req: Request<pb::Empty>,
    ) -> Result<Response<pb::SandboxList>, Status> {
        let sandboxes = self
            .reg
            .list_sandboxes()
            .into_iter()
            .map(sandbox_to)
            .collect();
        Ok(Response::new(pb::SandboxList { sandboxes }))
    }

    async fn free_sandbox(
        &self,
        req: Request<pb::SandboxRef>,
    ) -> Result<Response<pb::Empty>, Status> {
        let id = req.into_inner().id;
        let reg = self.reg.clone();
        block(move || reg.free_sandbox(&id)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn configure(
        &self,
        req: Request<pb::ConfigureRequest>,
    ) -> Result<Response<pb::Sandbox>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let limits = r.limits.as_ref().map(limits_from);
        let env = env_from(r.env);
        let info =
            block(move || reg.configure(&r.id, r.allow_network, r.allow_listen, limits, &env))
                .await?;
        Ok(Response::new(sandbox_to(info)))
    }

    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::RunResponse>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let ov = Overrides {
            allow_network: r.allow_network,
            limits: r.limits.as_ref().map(limits_from),
            env: env_from(r.env),
        };
        let out = block(move || reg.run(&r.id, &r.command, r.timeout_ms, ov)).await?;
        Ok(Response::new(pb::RunResponse {
            stdout: out.stdout,
            stderr: out.stderr,
            exit_code: out.exit_code,
        }))
    }

    type RunStreamStream = OutStream<pb::OutputChunk>;

    async fn run_stream(
        &self,
        req: Request<pb::RunRequest>,
    ) -> Result<Response<Self::RunStreamStream>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let (sid, session) = {
            let reg = reg.clone();
            let (id, command) = (r.id.clone(), r.command.clone());
            block(move || reg.start_session(&id, &command, false)).await?
        };
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            use pb::output_chunk::Kind;
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            loop {
                tick.tick().await;
                let out = session.read_stdout();
                if !out.is_empty()
                    && tx
                        .send(Ok(pb::OutputChunk {
                            kind: Some(Kind::Stdout(out)),
                        }))
                        .await
                        .is_err()
                {
                    break;
                }
                let err = session.read_stderr();
                if !err.is_empty()
                    && tx
                        .send(Ok(pb::OutputChunk {
                            kind: Some(Kind::Stderr(err)),
                        }))
                        .await
                        .is_err()
                {
                    break;
                }
                if let Some(code) = session.try_wait() {
                    let out = session.read_stdout();
                    if !out.is_empty() {
                        let _ = tx
                            .send(Ok(pb::OutputChunk {
                                kind: Some(Kind::Stdout(out)),
                            }))
                            .await;
                    }
                    let err = session.read_stderr();
                    if !err.is_empty() {
                        let _ = tx
                            .send(Ok(pb::OutputChunk {
                                kind: Some(Kind::Stderr(err)),
                            }))
                            .await;
                    }
                    let _ = tx
                        .send(Ok(pb::OutputChunk {
                            kind: Some(Kind::ExitCode(code)),
                        }))
                        .await;
                    break;
                }
            }
            // kill + thread joins are blocking; keep them off the async workers.
            let _ = tokio::task::spawn_blocking(move || reg.end_session(&sid)).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    type ShellStream = OutStream<pb::ShellOutput>;

    async fn shell(
        &self,
        req: Request<Streaming<pb::ShellInput>>,
    ) -> Result<Response<Self::ShellStream>, Status> {
        let reg = self.reg.clone();
        let mut inbound = req.into_inner();
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            use pb::shell_input::Kind as In;
            use pb::shell_output::Kind as Out;

            // The first message must be a Start describing the session.
            let start = match inbound.message().await {
                Ok(Some(pb::ShellInput {
                    kind: Some(In::Start(s)),
                })) => s,
                Ok(_) => {
                    let _ = tx
                        .send(Err(Status::invalid_argument("first message must be Start")))
                        .await;
                    return;
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };

            let (sid, session) = {
                let reg = reg.clone();
                let (id, command, pty) = (start.id, start.command, start.pty);
                match tokio::task::spawn_blocking(move || reg.start_session(&id, &command, pty))
                    .await
                {
                    Ok(Ok(v)) => v,
                    Ok(Err(e)) => {
                        let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                        return;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(Status::internal(format!("task join: {e}"))))
                            .await;
                        return;
                    }
                }
            };

            let mut tick = tokio::time::interval(Duration::from_millis(20));
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        let out = session.read_stdout();
                        if !out.is_empty()
                            && tx.send(Ok(pb::ShellOutput { kind: Some(Out::Output(out)) })).await.is_err()
                        {
                            break;
                        }
                        if let Some(code) = session.try_wait() {
                            let out = session.read_stdout();
                            if !out.is_empty() {
                                let _ = tx.send(Ok(pb::ShellOutput { kind: Some(Out::Output(out)) })).await;
                            }
                            let _ = tx.send(Ok(pb::ShellOutput { kind: Some(Out::ExitCode(code)) })).await;
                            break;
                        }
                    }
                    msg = inbound.message() => {
                        match msg {
                            Ok(Some(pb::ShellInput { kind: Some(In::Stdin(bytes)) })) => {
                                let _ = session.write_stdin(&bytes);
                            }
                            Ok(Some(pb::ShellInput { kind: Some(In::Resize(r)) })) => {
                                session.resize(r.rows as u16, r.cols as u16);
                            }
                            Ok(Some(_)) => {}
                            Ok(None) | Err(_) => {
                                // Client gone (terminal closed / disconnect). An
                                // interactive shell would otherwise outlive it
                                // forever, so end the session now.
                                break;
                            }
                        }
                    }
                }
            }
            // kill + thread joins are blocking; keep them off the async workers.
            let _ = tokio::task::spawn_blocking(move || reg.end_session(&sid)).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn write_file(
        &self,
        req: Request<pb::WriteFileRequest>,
    ) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.write_file(&r.id, &r.path, &r.data)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn read_file(
        &self,
        req: Request<pb::ReadFileRequest>,
    ) -> Result<Response<pb::FileData>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let data = block(move || reg.read_file(&r.id, &r.path)).await?;
        Ok(Response::new(pb::FileData { data }))
    }

    async fn copy_into(
        &self,
        req: Request<pb::CopyRequest>,
    ) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.copy_into(&r.id, &r.host_path, &r.guest_path)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn copy_out(&self, req: Request<pb::CopyRequest>) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.copy_out(&r.id, &r.guest_path, &r.host_path)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn cache_save(
        &self,
        req: Request<pb::CacheRequest>,
    ) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.cache_save(&r.id, &r.path, &r.key, &r.backend, &r.format)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn cache_restore(
        &self,
        req: Request<pb::CacheRequest>,
    ) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.cache_restore(&r.id, &r.path, &r.key, &r.backend, &r.format)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn cache_list(
        &self,
        req: Request<pb::CacheScope>,
    ) -> Result<Response<pb::CacheEntries>, Status> {
        let backend = req.into_inner().backend;
        let reg = self.reg.clone();
        let entries = block(move || reg.cache_list(&backend)).await?;
        Ok(Response::new(pb::CacheEntries {
            entries: entries
                .into_iter()
                .map(|e| pb::CacheEntry {
                    name: e.name,
                    size: e.size,
                })
                .collect(),
        }))
    }

    async fn cache_remove(
        &self,
        req: Request<pb::CacheRemoveRequest>,
    ) -> Result<Response<pb::CacheRemoved>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let removed = block(move || reg.cache_remove(&r.key, &r.backend, &r.format)).await?;
        Ok(Response::new(pb::CacheRemoved {
            removed,
            count: removed as u32,
        }))
    }

    async fn cache_clear(
        &self,
        req: Request<pb::CacheScope>,
    ) -> Result<Response<pb::CacheRemoved>, Status> {
        let backend = req.into_inner().backend;
        let reg = self.reg.clone();
        let count = block(move || reg.cache_clear(&backend)).await?;
        Ok(Response::new(pb::CacheRemoved {
            removed: count > 0,
            count,
        }))
    }

    async fn snapshot(
        &self,
        req: Request<pb::SnapshotRequest>,
    ) -> Result<Response<pb::SnapshotResponse>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let snapshot_id = block(move || reg.snapshot(&r.id, &r.snapshot_id)).await?;
        Ok(Response::new(pb::SnapshotResponse { snapshot_id }))
    }

    async fn rollback(
        &self,
        req: Request<pb::RollbackRequest>,
    ) -> Result<Response<pb::Empty>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        block(move || reg.rollback(&r.id, &r.snapshot_id)).await?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn branch(
        &self,
        req: Request<pb::BranchRequest>,
    ) -> Result<Response<pb::Sandbox>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let info = block(move || reg.branch(&r.snapshot_id, &r.name)).await?;
        Ok(Response::new(sandbox_to(info)))
    }

    async fn fork(&self, req: Request<pb::ForkRequest>) -> Result<Response<pb::Sandbox>, Status> {
        let r = req.into_inner();
        let reg = self.reg.clone();
        let info = block(move || reg.fork(&r.id, &r.name)).await?;
        Ok(Response::new(sandbox_to(info)))
    }

    async fn list_snapshots(
        &self,
        _req: Request<pb::Empty>,
    ) -> Result<Response<pb::CacheEntries>, Status> {
        let reg = self.reg.clone();
        let entries = block(move || reg.list_snapshots()).await?;
        Ok(Response::new(pb::CacheEntries {
            entries: entries
                .into_iter()
                .map(|e| pb::CacheEntry {
                    name: e.name,
                    size: e.size,
                })
                .collect(),
        }))
    }

    async fn delete_snapshot(
        &self,
        req: Request<pb::SnapshotRef>,
    ) -> Result<Response<pb::CacheRemoved>, Status> {
        let id = req.into_inner().id;
        let reg = self.reg.clone();
        let removed = block(move || reg.delete_snapshot(&id)).await?;
        Ok(Response::new(pb::CacheRemoved {
            removed,
            count: removed as u32,
        }))
    }

    async fn health(
        &self,
        _req: Request<pb::Empty>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        Ok(Response::new(pb::HealthResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: true,
        }))
    }
}
