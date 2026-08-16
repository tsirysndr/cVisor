package cvisor

import (
	"context"
	"encoding/base64"
	"encoding/json"
)

// Output is the captured result of a command run.
type Output struct {
	Stdout   string
	Stderr   string
	ExitCode int
}

// Limits are cgroup v2 resource caps; a nil field leaves that limit unset.
type Limits struct {
	MemoryMax  *int64 // bytes
	PidsMax    *int64 // process count
	CPUPercent *int32 // percent of one core
}

func (l *Limits) toVar() map[string]any {
	if l == nil {
		return nil
	}
	m := map[string]any{}
	if l.MemoryMax != nil {
		m["memoryMax"] = *l.MemoryMax
	}
	if l.PidsMax != nil {
		m["pidsMax"] = *l.PidsMax
	}
	if l.CPUPercent != nil {
		m["cpuPercent"] = *l.CPUPercent
	}
	return m
}

func envToVar(env map[string]string) []map[string]string {
	if len(env) == 0 {
		return nil
	}
	out := make([]map[string]string, 0, len(env))
	for k, v := range env {
		out = append(out, map[string]string{"key": k, "value": v})
	}
	return out
}

// RunOptions are the optional per-run overrides for RemoteSandbox.Run.
type RunOptions struct {
	TimeoutMs    int64
	AllowNetwork *bool
	Limits       *Limits
	Env          map[string]string
}

// ConfigureOptions are the fields RemoteSandbox.Configure may update; nil/empty
// fields are left unchanged.
type ConfigureOptions struct {
	AllowNetwork *bool
	AllowListen  *bool
	Limits       *Limits
	Env          map[string]string
}

// SandboxInfo mirrors the daemon's Sandbox type.
type SandboxInfo struct {
	ID           string       `json:"id"`
	Name         string       `json:"name"`
	AllowNetwork bool         `json:"allowNetwork"`
	AllowListen  bool         `json:"allowListen"`
	Env          []EnvVar     `json:"env"`
	Limits       SandboxLimit `json:"limits"`
}

// EnvVar is one guest environment variable.
type EnvVar struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// SandboxLimit mirrors the daemon's Limits type (nulls decode to nil).
type SandboxLimit struct {
	MemoryMax  *int64 `json:"memoryMax"`
	PidsMax    *int64 `json:"pidsMax"`
	CPUPercent *int32 `json:"cpuPercent"`
}

// CacheEntry is one entry in a cache backend or the snapshot list.
type CacheEntry struct {
	Name string `json:"name"`
	Size int64  `json:"size"`
}

// Health is the daemon's liveness/build info.
type Health struct {
	Version string `json:"version"`
	Ok      bool   `json:"ok"`
}

// RemoteSandbox is a high-level, typed client for the daemon that mirrors the
// GraphQL operations. It optionally binds to a sandbox id (empty id means an
// ephemeral sandbox for Run).
type RemoteSandbox struct {
	client *Client
	id     string
}

// NewRemoteSandbox builds a RemoteSandbox talking to the daemon at url. The
// returned handle has no bound sandbox id; use CreateSandbox, WithID, or Run
// (empty id runs ephemerally).
func NewRemoteSandbox(url, token string, opts ...Option) *RemoteSandbox {
	return &RemoteSandbox{client: NewClient(url, token, opts...)}
}

// Sandbox returns a RemoteSandbox bound to id, sharing this client.
func (c *Client) Sandbox(id string) *RemoteSandbox {
	return &RemoteSandbox{client: c, id: id}
}

// WithID returns a copy of the handle bound to a different sandbox id.
func (s *RemoteSandbox) WithID(id string) *RemoteSandbox {
	return &RemoteSandbox{client: s.client, id: id}
}

// ID reports the bound sandbox id ("" if unbound).
func (s *RemoteSandbox) ID() string { return s.id }

// Client returns the underlying GraphQL client.
func (s *RemoteSandbox) Client() *Client { return s.client }

// Health probes the daemon.
func (s *RemoteSandbox) Health(ctx context.Context) (Health, error) {
	data, err := s.client.Query(ctx, `{ health { version ok } }`, nil)
	if err != nil {
		return Health{}, err
	}
	var out struct {
		Health Health `json:"health"`
	}
	return out.Health, json.Unmarshal(data, &out)
}

// CreateSandbox creates a sandbox (empty name gets a random one) and binds this
// handle to it.
func (s *RemoteSandbox) CreateSandbox(ctx context.Context, name string) (SandboxInfo, error) {
	const doc = `mutation($name:String){ createSandbox(name:$name){ id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent} } }`
	var vars map[string]any
	if name != "" {
		vars = map[string]any{"name": name}
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return SandboxInfo{}, err
	}
	var out struct {
		CreateSandbox SandboxInfo `json:"createSandbox"`
	}
	if err := json.Unmarshal(data, &out); err != nil {
		return SandboxInfo{}, err
	}
	s.id = out.CreateSandbox.ID
	return out.CreateSandbox, nil
}

// ListSandboxes lists sandboxes, optionally full-text filtered and paginated.
// limit < 0 returns all.
func (s *RemoteSandbox) ListSandboxes(ctx context.Context, search string, limit, offset int) ([]SandboxInfo, error) {
	const doc = `query($search:String,$limit:Int,$offset:Int){ sandboxes(search:$search,limit:$limit,offset:$offset){ id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent} } }`
	vars := map[string]any{}
	if search != "" {
		vars["search"] = search
	}
	if limit >= 0 {
		vars["limit"] = limit
	}
	if offset > 0 {
		vars["offset"] = offset
	}
	data, err := s.client.Query(ctx, doc, vars)
	if err != nil {
		return nil, err
	}
	var out struct {
		Sandboxes []SandboxInfo `json:"sandboxes"`
	}
	return out.Sandboxes, json.Unmarshal(data, &out)
}

// FreeSandbox frees the bound sandbox and cleans up its overlay.
func (s *RemoteSandbox) FreeSandbox(ctx context.Context) error {
	_, err := s.client.Mutate(ctx, `mutation($id:String!){ freeSandbox(id:$id) }`, map[string]any{"id": s.id})
	return err
}

// Configure updates the bound sandbox's network/listen flags, limits, and env.
func (s *RemoteSandbox) Configure(ctx context.Context, opts ConfigureOptions) (SandboxInfo, error) {
	const doc = `mutation($id:String!,$allowNetwork:Boolean,$allowListen:Boolean,$limits:LimitsInput,$env:[EnvVarInput!]){ configure(id:$id,allowNetwork:$allowNetwork,allowListen:$allowListen,limits:$limits,env:$env){ id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent} } }`
	vars := map[string]any{"id": s.id}
	if opts.AllowNetwork != nil {
		vars["allowNetwork"] = *opts.AllowNetwork
	}
	if opts.AllowListen != nil {
		vars["allowListen"] = *opts.AllowListen
	}
	if m := opts.Limits.toVar(); m != nil {
		vars["limits"] = m
	}
	if e := envToVar(opts.Env); e != nil {
		vars["env"] = e
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return SandboxInfo{}, err
	}
	var out struct {
		Configure SandboxInfo `json:"configure"`
	}
	return out.Configure, json.Unmarshal(data, &out)
}

// Run executes command in the bound sandbox (or ephemerally when unbound),
// returning its decoded stdout/stderr and exit code.
func (s *RemoteSandbox) Run(ctx context.Context, command string, opts *RunOptions) (Output, error) {
	const doc = `mutation($id:String,$command:String!,$timeoutMs:Int,$allowNetwork:Boolean,$limits:LimitsInput,$env:[EnvVarInput!]){ run(id:$id,command:$command,timeoutMs:$timeoutMs,allowNetwork:$allowNetwork,limits:$limits,env:$env){ stdout stderr exitCode } }`
	vars := map[string]any{"command": command}
	if s.id != "" {
		vars["id"] = s.id
	}
	if opts != nil {
		if opts.TimeoutMs > 0 {
			vars["timeoutMs"] = opts.TimeoutMs
		}
		if opts.AllowNetwork != nil {
			vars["allowNetwork"] = *opts.AllowNetwork
		}
		if m := opts.Limits.toVar(); m != nil {
			vars["limits"] = m
		}
		if e := envToVar(opts.Env); e != nil {
			vars["env"] = e
		}
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return Output{}, err
	}
	var out struct {
		Run struct {
			Stdout   string `json:"stdout"`
			Stderr   string `json:"stderr"`
			ExitCode int    `json:"exitCode"`
		} `json:"run"`
	}
	if err := json.Unmarshal(data, &out); err != nil {
		return Output{}, err
	}
	return Output{Stdout: out.Run.Stdout, Stderr: out.Run.Stderr, ExitCode: out.Run.ExitCode}, nil
}

// WriteFile writes data to path inside the bound sandbox's overlay.
func (s *RemoteSandbox) WriteFile(ctx context.Context, path string, data []byte) error {
	const doc = `mutation($id:String!,$path:String!,$data:String!){ writeFile(id:$id,path:$path,dataBase64:$data) }`
	vars := map[string]any{"id": s.id, "path": path, "data": base64.StdEncoding.EncodeToString(data)}
	_, err := s.client.Mutate(ctx, doc, vars)
	return err
}

// ReadFile reads path from the bound sandbox, decoding the base64 payload.
func (s *RemoteSandbox) ReadFile(ctx context.Context, path string) ([]byte, error) {
	const doc = `query($id:String!,$path:String!){ readFile(id:$id,path:$path) }`
	data, err := s.client.Query(ctx, doc, map[string]any{"id": s.id, "path": path})
	if err != nil {
		return nil, err
	}
	var out struct {
		ReadFile string `json:"readFile"`
	}
	if err := json.Unmarshal(data, &out); err != nil {
		return nil, err
	}
	return base64.StdEncoding.DecodeString(out.ReadFile)
}

// Snapshot snapshots the bound sandbox's overlay (empty snapshotID gets a
// generated one) and returns the snapshot id.
func (s *RemoteSandbox) Snapshot(ctx context.Context, snapshotID string) (string, error) {
	const doc = `mutation($id:String!,$snapshotId:String){ snapshot(id:$id,snapshotId:$snapshotId) }`
	vars := map[string]any{"id": s.id}
	if snapshotID != "" {
		vars["snapshotId"] = snapshotID
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return "", err
	}
	var out struct {
		Snapshot string `json:"snapshot"`
	}
	return out.Snapshot, json.Unmarshal(data, &out)
}

// Rollback replaces the bound sandbox's overlay with a snapshot.
func (s *RemoteSandbox) Rollback(ctx context.Context, snapshotID string) error {
	const doc = `mutation($id:String!,$snapshotId:String!){ rollback(id:$id,snapshotId:$snapshotId) }`
	_, err := s.client.Mutate(ctx, doc, map[string]any{"id": s.id, "snapshotId": snapshotID})
	return err
}

// Branch creates a new sandbox from a snapshot.
func (s *RemoteSandbox) Branch(ctx context.Context, snapshotID, name string) (SandboxInfo, error) {
	const doc = `mutation($snapshotId:String!,$name:String){ branch(snapshotId:$snapshotId,name:$name){ id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent} } }`
	vars := map[string]any{"snapshotId": snapshotID}
	if name != "" {
		vars["name"] = name
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return SandboxInfo{}, err
	}
	var out struct {
		Branch SandboxInfo `json:"branch"`
	}
	return out.Branch, json.Unmarshal(data, &out)
}

// Fork forks the bound sandbox's current overlay into a new sandbox.
func (s *RemoteSandbox) Fork(ctx context.Context, name string) (SandboxInfo, error) {
	const doc = `mutation($id:String!,$name:String){ fork(id:$id,name:$name){ id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent} } }`
	vars := map[string]any{"id": s.id}
	if name != "" {
		vars["name"] = name
	}
	data, err := s.client.Mutate(ctx, doc, vars)
	if err != nil {
		return SandboxInfo{}, err
	}
	var out struct {
		Fork SandboxInfo `json:"fork"`
	}
	return out.Fork, json.Unmarshal(data, &out)
}

// Snapshots lists saved overlay snapshots.
func (s *RemoteSandbox) Snapshots(ctx context.Context) ([]CacheEntry, error) {
	data, err := s.client.Query(ctx, `{ snapshots { name size } }`, nil)
	if err != nil {
		return nil, err
	}
	var out struct {
		Snapshots []CacheEntry `json:"snapshots"`
	}
	return out.Snapshots, json.Unmarshal(data, &out)
}

// DeleteSnapshot deletes a snapshot; returns whether it existed.
func (s *RemoteSandbox) DeleteSnapshot(ctx context.Context, id string) (bool, error) {
	data, err := s.client.Mutate(ctx, `mutation($id:String!){ deleteSnapshot(id:$id) }`, map[string]any{"id": id})
	if err != nil {
		return false, err
	}
	var out struct {
		DeleteSnapshot bool `json:"deleteSnapshot"`
	}
	return out.DeleteSnapshot, json.Unmarshal(data, &out)
}

// CacheSave archives a sandbox path under key. Empty backend/format use the
// daemon defaults (disk backend, gzip).
func (s *RemoteSandbox) CacheSave(ctx context.Context, path, key, backend, format string) error {
	const doc = `mutation($id:String!,$path:String!,$key:String!,$backend:String,$format:String){ cacheSave(id:$id,path:$path,key:$key,backend:$backend,format:$format) }`
	vars := map[string]any{"id": s.id, "path": path, "key": key}
	if backend != "" {
		vars["backend"] = backend
	}
	if format != "" {
		vars["format"] = format
	}
	_, err := s.client.Mutate(ctx, doc, vars)
	return err
}

// CacheRestore restores a cached entry into a sandbox path.
func (s *RemoteSandbox) CacheRestore(ctx context.Context, path, key, backend, format string) error {
	const doc = `mutation($id:String!,$path:String!,$key:String!,$backend:String,$format:String){ cacheRestore(id:$id,path:$path,key:$key,backend:$backend,format:$format) }`
	vars := map[string]any{"id": s.id, "path": path, "key": key}
	if backend != "" {
		vars["backend"] = backend
	}
	if format != "" {
		vars["format"] = format
	}
	_, err := s.client.Mutate(ctx, doc, vars)
	return err
}

// CacheList lists entries in a cache backend (empty backend = default).
func (s *RemoteSandbox) CacheList(ctx context.Context, backend string) ([]CacheEntry, error) {
	const doc = `query($backend:String){ cacheList(backend:$backend){ name size } }`
	vars := map[string]any{}
	if backend != "" {
		vars["backend"] = backend
	}
	data, err := s.client.Query(ctx, doc, vars)
	if err != nil {
		return nil, err
	}
	var out struct {
		CacheList []CacheEntry `json:"cacheList"`
	}
	return out.CacheList, json.Unmarshal(data, &out)
}
