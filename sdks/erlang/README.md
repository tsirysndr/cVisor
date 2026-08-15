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
1> {ok, Stdout, Stderr, ExitCode} = cvisor:run(<<"echo hello">>).
{ok,<<"hello\n">>,<<>>,0}

2> cvisor:run("printf 'a\nb\nc\n' | grep b").
{ok,<<"b\n">>,<<>>,0}

3> %% Writes land in the sandbox's own filesystem view, not the host.
3> cvisor:run(<<"echo secret > /tmp/f && cat /tmp/f">>).
{ok,<<"secret\n">>,<<>>,0}

4> cvisor:run(<<"uname -n">>).
{ok,<<"cvisor\n">>,<<>>,0}

5> %% run/2 SIGKILLs the guest after a timeout (ms); a timed-out run
5> %% reports exit code 137.
5> cvisor:run(<<"sleep 30">>, 300).
{ok,<<>>,<<>>,137}

6> %% Deny outbound INET/INET6 networking for subsequent runs
6> %% (allowed by default).
6> cvisor:set_allow_network(false).
ok
```

`cvisor:run/1` accepts a binary or a string, blocks until the sandboxed
command exits (on a dirty I/O scheduler, so it does not stall the VM), and
returns `{ok, Stdout, Stderr, ExitCode}` or `{error, Reason}`. The exit code
follows shell convention: the guest's status, or 128+signo when killed by a
signal. `cvisor:run/2` takes a timeout in milliseconds (0 = no limit).
`cvisor:set_allow_network/1` takes a boolean and applies to sandboxes
created by subsequent runs.

From Elixir:

```elixir
iex> :cvisor.run("echo hello from elixir")
{:ok, "hello from elixir\n", "", 0}
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
