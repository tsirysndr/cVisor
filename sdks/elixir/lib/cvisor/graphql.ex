defmodule Cvisor.GraphQL do
  @moduledoc """
  Optional GraphQL client for the cVisor daemon's HTTP API.

  Uses Erlang's `:httpc`/`:inets` for HTTP and OTP 27's `:json` module for
  encode/decode — no extra Hex dependency and no native library — so it works
  on any platform (macOS included), unlike the NIF-backed `Cvisor` runtime.

      gql = Cvisor.GraphQL.new("http://127.0.0.1:8080/graphql", token)
      {:ok, data} = Cvisor.GraphQL.query(gql, "{ health { version ok } }")
      # data => %{"health" => %{"version" => "0.1.0", "ok" => true}}

  Requires OTP 27+ for the `:json` module.
  """

  @typedoc "A GraphQL client: an endpoint URL and a bearer token."
  @type t :: %__MODULE__{url: binary(), token: binary()}

  defstruct [:url, :token]

  @doc "Build a client for `url` authenticating with bearer `token`."
  @spec new(binary(), binary()) :: t()
  def new(url, token) when is_binary(url) and is_binary(token) do
    %__MODULE__{url: url, token: token}
  end

  @doc """
  Run a GraphQL query `document` with optional `variables`; returns
  `{:ok, data}` with the decoded `data` map, or `{:error, reason}`.
  """
  @spec query(t(), binary(), map()) :: {:ok, map()} | {:error, term()}
  def query(%__MODULE__{} = client, document, variables \\ %{}) do
    request(client, document, variables)
  end

  @doc "Run a GraphQL mutation `document`; see `query/3`."
  @spec mutate(t(), binary(), map()) :: {:ok, map()} | {:error, term()}
  def mutate(%__MODULE__{} = client, document, variables \\ %{}) do
    request(client, document, variables)
  end

  defp request(%__MODULE__{url: url, token: token}, document, variables) do
    ensure_started()
    body = :json.encode(%{"query" => document, "variables" => variables}) |> IO.iodata_to_binary()
    headers = [{~c"authorization", ~c"Bearer " ++ String.to_charlist(token)}]
    req = {String.to_charlist(url), headers, ~c"application/json", body}

    case :httpc.request(:post, req, [], body_format: :binary) do
      {:ok, {{_version, status, _reason}, _resp_headers, resp_body}}
      when status >= 200 and status < 300 ->
        decode(resp_body)

      {:ok, {{_version, status, reason}, _resp_headers, _resp_body}} ->
        {:error, {:http_error, status, to_string(reason)}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp decode(resp_body) do
    case :json.decode(resp_body) do
      %{"errors" => [_ | _] = errors} ->
        {:error, {:graphql, join_messages(errors)}}

      %{"data" => data} ->
        {:ok, data}

      other ->
        {:error, {:unexpected_response, other}}
    end
  end

  defp join_messages(errors) do
    errors
    |> Enum.map(fn e -> Map.get(e, "message", "unknown error") end)
    |> Enum.join("; ")
  end

  defp ensure_started do
    {:ok, _} = Application.ensure_all_started(:inets)
    {:ok, _} = Application.ensure_all_started(:ssl)
    :ok
  end
end
