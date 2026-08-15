defmodule CvisorTest do
  use ExUnit.Case, async: false
  doctest Cvisor

  test "runs a command and captures stdout" do
    out = Cvisor.new() |> Cvisor.run("echo hello")
    assert out.stdout == "hello\n"
    assert out.stderr == ""
    assert out.exit_code == 0
  end

  test "exit code follows shell convention" do
    assert (Cvisor.new() |> Cvisor.run("exit 3")).exit_code == 3
  end

  test "environment variables are passed" do
    out =
      Cvisor.new()
      |> Cvisor.env("FOO", "bar")
      |> Cvisor.env("GREETING", "hi there")
      |> Cvisor.run("echo $FOO-$GREETING")

    assert out.stdout == "bar-hi there\n"
  end

  test "timeout SIGKILLs a runaway command (exit 137)" do
    out = Cvisor.new() |> Cvisor.timeout(500) |> Cvisor.run("sleep 30")
    assert out.exit_code == 137
  end

  test "resource limits pipe through and do not break a run" do
    # No writable cgroup v2 in the test container, so limits gracefully no-op;
    # a limited run must still succeed.
    out =
      Cvisor.new()
      |> Cvisor.memory_limit(256 * 1024 * 1024)
      |> Cvisor.pids_limit(128)
      |> Cvisor.cpu_limit(50)
      |> Cvisor.run("echo limited")

    assert out.stdout == "limited\n"
    assert out.exit_code == 0
  end
end
