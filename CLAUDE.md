# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cVisor is an in-process Linux sandbox SDK and runtime written in **Rust** (ported from an original Zig prototype). It intercepts and virtualizes Linux syscalls from userspace using seccomp user notifier, providing isolation without VM overhead. Unlike gVisor (which runs as a separate service), cVisor runs directly in your application for millisecond-level sandbox lifecycle.

The goal of cVisor is to be a lightweight sandbox for untrusted user or LLM-generated code run on the server. Its most minimal implementation creates a virtualized filesystem and runs a bash command inside of it, but the goal is to increase sandboxing over time. This is intended as alternative to docker, gvisor, or other vm-based sandboxes.

**Status**: Rewritten from the original Zig prototype to **pure Rust**. The Rust
implementation under `crates/` is the sole codebase; it runs the full SDK surface
(45 syscall handlers, all filesystem backends, the process/namespace model,
virtualized `/proc`, and six language SDKs). The Zig tree has been removed.

**Greenfield project**: No users, no backward compatibility concerns. Delete dead code freely.

## Rust implementation (primary)

The Rust rewrite is a Cargo workspace:

```
Cargo.toml                     # workspace: crates/* + xtask
crates/
  cvisor-core/                 # the sandbox runtime (lib + `cvisor`/`smoke` bins)
    src/
      error.rs types.rs log_buffer.rs      # errno, stat ABI, output capture
      setup.rs supervisor.rs               # fork/handshake + recv→dispatch→send loop
      seccomp/{filter,notif,notifier}.rs   # BPF filter + notify ioctls (+ addfd trait)
      mem/                                 # GuestMem: process_vm_* (Real) / local ptr (tests)
      procinfo/                            # /proc parsing + kcmp (Real) / mocks (tests)
      virt/
        path.rs overlay_root.rs tombstones.rs symlinks.rs
        proc/                              # Threads arena: per-thread fd tables, namespaces
        fs/{fd_table,file,dirent,fs_info}.rs
        fs/backend/{mod,sys,procfile}.rs   # passthrough/cow/tmp/proc backends
  cvisor-ffi/                  # C-ABI cdylib (libcvisor.so) for the FFI SDKs
  cvisor-node/                 # napi-rs bindings → libcvisor.node
xtask/                         # `cargo xtask test|run|ffi|run-node|node-artifacts`
```

### Build & test commands (Rust)

```bash
cargo test -p cvisor-core                              # pure-logic tests on the host (macOS ok)
cargo xtask test [--arch aarch64|x86_64]              # full unit+e2e suite in Alpine (Docker, musl)
cargo xtask run                                        # run the sandbox binary in Alpine
cargo xtask ffi [--arch ...]                           # build libcvisor.so, patchelf, distribute to SDKs
cargo xtask run-node                                   # build .node + run sdks/node/test.ts in bun
cargo xtask node-artifacts                             # build libcvisor.node for all 4 platforms
```

**Requires**: Rust (stable) with the 4 linux targets, `cargo-zigbuild`, Docker.
Cross-compilation uses cargo-zigbuild; the musl `.so` has its `NEEDED` patched
to `libc.musl-<arch>.so.1` so it loads on any musl image.

### Key design notes (Rust)

- **Concurrency**: 1 reader thread (RECV ioctl) + bounded worker pool; all virtual
  state behind one `Mutex<VirtState>` (matches the original single global lock).
- **Refcounting**: Zig's manual refcounts became **id-keyed arenas inside `Threads`**
  (per-thread FD tables + fs-info, refcounted slots), so `Thread` stores ids not pointers.
- **Test injection** (replaces Zig `builtin.is_test`): `GuestMem`/`ProcInfo`/`Notifier`
  traits with Real vs Local/Mock impls, injected into `Supervisor`. `cargo test` is
  multithreaded, so no global mocks — each test builds its own supervisor.
- **e2e tests** (`tests/passthrough.rs`) run real shell commands under seccomp in Alpine.
- **Adding a handler**: add a `sys_*` method + a dispatch arm in `supervisor.rs`; unit-test
  it via the injection helpers and/or add an e2e command.

Two bugs from the Zig original are fixed in the port: the racy `dup(0)` notify-fd
handshake (now a race-free `/proc/<pid>/fd` scan + `pidfd_getfd`), and the
double-close from `FdTable.clone` sharing a raw fd (now dup-on-clone).

### Rust guidelines

- Keep `cargo clippy --tests` clean and code `rustfmt`-formatted.
- Handlers return `SysResult<T>` (`SysError` carries an `Errno`); the dispatcher
  turns errors into seccomp error replies. Prefer `?` over manual matching.
- Pure-logic modules (path routing, tombstones, dirent, errno, procfile parsing)
  must compile on any host so they unit-test natively; everything touching kernel
  APIs is gated with `#[cfg(target_os = "linux")]`.
- `unsafe` is confined to the FFI/syscall wrappers and the guest-memory bridge;
  each `unsafe` block carries a `// SAFETY:` note.
- Batch guest I/O (single capped read/write per syscall), avoid per-byte syscalls.

## SDKs

See `sdks/README.md`. Node uses napi (`libcvisor.node`); Bun, Deno, Python
(uv), Ruby (fiddle), and Erlang (NIF) all wrap the shared `libcvisor.so` C ABI.


## Key Linux APIs Used
- Seccomp user notifier (`SECCOMP_SET_MODE_FILTER`, `SECCOMP_IOCTL_NOTIF_*`)
- BPF filter programs
- `process_vm_readv`/`process_vm_writev` for cross-process memory
- `pidfd_open`/`pidfd_getfd` for FD operations across processes

**Preference**: Use `pidfd_getfd` to access child FDs rather than `proc/pid/fd` symlinks. This is more reliable and doesn't require filesystem access.


## Comment Style
- Only include comments if the code is not self-explanatory.
- Comments are intended to inform future readers about the code. Do not include commentary related to the conversations had with the user, which may look something like "Do ... (this is what we agreed on)". 
- Do NOT create section dividers like `// =============================================================================`. These are not useful and clutter the code. Do not add them.
- Do not remove or modify comments unless they are no longer accurate.