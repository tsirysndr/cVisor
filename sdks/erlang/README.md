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
{deps, [{cvisor, "0.2.0"}]}.
```

Or, from Elixir, to your `mix.exs`:

```elixir
{:cvisor, "~> 0.2.0"}
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

## Streaming & interactive shells

`cvisor:run/1` buffers all output and returns once the command exits. For
long-running commands you can instead stream output as it is produced, or open
an interactive PTY shell you can type into. Both are built on a *session*: a
sandbox plus a running command that you poll from Erlang.

### run_streaming/1,2

Runs a (non-PTY) command and invokes your callbacks with each chunk of output
as it arrives, blocking the calling process until the command exits and
returning its exit code:

```erlang
Code = cvisor:run_streaming(
  <<"for i in 1 2 3; do echo line$i; sleep 0.1; done">>,
  [{on_stdout, fun(Bin) -> io:put_chars(Bin) end},
   {on_stderr, fun(Bin) -> io:put_chars(Bin) end},
   {poll_ms, 15}]).
%% prints line1 / line2 / line3 as they appear; Code = 0
```

Options (all optional): `{on_stdout, fun((binary()) -> any())}`,
`{on_stderr, fun((binary()) -> any())}`, and `{poll_ms, integer()}` (poll
interval in milliseconds, default 15). `run_streaming(Cmd)` is
`run_streaming(Cmd, [])`.

### shell/0,1

Opens an interactive PTY shell (`/bin/sh -i`) and returns an opaque session
handle. Output streams are merged (as with a real terminal), and stdin is
writable, so the command sees a TTY (`test -t 1` succeeds):

```erlang
{ok, S} = cvisor:shell([{on_output, fun(Bin) -> io:put_chars(Bin) end}]),
cvisor:session_write(S, <<"echo hello from the shell\n">>),
cvisor:session_resize(S, 40, 120),
cvisor:session_write(S, <<"exit 0\n">>),
Code = cvisor:session_wait(S),   %% blocks until the shell exits
cvisor:session_free(S).
```

Options: `{on_output, fun((binary()) -> any())}` (if given, a poller process
is spawned that drains the merged output and calls the fun with each chunk
until the shell exits) and `{poll_ms, integer()}` (default 15). `shell()` is
`shell([])`. The caller owns the session and must call `session_free/1` when
done (it is also freed automatically when the handle is garbage-collected).

### Session functions

The lower-level session API, used by both helpers above:

| Function                       | Description                                                        |
| ------------------------------ | ------------------------------------------------------------------ |
| `session_start(Cmd, Pty)`      | Start a session. `Pty` is `0` (plain command) or `1` (PTY shell).  |
| `session_read_stdout(S)`       | Drain and return new stdout bytes (`<<>>` if none; merged for PTY).|
| `session_read_stderr(S)`       | Drain and return new stderr bytes (empty for PTY sessions).        |
| `session_write(S, Data)`       | Write to stdin (PTY only); returns bytes written or `-1`.          |
| `session_resize(S, Rows, Cols)`| Resize the PTY window.                                             |
| `session_try_wait(S)`          | Non-blocking: `{done, ExitCode}` or `running`.                    |
| `session_wait(S)`              | Block (on a dirty I/O scheduler) until exit; returns the code.     |
| `session_kill(S)`              | SIGKILL the session's command.                                     |
| `session_free(S)`              | Free the session and its sandbox (idempotent).                     |

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
