%% cVisor Erlang SDK — a NIF-backed wrapper over the libcvisor C ABI.
%%
%%   {ok, Stdout, Stderr, ExitCode} = cvisor:run(<<"echo hello">>).
%%   %% Stdout = <<"hello\n">>, ExitCode = 0
%%
%% Linux-only. The NIF dlopen's libcvisor.so; set CVISOR_LIB to override the
%% path, otherwise it is loaded from the application's priv directory.
-module(cvisor).

-export([run/1, run/2, set_allow_network/1]).
-on_load(init/0).

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

%% NIF stubs — replaced on load.
run_nif(_Command, _TimeoutMs) ->
    erlang:nif_error(nif_not_loaded).

set_allow_network_nif(_Allow) ->
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
