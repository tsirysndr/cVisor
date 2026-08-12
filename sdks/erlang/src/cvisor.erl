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
    SoName =
        case code:priv_dir(?APPNAME) of
            {error, bad_name} ->
                %% Not running inside a release/app tree: look next to the beam.
                EbinDir = filename:dirname(code:which(?MODULE)),
                filename:join([filename:dirname(EbinDir), "priv", ?LIBNAME]);
            Dir ->
                filename:join(Dir, ?LIBNAME)
        end,
    erlang:load_nif(SoName, 0).
