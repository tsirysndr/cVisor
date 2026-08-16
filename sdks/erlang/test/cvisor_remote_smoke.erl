%% Remote-GraphQL e2e smoke for the Erlang SDK's pure-OTP client
%% (cvisor_remote over httpc — no NIF). Compile cvisor_graphql, cvisor_remote and
%% this module, then run against a running cvisord:
%%   erl -noshell -pa ebin -eval "cvisor_remote_smoke:run()" -s init stop
-module(cvisor_remote_smoke).
-export([run/0]).

run() ->
    Url = getenv("CVISOR_GRAPHQL_URL", "http://127.0.0.1:8080/graphql"),
    Token = getenv("CVISOR_TOKEN", ""),
    C = cvisor_remote:connect(list_to_binary(Url), list_to_binary(Token)),

    {ok, #{<<"ok">> := true}} = cvisor_remote:health(C),

    {ok, #{<<"stdout">> := <<"hello\n">>, <<"exitCode">> := 0}} =
        cvisor_remote:run(C, <<"echo hello">>),

    {ok, #{<<"id">> := Id}} = cvisor_remote:create_sandbox(C),
    {ok, true} = cvisor_remote:write_file(C, Id, <<"/tmp/data.txt">>, <<"round-trip\n">>),
    {ok, <<"round-trip\n">>} = cvisor_remote:read_file(C, Id, <<"/tmp/data.txt">>),
    {ok, true} = cvisor_remote:free_sandbox(C, Id),

    io:format("ERLANG_GRAPHQL_OK~n").

getenv(Name, Default) ->
    case os:getenv(Name) of
        false -> Default;
        "" -> Default;
        Value -> Value
    end.
