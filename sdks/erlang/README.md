# cVisor — Erlang SDK

An Erlang NIF over the `libcvisor` C ABI. Runs shell commands in an
in-process Linux sandbox — no containers, no VMs, sandbox startup in
~2 milliseconds. Linux-only.

[cVisor](https://github.com/tsirysndr/cVisor) intercepts and virtualizes
Linux syscalls from userspace with the seccomp user notifier, giving the
sandboxed command a copy-on-write view of the filesystem and a virtualized
`/proc`. It is designed for safely running untrusted or LLM-generated
commands directly inside your application.

## Install

Add the package to your `rebar.config`:

```erlang
{deps, [{cvisor, "0.1.0"}]}.
```

Or, from Elixir, to your `mix.exs`:

```elixir
{:cvisor, "~> 0.1.0"}
```

The prebuilt `libcvisor` (aarch64 and x86_64, musl) ships in the package; the
small NIF shim is compiled on install, so a C compiler (`cc`) must be on the
PATH.

## Usage

```erlang
1> {ok, Stdout, Stderr} = cvisor:run(<<"echo hello">>).
{ok,<<"hello\n">>,<<>>}

2> cvisor:run("printf 'a\nb\nc\n' | grep b").
{ok,<<"b\n">>,<<>>}

3> %% Writes land in the sandbox's own filesystem view, not the host.
3> cvisor:run(<<"echo secret > /tmp/f && cat /tmp/f">>).
{ok,<<"secret\n">>,<<>>}

4> cvisor:run(<<"uname -n">>).
{ok,<<"cvisor\n">>,<<>>}
```

`cvisor:run/1` accepts a binary or a string, blocks until the sandboxed
command exits (on a dirty I/O scheduler, so it does not stall the VM), and
returns `{ok, Stdout, Stderr}` or `{error, Reason}`.

From Elixir:

```elixir
iex> :cvisor.run("echo hello from elixir")
{:ok, "hello from elixir\n", ""}
```

## Requirements

- Linux (aarch64 or x86_64) with the seccomp user notifier
  (kernel >= 5.0; unprivileged, no root needed)
- Erlang/OTP with dirty schedulers (any modern OTP)
- A C compiler at install time for the NIF shim

## Development

The NIF `dlopen`s `libcvisor.so`. Build it from the repo root
(`cargo xtask ffi`), which drops `priv/libcvisor-<arch>.so` into this SDK, or
point the NIF at any build via the `CVISOR_LIB` environment variable:

```bash
# from the repo root — builds and distributes libcvisor-<arch>.so
cargo xtask ffi

cd sdks/erlang
make all                            # NIF shim + beam files (via erlc)
erlc -o ebin test/cvisor_test.erl
erl -noshell -pa ebin -eval "cvisor_test:run()" -s init stop
```
