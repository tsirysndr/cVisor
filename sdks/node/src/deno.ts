// Deno entry point (the "deno" export condition): a Deno.dlopen wrapper over
// the libcvisor C ABI, exposing the same API as the napi entry (index.ts).
// Needs --allow-ffi (plus --allow-env for the CVISOR_LIB override).

import { libraryPath } from "./libpath";
import { buildCommand, bytesToStream, createOutput, Output, RunOptions } from "./output";
import {
  makeShell,
  runStreaming as runStreamingImpl,
  SessionNative,
  Shell,
  ShellOptions,
  StreamOptions,
} from "./session";

export type { Output, RunOptions } from "./output";
export type { Shell, ShellOptions, StreamOptions } from "./session";

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
  cvisor_session_start: {
    parameters: ["pointer", "buffer", "i32"],
    result: "pointer",
  },
  cvisor_session_read_stdout: {
    parameters: ["pointer", "buffer"],
    result: "pointer",
  },
  cvisor_session_read_stderr: {
    parameters: ["pointer", "buffer"],
    result: "pointer",
  },
  cvisor_session_write_stdin: {
    parameters: ["pointer", "buffer", "usize"],
    result: "i64",
  },
  cvisor_session_resize: {
    parameters: ["pointer", "u16", "u16"],
    result: "void",
  },
  cvisor_session_try_wait: {
    parameters: ["pointer", "buffer"],
    result: "i32",
  },
  cvisor_session_kill: { parameters: ["pointer"], result: "void" },
  cvisor_session_free: { parameters: ["pointer"], result: "void" },
});

/** Bind a Deno.dlopen session pointer to the runtime-agnostic SessionNative. */
function denoSession(sess: Pointer): SessionNative {
  const drain = (accessor: Accessor) => {
    const bytes = readOutput(sess, accessor);
    return bytes.length > 0 ? bytes : null;
  };
  return {
    readStdout: () => drain(lib.symbols.cvisor_session_read_stdout),
    readStderr: () => drain(lib.symbols.cvisor_session_read_stderr),
    writeStdin: (data) => Number(lib.symbols.cvisor_session_write_stdin(sess, data, BigInt(data.length))),
    resize: (rows, cols) => lib.symbols.cvisor_session_resize(sess, rows, cols),
    tryWait: () => {
      const done = new Uint8Array(4);
      const code = lib.symbols.cvisor_session_try_wait(sess, done);
      return new DataView(done.buffer).getInt32(0, true) ? code : null;
    },
    kill: () => lib.symbols.cvisor_session_kill(sess),
    close: () => lib.symbols.cvisor_session_free(sess),
  };
}

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

  /** Run a command in the background, streaming output to the callbacks. */
  runStreaming(command: string, options: StreamOptions = {}): Promise<number> {
    const cmd = new TextEncoder().encode(command + "\0");
    const sess = lib.symbols.cvisor_session_start(this.#ptr, cmd, 0);
    if (sess === null) throw new Error("session start failed");
    return runStreamingImpl(denoSession(sess), options);
  }

  /** Start an interactive `/bin/sh` on a PTY. */
  shell(options: ShellOptions = {}): Shell {
    const sess = lib.symbols.cvisor_session_start(this.#ptr, null, 1);
    if (sess === null) throw new Error("session start failed");
    return makeShell(denoSession(sess), options);
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
