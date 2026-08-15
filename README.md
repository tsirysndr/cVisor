### cVisor - Embedded Bash Sandbox for Agents
[![CI](https://github.com/tsirysndr/bVisor/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/bVisor/actions/workflows/ci.yml)

cVisor is an SDK and runtime for safely executing bash commands locally, without the need for remote sandboxes or local VMs/containers. 

Inspired by [gVisor](https://github.com/google/gVisor), cVisor runs programs directly on the host machine, providing isolation by intercepting and virtualizing [Linux syscalls](https://en.wikipedia.org/wiki/System_call) from userspace. 

Unlike gVisor, cVisor is built to run directly in your application, spinning up sandboxes in ~2 milliseconds. This makes it ideal for ephemeral tasks commonly performed by LLM agents, such as code execution or filesystem operations.

**Status**: cVisor is an early proof-of-concept and should not yet be used in production. If you detect any discrepancies between cVisor's behavior and the linux kernel, please file an issue.

**Compatibility**: cVisor currently ships for Linux hosts only, with support for ARM and X86 architectures and glibc/musl ABIs.

> **Note**: cVisor is a fork of [bVisor](https://github.com/butter-dot-dev/bVisor), rewritten in **Rust** (the original is written in Zig).

## Quick try

Drop into a Python REPL with cvisor installed, from any machine with Docker:

```bash
docker run -it --rm \
  --security-opt seccomp=unconfined --security-opt apparmor=unconfined \
  ghcr.io/astral-sh/uv:python3.12-alpine \
  uv run --with cvisor python
```

```python
>>> from cvisor import Sandbox
>>> sb = Sandbox()
>>> print(sb.run("echo hi; uname -n").stdout)
hi
cvisor
```

The `--security-opt` flags are required: cVisor installs its own seccomp
filter, which Docker's default profiles block.

## Usage

The cVisor runtime ships wrapped in a Typescript SDK, installed via npm.

```bash
npm install cvisor
```

Example usage:
```typescript
import { Sandbox } from "cvisor";

const sb = new Sandbox();
const output = sb.runCmd("echo 'Hello, world!'");

console.log(await output.stdout());
```

This executes `echo 'Hello, world!'` inside a sandbox.

Filesystem operations are safely virtualized:
```typescript
sb.runCmd("echo 'Hello, world!' > /tmp/test.txt"); // only visible from this sandbox
```

Unsafe commands are blocked:
```typescript
sb.runCmd("chroot /tmp"); // error
```

Python, Ruby, Erlang, Clojure, Bun, and Deno SDKs are also published — see
[sdks/README.md](sdks/README.md).

## Command-line

The `cvisor` binary runs a command in the sandbox, drops you into an interactive
sandboxed shell, or copies files in and out:

```bash
cvisor -- uname -a                                # run a command, streaming its output
cvisor                                            # interactive shell (bash if present) on a PTY
cvisor -e FOO=bar -e TOKEN=xyz -- printenv FOO    # pass environment variables
cvisor --no-network -- ...                        # deny outbound networking
cvisor --allow-listen -- ...                      # permit inbound TCP servers
cvisor --timeout 5000 -- ...                      # SIGKILL the guest after 5s

# Named, persistent sandboxes + file/dir transfer (survive across invocations):
cvisor --sandbox dev cp ./src sb:/app              # host -> sandbox (recursive)
cvisor --sandbox dev -- python3 /app/main.py       # run against the copied files
cvisor --sandbox dev cp sb:/app/dist ./dist        # sandbox -> host (recursive)

# Cache a directory (backup/restore, keyed) — for build caches, deps, etc.:
cvisor --sandbox dev cache save deps-v1 sb:/app/node_modules
cvisor --sandbox ci  cache restore deps-v1 sb:/app/node_modules   # exact or prefix hit
cvisor cache ls                    # list cached archives
cvisor cache rm deps-v1            # remove one, or `cache rm --all`

# Check the host can run the sandbox:
cvisor doctor
```

It exits with the guest's exit code. With no `--` command and a terminal on
stdin, it starts an interactive shell (`isatty`, job control, and line editing
all work); otherwise it runs a shell reading stdin. In `cp`/`cache`, prefix the
sandbox side with `sb:`. Without `--sandbox`, runs are ephemeral; `--sandbox
<name>` gives a persistent overlay so copied, written, and restored files
persist.

`cp` and `cache save` are recursive and skip paths matched by `.gitignore` /
`.dockerignore`. `cache` archives to the host disk by default (or
`--cache-backend s3://bucket/prefix`) as gzip (default), `--format estargz`,
`zstd`, or `none`; `cache restore <KEY>` takes an exact key or falls back to the
newest archive with that key prefix. The distributed binaries built by CI enable
all formats and the S3 backend.

### Docker

A prebuilt Alpine image (all features — every archive format and the S3 cache
backend) is published to GHCR. cVisor installs its own seccomp filter, so run
with the default profile disabled:

```bash
docker run --rm -it --security-opt seccomp=unconfined ghcr.io/tsirysndr/cvisor           # interactive shell
docker run --rm --security-opt seccomp=unconfined ghcr.io/tsirysndr/cvisor -- uname -a    # run a command
```

Or build it yourself from the repo `Dockerfile` (`docker build -t cvisor .`).
Prebuilt static binaries for linux x86_64 and aarch64 are attached to each
[GitHub Release](https://github.com/tsirysndr/cVisor/releases) (tag `v*`).

### Nix

A [crane](https://github.com/ipetkov/crane)-based flake builds the CLI (all
features) for `x86_64-linux` and `aarch64-linux`. With flakes enabled:

```bash
nix run    github:tsirysndr/cVisor               # run without installing
nix build  github:tsirysndr/cVisor#cvisor        # build -> ./result/bin/cvisor
nix profile install github:tsirysndr/cVisor      # install into your profile
nix develop github:tsirysndr/cVisor              # dev shell (rust toolchain + tools)
```

(If flakes aren't on yet, add `--extra-experimental-features 'nix-command flakes'`
to each command, or enable them in `nix.conf`.)

#### Speed it up with Cachix

The flake builds the full feature set (zstd + the S3 cache backend), which pulls
in C dependencies (`ring`, `zstd-sys`) — a cold build compiles a lot. The
project pushes prebuilt store paths to a public [Cachix](https://cachix.org)
cache, so pointing Nix at it turns the "build" into a download:

```bash
# one-time: install the cachix client and trust the cvisor cache
nix profile install nixpkgs#cachix
cachix use cvisor

# now builds/runs pull prebuilt paths instead of compiling
nix run github:tsirysndr/cVisor -- uname -a
```

`cachix use cvisor` adds the cache as a substituter and its public key to your
Nix config. Alternatively, add it to `nix.conf` (or a flake `nixConfig`) by hand:

```
extra-substituters = https://cvisor.cachix.org
extra-trusted-public-keys = cvisor.cachix.org-1:<key shown by `cachix use cvisor`>
```

## Examples

Here are a selection of full examples which currently work in cVisor:
- [Hello World](sdks/node/examples/hello-world.ts) - Run your first command in the sandbox
- [Running Python](sdks/node/examples/python-hello.ts) - Write and execute a Python script 
- [Testing Sandbox Boundaries](sdks/node/examples/sandbox-boundaries.ts) - See how the sandbox handles host fingerprinting, blocked paths, and filesystem isolation
- [Filesystem Operations](sdks/node/examples/nested-dirs.ts) - Demonstrate directory creation, file operations, running scripts

## Architecture

cVisor is built on [Seccomp user notifier](https://man7.org/linux/man-pages/man2/seccomp.2.html), a Linux kernel feature that allows userspace processes to intercept and optionally handle syscalls from a child process. This allows cVisor to block or mock the kernel API (such as filesystem read/write, network access, etc.) to ensure the child process remains sandboxed.

Other than the overhead of syscall emulation, child processes run natively.

cVisor is imageless, meaning it does not require a base image to run. It runs with direct visibility to the host filesystem. This allows system dependencies such as `npm` to work out of the box. Isolation is achieved via a copy-on-write overlay on top of the host filesystem. Files opened with write flags are copied to a sandbox-local directory. Read-only files are passed through to the real filesystem.

## Syscall Support

Every Linux syscall falls into one of four categories in cVisor:

#### Virtualized
Syscalls are intercepted and handled in userspace by the cVisor virtual kernel.

| | Syscalls |
|-|----------|
| File I/O | `openat`, `close`, `read`, `write`, `readv`, `writev`, `lseek`, `dup`, `dup3`, `fcntl`, `ioctl`, `pipe2` |
| File metadata | `fstat`, `fstatat64`, `faccessat`, `utimensat`, `fchmodat` |
| Directory | `getcwd`, `chdir`, `fchdir`, `getdents64`, `mkdirat`, `unlinkat`, `symlinkat`, `readlinkat` |
| Process | `getpid`, `getppid`, `gettid`, `kill`, `tkill`, `exit`, `exit_group`, `execve` |
| Networking | `socket`, `socketpair`, `connect`, `shutdown`, `sendto`, `recvfrom`, `sendmsg`, `recvmsg` |
| System info | `uname`, `sysinfo` |
| Events | `eventfd2` |

Note that cVisor may still call into the underlying kernel to virtualize any given syscall.

#### Passthrough
Syscalls are forwarded to the kernel unmodified. These syscalls are process-local or read-only and do not require any virtualization.

| | Syscalls |
|-|----------|
| Process | `clone`, `wait4`, `waitid`, `set_tid_address` |
| Identity | `getuid`, `geteuid`, `getgid`, `getegid` |
| Memory | `brk`, `mmap`, `mprotect`, `munmap`, `mremap`, `madvise` |
| Signals | `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn`, `rt_sigsuspend`, `rt_sigpending`, `rt_sigtimedwait`, `sigaltstack`, `restart_syscall` |
| Time | `clock_gettime`, `clock_getres`, `gettimeofday`, `nanosleep`, `clock_nanosleep` |
| Sync | `futex`, `futex_wait`, `futex_wake`, `futex_requeue`, `futex_waitv`, `set_robust_list`, `rseq` |
| Random | `getrandom` |

#### Blocked
Syscalls are blocked and return `ENOSYS` or `EPERM`. These could allow sandbox escape or privilege escalation.

| | Syscalls |
|-|----------|
| Privilege escalation | `ptrace`, `mount`, `umount2`, `chroot`, `pivot_root`, `reboot`, `setns`, `unshare`, `seccomp`, `bpf` |
| Cross-process memory | `process_vm_readv`, `process_vm_writev` |
| Kernel modules | `kexec_load`, `kexec_file_load`, `init_module`, `finit_module`, `delete_module` |
| Resource control | `setrlimit`, `prlimit64` |
| Execution domain | `personality` |
| Server sockets | `bind`, `listen`, `accept`, `accept4` |

#### Roadmap
Not yet handled but likely necessary for Bash compatibility. Currently return `ENOSYS`.

| | Syscalls |
|-|----------|
| System info | `getrlimit`, `getrusage` |
| Resource limits | not started (cgroups) |

<details>
<summary>See full list of other unhandled syscalls (~240)</summary>

| | Syscalls |
|-|----------|
| File I/O | `pread64`, `pwrite64`, `preadv`, `pwritev`, `preadv2`, `pwritev2`, `sendfile`, `splice`, `tee`, `vmsplice`, `readahead`, `copy_file_range` |
| File metadata | `statx`, `statfs`, `fstatfs`, `truncate`, `ftruncate`, `fallocate`, `fadvise64`, `flock`, `fchmod`, `fchmodat2`, `fchown`, `fchownat`, `faccessat2`, `cachestat` |
| Directory | `mknodat`, `linkat`, `renameat`, `renameat2` |
| Process | `execveat`, `clone3`, `tgkill`, `prctl`, `pidfd_open`, `pidfd_getfd`, `pidfd_send_signal`, `kcmp`, `userfaultfd` |
| System info | `syslog`, `umask`, `getcpu`, `acct`, `vhangup`, `sethostname`, `setdomainname` |
| Identity (write) | `setuid`, `setgid`, `setreuid`, `setregid`, `setresuid`, `getresuid`, `setresgid`, `getresgid`, `setfsuid`, `setfsgid`, `getgroups`, `setgroups`, `setpriority`, `getpriority` |
| Session/pgid | `setpgid`, `getpgid`, `getsid`, `setsid` |
| Memory | `msync`, `mlock`, `munlock`, `mlockall`, `munlockall`, `mincore`, `remap_file_pages`, `mbind`, `get_mempolicy`, `set_mempolicy`, `set_mempolicy_home_node`, `migrate_pages`, `move_pages`, `process_madvise`, `mlock2`, `memfd_create`, `memfd_secret`, `map_shadow_stack`, `pkey_mprotect`, `pkey_alloc`, `pkey_free`, `mseal`, `membarrier`, `process_mrelease` |
| Signals | `rt_sigqueueinfo`, `rt_tgsigqueueinfo`, `signalfd4` |
| Time | `clock_settime`, `clock_adjtime`, `settimeofday`, `adjtimex`, `getitimer`, `setitimer`, `times`, `timer_create`, `timer_gettime`, `timer_getoverrun`, `timer_settime`, `timer_delete`, `timerfd_create`, `timerfd_settime`, `timerfd_gettime` |
| Networking | `getsockname`, `getpeername`, `setsockopt`, `getsockopt`, `sendmmsg`, `recvmmsg` |
| Polling/events | `epoll_create1`, `epoll_ctl`, `epoll_pwait`, `epoll_pwait2`, `pselect6`, `ppoll` |
| File sync | `sync`, `fsync`, `fdatasync`, `sync_file_range`, `syncfs` |
| File handles | `name_to_handle_at`, `open_by_handle_at`, `openat2`, `close_range` |
| Async I/O | `io_setup`, `io_destroy`, `io_submit`, `io_cancel`, `io_getevents`, `io_pgetevents`, `io_uring_setup`, `io_uring_enter`, `io_uring_register` |
| IPC | `mq_open`, `mq_unlink`, `mq_timedsend`, `mq_timedreceive`, `mq_notify`, `mq_getsetattr`, `msgget`, `msgctl`, `msgrcv`, `msgsnd`, `semget`, `semctl`, `semtimedop`, `semop`, `shmget`, `shmctl`, `shmat`, `shmdt` |
| Extended attributes | `setxattr`, `lsetxattr`, `fsetxattr`, `getxattr`, `lgetxattr`, `fgetxattr`, `listxattr`, `llistxattr`, `flistxattr`, `removexattr`, `lremovexattr`, `fremovexattr`, `setxattrat`, `getxattrat`, `listxattrat`, `removexattrat` |
| Scheduling | `sched_setparam`, `sched_setscheduler`, `sched_getscheduler`, `sched_getparam`, `sched_setaffinity`, `sched_getaffinity`, `sched_yield`, `sched_get_priority_max`, `sched_get_priority_min`, `sched_rr_get_interval`, `sched_setattr`, `sched_getattr` |
| Capabilities | `capget`, `capset` |
| Mount/namespace | `mount_setattr`, `move_mount`, `fsopen`, `fsconfig`, `fsmount`, `fspick`, `open_tree`, `open_tree_attr`, `statmount`, `listmount` |
| Security | `landlock_create_ruleset`, `landlock_add_rule`, `landlock_restrict_self`, `lsm_get_self_attr`, `lsm_set_self_attr`, `lsm_list_modules` |
| Keys | `add_key`, `request_key`, `keyctl` |
| Inotify/fanotify | `inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`, `fanotify_init`, `fanotify_mark` |
| I/O priority | `ioprio_set`, `ioprio_get` |
| Swap | `swapon`, `swapoff` |
| Misc | `nfsservctl`, `quotactl`, `quotactl_fd`, `lookup_dcookie`, `perf_event_open`, `get_robust_list`, `file_getattr`, `file_setattr` |

</details>

## Development Guide

#### Rust

cVisor is written in **Rust** (a Cargo workspace under `crates/`). It depends on
Linux kernel features but is developed primarily on ARM Macs; cross-compilation
uses [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild), and all
kernel-facing tests run in Docker.

**Requires**: a stable Rust toolchain with the four linux targets, `cargo-zigbuild`, and Docker.

```bash
cargo test -p cvisor-core        # pure-logic unit tests on the host (macOS ok)
cargo xtask test                 # full unit + e2e suite in Alpine (Docker, cross-compiled musl)
cargo xtask run                  # E2E smoke scorecard in the sandbox in Docker
cargo xtask ffi                  # build libcvisor.so and distribute it to the FFI SDKs
cargo xtask run-node             # build libcvisor.node + run the Node SDK test.ts in bun
cargo xtask node-artifacts       # build libcvisor.node for all 4 platform packages
```

Build with the `fail-loudly` feature to panic on an unhandled syscall instead of
returning ENOSYS:

```bash
cargo build -p cvisor-core --features fail-loudly
```

See `sdks/README.md` for the language SDKs (Node, Bun, Deno, Python, Ruby, Erlang, Clojure).
