// Deno entry point (the "deno" export condition): a Deno.dlopen wrapper over
// the libcvisor C ABI, exposing the same API as the napi entry (index.ts).
// Needs --allow-ffi (plus --allow-env for the CVISOR_LIB override).

import { libraryPath } from "./libpath";
import { buildCommand, bytesToStream, createOutput, Output, RunOptions } from "./output";

export type { Output, RunOptions } from "./output";

// This package compiles without Deno's type definitions; the entry only ever
// executes under Deno, where the global is present.
declare const Deno: any;

const lib = Deno.dlopen(libraryPath(), {
  cvisor_sandbox_new: { parameters: [], result: "pointer" },
  cvisor_sandbox_free: { parameters: ["pointer"], result: "void" },
  cvisor_sandbox_set_log_level: {
    parameters: ["pointer", "i32"],
    result: "void",
  },
  cvisor_sandbox_set_allow_network: {
    parameters: ["pointer", "i32"],
    result: "void",
  },
  cvisor_run: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_run_timeout: {
    parameters: ["pointer", "buffer", "u64"],
    result: "pointer",
  },
  cvisor_output_free: { parameters: ["pointer"], result: "void" },
  cvisor_output_exit_code: { parameters: ["pointer"], result: "i32" },
  cvisor_output_stdout: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_output_stderr: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_bytes_free: { parameters: ["pointer", "usize"], result: "void" },
});

type Pointer = unknown;
type Accessor = (o: Pointer, lenBuf: Uint8Array) => Pointer;

function readOutput(out: Pointer, accessor: Accessor): Uint8Array {
  const lenBuf = new Uint8Array(8);
  const p = accessor(out, lenBuf);
  const n = Number(new DataView(lenBuf.buffer).getBigUint64(0, true));
  if (p === null || n === 0) return new Uint8Array(0);
  const view = new Deno.UnsafePointerView(p);
  const copy = new Uint8Array(n);
  view.copyInto(copy);
  lib.symbols.cvisor_bytes_free(p, BigInt(n));
  return copy;
}

export class Sandbox {
  #ptr: Pointer;

  constructor() {
    this.#ptr = lib.symbols.cvisor_sandbox_new();
    if (this.#ptr === null) throw new Error("failed to create sandbox");
  }

  setLogLevel(level: "OFF" | "DEBUG"): void {
    lib.symbols.cvisor_sandbox_set_log_level(this.#ptr, level === "DEBUG" ? 1 : 0);
  }

  /** Enable or disable outbound INET/INET6 networking (default on). */
  setAllowNetwork(allow: boolean): void {
    lib.symbols.cvisor_sandbox_set_allow_network(this.#ptr, allow ? 1 : 0);
  }

  /** Run a command to completion; the returned Output's streams replay the
   * captured bytes so the shape matches the napi entry. */
  runCmd(command: string, options: RunOptions = {}): Output {
    const cmd = new TextEncoder().encode(command + "\0");
    const timeoutMs = options.timeoutMs && options.timeoutMs > 0 ? options.timeoutMs : 0;
    const out = timeoutMs
      ? lib.symbols.cvisor_run_timeout(this.#ptr, cmd, BigInt(timeoutMs))
      : lib.symbols.cvisor_run(this.#ptr, cmd);
    if (out === null) throw new Error("sandbox run failed");
    try {
      const stdoutBytes = readOutput(out, lib.symbols.cvisor_output_stdout);
      const stderrBytes = readOutput(out, lib.symbols.cvisor_output_stderr);
      const exitCode = lib.symbols.cvisor_output_exit_code(out);
      return createOutput(bytesToStream(stdoutBytes), bytesToStream(stderrBytes), exitCode);
    } finally {
      lib.symbols.cvisor_output_free(out);
    }
  }

  /** Tagged-template command runner: `sb.sh\`ls -l ${dir}\``. */
  sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
    return this.runCmd(buildCommand(strings, values));
  }

  close(): void {
    if (this.#ptr !== null) {
      lib.symbols.cvisor_sandbox_free(this.#ptr);
      this.#ptr = null;
    }
  }
}

let defaultSandbox: Sandbox | undefined;

/**
 * Run a command in a shared, lazily-created sandbox via a tagged template:
 *
 *   import { sh } from "cvisor";
 *   const files = await sh`ls -l`.stdout();
 */
export function sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
  defaultSandbox ??= new Sandbox();
  return defaultSandbox.sh(strings, ...values);
}
