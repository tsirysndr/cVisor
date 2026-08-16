//go:build !(linux && cgo)

package cvisor

import "errors"

// ErrNativeUnsupported is returned by the native FFI path off Linux (or when
// cgo is disabled). Use Client / RemoteSandbox — the GraphQL client — instead;
// it works on every platform.
var ErrNativeUnsupported = errors.New("cvisor: native (libcvisor) sandbox is Linux-only (requires linux && cgo); use RemoteSandbox over the daemon's GraphQL API instead")

// Sandbox is the native in-process sandbox. It is only functional on Linux with
// cgo enabled; this stub exists so the type and constructor are present on every
// platform and return a clear error rather than failing to link.
type Sandbox struct{}

// NewSandbox always fails off Linux with ErrNativeUnsupported.
func NewSandbox() (*Sandbox, error) { return nil, ErrNativeUnsupported }
