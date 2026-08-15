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
    ok = cvisor:set_env(<<"FOO">>, <<"bar">>),
    {ok, <<"bar\n">>, _, 0} = cvisor:run(<<"echo $FOO">>),
    test_limits(),
    test_streaming(),
    test_shell(),
    test_session_files(),
    test_cache(),
    io:format("ERLANG_SDK_OK~n").

%% Set a small pids limit and confirm a normal run still works. The test
%% container has no writable cgroup v2, so the limit gracefully no-ops; this
%% only proves the plumbing compiles and does not break a run.
test_limits() ->
    ok = cvisor:set_limits(undefined, 0, 100, 0),
    {ok, <<"ok\n">>, _, 0} = cvisor:run(<<"echo ok">>),
    ok = cvisor:set_limits(undefined, 0, 0, 0),
    ok.

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

%% Seed a file into a session's sandbox with session_write_file, read it back
%% with session_read_file, and confirm it is visible to a command run on the
%% same PTY session. Also assert set_allow_listen/1 toggles cleanly.
test_session_files() ->
    ok = cvisor:set_allow_listen(true),
    ok = cvisor:set_allow_listen(false),
    {ok, S} = cvisor:shell([]),
    ok = cvisor:session_write_file(S, <<"/tmp/x">>, <<"hi">>),
    <<"hi">> = cvisor:session_read_file(S, <<"/tmp/x">>),
    <<>> = cvisor:session_read_file(S, <<"/tmp/does-not-exist">>),
    _ = cvisor:session_write(S, <<"cat /tmp/x; echo; exit 0\n">>),
    0 = cvisor:session_wait(S),
    Out = drain_all(S, <<>>),
    true = binary_contains(Out, <<"hi">>),
    ok = cvisor:session_free(S),
    ok.

%% On a single session: write a file into /tmp/proj, save that directory to the
%% cache under a key, restore it into a different directory (/tmp/proj2), and
%% confirm a command run on the same session sees the restored file. Exercises
%% the default disk backend and gzip format.
test_cache() ->
    {ok, S} = cvisor:shell([]),
    ok = cvisor:session_write_file(S, <<"/tmp/proj/data.txt">>, <<"cached-payload">>),
    ok = cvisor:session_cache_save(S, <<"/tmp/proj">>, <<"k1">>, <<>>, <<"gzip">>),
    ok = cvisor:session_cache_restore(S, <<"/tmp/proj2">>, <<"k1">>, <<>>, <<"gzip">>),
    <<"cached-payload">> = cvisor:session_read_file(S, <<"/tmp/proj2/data.txt">>),
    _ = cvisor:session_write(S, <<"cat /tmp/proj2/data.txt; echo; exit 0\n">>),
    0 = cvisor:session_wait(S),
    Out = drain_all(S, <<>>),
    true = binary_contains(Out, <<"cached-payload">>),
    ok = cvisor:session_free(S),
    ok.

drain_all(S, Acc) ->
    case cvisor:session_read_stdout(S) of
        <<>> -> Acc;
        Bin -> drain_all(S, <<Acc/binary, Bin/binary>>)
    end.

binary_contains(Haystack, Needle) ->
    binary:match(Haystack, Needle) =/= nomatch.
