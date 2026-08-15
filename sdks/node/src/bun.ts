// Bun entry point (the "bun" export condition): a bun:ffi wrapper over the
// libcvisor C ABI, exposing the same API as the napi entry (index.ts).

import { dlopen, FFIType, ptr, toArrayBuffer, type Pointer } from "bun:ffi";
import { libraryPath } from "./libpath";
import { buildCommand, bytesToStream, createOutput, Output, RunOptions } from "./output";

export type { Output, RunOptions } from "./output";

const { symbols } = dlopen(libraryPath(), {
  cvisor_sandbox_new: { args: [], returns: FFIType.ptr },
  cvisor_sandbox_free: { args: [FFIType.ptr], returns: FFIType.void },
  cvisor_sandbox_set_log_level: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.void,
  },
  cvisor_sandbox_set_allow_network: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.void,
  },
  cvisor_run: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  cvisor_run_timeout: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.u64],
    returns: FFIType.ptr,
  },
  cvisor_output_free: { args: [FFIType.ptr], returns: FFIType.void },
  cvisor_output_exit_code: { args: [FFIType.ptr], returns: FFIType.i32 },
  cvisor_output_stdout: {
    args: [FFIType.ptr, FFIType.ptr],
    returns: FFIType.ptr,
  },
  cvisor_output_stderr: {
    args: [FFIType.ptr, FFIType.ptr],
    returns: FFIType.ptr,
  },
  cvisor_bytes_free: { args: [FFIType.ptr, FFIType.u64], returns: FFIType.void },
});

function readOutput(
  out: Pointer,
  accessor: (o: Pointer, lenPtr: Pointer) => Pointer | null,
): Uint8Array {
  const len = new BigUint64Array(1);
  const p = accessor(out, ptr(len));
  const n = Number(len[0]);
  if (!p || n === 0) return new Uint8Array(0);
  // Copy out of native memory before freeing it.
  const copy = new Uint8Array(toArrayBuffer(p, 0, n)).slice();
  symbols.cvisor_bytes_free(p, BigInt(n));
  return copy;
}

export class Sandbox {
  private ptr: Pointer | null;

  constructor() {
    this.ptr = symbols.cvisor_sandbox_new();
    if (!this.ptr) throw new Error("failed to create sandbox");
  }

  setLogLevel(level: "OFF" | "DEBUG"): void {
    symbols.cvisor_sandbox_set_log_level(this.ptr, level === "DEBUG" ? 1 : 0);
  }

  /** Enable or disable outbound INET/INET6 networking (default on). */
  setAllowNetwork(allow: boolean): void {
    symbols.cvisor_sandbox_set_allow_network(this.ptr, allow ? 1 : 0);
  }

  /** Run a command to completion; the returned Output's streams replay the
   * captured bytes so the shape matches the napi entry. */
  runCmd(command: string, options: RunOptions = {}): Output {
    const cmd = Buffer.from(command + "\0", "utf8");
    const timeoutMs = options.timeoutMs && options.timeoutMs > 0 ? options.timeoutMs : 0;
    const out = timeoutMs
      ? symbols.cvisor_run_timeout(this.ptr, ptr(cmd), BigInt(timeoutMs))
      : symbols.cvisor_run(this.ptr, ptr(cmd));
    if (!out) throw new Error("sandbox run failed");
    try {
      const stdoutBytes = readOutput(out, symbols.cvisor_output_stdout);
      const stderrBytes = readOutput(out, symbols.cvisor_output_stderr);
      const exitCode = symbols.cvisor_output_exit_code(out);
      return createOutput(bytesToStream(stdoutBytes), bytesToStream(stderrBytes), exitCode);
    } finally {
      symbols.cvisor_output_free(out);
    }
  }

  /** Tagged-template command runner: `sb.sh\`ls -l ${dir}\``. */
  sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
    return this.runCmd(buildCommand(strings, values));
  }

  close(): void {
    if (this.ptr) {
      symbols.cvisor_sandbox_free(this.ptr);
      this.ptr = null;
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
