# @cvisor/cli

The [cVisor](https://github.com/tsirysndr/cVisor) command line interface — a
lightweight in-process Linux sandbox for untrusted and LLM-generated code.

```bash
npm install -g @cvisor/cli
cvisor run -- echo hello
```

## What gets installed

The package itself is a thin launcher; the prebuilt binaries come from an
optional dependency picked by npm for your host:

| Host                   | Package                    | Binaries           |
| ---------------------- | -------------------------- | ------------------ |
| Linux x86_64           | `@cvisor/cli-linux-x64`    | `cvisor`, `cvisord` |
| Linux aarch64          | `@cvisor/cli-linux-arm64`  | `cvisor`, `cvisord` |
| macOS Apple silicon    | `@cvisor/cli-darwin-arm64` | `cvisor`           |

The Linux binaries are static musl builds, so they run on glibc and musl
distros alike. `cvisord`, the daemon, is Linux-only: on macOS the CLI boots a
microVM and drives the daemon running inside it.
