%% e2e test for the cVisor Erlang SDK (NIF). Compile the NIF + this module,
%% then run in a musl erlang container with CVISOR_LIB set, under
%% seccomp=unconfined:
%%   erl -noshell -pa ebin -eval "cvisor_test:run()" -s init stop
-module(cvisor_test).
-export([run/0]).

run() ->
    {ok, <<"hello from erlang\n">>, _} = cvisor:run(<<"echo hello from erlang">>),
    {ok, <<"b\n">>, _} = cvisor:run(<<"printf 'a\nb\nc\n' | grep b">>),
    {ok, <<"x\n">>, _} = cvisor:run(<<"echo x > /tmp/f && grep x /tmp/f">>),
    {ok, <<"cvisor\n">>, _} = cvisor:run(<<"uname -n">>),
    {ok, <<"Name:\tcvisor-guest\n">>, _} = cvisor:run(<<"grep Name /proc/self/status">>),
    io:format("ERLANG_SDK_OK~n").
