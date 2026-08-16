# @cvisor/daemon

`cvisord`, the [cVisor](https://github.com/tsirysndr/cVisor) daemon — it serves
the sandbox runtime over gRPC + GraphQL, so any SDK, the `cvisor --remote` CLI,
and the web UI can drive sandboxes on that host.

```bash
npm install -g @cvisor/daemon
cvisord
```

The CLI itself ships separately as [`@cvisor/cli`](https://www.npmjs.com/package/@cvisor/cli).

## What gets installed

The package is a thin launcher; the prebuilt binary comes from an optional
dependency picked by npm for your host:

| Host          | Package                   | Binary    |
| ------------- | ------------------------- | --------- |
| Linux x86_64  | `@cvisor/cli-linux-x64`   | `cvisord` |
| Linux aarch64 | `@cvisor/cli-linux-arm64` | `cvisord` |

The daemon is **Linux-only** — the sandbox runtime needs a Linux kernel — so
there is no macOS package here. On macOS, `cvisor` boots a microVM and drives
the `cvisord` running inside it; to reach a daemon on another host, use
`cvisor --remote <addr>`.

Those platform packages carry both binaries, so installing this alongside
`@cvisor/cli` downloads them once.
