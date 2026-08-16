//go:build linux && cgo

package cvisor

// Native in-process sandbox bound to libcvisor via cgo. Linux-only.
//
// Link against the shared library at build time. Point the linker at the .so
// with CGO flags, e.g.:
//
//	CGO_ENABLED=1 \
//	CGO_LDFLAGS="-L/path/to/libcvisor -lcvisor -Wl,-rpath,/path/to/libcvisor" \
//	go build -tags cvisor_ffi ./...
//
// or set CVISOR_LIB and load it however your build prefers. The GraphQL client
// (client.go / remote.go) needs none of this and works on every platform.

/*
#cgo LDFLAGS: -lcvisor
#include <stdlib.h>
#include <stdint.h>
#include <stddef.h>

typedef struct CvisorSandbox CvisorSandbox;
typedef struct CvisorOutput CvisorOutput;

extern CvisorSandbox* cvisor_sandbox_new(void);
extern void           cvisor_sandbox_free(CvisorSandbox*);
extern void           cvisor_sandbox_set_log_level(CvisorSandbox*, int);
extern void           cvisor_sandbox_set_allow_network(CvisorSandbox*, int);
extern void           cvisor_sandbox_set_allow_listen(CvisorSandbox*, int);
extern void           cvisor_sandbox_set_env(CvisorSandbox*, const char*, const char*);
extern void           cvisor_sandbox_set_limits(CvisorSandbox*, uint64_t, uint64_t, uint32_t);
extern int            cvisor_sandbox_write_file(CvisorSandbox*, const char*, const uint8_t*, size_t);
extern uint8_t*       cvisor_sandbox_read_file(CvisorSandbox*, const char*, size_t*);
extern CvisorOutput*  cvisor_run(CvisorSandbox*, const char*);
extern CvisorOutput*  cvisor_run_timeout(CvisorSandbox*, const char*, uint64_t);
extern int            cvisor_output_exit_code(CvisorOutput*);
extern void           cvisor_output_free(CvisorOutput*);
extern uint8_t*       cvisor_output_stdout(CvisorOutput*, size_t*);
extern uint8_t*       cvisor_output_stderr(CvisorOutput*, size_t*);
extern void           cvisor_bytes_free(uint8_t*, size_t);
*/
import "C"

import (
	"errors"
	"fmt"
	"unsafe"
)

// Sandbox is an in-process native sandbox (Linux only).
type Sandbox struct {
	ptr *C.CvisorSandbox
}

// NewSandbox creates a native sandbox handle.
func NewSandbox() (*Sandbox, error) {
	p := C.cvisor_sandbox_new()
	if p == nil {
		return nil, errors.New("cvisor: failed to create native sandbox")
	}
	return &Sandbox{ptr: p}, nil
}

// Close frees the sandbox and cleans up its overlay.
func (s *Sandbox) Close() {
	if s.ptr != nil {
		C.cvisor_sandbox_free(s.ptr)
		s.ptr = nil
	}
}

// SetAllowNetwork toggles outbound INET/INET6 networking (default on).
func (s *Sandbox) SetAllowNetwork(allow bool) {
	C.cvisor_sandbox_set_allow_network(s.ptr, boolToC(allow))
}

// SetAllowListen toggles inbound TCP servers (default off).
func (s *Sandbox) SetAllowListen(allow bool) {
	C.cvisor_sandbox_set_allow_listen(s.ptr, boolToC(allow))
}

// SetLogLevel sets the log level (0 = off, 1 = debug).
func (s *Sandbox) SetLogLevel(level int) {
	C.cvisor_sandbox_set_log_level(s.ptr, C.int(level))
}

// SetEnv sets a guest environment variable for subsequent runs.
func (s *Sandbox) SetEnv(key, value string) {
	ck := C.CString(key)
	cv := C.CString(value)
	defer C.free(unsafe.Pointer(ck))
	defer C.free(unsafe.Pointer(cv))
	C.cvisor_sandbox_set_env(s.ptr, ck, cv)
}

// SetLimits caps guest cgroup v2 resources; 0 leaves a limit unset.
func (s *Sandbox) SetLimits(memoryMax, pidsMax uint64, cpuPercent uint32) {
	C.cvisor_sandbox_set_limits(s.ptr, C.uint64_t(memoryMax), C.uint64_t(pidsMax), C.uint32_t(cpuPercent))
}

// WriteFile seeds a file into the sandbox overlay.
func (s *Sandbox) WriteFile(path string, data []byte) error {
	cp := C.CString(path)
	defer C.free(unsafe.Pointer(cp))
	var dptr *C.uint8_t
	if len(data) > 0 {
		dptr = (*C.uint8_t)(unsafe.Pointer(&data[0]))
	}
	rc := C.cvisor_sandbox_write_file(s.ptr, cp, dptr, C.size_t(len(data)))
	if rc != 0 {
		return fmt.Errorf("cvisor: write_file failed (errno %d)", -int(rc))
	}
	return nil
}

// ReadFile reads the guest's view of path.
func (s *Sandbox) ReadFile(path string) ([]byte, error) {
	cp := C.CString(path)
	defer C.free(unsafe.Pointer(cp))
	var n C.size_t
	ptr := C.cvisor_sandbox_read_file(s.ptr, cp, &n)
	return takeBytes(ptr, n), nil
}

// Run executes cmd in the sandbox, blocking until it exits.
func (s *Sandbox) Run(cmd string) (Output, error) {
	return s.RunTimeout(cmd, 0)
}

// RunTimeout runs cmd, SIGKILLing the guest after timeoutMs (0 = no limit).
func (s *Sandbox) RunTimeout(cmd string, timeoutMs uint64) (Output, error) {
	ccmd := C.CString(cmd)
	defer C.free(unsafe.Pointer(ccmd))
	var out *C.CvisorOutput
	if timeoutMs > 0 {
		out = C.cvisor_run_timeout(s.ptr, ccmd, C.uint64_t(timeoutMs))
	} else {
		out = C.cvisor_run(s.ptr, ccmd)
	}
	if out == nil {
		return Output{}, errors.New("cvisor: native run failed")
	}
	defer C.cvisor_output_free(out)
	var n C.size_t
	stdout := takeBytes(C.cvisor_output_stdout(out, &n), n)
	stderr := takeBytes(C.cvisor_output_stderr(out, &n), n)
	return Output{
		Stdout:   string(stdout),
		Stderr:   string(stderr),
		ExitCode: int(C.cvisor_output_exit_code(out)),
	}, nil
}

func boolToC(b bool) C.int {
	if b {
		return 1
	}
	return 0
}

// takeBytes copies a libcvisor-allocated buffer into Go memory and frees it.
func takeBytes(ptr *C.uint8_t, n C.size_t) []byte {
	if ptr == nil || n == 0 {
		return nil
	}
	b := C.GoBytes(unsafe.Pointer(ptr), C.int(n))
	C.cvisor_bytes_free(ptr, n)
	return b
}
