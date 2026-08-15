defmodule Cvisor do
  @moduledoc """
  Elixir SDK for [cVisor](https://github.com/tsirysndr/cVisor), an in-process
  Linux sandbox.

  A `Cvisor` value is an immutable configuration you build up with the pipe
  operator, then execute with `run/2`:

      import Cvisor

      Cvisor.new()
      |> memory_limit(256 * 1024 * 1024)   # 256 MiB (cgroup memory.max)
      |> pids_limit(128)                    # cgroup pids.max
      |> cpu_limit(50)                      # 50% of one core (cgroup cpu.max)
      |> env("TOKEN", "xyz")
      |> timeout(5_000)
      |> run("echo hello")
      # => %Cvisor.Output{stdout: "hello\\n", stderr: "", exit_code: 0}

  Each builder function returns an updated `Cvisor` struct, so they chain. The
  configuration is applied to the shared NIF runtime at `run/2` time. Resource
  limits, the network/listen toggles, and the timeout are set explicitly on
  every run; environment variables layer over the guest defaults (`PATH`/`HOME`).

  Linux-only at runtime (the underlying `cvisor` NIF loads `libcvisor.so`).
  """

  alias Cvisor.Output

  @typedoc "An immutable sandbox configuration, built up via the pipe operator."
  @type t :: %__MODULE__{
          env: [{binary(), binary()}],
          memory_max: non_neg_integer(),
          pids_max: non_neg_integer(),
          cpu_percent: non_neg_integer(),
          allow_network: boolean(),
          allow_listen: boolean(),
          timeout_ms: non_neg_integer()
        }

  defstruct env: [],
            memory_max: 0,
            pids_max: 0,
            cpu_percent: 0,
            allow_network: true,
            allow_listen: false,
            timeout_ms: 0

  @doc "A fresh sandbox configuration with defaults (networking on, no limits)."
  @spec new() :: t()
  def new, do: %__MODULE__{}

  @doc """
  Cap guest memory at `bytes` (cgroup `memory.max`); the guest is OOM-killed
  past it. `0` leaves memory unlimited. Requires a writable cgroup v2 hierarchy;
  where one is unavailable the limit gracefully no-ops.
  """
  @spec memory_limit(t(), non_neg_integer()) :: t()
  def memory_limit(%__MODULE__{} = sb, bytes) when is_integer(bytes) and bytes >= 0,
    do: %{sb | memory_max: bytes}

  @doc "Cap the number of processes/threads in the guest tree (cgroup `pids.max`)."
  @spec pids_limit(t(), non_neg_integer()) :: t()
  def pids_limit(%__MODULE__{} = sb, count) when is_integer(count) and count >= 0,
    do: %{sb | pids_max: count}

  @doc """
  Cap guest CPU as a percentage of one core (cgroup `cpu.max`): `50` is half a
  core, `200` is two cores. `0` leaves CPU unlimited.
  """
  @spec cpu_limit(t(), non_neg_integer()) :: t()
  def cpu_limit(%__MODULE__{} = sb, percent) when is_integer(percent) and percent >= 0,
    do: %{sb | cpu_percent: percent}

  @doc """
  Set a guest environment variable, layered over the defaults (`PATH`/`HOME`);
  a repeated key overrides its previous value.
  """
  @spec env(t(), binary() | atom(), binary() | atom()) :: t()
  def env(%__MODULE__{} = sb, key, value) do
    k = to_string(key)
    kept = Enum.reject(sb.env, fn {ek, _} -> ek == k end)
    %{sb | env: kept ++ [{k, to_string(value)}]}
  end

  @doc "Allow (default) or deny outbound INET/INET6 networking for the guest."
  @spec network(t(), boolean()) :: t()
  def network(%__MODULE__{} = sb, allow?) when is_boolean(allow?),
    do: %{sb | allow_network: allow?}

  @doc """
  Allow inbound TCP servers — bind a fixed port, `listen`, `accept` (off by
  default, outbound-only).
  """
  @spec allow_listen(t(), boolean()) :: t()
  def allow_listen(%__MODULE__{} = sb, allow?) when is_boolean(allow?),
    do: %{sb | allow_listen: allow?}

  @doc "SIGKILL the guest after `ms` milliseconds (`0` = no limit). A timed-out run reports exit code 137."
  @spec timeout(t(), non_neg_integer()) :: t()
  def timeout(%__MODULE__{} = sb, ms) when is_integer(ms) and ms >= 0,
    do: %{sb | timeout_ms: ms}

  @doc """
  Run `command` (via `/bin/sh -c`) in the sandbox with this configuration,
  blocking until the guest exits. Returns a `Cvisor.Output`. Raises if the
  underlying runtime returns an error.
  """
  @spec run(t(), binary()) :: Output.t()
  def run(%__MODULE__{} = sb, command) when is_binary(command) do
    :ok = :cvisor.set_allow_network(sb.allow_network)
    :ok = :cvisor.set_allow_listen(sb.allow_listen)
    :ok = :cvisor.set_limits(nil, sb.memory_max, sb.pids_max, sb.cpu_percent)
    Enum.each(sb.env, fn {k, v} -> :ok = :cvisor.set_env(k, v) end)

    case :cvisor.run(command, sb.timeout_ms) do
      {:ok, stdout, stderr, exit_code} ->
        %Output{stdout: stdout, stderr: stderr, exit_code: exit_code}

      {:error, reason} ->
        raise "cvisor: run failed (#{inspect(reason)})"
    end
  end
end
