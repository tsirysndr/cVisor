defmodule Cvisor.Remote do
  @moduledoc """
  A GraphQL-backed high-level API mirroring the NIF-backed `Cvisor` runtime, but
  driven over HTTP against a running cVisor daemon.

  Pure `:httpc` + `:json` (via `Cvisor.GraphQL`) — no Hex deps and no native
  library — so it runs on any platform (macOS included). This is what makes the
  SDK usable off Linux, where the NIF-backed `Cvisor` is unavailable.

      remote = Cvisor.Remote.connect("http://127.0.0.1:8080/graphql", token)
      {:ok, out} = Cvisor.Remote.run(remote, "echo hello")
      # out => %{"stdout" => "hello\\n", "stderr" => "", "exitCode" => 0}
  """

  alias Cvisor.GraphQL

  @sandbox_fields "{ id name allowNetwork allowListen env { key value } " <>
                    "limits { memoryMax pidsMax cpuPercent } }"

  @typedoc "A remote client (a `Cvisor.GraphQL` struct)."
  @type t :: GraphQL.t()

  @doc "Connect to the daemon's GraphQL endpoint at `url` with bearer `token`."
  @spec connect(binary(), binary()) :: t()
  def connect(url, token), do: GraphQL.new(url, token)

  @doc "Daemon liveness and build info."
  @spec health(t()) :: {:ok, map()} | {:error, term()}
  def health(client) do
    pick("health", GraphQL.query(client, "{ health { version ok } }"))
  end

  @doc """
  Run a command in a fresh ephemeral sandbox, SIGKILLing after `timeout_ms`
  (`0` = no limit). Returns `{:ok, %{"stdout" => ..., "stderr" => ...,
  "exitCode" => ...}}`.
  """
  @spec run(t(), binary(), non_neg_integer()) :: {:ok, map()} | {:error, term()}
  def run(client, command, timeout_ms \\ 0) do
    doc = """
    mutation($command: String!, $timeoutMs: Int) {
      run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
    }
    """

    pick("run", GraphQL.mutate(client, doc, %{"command" => command, "timeoutMs" => timeout_ms}))
  end

  @doc "Create a sandbox; a nil/empty name gets a random docker-style name."
  @spec create_sandbox(t(), binary() | nil) :: {:ok, map()} | {:error, term()}
  def create_sandbox(client, name \\ nil) do
    doc = "mutation($name: String) { createSandbox(name: $name) #{@sandbox_fields} }"
    pick("createSandbox", GraphQL.mutate(client, doc, %{"name" => name}))
  end

  @doc "All live sandboxes."
  @spec list_sandboxes(t()) :: {:ok, [map()]} | {:error, term()}
  def list_sandboxes(client) do
    pick("sandboxes", GraphQL.query(client, "{ sandboxes #{@sandbox_fields} }"))
  end

  @doc "Free a sandbox and clean up its overlay."
  @spec free_sandbox(t(), binary()) :: {:ok, boolean()} | {:error, term()}
  def free_sandbox(client, id) do
    pick("freeSandbox", GraphQL.mutate(client, "mutation($id: String!) { freeSandbox(id: $id) }", %{"id" => id}))
  end

  @doc """
  Update a sandbox's network/listen flags, limits, and env. `opts` may hold
  `:allow_network`, `:allow_listen`, `:limits` (a `LimitsInput` map) and `:env`
  (a list of `EnvVarInput` maps); omitted keys are sent as GraphQL `null`.
  """
  @spec configure(t(), binary(), keyword() | map()) :: {:ok, map()} | {:error, term()}
  def configure(client, id, opts \\ []) do
    doc = """
    mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean,
             $limits: LimitsInput, $env: [EnvVarInput!]) {
      configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen,
                limits: $limits, env: $env) #{@sandbox_fields}
    }
    """

    vars = %{
      "id" => id,
      "allowNetwork" => get(opts, :allow_network),
      "allowListen" => get(opts, :allow_listen),
      "limits" => get(opts, :limits),
      "env" => get(opts, :env)
    }

    pick("configure", GraphQL.mutate(client, doc, vars))
  end

  @doc "Write bytes (or a string) to a file inside a sandbox (base64 over the wire)."
  @spec write_file(t(), binary(), binary(), binary()) :: {:ok, boolean()} | {:error, term()}
  def write_file(client, id, path, data) do
    doc = """
    mutation($id: String!, $path: String!, $data: String!) {
      writeFile(id: $id, path: $path, dataBase64: $data)
    }
    """

    vars = %{"id" => id, "path" => path, "data" => Base.encode64(data)}
    pick("writeFile", GraphQL.mutate(client, doc, vars))
  end

  @doc "Read a file from a sandbox (base64-decoded from the wire); returns bytes."
  @spec read_file(t(), binary(), binary()) :: {:ok, binary()} | {:error, term()}
  def read_file(client, id, path) do
    doc = "query($id: String!, $path: String!) { readFile(id: $id, path: $path) }"

    case pick("readFile", GraphQL.query(client, doc, %{"id" => id, "path" => path})) do
      {:ok, encoded} ->
        case Base.decode64(encoded) do
          {:ok, bytes} -> {:ok, bytes}
          :error -> {:error, :invalid_base64}
        end

      err ->
        err
    end
  end

  @doc "Archive a sandbox path into the cache under `key`."
  @spec cache_save(t(), binary(), binary(), binary(), binary(), binary()) ::
          {:ok, boolean()} | {:error, term()}
  def cache_save(client, id, path, key, backend \\ "", format \\ "gzip") do
    pick("cacheSave", cache_op(client, "cacheSave", id, path, key, backend, format))
  end

  @doc "Restore a cached entry into a sandbox path."
  @spec cache_restore(t(), binary(), binary(), binary(), binary(), binary()) ::
          {:ok, boolean()} | {:error, term()}
  def cache_restore(client, id, path, key, backend \\ "", format \\ "gzip") do
    pick("cacheRestore", cache_op(client, "cacheRestore", id, path, key, backend, format))
  end

  @doc "List entries in a cache backend (default backend when nil)."
  @spec cache_list(t(), binary() | nil) :: {:ok, [map()]} | {:error, term()}
  def cache_list(client, backend \\ nil) do
    doc = "query($backend: String) { cacheList(backend: $backend) { name size } }"
    pick("cacheList", GraphQL.query(client, doc, %{"backend" => backend}))
  end

  @doc "Snapshot a sandbox's overlay; returns the snapshot id."
  @spec snapshot(t(), binary(), binary() | nil) :: {:ok, binary()} | {:error, term()}
  def snapshot(client, id, snapshot_id \\ nil) do
    doc = "mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }"
    pick("snapshot", GraphQL.mutate(client, doc, %{"id" => id, "snapshotId" => snapshot_id}))
  end

  @doc "Replace a sandbox's overlay with a snapshot (discard changes since)."
  @spec rollback(t(), binary(), binary()) :: {:ok, boolean()} | {:error, term()}
  def rollback(client, id, snapshot_id) do
    doc = "mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }"
    pick("rollback", GraphQL.mutate(client, doc, %{"id" => id, "snapshotId" => snapshot_id}))
  end

  @doc "Create a new sandbox from a snapshot."
  @spec branch(t(), binary(), binary() | nil) :: {:ok, map()} | {:error, term()}
  def branch(client, snapshot_id, name \\ nil) do
    doc =
      "mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) #{@sandbox_fields} }"

    pick("branch", GraphQL.mutate(client, doc, %{"snapshotId" => snapshot_id, "name" => name}))
  end

  @doc "Fork a live sandbox's overlay into a new sandbox."
  @spec fork(t(), binary(), binary() | nil) :: {:ok, map()} | {:error, term()}
  def fork(client, id, name \\ nil) do
    doc = "mutation($id: String!, $name: String) { fork(id: $id, name: $name) #{@sandbox_fields} }"
    pick("fork", GraphQL.mutate(client, doc, %{"id" => id, "name" => name}))
  end

  @doc "List saved overlay snapshots."
  @spec snapshots(t()) :: {:ok, [map()]} | {:error, term()}
  def snapshots(client) do
    pick("snapshots", GraphQL.query(client, "{ snapshots { name size } }"))
  end

  @doc "Delete a snapshot; the boolean reports whether it existed."
  @spec delete_snapshot(t(), binary()) :: {:ok, boolean()} | {:error, term()}
  def delete_snapshot(client, id) do
    pick("deleteSnapshot", GraphQL.mutate(client, "mutation($id: String!) { deleteSnapshot(id: $id) }", %{"id" => id}))
  end

  defp cache_op(client, field, id, path, key, backend, format) do
    doc = """
    mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
      #{field}(id: $id, path: $path, key: $key, backend: $backend, format: $format)
    }
    """

    GraphQL.mutate(client, doc, %{
      "id" => id,
      "path" => path,
      "key" => key,
      "backend" => backend,
      "format" => format
    })
  end

  defp get(opts, key) when is_list(opts), do: Keyword.get(opts, key)
  defp get(opts, key) when is_map(opts), do: Map.get(opts, key)

  defp pick(field, {:ok, data}) when is_map(data) do
    case Map.fetch(data, field) do
      {:ok, value} -> {:ok, value}
      :error -> {:error, {:missing_field, field}}
    end
  end

  defp pick(_field, {:ok, other}), do: {:error, {:unexpected_response, other}}
  defp pick(_field, {:error, _} = err), do: err
end
