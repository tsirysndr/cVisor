---
name: cvisor-cli
description: Reference for the cvisor CLI — an in-process Linux sandbox that virtualizes syscalls from userspace (seccomp user-notifier) to run untrusted or LLM-generated commands, with an interactive sandboxed shell, persistent named sandboxes, overlay snapshots, a directory cache, and a remote mode that drives a cvisord daemon over gRPC. Use when running, sandboxing, snapshotting, caching, or troubleshooting cvisor, or when scripting against the CLI. Covers every subcommand and its flags.
license: MIT
metadata:
  author: tsirysndr
  version: "1.0.0"
  homepage: https://github.com/tsirysndr/cVisor
---

# cvisor CLI

`cvisor` runs a command — or an interactive shell — inside a lightweight,
**in-process** Linux sandbox. It intercepts and virtualizes syscalls from
userspace with the **seccomp user-notifier**, giving each run a virtualized
filesystem (a copy-on-write overlay over the host) and network/resource controls
without a VM or a separate service. It's meant for untrusted or LLM-generated
code: a Docker/gVisor alternative with millisecond sandbox lifecycle.

The **local sandbox is Linux-only**. From any host (macOS included) use
**remote mode** (`--remote`) to drive a `cvisord` daemon over gRPC, or `cvisor ui`
to open the web UI against a daemon's GraphQL endpoint.

**For the full flag list of every command, read `references/cli-reference.md`.**

## Command map

Run / shell (Linux, in-process):
- `cvisor -- <cmd> [args]` — run a command in an ephemeral sandbox, streaming its stdio.
- `cvisor` — open an interactive sandboxed shell on a PTY (bash if present, else `/bin/sh`).
- `cvisor --sandbox <name> -- <cmd>` — run in a **persistent** named sandbox (files survive across invocations).

Files:
- `cvisor [--sandbox <name>] cp <SRC> <DST>` — copy a file/dir in or out; prefix the sandbox side with `sb:` (recursive; `.gitignore`/`.dockerignore` are honored).

Directory cache (archive a sandbox dir, restore it elsewhere):
- `cvisor [--sandbox <name>] cache save    <KEY> <sb:PATH>` — archive a sandbox dir under `KEY`.
- `cvisor [--sandbox <name>] cache restore <KEY> <sb:PATH>` — unpack it back into a sandbox dir.
- `cvisor cache ls` — list cached archives (size + name).
- `cvisor cache rm <KEY> | --all` — delete one entry or clear the backend.

Snapshots (capture/reuse a sandbox's overlay):
- `cvisor --sandbox <name> snapshot [ID]` — snapshot the overlay; prints the id (generated if omitted).
- `cvisor --sandbox <name> rollback <ID>` — replace the overlay with a snapshot (discard changes since).
- `cvisor --sandbox <name> branch <ID>` — populate the sandbox's overlay from a snapshot.
- `cvisor snapshots [ls]` — list snapshots (`id<TAB>size`).
- `cvisor snapshots rm <ID>` — delete a snapshot.

Diagnostics:
- `cvisor doctor` — check the host can run the sandbox (kernel, seccomp user-notifier, `pidfd_open`, `/proc`, an end-to-end smoke run). Exits 1 on any failure.

Remote mode (any OS; gRPC client to a `cvisord` daemon):
- `cvisor --remote <ADDR> [--token <T>] [<SANDBOX>] -- <cmd>` — run a command on a remote sandbox.
- `cvisor --remote <ADDR> [--token <T>] [<SANDBOX>]` — open an interactive PTY shell on the daemon.

Web UI:
- `cvisor ui [--daemon <URL>] [--token <T>] [--port <PORT>] [--no-open]` — serve the embedded web UI and open a browser (talks to a daemon's GraphQL endpoint).

## Common options (local run / shell)

- `--sandbox <name>` — use a persistent, named sandbox. Without it, run/shell are **ephemeral** (a fresh random overlay); `cp`/`cache` default to the sandbox named `default`.
- `-e, --env KEY=VAL` — set a guest env var (repeatable).
- `--no-network` — deny outbound INET/INET6 networking.
- `--allow-listen` — permit inbound TCP servers (bind a fixed port, `listen`/`accept`); off by default (outbound-only).
- `--timeout <ms>` — SIGKILL the guest after N ms (a timed-out run reports exit code 137).
- `--memory <size>` — cap guest memory, e.g. `256m`, `1g` (cgroup `memory.max`).
- `--pids <n>` — cap guest process count (cgroup `pids.max`).
- `--cpu <percent>` — cap CPU as a percent of one core: `50` = half a core, `200` = two cores (cgroup `cpu.max`).
- `--format <fmt>` — cache archive format: `gzip` (default), `zstd`, `estargz`, `none`.
- `--cache-backend <b>` — cache store: `disk` (default), `disk:/path`, or `s3://bucket/prefix?region=..&endpoint=..`.

## Key behaviors to remember

- **seccomp=unconfined is required.** cVisor installs its own seccomp filter, so
  under Docker run with `--security-opt seccomp=unconfined` (and, for ptrace-class
  ops, `--security-opt apparmor=unconfined`). If runs fail, run `cvisor doctor`.
- **Ephemeral vs named.** No `--sandbox` = a throwaway overlay per invocation.
  `--sandbox <name>` persists the overlay, so writes and snapshots survive across
  runs. `cp`/`cache` with no `--sandbox` target the `default` sandbox.
- **Filesystem is a CoW overlay over the host.** Reads pass through to the host
  (`cow` paths); writes land in the sandbox's private overlay. `/proc` is
  virtualized (e.g. `uname -n` → `cvisor`).
- **`cp` needs exactly one `sb:` side** — host↔sandbox only (not host↔host or
  sandbox↔sandbox). Directory copies and `cache save` skip ignored paths.
- **Resource limits need a writable cgroup v2 hierarchy;** where one is
  unavailable (many CI containers) the limits gracefully no-op and the run still
  succeeds.
- **`zstd` and `s3` are build-time features.** The distributed builds are pure
  Rust (gzip/estargz/none, disk backend); `--format zstd` and
  `--cache-backend s3://…` need a build with those features enabled.
- **Remote mode selection.** `--remote <addr>` or a non-empty `CVISOR_REMOTE`
  env var switches the CLI into the gRPC client; a token is required
  (`--token` or `CVISOR_TOKEN`). The daemon (`cvisord`) serves gRPC on
  `:50051` and GraphQL on `:8080` by default.

## Examples

```sh
# Run one command in a throwaway sandbox
cvisor -- uname -a

# Interactive sandboxed shell
cvisor

# Deny networking and cap resources
cvisor --no-network --memory 256m --pids 128 --cpu 50 -- ./build.sh

# Persistent sandbox: seed a file, then use it across runs
echo 'hi' > /tmp/x
cvisor --sandbox dev cp /tmp/x sb:/work/x
cvisor --sandbox dev -- cat /work/x            # -> hi

# Snapshot / rollback a persistent sandbox
cvisor --sandbox dev -- sh -c 'echo v1 > /work/state'
id=$(cvisor --sandbox dev snapshot)            # prints the snapshot id
cvisor --sandbox dev -- sh -c 'echo v2 > /work/state'
cvisor --sandbox dev rollback "$id"            # back to v1
cvisor snapshots                               # list them

# Directory cache round-trip (default disk backend, gzip)
cvisor --sandbox dev -- sh -c 'mkdir -p /work/deps && echo dep > /work/deps/a'
cvisor --sandbox dev cache save  deps-v1 sb:/work/deps
cvisor --sandbox other cache restore deps-v1 sb:/work/deps
cvisor cache ls

# Check the host, then remote-run against a daemon
cvisor doctor
cvisor --remote 127.0.0.1:50051 --token "$CVISOR_TOKEN" -- echo hi
cvisor --remote 127.0.0.1:50051 --token "$CVISOR_TOKEN"        # remote shell

# Serve the web UI against a daemon
cvisor ui --daemon http://localhost:8080 --token "$CVISOR_TOKEN"
```
