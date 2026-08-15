%% e2e test for the cVisor Erlang SDK (NIF). Compile the NIF + this module,
%% then run in a musl erlang container with CVISOR_LIB set, under
%% seccomp=unconfined:
%%   erl -noshell -pa ebin -eval "cvisor_test:run()" -s init stop
-module(cvisor_test).
-export([run/0]).

run() ->
    {ok, <<"hello from erlang\n">>, _, 0} = cvisor:run(<<"echo hello from erlang">>),
    {ok, <<"b\n">>, _, 0} = cvisor:run(<<"printf 'a\nb\nc\n' | grep b">>),
    {ok, <<"x\n">>, _, 0} = cvisor:run(<<"echo x > /tmp/f && grep x /tmp/f">>),
    {ok, <<"cvisor\n">>, _, 0} = cvisor:run(<<"uname -n">>),
    {ok, <<"Name:\tcvisor-guest\n">>, _, 0} = cvisor:run(<<"grep Name /proc/self/status">>),
    {ok, _, _, 7} = cvisor:run(<<"exit 7">>),
    {ok, _, _, 137} = cvisor:run(<<"sleep 30">>, 300),
    {ok, <<"hi\n">>, _, 0} =
        cvisor:run(<<"echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a">>),
    io:format("ERLANG_SDK_OK~n").
