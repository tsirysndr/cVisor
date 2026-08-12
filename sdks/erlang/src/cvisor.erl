%% cVisor Erlang SDK — a NIF-backed wrapper over the libcvisor C ABI.
%%
%%   {ok, Stdout, Stderr} = cvisor:run(<<"echo hello">>).
%%   %% Stdout = <<"hello\n">>
%%
%% Linux-only. The NIF dlopen's libcvisor.so; set CVISOR_LIB to override the
%% path, otherwise it is loaded from the application's priv directory.
-module(cvisor).

-export([run/1]).
-on_load(init/0).

-define(APPNAME, cvisor).
-define(LIBNAME, "cvisor_nif").

%% @doc Run a shell command in the sandbox, blocking until it exits.
%% Returns the captured stdout and stderr as binaries.
-spec run(binary() | string()) ->
    {ok, binary(), binary()} | {error, atom()}.
run(Command) when is_list(Command) ->
    run(list_to_binary(Command));
run(Command) when is_binary(Command) ->
    run_nif(Command).

%% NIF stub — replaced on load.
run_nif(_Command) ->
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
