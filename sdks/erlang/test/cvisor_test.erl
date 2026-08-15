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
    test_streaming(),
    test_shell(),
    io:format("ERLANG_SDK_OK~n").

%% Stream a command that emits lines over time; collect the stdout chunks and
%% assert both the joined output and the exit code.
test_streaming() ->
    Self = self(),
    OnStdout = fun(Bin) -> Self ! {chunk, Bin} end,
    0 = cvisor:run_streaming(
          <<"for i in 1 2 3; do echo line$i; sleep 0.1; done">>,
          [{on_stdout, OnStdout}, {poll_ms, 15}]),
    <<"line1\nline2\nline3\n">> = gather_chunks(),
    ok.

gather_chunks() ->
    gather_chunks(<<>>).

gather_chunks(Acc) ->
    receive
        {chunk, Bin} -> gather_chunks(<<Acc/binary, Bin/binary>>)
    after 0 ->
        Acc
    end.

%% Drive an interactive PTY shell: write commands, wait for exit, drain the
%% merged output, and assert on it.
test_shell() ->
    {ok, S} = cvisor:shell([]),
    _ = cvisor:session_write(S, <<"echo SHELL_OK\n">>),
    _ = cvisor:session_write(S, <<"test -t 1 && echo IS_TTY\n">>),
    _ = cvisor:session_write(S, <<"exit 4\n">>),
    4 = cvisor:session_wait(S),
    Out = drain_all(S, <<>>),
    true = binary_contains(Out, <<"SHELL_OK">>),
    true = binary_contains(Out, <<"IS_TTY">>),
    ok = cvisor:session_free(S),
    ok.

drain_all(S, Acc) ->
    case cvisor:session_read_stdout(S) of
        <<>> -> Acc;
        Bin -> drain_all(S, <<Acc/binary, Bin/binary>>)
    end.

binary_contains(Haystack, Needle) ->
    binary:match(Haystack, Needle) =/= nomatch.
