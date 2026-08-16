%% Optional GraphQL client for the cVisor daemon's HTTP API.
%%
%% Uses OTP's `httpc' (inets) for HTTP and OTP-27's `json' module for
%% encode/decode — no native library — so it works on any platform (macOS
%% included), unlike the NIF-backed {@link cvisor} module.
%%
%%   {ok, Data} = cvisor_graphql:query(<<"http://127.0.0.1:8080/graphql">>,
%%                                      Token, <<"{ health { version ok } }">>, #{}).
%%   %% Data = #{<<"health">> => #{<<"version">> => <<"0.1.0">>, <<"ok">> => true}}
%%
%% Requires the `inets' and `ssl' applications; ensure_started/0 starts them.
%% Requires OTP 27+ for the `json' module.
-module(cvisor_graphql).

-export([query/4, mutate/4, ensure_started/0]).

%% @doc Run a GraphQL query document against `Url' with bearer `Token'.
%% `Variables' is a map (use `#{}' for none). Returns `{ok, Data}' with the
%% decoded `data' object, or `{error, Reason}'.
-spec query(binary() | string(), binary() | string(), binary() | string(), map()) ->
    {ok, map()} | {error, term()}.
query(Url, Token, Document, Variables) ->
    request(Url, Token, Document, Variables).

%% @doc Run a GraphQL mutation document; see {@link query/4}.
-spec mutate(binary() | string(), binary() | string(), binary() | string(), map()) ->
    {ok, map()} | {error, term()}.
mutate(Url, Token, Document, Variables) ->
    request(Url, Token, Document, Variables).

%% @doc Start the `inets' and `ssl' applications the HTTP client depends on.
%% Idempotent; safe to call before the first request.
-spec ensure_started() -> ok.
ensure_started() ->
    {ok, _} = application:ensure_all_started(inets),
    {ok, _} = application:ensure_all_started(ssl),
    ok.

request(Url, Token, Document, Variables) ->
    ensure_started(),
    Body = iolist_to_binary(json:encode(#{
        <<"query">> => to_bin(Document),
        <<"variables">> => Variables
    })),
    Headers = [{"authorization", "Bearer " ++ to_str(Token)}],
    Req = {to_str(Url), Headers, "application/json", Body},
    case httpc:request(post, Req, [], [{body_format, binary}]) of
        {ok, {{_Version, Status, _Reason}, _RespHeaders, RespBody}} when Status >= 200, Status < 300 ->
            decode(RespBody);
        {ok, {{_Version, Status, Reason}, _RespHeaders, _RespBody}} ->
            {error, {http_error, Status, Reason}};
        {error, Reason} ->
            {error, Reason}
    end.

decode(RespBody) ->
    case json:decode(RespBody) of
        #{<<"errors">> := Errors} when is_list(Errors), Errors =/= [] ->
            {error, {graphql, join_messages(Errors)}};
        #{<<"data">> := Data} ->
            {ok, Data};
        Other ->
            {error, {unexpected_response, Other}}
    end.

join_messages(Errors) ->
    Messages = [maps:get(<<"message">>, E, <<"unknown error">>) || E <- Errors],
    iolist_to_binary(lists:join(<<"; ">>, Messages)).

to_bin(B) when is_binary(B) -> B;
to_bin(S) when is_list(S) -> list_to_binary(S).

to_str(B) when is_binary(B) -> binary_to_list(B);
to_str(S) when is_list(S) -> S.
