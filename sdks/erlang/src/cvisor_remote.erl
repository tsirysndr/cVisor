%% A GraphQL-backed high-level API mirroring the NIF-backed {@link cvisor}
%% runtime, but driven over HTTP against a running cVisor daemon. Pure `httpc' +
%% OTP-27 `json' (via {@link cvisor_graphql}) — no native library — so it runs
%% on any platform (macOS included). This is what makes the SDK usable off Linux.
%%
%%   C = cvisor_remote:connect(<<"http://127.0.0.1:8080/graphql">>, Token),
%%   {ok, R} = cvisor_remote:run(C, <<"echo hello">>).
%%   %% R = #{<<"stdout">> => <<"hello\n">>, <<"stderr">> => <<>>, <<"exitCode">> => 0}
-module(cvisor_remote).

-export([connect/2, health/1, run/2, run/3, create_sandbox/1, create_sandbox/2,
         list_sandboxes/1, free_sandbox/2, configure/3, write_file/4,
         read_file/3, cache_save/4, cache_save/6, cache_restore/4,
         cache_restore/6, cache_list/1, cache_list/2, snapshot/2, snapshot/3,
         rollback/3, branch/2, branch/3, fork/2, fork/3, snapshots/1,
         delete_snapshot/2]).

-type client() :: #{url := binary() | string(), token := binary() | string()}.

-define(SANDBOX_FIELDS,
        <<"{ id name allowNetwork allowListen env { key value } "
          "limits { memoryMax pidsMax cpuPercent } }">>).

%% @doc Build a client for the daemon at `Url' authenticating with `Token'.
-spec connect(binary() | string(), binary() | string()) -> client().
connect(Url, Token) ->
    #{url => Url, token => Token}.

%% @doc Daemon liveness and build info.
-spec health(client()) -> {ok, map()} | {error, term()}.
health(Client) ->
    pick(<<"health">>, do_query(Client, <<"{ health { version ok } }">>, #{})).

%% @doc Run a command in a fresh ephemeral sandbox. See {@link run/3}.
-spec run(client(), binary() | string()) -> {ok, map()} | {error, term()}.
run(Client, Command) ->
    run(Client, Command, 0).

%% @doc Run `Command' in a fresh ephemeral sandbox, SIGKILLing after `TimeoutMs'
%% (0 = no limit). Returns `{ok, #{<<"stdout">> => ..., <<"stderr">> => ...,
%% <<"exitCode">> => ...}}'.
-spec run(client(), binary() | string(), non_neg_integer()) ->
    {ok, map()} | {error, term()}.
run(Client, Command, TimeoutMs) ->
    Doc = <<"mutation($command: String!, $timeoutMs: Int) {"
            " run(command: $command, timeoutMs: $timeoutMs)"
            " { stdout stderr exitCode } }">>,
    Vars = #{<<"command">> => to_bin(Command), <<"timeoutMs">> => TimeoutMs},
    pick(<<"run">>, do_mutate(Client, Doc, Vars)).

%% @doc Create a sandbox with a random docker-style name.
-spec create_sandbox(client()) -> {ok, map()} | {error, term()}.
create_sandbox(Client) ->
    create_sandbox(Client, null).

%% @doc Create a sandbox named `Name' (`null' or `<<>>' picks a random name).
-spec create_sandbox(client(), binary() | null) -> {ok, map()} | {error, term()}.
create_sandbox(Client, Name) ->
    Doc = <<"mutation($name: String) { createSandbox(name: $name) ",
            ?SANDBOX_FIELDS/binary, " }">>,
    pick(<<"createSandbox">>, do_mutate(Client, Doc, #{<<"name">> => Name})).

%% @doc All live sandboxes.
-spec list_sandboxes(client()) -> {ok, [map()]} | {error, term()}.
list_sandboxes(Client) ->
    Doc = <<"{ sandboxes ", ?SANDBOX_FIELDS/binary, " }">>,
    pick(<<"sandboxes">>, do_query(Client, Doc, #{})).

%% @doc Free a sandbox and clean up its overlay.
-spec free_sandbox(client(), binary()) -> {ok, boolean()} | {error, term()}.
free_sandbox(Client, Id) ->
    pick(<<"freeSandbox">>,
         do_mutate(Client, <<"mutation($id: String!) { freeSandbox(id: $id) }">>,
                   #{<<"id">> => Id})).

%% @doc Update a sandbox's network/listen flags, limits, and env. `Opts' is a map
%% with any of the keys `allow_network' (boolean), `allow_listen' (boolean),
%% `limits' (a `LimitsInput' map) and `env' (a list of `EnvVarInput' maps);
%% omitted keys are sent as GraphQL `null'.
-spec configure(client(), binary(), map()) -> {ok, map()} | {error, term()}.
configure(Client, Id, Opts) ->
    Doc = <<"mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean,"
            " $limits: LimitsInput, $env: [EnvVarInput!]) {"
            " configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen,"
            " limits: $limits, env: $env) ", ?SANDBOX_FIELDS/binary, " }">>,
    Vars = #{<<"id">> => Id,
             <<"allowNetwork">> => maps:get(allow_network, Opts, null),
             <<"allowListen">> => maps:get(allow_listen, Opts, null),
             <<"limits">> => maps:get(limits, Opts, null),
             <<"env">> => maps:get(env, Opts, null)},
    pick(<<"configure">>, do_mutate(Client, Doc, Vars)).

%% @doc Write `Data' to `Path' inside a sandbox (base64-encoded over the wire).
-spec write_file(client(), binary(), binary(), binary()) ->
    {ok, boolean()} | {error, term()}.
write_file(Client, Id, Path, Data) ->
    Doc = <<"mutation($id: String!, $path: String!, $data: String!) {"
            " writeFile(id: $id, path: $path, dataBase64: $data) }">>,
    Vars = #{<<"id">> => Id, <<"path">> => Path,
             <<"data">> => base64:encode(Data)},
    pick(<<"writeFile">>, do_mutate(Client, Doc, Vars)).

%% @doc Read `Path' from a sandbox (base64-decoded from the wire); returns bytes.
-spec read_file(client(), binary(), binary()) -> {ok, binary()} | {error, term()}.
read_file(Client, Id, Path) ->
    Doc = <<"query($id: String!, $path: String!) { readFile(id: $id, path: $path) }">>,
    case pick(<<"readFile">>, do_query(Client, Doc, #{<<"id">> => Id, <<"path">> => Path})) of
        {ok, Encoded} -> {ok, base64:decode(Encoded)};
        Err -> Err
    end.

%% @doc Archive a sandbox path into the cache under `Key' (disk backend, gzip).
-spec cache_save(client(), binary(), binary(), binary()) ->
    {ok, boolean()} | {error, term()}.
cache_save(Client, Id, Path, Key) ->
    cache_save(Client, Id, Path, Key, <<>>, <<"gzip">>).

%% @doc Archive a sandbox path into the cache under `Key' with an explicit
%% `Backend' and `Format'.
-spec cache_save(client(), binary(), binary(), binary(), binary(), binary()) ->
    {ok, boolean()} | {error, term()}.
cache_save(Client, Id, Path, Key, Backend, Format) ->
    pick(<<"cacheSave">>, cache_op(Client, <<"cacheSave">>, Id, Path, Key, Backend, Format)).

%% @doc Restore a cached entry into a sandbox path (disk backend, gzip).
-spec cache_restore(client(), binary(), binary(), binary()) ->
    {ok, boolean()} | {error, term()}.
cache_restore(Client, Id, Path, Key) ->
    cache_restore(Client, Id, Path, Key, <<>>, <<"gzip">>).

%% @doc Restore a cached entry into a sandbox path with an explicit `Backend'
%% and `Format'.
-spec cache_restore(client(), binary(), binary(), binary(), binary(), binary()) ->
    {ok, boolean()} | {error, term()}.
cache_restore(Client, Id, Path, Key, Backend, Format) ->
    pick(<<"cacheRestore">>, cache_op(Client, <<"cacheRestore">>, Id, Path, Key, Backend, Format)).

%% @doc List entries in the default cache backend.
-spec cache_list(client()) -> {ok, [map()]} | {error, term()}.
cache_list(Client) ->
    cache_list(Client, null).

%% @doc List entries in cache `Backend' (`null' = default).
-spec cache_list(client(), binary() | null) -> {ok, [map()]} | {error, term()}.
cache_list(Client, Backend) ->
    Doc = <<"query($backend: String) { cacheList(backend: $backend) { name size } }">>,
    pick(<<"cacheList">>, do_query(Client, Doc, #{<<"backend">> => Backend})).

%% @doc Snapshot a sandbox's overlay with a generated id; returns the id.
-spec snapshot(client(), binary()) -> {ok, binary()} | {error, term()}.
snapshot(Client, Id) ->
    snapshot(Client, Id, null).

%% @doc Snapshot a sandbox's overlay under `SnapshotId' (`null' = generated).
-spec snapshot(client(), binary(), binary() | null) -> {ok, binary()} | {error, term()}.
snapshot(Client, Id, SnapshotId) ->
    Doc = <<"mutation($id: String!, $snapshotId: String) {"
            " snapshot(id: $id, snapshotId: $snapshotId) }">>,
    pick(<<"snapshot">>, do_mutate(Client, Doc, #{<<"id">> => Id, <<"snapshotId">> => SnapshotId})).

%% @doc Replace a sandbox's overlay with a snapshot (discard changes since).
-spec rollback(client(), binary(), binary()) -> {ok, boolean()} | {error, term()}.
rollback(Client, Id, SnapshotId) ->
    Doc = <<"mutation($id: String!, $snapshotId: String!) {"
            " rollback(id: $id, snapshotId: $snapshotId) }">>,
    pick(<<"rollback">>, do_mutate(Client, Doc, #{<<"id">> => Id, <<"snapshotId">> => SnapshotId})).

%% @doc Create a new sandbox from a snapshot with a random name.
-spec branch(client(), binary()) -> {ok, map()} | {error, term()}.
branch(Client, SnapshotId) ->
    branch(Client, SnapshotId, null).

%% @doc Create a new sandbox named `Name' from a snapshot.
-spec branch(client(), binary(), binary() | null) -> {ok, map()} | {error, term()}.
branch(Client, SnapshotId, Name) ->
    Doc = <<"mutation($snapshotId: String!, $name: String) {"
            " branch(snapshotId: $snapshotId, name: $name) ",
            ?SANDBOX_FIELDS/binary, " }">>,
    pick(<<"branch">>, do_mutate(Client, Doc, #{<<"snapshotId">> => SnapshotId, <<"name">> => Name})).

%% @doc Fork a live sandbox's overlay into a new sandbox with a random name.
-spec fork(client(), binary()) -> {ok, map()} | {error, term()}.
fork(Client, Id) ->
    fork(Client, Id, null).

%% @doc Fork a live sandbox's overlay into a new sandbox named `Name'.
-spec fork(client(), binary(), binary() | null) -> {ok, map()} | {error, term()}.
fork(Client, Id, Name) ->
    Doc = <<"mutation($id: String!, $name: String) {"
            " fork(id: $id, name: $name) ", ?SANDBOX_FIELDS/binary, " }">>,
    pick(<<"fork">>, do_mutate(Client, Doc, #{<<"id">> => Id, <<"name">> => Name})).

%% @doc List saved overlay snapshots.
-spec snapshots(client()) -> {ok, [map()]} | {error, term()}.
snapshots(Client) ->
    pick(<<"snapshots">>, do_query(Client, <<"{ snapshots { name size } }">>, #{})).

%% @doc Delete a snapshot; the boolean reports whether it existed.
-spec delete_snapshot(client(), binary()) -> {ok, boolean()} | {error, term()}.
delete_snapshot(Client, Id) ->
    pick(<<"deleteSnapshot">>,
         do_mutate(Client, <<"mutation($id: String!) { deleteSnapshot(id: $id) }">>,
                   #{<<"id">> => Id})).

cache_op(Client, Field, Id, Path, Key, Backend, Format) ->
    Doc = <<"mutation($id: String!, $path: String!, $key: String!,"
            " $backend: String, $format: String) { ", Field/binary,
            "(id: $id, path: $path, key: $key, backend: $backend, format: $format) }">>,
    do_mutate(Client, Doc, #{<<"id">> => Id, <<"path">> => Path, <<"key">> => Key,
                             <<"backend">> => Backend, <<"format">> => Format}).

do_query(#{url := Url, token := Token}, Doc, Vars) ->
    cvisor_graphql:query(Url, Token, Doc, Vars).

do_mutate(#{url := Url, token := Token}, Doc, Vars) ->
    cvisor_graphql:mutate(Url, Token, Doc, Vars).

pick(Field, {ok, Data}) when is_map(Data) ->
    case maps:find(Field, Data) of
        {ok, Value} -> {ok, Value};
        error -> {error, {missing_field, Field}}
    end;
pick(_Field, {ok, Other}) ->
    {error, {unexpected_response, Other}};
pick(_Field, {error, _} = Err) ->
    Err.

to_bin(B) when is_binary(B) -> B;
to_bin(S) when is_list(S) -> list_to_binary(S).
