# cVisor Go SDK

A Go client for the [cVisor](../../README.md) sandbox runtime.

Two layers, one package (`github.com/tsirysndr/cvisor-go`, imported as `cvisor`):

| Layer                    | Transport                     | Platforms                 |
| ------------------------ | ----------------------------- | ------------------------- |
| `Client` / `RemoteSandbox` | daemon GraphQL over HTTP      | any (macOS, Linux, ...)   |
| `Sandbox`                | in-process `libcvisor` (cgo)  | Linux only (`linux && cgo`) |

The GraphQL path is pure standard library (`net/http`, `encoding/json`,
`encoding/base64`, `context`) — no cgo, no `libcvisor`. It is the primary API and
the one that works on macOS.

## Install

```bash
go get github.com/tsirysndr/cvisor-go
```

## Connect to a daemon (works on any OS)

Point the client at a running `cvisord` GraphQL endpoint and pass its bearer
token:

```go
package main

import (
	"context"
	"fmt"
	"log"

	cvisor "github.com/tsirysndr/cvisor-go"
)

func main() {
	ctx := context.Background()
	sb := cvisor.NewRemoteSandbox("http://127.0.0.1:8080/graphql", "my-token")

	// Liveness.
	h, err := sb.Health(ctx)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("daemon %s ok=%v\n", h.Version, h.Ok)

	// Run a one-shot command in an ephemeral sandbox.
	out, err := sb.Run(ctx, "echo hi", nil)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Print(out.Stdout) // "hi\n"
	fmt.Println("exit", out.ExitCode)

	// Or create a persistent sandbox and seed a file.
	info, err := sb.CreateSandbox(ctx, "") // random docker-style name
	if err != nil {
		log.Fatal(err)
	}
	_ = info
	if err := sb.WriteFile(ctx, "/work/hello.txt", []byte("hi\n")); err != nil {
		log.Fatal(err)
	}
	out, _ = sb.Run(ctx, "cat /work/hello.txt", nil)
	fmt.Print(out.Stdout)

	data, _ := sb.ReadFile(ctx, "/work/hello.txt")
	fmt.Printf("%s", data)
}
```

### API surface (`RemoteSandbox`)

- `Health(ctx) (Health, error)`
- `CreateSandbox(ctx, name) (SandboxInfo, error)` — also binds this handle to the new id
- `ListSandboxes(ctx, search, limit, offset) ([]SandboxInfo, error)`
- `FreeSandbox(ctx) error`
- `Configure(ctx, ConfigureOptions) (SandboxInfo, error)`
- `Run(ctx, cmd, *RunOptions) (Output, error)`
- `Snapshot(ctx, snapshotID) (string, error)` / `Rollback(ctx, snapshotID) error`
- `Branch(ctx, snapshotID, name) (SandboxInfo, error)` / `Fork(ctx, name) (SandboxInfo, error)`
- `Snapshots(ctx) ([]CacheEntry, error)` / `DeleteSnapshot(ctx, id) (bool, error)`
- `WriteFile(ctx, path, data) error` / `ReadFile(ctx, path) ([]byte, error)`
- `CacheSave/CacheRestore(ctx, path, key, backend, format) error` / `CacheList(ctx, backend) ([]CacheEntry, error)`

`RunOptions` and `ConfigureOptions` use pointer fields for optionals (`AllowNetwork
*bool`, `Limits *Limits`, `Env map[string]string`, `TimeoutMs int64`). Binary
payloads (`WriteFile`/`ReadFile`) are base64-encoded/decoded for you.

For raw access, drop to the client: `cvisor.NewClient(url, token).Query(ctx, doc,
vars)` / `.Mutate(...)` return `json.RawMessage`.

## Native in-process sandbox (Linux only)

On Linux with cgo you can run the sandbox in-process via `libcvisor` — no daemon:

```go
//go:build linux && cgo

sb, err := cvisor.NewSandbox()
if err != nil {
	log.Fatal(err)
}
defer sb.Close()
out, _ := sb.Run("echo hi")
fmt.Print(out.Stdout)
```

This path is compiled only under the `linux && cgo` build tag. On any other
platform `cvisor.NewSandbox()` returns `cvisor.ErrNativeUnsupported` (a clear
"Linux-only" error) — it never fails to link or load a library. Non-Linux builds
(`go build ./...` on macOS) compile the GraphQL client only.

### Building the native path

`libcvisor` must be linkable. Build it from the repo root (`cargo xtask ffi`),
then point the linker at it:

```bash
CGO_ENABLED=1 \
CGO_LDFLAGS="-L/path/to/libcvisor -lcvisor -Wl,-rpath,/path/to/libcvisor" \
go build ./...
```

At runtime the loader finds the `.so` via the rpath above (or `LD_LIBRARY_PATH`).
