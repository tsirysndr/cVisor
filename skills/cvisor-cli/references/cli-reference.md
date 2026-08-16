# cvisor CLI — full command reference

Every command, its arguments, and flags. The binary is `cvisor`.

There are three modes:

- **Local sandbox** (Linux only): run/shell/cp/cache/snapshot/doctor, in-process.
- **Remote** (`--remote`, any OS): a gRPC client to a `cvisord` daemon.
- **Web UI** (`cvisor ui`, any OS): serves the embedded UI against a daemon.

`cvisor -h` / `--help` prints usage to stdout and exits 0.

---

## Local sandbox (Linux)

The local sandbox installs its own seccomp filter, so it needs
`seccomp=unconfined` (under Docker: `--security-opt seccomp=unconfined
--security-opt apparmor=unconfined`). On non-Linux hosts the local mode errors;
use `--remote` instead.

### `cvisor [OPTIONS]`
Open an interactive sandboxed shell. Uses `/bin/bash` or `/usr/bin/bash` if
present, else `/bin/sh`. On a TTY it runs on a PTY in raw mode; when stdin is not
a terminal (piped/redirected) it runs a plain shell reading the inherited fd 0.

### `cvisor [OPTIONS] -- <cmd> [args...]`
Run `<cmd>` in the sandbox with stdio wired straight to the terminal; the CLI
exits with the command's exit code. Everything after `--` is the command.

### `cvisor [OPTIONS] cp <SRC> <DST>`
Copy a file or directory tree between host and sandbox overlay. Exactly one of
`SRC`/`DST` must be an `sb:`-prefixed sandbox path:

- `cvisor --sandbox dev cp ./src sb:/app`        — host → sandbox (recursive).
- `cvisor --sandbox dev cp sb:/app/dist ./dist`  — sandbox → host (recursive).

Two `sb:` paths, or none, is an error. Directory copies skip paths matched by
`.gitignore` / `.dockerignore`. With no `--sandbox`, targets the `default` sandbox.

### `cvisor [OPTIONS] cache save <KEY> <sb:PATH>`
Archive the sandbox directory `sb:PATH` under `KEY` into the cache backend.
Contents respect `.gitignore` / `.dockerignore`. `<sb:PATH>` must carry the `sb:`
prefix. Format from `--format`, backend from `--cache-backend`.

### `cvisor [OPTIONS] cache restore <KEY> <sb:PATH>`
Unpack the archive stored under `KEY` back into the sandbox directory `sb:PATH`.
Backend/format must match the save.

### `cvisor [OPTIONS] cache ls`
List cache entries in the configured backend, one per line: right-aligned
human-readable size, then the entry name.

### `cvisor [OPTIONS] cache rm <KEY>`  /  `cvisor [OPTIONS] cache rm --all`
Delete the entry under `KEY` (exit 1 if it did not exist), or with `--all` clear
the whole backend (prints how many entries were removed).

### `cvisor --sandbox <name> snapshot [ID]`
Snapshot the named sandbox's overlay and print the snapshot id. If `ID` is
omitted a `snap-XXXXXXXX` id is generated. Operates on a sandbox overlay, so pair
with `--sandbox`.

### `cvisor --sandbox <name> rollback <ID>`
Replace the named sandbox's overlay with snapshot `ID`, discarding changes made
since. Exit 1 on failure.

### `cvisor --sandbox <name> branch <ID>`
Populate the named sandbox's overlay from snapshot `ID` (seed a sandbox from a
saved snapshot). Exit 1 on failure.

### `cvisor snapshots [ls]`
List all saved snapshots as `id<TAB>size`. `ls` is the default subcommand.

### `cvisor snapshots rm <ID>`
Delete snapshot `ID`. Exit 1 if there is no such snapshot.

### `cvisor doctor`
Check that this host can run the sandbox, printing a `✔`/`✘` line per
prerequisite: Linux kernel + version, seccomp user-notifier (installs an
all-trap filter in a child), `pidfd_open`, `/proc` mounted, and an end-to-end
smoke run (`true` in the sandbox). Exits 0 only if all checks pass, else 1. A
failing smoke run usually means seccomp is not unconfined.

### Local OPTIONS

Placed before the command/subcommand (they are parsed until `--`, `cp`,
`cache`, `snapshot`, `rollback`, `branch`, `snapshots`, or `doctor`):

- `--sandbox <name>` — use a persistent, named sandbox (overlay survives across
  runs). Without it: run/shell are ephemeral (fresh random overlay); `cp`/`cache`
  default to the sandbox named `default`.
- `-e, --env <KEY=VAL>` — set a guest environment variable (repeatable). Must be
  `KEY=VALUE`.
- `--no-network` — deny outbound INET/INET6 networking.
- `--allow-listen` — permit inbound TCP servers (bind a fixed port, `listen`,
  `accept`). Off by default.
- `--timeout <ms>` — SIGKILL the guest after `<ms>` milliseconds. A timed-out run
  reports exit code 137.
- `--memory <size>` — cap guest memory (cgroup `memory.max`). Accepts a size like
  `256m`, `1g`.
- `--pids <n>` — cap the number of guest processes/threads (cgroup `pids.max`).
- `--cpu <percent>` — cap CPU as a percentage of one core (cgroup `cpu.max`):
  `50` = half a core, `200` = two cores.
- `--format <fmt>` — cache archive format: `gzip` (default), `zstd`, `estargz`,
  `none`. `zstd` requires a build with the `zstd` feature.
- `--cache-backend <b>` — cache store: `disk` (default), `disk:/path`, or
  `s3://bucket/prefix?region=..&endpoint=..`. `s3://` requires a build with the
  `s3` feature.
- `-h, --help` — show help (exit 0).

Resource limits (`--memory`/`--pids`/`--cpu`) need a writable cgroup v2
hierarchy; where unavailable they gracefully no-op and the run still succeeds.

---

## Remote mode (any OS)

Engaged when `--remote <ADDR>` is passed or the `CVISOR_REMOTE` env var is
non-empty (its presence alone selects client mode). A bearer token is required.
This is a gRPC client to a `cvisord` daemon, so it works from macOS too.

### `cvisor --remote <ADDR> [OPTIONS] [<SANDBOX>] [-- <cmd> [args...]]`

- `<ADDR>` — daemon address, e.g. `127.0.0.1:50051`, `host:50051`, or a full
  `http://host:50051` / `https://host:50051` URL (bare addresses get `http://`).
- `<SANDBOX>` — optional positional sandbox ref (id or name) to target; omitted
  runs ephemerally. Equivalent to `--sandbox <REF>`.
- With a trailing `-- <cmd>`, runs the command and streams stdout/stderr, exiting
  with its code. With **no** command, opens an interactive PTY shell.

OPTIONS:
- `--remote <ADDR>` — daemon address (overrides `CVISOR_REMOTE`).
- `--token <TOKEN>` — bearer token (or env `CVISOR_TOKEN`). Required.
- `--sandbox <REF>` — sandbox ref (alias for the positional `<SANDBOX>`).
- `--timeout <ms>` — SIGKILL the remote guest after `<ms>` milliseconds.
- `-h, --help` — show help.

Note: local-only flags (`--no-network`, `--memory`, `--env`, cache/snapshot
subcommands, …) are not part of remote mode; remote mode covers run and shell.

---

## Web UI (any OS)

### `cvisor ui [OPTIONS]`
Serve the embedded cVisor web UI from a small static server and open it in a
browser. The UI talks to a daemon's GraphQL endpoint.

OPTIONS:
- `--daemon <URL>` — daemon GraphQL base URL (default `http://localhost:8080`,
  env `CVISOR_DAEMON`).
- `--token <TOKEN>` — bearer token for the daemon (or env `CVISOR_TOKEN`).
- `--port <PORT>` — local port to serve the UI on (default `4321`).
- `--no-open` — do not open a browser.
- `-h, --help` — show help.

---

## Environment variables

- `CVISOR_REMOTE` — daemon address; a non-empty value selects remote mode. `--remote` overrides it.
- `CVISOR_TOKEN` — bearer token for remote mode and `cvisor ui`. `--token` overrides it.
- `CVISOR_DAEMON` — default daemon GraphQL URL for `cvisor ui`. `--daemon` overrides it.

## Exit codes

- `0` — success (or, for `-- <cmd>`, the command's own exit code when it is 0).
- The command's exit code for `-- <cmd>` runs (e.g. `137` for a `--timeout` SIGKILL).
- `1` — a runtime error (snapshot/cache/cp failure, connect failure, etc.).
- `2` — a usage error (bad flags, missing arguments, wrong `cp`/`cache` paths).

## The daemon it talks to (`cvisord`)

Remote mode and `cvisor ui` target a `cvisord` daemon, which exposes the same
runtime over **gRPC** (default `0.0.0.0:50051`, env `CVISOR_GRPC_ADDR`) and
**GraphQL** (default `0.0.0.0:8080`, env `CVISOR_HTTP_ADDR`), both guarded by a
bearer token (`CVISOR_TOKEN`, else one is generated and printed at startup).
Sandbox state persists in SQLite (`CVISOR_DB`, default `/tmp/.cvisor/cvisor.db`).
