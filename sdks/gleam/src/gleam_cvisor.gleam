//// Gleam SDK for [cVisor](https://github.com/tsirysndr/cVisor), an in-process
//// Linux sandbox.
////
//// A `Sandbox` is an immutable configuration you build up with the pipe
//// operator, then execute with `run`:
////
//// ```gleam
//// import gleam_cvisor as cvisor
////
//// pub fn main() {
////   let out =
////     cvisor.new()
////     |> cvisor.memory_limit(256 * 1024 * 1024)   // 256 MiB (cgroup memory.max)
////     |> cvisor.pids_limit(128)                    // cgroup pids.max
////     |> cvisor.cpu_limit(50)                      // 50% of one core (cgroup cpu.max)
////     |> cvisor.env("TOKEN", "xyz")
////     |> cvisor.timeout(5000)
////     |> cvisor.run("echo hello")
////   // out == Output(stdout: "hello\n", stderr: "", exit_code: 0)
//// }
//// ```
////
//// Each builder returns an updated `Sandbox`, so they chain. The configuration
//// is applied to the shared NIF runtime at `run` time. Erlang target only (the
//// underlying `cvisor` NIF loads `libcvisor.so`); Linux-only at runtime.

import gleam/bit_array
import gleam/list
import gleam/result

/// An immutable sandbox configuration, built up via the pipe operator.
pub opaque type Sandbox {
  Sandbox(
    env: List(#(BitArray, BitArray)),
    memory_max: Int,
    pids_max: Int,
    cpu_percent: Int,
    allow_network: Bool,
    allow_listen: Bool,
    timeout_ms: Int,
  )
}

/// The result of a sandboxed `run`: captured `stdout`/`stderr` and the guest's
/// `exit_code` (shell convention: the exit status, or `128 + signo` when killed
/// by a signal — e.g. `137` for a timeout SIGKILL).
pub type Output {
  Output(stdout: String, stderr: String, exit_code: Int)
}

/// A fresh sandbox configuration with defaults (networking on, no limits).
pub fn new() -> Sandbox {
  Sandbox(
    env: [],
    memory_max: 0,
    pids_max: 0,
    cpu_percent: 0,
    allow_network: True,
    allow_listen: False,
    timeout_ms: 0,
  )
}

/// Cap guest memory at `bytes` (cgroup `memory.max`); the guest is OOM-killed
/// past it. `0` leaves memory unlimited. Requires a writable cgroup v2
/// hierarchy; where one is unavailable the limit gracefully no-ops.
pub fn memory_limit(sandbox: Sandbox, bytes: Int) -> Sandbox {
  Sandbox(..sandbox, memory_max: bytes)
}

/// Cap the number of processes/threads in the guest tree (cgroup `pids.max`).
pub fn pids_limit(sandbox: Sandbox, count: Int) -> Sandbox {
  Sandbox(..sandbox, pids_max: count)
}

/// Cap guest CPU as a percentage of one core (cgroup `cpu.max`): `50` is half a
/// core, `200` is two cores. `0` leaves CPU unlimited.
pub fn cpu_limit(sandbox: Sandbox, percent: Int) -> Sandbox {
  Sandbox(..sandbox, cpu_percent: percent)
}

/// Set a guest environment variable, layered over the defaults (`PATH`/`HOME`);
/// a repeated key overrides its previous value.
pub fn env(sandbox: Sandbox, key: String, value: String) -> Sandbox {
  let k = bit_array.from_string(key)
  let kept = list.filter(sandbox.env, fn(pair) { pair.0 != k })
  Sandbox(..sandbox, env: list.append(kept, [#(k, bit_array.from_string(value))]))
}

/// Allow (default) or deny outbound INET/INET6 networking for the guest.
pub fn network(sandbox: Sandbox, allow: Bool) -> Sandbox {
  Sandbox(..sandbox, allow_network: allow)
}

/// Allow inbound TCP servers — bind a fixed port, `listen`, `accept` (off by
/// default, outbound-only).
pub fn allow_listen(sandbox: Sandbox, allow: Bool) -> Sandbox {
  Sandbox(..sandbox, allow_listen: allow)
}

/// SIGKILL the guest after `ms` milliseconds (`0` = no limit). A timed-out run
/// reports exit code 137.
pub fn timeout(sandbox: Sandbox, ms: Int) -> Sandbox {
  Sandbox(..sandbox, timeout_ms: ms)
}

/// Run `command` (via `/bin/sh -c`) in the sandbox with this configuration,
/// blocking until the guest exits.
pub fn run(sandbox: Sandbox, command: String) -> Output {
  let #(out, err, code) =
    ffi_run(
      bit_array.from_string(command),
      sandbox.timeout_ms,
      sandbox.memory_max,
      sandbox.pids_max,
      sandbox.cpu_percent,
      sandbox.allow_network,
      sandbox.allow_listen,
      sandbox.env,
    )
  Output(stdout: to_string(out), stderr: to_string(err), exit_code: code)
}

fn to_string(bytes: BitArray) -> String {
  bit_array.to_string(bytes) |> result.unwrap("")
}

@external(erlang, "cvisor_ffi", "run")
fn ffi_run(
  command: BitArray,
  timeout_ms: Int,
  memory_max: Int,
  pids_max: Int,
  cpu_percent: Int,
  allow_network: Bool,
  allow_listen: Bool,
  env: List(#(BitArray, BitArray)),
) -> #(BitArray, BitArray, Int)
