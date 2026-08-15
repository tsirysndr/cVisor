%% cVisor Erlang SDK — a NIF-backed wrapper over the libcvisor C ABI.
%%
%%   {ok, Stdout, Stderr, ExitCode} = cvisor:run(<<"echo hello">>).
%%   %% Stdout = <<"hello\n">>, ExitCode = 0
%%
%% Linux-only. The NIF dlopen's libcvisor.so; set CVISOR_LIB to override the
%% path, otherwise it is loaded from the application's priv directory.
-module(cvisor).

-export([run/1, run/2, set_allow_network/1]).
-export([run_streaming/1, run_streaming/2, shell/0, shell/1]).
-export([session_start/2, session_read_stdout/1, session_read_stderr/1,
         session_write/2, session_resize/3, session_try_wait/1,
         session_wait/1, session_kill/1, session_free/1]).
-on_load(init/0).

-type session() :: reference().

-define(APPNAME, cvisor).
-define(LIBNAME, "cvisor_nif").

%% @doc Run a shell command in the sandbox, blocking until it exits.
%% Returns the captured stdout and stderr as binaries, and the guest's
%% exit code (shell convention: status, or 128+signo when killed).
-spec run(binary() | string()) ->
    {ok, binary(), binary(), integer()} | {error, atom()}.
run(Command) ->
    run(Command, 0).

%% @doc Like {@link run/1}, but SIGKILLs the guest after `TimeoutMs'
%% milliseconds (0 = no limit). A timed-out run reports exit code 137.
-spec run(binary() | string(), non_neg_integer()) ->
    {ok, binary(), binary(), integer()} | {error, atom()}.
run(Command, TimeoutMs) when is_list(Command) ->
    run(list_to_binary(Command), TimeoutMs);
run(Command, TimeoutMs) when is_binary(Command), is_integer(TimeoutMs), TimeoutMs >= 0 ->
    run_nif(Command, TimeoutMs).

%% @doc Allow or deny outbound INET/INET6 networking for sandboxes
%% created by subsequent runs. The default is to allow.
-spec set_allow_network(boolean()) -> ok.
set_allow_network(true) ->
    set_allow_network_nif(1);
set_allow_network(false) ->
    set_allow_network_nif(0).

%% @doc Start a streaming session for `Cmd', draining stdout/stderr in the
%% calling process until the command exits. Equivalent to
%% `run_streaming(Cmd, [])'.
-spec run_streaming(binary() | string()) -> integer() | {error, atom()}.
run_streaming(Cmd) ->
    run_streaming(Cmd, []).

%% @doc Run `Cmd' as a non-PTY streaming session, invoking the supplied
%% callbacks with each chunk of output as it arrives. Options:
%% <ul>
%%   <li>`{on_stdout, fun((binary()) -> any())}' — called per non-empty
%%       stdout chunk.</li>
%%   <li>`{on_stderr, fun((binary()) -> any())}' — called per non-empty
%%       stderr chunk.</li>
%%   <li>`{poll_ms, integer()}' — poll interval in ms (default 15).</li>
%% </ul>
%% Blocks the calling process until the command exits, performs a final
%% drain, frees the session, and returns the exit code.
-spec run_streaming(binary() | string(), [term()]) ->
    integer() | {error, atom()}.
run_streaming(Cmd, Opts) when is_list(Cmd) ->
    run_streaming(list_to_binary(Cmd), Opts);
run_streaming(Cmd, Opts) when is_binary(Cmd), is_list(Opts) ->
    OnStdout = proplists:get_value(on_stdout, Opts, fun(_) -> ok end),
    OnStderr = proplists:get_value(on_stderr, Opts, fun(_) -> ok end),
    PollMs = proplists:get_value(poll_ms, Opts, 15),
    case session_start(Cmd, 0) of
        {ok, S} ->
            Code = stream_loop(S, OnStdout, OnStderr, PollMs),
            session_free(S),
            Code;
        {error, _} = Err ->
            Err
    end.

stream_loop(S, OnStdout, OnStderr, PollMs) ->
    drain(session_read_stdout(S), OnStdout),
    drain(session_read_stderr(S), OnStderr),
    case session_try_wait(S) of
        {done, Code} ->
            %% Final drain: bytes produced between the last read and exit.
            drain(session_read_stdout(S), OnStdout),
            drain(session_read_stderr(S), OnStderr),
            Code;
        running ->
            timer:sleep(PollMs),
            stream_loop(S, OnStdout, OnStderr, PollMs)
    end.

drain(<<>>, _Fun) ->
    ok;
drain(Bin, Fun) when is_binary(Bin) ->
    Fun(Bin),
    ok.

%% @doc Start an interactive PTY shell (`/bin/sh -i'). Equivalent to
%% `shell([])'.
-spec shell() -> {ok, session()} | {error, atom()}.
shell() ->
    shell([]).

%% @doc Start an interactive PTY shell session and return its handle. Write
%% to it with {@link session_write/2}, resize with {@link session_resize/3},
%% and wait for exit with {@link session_wait/1}. Options:
%% <ul>
%%   <li>`{on_output, fun((binary()) -> any())}' — if given, a poller process
%%       is spawned that drains the merged PTY output and calls the fun with
%%       each chunk until the shell exits (with a final drain).</li>
%%   <li>`{poll_ms, integer()}' — poll interval in ms (default 15).</li>
%% </ul>
%% The caller is responsible for calling {@link session_free/1} when done.
-spec shell([term()]) -> {ok, session()} | {error, atom()}.
shell(Opts) when is_list(Opts) ->
    PollMs = proplists:get_value(poll_ms, Opts, 15),
    case session_start(<<>>, 1) of
        {ok, S} ->
            case proplists:get_value(on_output, Opts) of
                undefined ->
                    ok;
                OnOutput when is_function(OnOutput, 1) ->
                    spawn(fun() -> pty_poll_loop(S, OnOutput, PollMs) end)
            end,
            {ok, S};
        {error, _} = Err ->
            Err
    end.

pty_poll_loop(S, OnOutput, PollMs) ->
    drain(session_read_stdout(S), OnOutput),
    case session_try_wait(S) of
        {done, _Code} ->
            drain(session_read_stdout(S), OnOutput),
            ok;
        running ->
            timer:sleep(PollMs),
            pty_poll_loop(S, OnOutput, PollMs)
    end.

%% @doc Start a session for `Cmd'. `Pty' is 0 for a plain command (separate
%% stdout/stderr) or 1 for an interactive PTY shell (merged output, stdin
%% writable). Returns an opaque session handle.
-spec session_start(binary(), 0 | 1) -> {ok, session()} | {error, atom()}.
session_start(Cmd, Pty) when is_binary(Cmd), (Pty =:= 0 orelse Pty =:= 1) ->
    session_start_nif(Cmd, Pty).

%% @doc Drain and return any new stdout bytes (`<<>>' if none). For a PTY
%% session this returns the merged output stream.
-spec session_read_stdout(session()) -> binary().
session_read_stdout(S) ->
    session_read_stdout_nif(S).

%% @doc Drain and return any new stderr bytes (`<<>>' if none). Empty for
%% PTY sessions, whose streams are merged into stdout.
-spec session_read_stderr(session()) -> binary().
session_read_stderr(S) ->
    session_read_stderr_nif(S).

%% @doc Write `Data' to the session's stdin (PTY sessions only). Returns the
%% number of bytes written, or -1 on error.
-spec session_write(session(), binary()) -> integer().
session_write(S, Data) when is_binary(Data) ->
    session_write_stdin_nif(S, Data).

%% @doc Resize the session's PTY window to `Rows' by `Cols'.
-spec session_resize(session(), non_neg_integer(), non_neg_integer()) -> ok.
session_resize(S, Rows, Cols)
  when is_integer(Rows), Rows >= 0, is_integer(Cols), Cols >= 0 ->
    session_resize_nif(S, Rows, Cols).

%% @doc Non-blocking check for session completion. Returns `{done, ExitCode}'
%% once the command has exited, or `running' otherwise.
-spec session_try_wait(session()) -> {done, integer()} | running.
session_try_wait(S) ->
    session_try_wait_nif(S).

%% @doc Block until the session exits and return its exit code. Runs on a
%% dirty I/O scheduler so it does not stall the VM.
-spec session_wait(session()) -> integer().
session_wait(S) ->
    session_wait_nif(S).

%% @doc Send SIGKILL to the session's command.
-spec session_kill(session()) -> ok.
session_kill(S) ->
    session_kill_nif(S).

%% @doc Free the session and its sandbox. Idempotent; also runs automatically
%% when the handle is garbage-collected.
-spec session_free(session()) -> ok.
session_free(S) ->
    session_free_nif(S).

%% NIF stubs — replaced on load.
run_nif(_Command, _TimeoutMs) ->
    erlang:nif_error(nif_not_loaded).

set_allow_network_nif(_Allow) ->
    erlang:nif_error(nif_not_loaded).

session_start_nif(_Cmd, _Pty) ->
    erlang:nif_error(nif_not_loaded).

session_read_stdout_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

session_read_stderr_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

session_write_stdin_nif(_S, _Data) ->
    erlang:nif_error(nif_not_loaded).

session_resize_nif(_S, _Rows, _Cols) ->
    erlang:nif_error(nif_not_loaded).

session_try_wait_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

session_wait_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

session_kill_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

session_free_nif(_S) ->
    erlang:nif_error(nif_not_loaded).

init() ->
    PrivDir =
        case code:priv_dir(?APPNAME) of
            {error, bad_name} ->
                %% Not running inside a release/app tree: look next to the beam.
                EbinDir = filename:dirname(code:which(?MODULE)),
                filename:join(filename:dirname(EbinDir), "priv");
            Dir ->
                Dir
        end,
    ensure_lib_env(PrivDir),
    case erlang:load_nif(filename:join(PrivDir, ?LIBNAME), 0) of
        ok ->
            ok;
        {error, Reason} ->
            %% Keep the module loadable without the NIF (e.g. ex_doc builds on
            %% non-Linux hosts); calls then raise nif_not_loaded.
            logger:warning("cvisor: NIF not loaded: ~p", [Reason]),
            ok
    end.

%% Point the NIF at the bundled libcvisor-<arch>.so unless the caller already
%% chose a library via CVISOR_LIB. The NIF reads the variable when it dlopens.
ensure_lib_env(PrivDir) ->
    case os:getenv("CVISOR_LIB") of
        false ->
            [Arch | _] = string:split(erlang:system_info(system_architecture), "-"),
            Lib = filename:join(PrivDir, "libcvisor-" ++ Arch ++ ".so"),
            case filelib:is_file(Lib) of
                true -> os:putenv("CVISOR_LIB", Lib);
                false -> ok
            end;
        _ ->
            ok
    end.
