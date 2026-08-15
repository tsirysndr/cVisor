# cVisor — Elixir SDK

Elixir bindings for [cVisor](https://github.com/tsirysndr/cVisor), an in-process
Linux sandbox. A thin, pipe-friendly builder over the NIF-backed `cvisor`
runtime (the Erlang SDK). Linux-only at runtime.

Published on Hex as [`cvisor_ex`](https://hex.pm/packages/cvisor_ex).

## Install

```elixir
def deps do
  [{:cvisor_ex, "~> 0.1"}]
end
```

The underlying `cvisor` package ships a prebuilt `libcvisor.so` and compiles a
small NIF shim at build time (a C compiler is required on install).

## Usage

A `Cvisor` value is an immutable configuration you build up with the pipe
operator, then execute with `run/2`:

```elixir
alias Cvisor

out =
  Cvisor.new()
  |> Cvisor.run("echo hello")

out.stdout      # => "hello\n"
out.exit_code   # => 0  (shell convention: status, or 128 + signal)
```

### Resource limits

Cap the guest's memory, process count, and CPU via cgroup v2 — pipe them in:

```elixir
Cvisor.new()
|> Cvisor.memory_limit(256 * 1024 * 1024)   # 256 MiB (memory.max)
|> Cvisor.pids_limit(128)                     # pids.max
|> Cvisor.cpu_limit(50)                       # 50% of one core (cpu.max)
|> Cvisor.run("./build.sh")
```

An over-cap allocation is OOM-killed; `pids_limit` caps the whole guest tree.
Limits require a writable cgroup v2 hierarchy — where one is unavailable they
gracefully no-op and the run proceeds unlimited.

### Environment, network, and timeout

```elixir
Cvisor.new()
|> Cvisor.env("TOKEN", "xyz")     # layered over the default PATH/HOME
|> Cvisor.network(false)          # deny outbound INET/INET6 (default: allowed)
|> Cvisor.allow_listen(true)      # permit inbound TCP servers (default: off)
|> Cvisor.timeout(5_000)          # SIGKILL after 5s (a timed-out run reports 137)
|> Cvisor.run("curl https://example.com")
```

Each builder returns an updated `Cvisor` struct, so they chain; the
configuration is applied to the shared runtime when `run/2` executes.

## Development

The SDK depends on the `cvisor` Erlang runtime. In the monorepo it is a path
dependency (`../erlang`); building it compiles the NIF and needs the prebuilt
`libcvisor.so` in the Erlang SDK's `priv/` (run `cargo xtask ffi` from the repo
root first). Point the runtime at a specific library with `CVISOR_LIB` if the
bundled one isn't found.

Run the tests in a musl Elixir container (Linux syscalls + seccomp required):

```bash
docker run --rm --security-opt seccomp=unconfined \
  -e CVISOR_LIB="$PWD/sdks/erlang/priv/libcvisor-$(uname -m).so" \
  -v "$PWD":/w -w /w/sdks/elixir elixir:otp-27-alpine \
  sh -c 'apk add --no-cache build-base >/dev/null &&
         mix local.rebar --force && mix local.hex --force &&
         mix deps.get && mix test'
```

Releases are tagged `elixir-sdk-v*` in the monorepo.
