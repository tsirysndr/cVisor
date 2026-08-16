// Remote-GraphQL e2e smoke for the Go SDK's pure-stdlib client
// (cvisor.RemoteSandbox over net/http — no cgo/libcvisor). Run against a running
// cvisord:
//
//	CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... \
//	  CGO_ENABLED=0 go run ./cmd/remote_smoke
package main

import (
	"bytes"
	"context"
	"fmt"
	"os"

	cvisor "github.com/tsirysndr/cvisor-go"
)

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "remote smoke failed: "+format+"\n", args...)
	os.Exit(1)
}

func must(err error) {
	if err != nil {
		fail("%v", err)
	}
}

func main() {
	url := os.Getenv("CVISOR_GRAPHQL_URL")
	if url == "" {
		url = "http://127.0.0.1:8080/graphql"
	}
	token := os.Getenv("CVISOR_TOKEN")
	ctx := context.Background()

	remote := cvisor.NewRemoteSandbox(url, token)

	health, err := remote.Health(ctx)
	must(err)
	if !health.Ok {
		fail("health not ok: %+v", health)
	}

	out, err := remote.Run(ctx, "echo hello", nil)
	must(err)
	if out.Stdout != "hello\n" || out.ExitCode != 0 {
		fail("run: %+v", out)
	}

	info, err := remote.CreateSandbox(ctx, "")
	must(err)
	if info.ID == "" {
		fail("CreateSandbox returned no id: %+v", info)
	}
	must(remote.WriteFile(ctx, "/tmp/data.txt", []byte("round-trip\n")))
	data, err := remote.ReadFile(ctx, "/tmp/data.txt")
	must(err)
	if !bytes.Equal(data, []byte("round-trip\n")) {
		fail("read round-trip: %q", string(data))
	}
	must(remote.FreeSandbox(ctx))

	fmt.Println("GO_GRAPHQL_OK")
}
