%% Thin Erlang shim adapting the NIF-backed `cvisor` module to a Gleam-typed
%% return. Applies the accumulated sandbox configuration to the shared runtime,
%% then runs the command and reshapes {ok, Out, Err, Code} into a plain
%% {Out, Err, Code} tuple (Gleam #(BitArray, BitArray, Int)).
-module(cvisor_ffi).
-export([run/8]).

run(Command, TimeoutMs, MemMax, PidsMax, CpuPercent, AllowNetwork, AllowListen, Env) ->
    ok = cvisor:set_allow_network(AllowNetwork),
    ok = cvisor:set_allow_listen(AllowListen),
    ok = cvisor:set_limits(undefined, MemMax, PidsMax, CpuPercent),
    lists:foreach(fun({K, V}) -> ok = cvisor:set_env(K, V) end, Env),
    case cvisor:run(Command, TimeoutMs) of
        {ok, Stdout, Stderr, ExitCode} ->
            {Stdout, Stderr, ExitCode};
        {error, Reason} ->
            erlang:error({cvisor_run_failed, Reason})
    end.
