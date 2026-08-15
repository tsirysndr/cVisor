defmodule Cvisor.Output do
  @moduledoc """
  The result of a sandboxed `Cvisor.run/2`: captured `stdout`/`stderr` (binaries)
  and the guest's `exit_code` (shell convention: the exit status, or `128 + signo`
  when the guest was killed by a signal — e.g. `137` for a timeout SIGKILL).
  """

  @enforce_keys [:stdout, :stderr, :exit_code]
  defstruct [:stdout, :stderr, :exit_code]

  @type t :: %__MODULE__{
          stdout: binary(),
          stderr: binary(),
          exit_code: integer()
        }
end
