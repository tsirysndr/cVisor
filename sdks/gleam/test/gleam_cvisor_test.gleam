import gleam_cvisor as cvisor
import gleeunit
import gleeunit/should

pub fn main() {
  gleeunit.main()
}

pub fn run_test() {
  let out = cvisor.new() |> cvisor.run("echo hello")
  out.stdout |> should.equal("hello\n")
  out.stderr |> should.equal("")
  out.exit_code |> should.equal(0)
}

pub fn exit_code_test() {
  let out = cvisor.new() |> cvisor.run("exit 3")
  out.exit_code |> should.equal(3)
}

pub fn env_test() {
  let out =
    cvisor.new()
    |> cvisor.env("FOO", "bar")
    |> cvisor.env("GREETING", "hi there")
    |> cvisor.run("echo $FOO-$GREETING")
  out.stdout |> should.equal("bar-hi there\n")
}

pub fn timeout_test() {
  let out = cvisor.new() |> cvisor.timeout(500) |> cvisor.run("sleep 30")
  out.exit_code |> should.equal(137)
}

// No writable cgroup v2 in the test container, so limits gracefully no-op;
// a limited run must still succeed.
pub fn limits_test() {
  let out =
    cvisor.new()
    |> cvisor.memory_limit(256 * 1024 * 1024)
    |> cvisor.pids_limit(128)
    |> cvisor.cpu_limit(50)
    |> cvisor.run("echo limited")
  out.stdout |> should.equal("limited\n")
  out.exit_code |> should.equal(0)
}
