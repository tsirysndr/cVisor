// Deno entry point (the "deno" export condition): a Deno.dlopen wrapper over
// the libcvisor C ABI, exposing the same API as the napi entry (index.ts).
// Needs --allow-ffi (plus --allow-env for the CVISOR_LIB override).

import { libraryPath } from "./libpath";
import { buildCommand, bytesToStream, CacheOptions, createOutput, Limits, Output, RunOptions } from "./output";
import {
  makeShell,
  runStreaming as runStreamingImpl,
  SessionNative,
  Shell,
  ShellOptions,
  StreamOptions,
} from "./session";

export type { Output, RunOptions, Limits } from "./output";
export type { Shell, ShellOptions, StreamOptions } from "./session";
export { GraphQLClient } from "./graphql";
export { RemoteSandbox } from "./remote";
export type { RemoteSandboxInfo, RemoteRunResult, RemoteLimits, ConfigureOptions } from "./remote";

// This package compiles without Deno's type definitions; the entry only ever
// executes under Deno, where the global is present.
declare const Deno: any;

// The libcvisor .so is Linux-only, so loading it is deferred until a Sandbox
// (the FFI API) is actually constructed. Merely importing this module — e.g. to
// use the pure-fetch GraphQLClient / RemoteSandbox — must not dlopen, so it
// works on macOS. `lib.symbols` dlopens on first access.
function loadLib() {
  return Deno.dlopen(libraryPath(), {
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
  cvisor_sandbox_set_allow_listen: {
    parameters: ["pointer", "i32"],
    result: "void",
  },
  cvisor_sandbox_set_env: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "void",
  },
  cvisor_sandbox_set_limits: {
    parameters: ["pointer", "u64", "u64", "u32"],
    result: "void",
  },
  cvisor_sandbox_write_file: {
    parameters: ["pointer", "buffer", "buffer", "usize"],
    result: "i32",
  },
  cvisor_sandbox_read_file: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "pointer",
  },
  cvisor_sandbox_copy_into: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "i32",
  },
  cvisor_sandbox_copy_out: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "i32",
  },
  cvisor_cache_save: {
    parameters: ["pointer", "buffer", "buffer", "buffer", "buffer"],
    result: "i32",
  },
  cvisor_cache_restore: {
    parameters: ["pointer", "buffer", "buffer", "buffer", "buffer"],
    result: "i32",
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
}

let loadedLib: ReturnType<typeof loadLib> | undefined;
const lib = {
  get symbols() {
    loadedLib ??= loadLib();
    return loadedLib.symbols;
  },
};

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

/** NUL-terminated UTF-8 buffer for a Deno FFI `buffer` string arg. */
function cstr(s: string): Uint8Array {
  return new TextEncoder().encode(s + "\0");
}

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

  /** Enable or disable inbound TCP servers — bind fixed port, listen (default off). */
  setAllowListen(allow: boolean): void {
    lib.symbols.cvisor_sandbox_set_allow_listen(this.#ptr, allow ? 1 : 0);
  }

  /** Set an environment variable for the guest (applies to subsequent runs). */
  setEnv(key: string, value: string): void {
    lib.symbols.cvisor_sandbox_set_env(this.#ptr, cstr(key), cstr(value));
  }

  /** Cap guest resources via cgroup v2 (applies to subsequent runs). */
  setLimits(limits: Limits): void {
    lib.symbols.cvisor_sandbox_set_limits(
      this.#ptr,
      BigInt(limits.memoryMax ?? 0),
      BigInt(limits.pidsMax ?? 0),
      limits.cpuPercent ?? 0,
    );
  }

  /** Write a file into the sandbox overlay (visible to later runs of this sandbox). */
  writeFile(path: string, data: Uint8Array | string): void {
    const p = new TextEncoder().encode(path + "\0");
    const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
    const rc = lib.symbols.cvisor_sandbox_write_file(this.#ptr, p, bytes, BigInt(bytes.length));
    if (rc !== 0) throw new Error(`writeFile failed (errno ${-rc})`);
  }

  /** Read a file from the sandbox overlay (the guest's view). */
  readFile(path: string): Uint8Array {
    const p = new TextEncoder().encode(path + "\0");
    const lenBuf = new Uint8Array(8);
    const dataPtr = lib.symbols.cvisor_sandbox_read_file(this.#ptr, p, lenBuf);
    const n = Number(new DataView(lenBuf.buffer).getBigUint64(0, true));
    if (dataPtr === null || n === 0) return new Uint8Array(0);
    const view = new Deno.UnsafePointerView(dataPtr);
    const copy = new Uint8Array(n);
    view.copyInto(copy);
    lib.symbols.cvisor_bytes_free(dataPtr, BigInt(n));
    return copy;
  }

  /** Copy a host file or directory tree into the sandbox (recursive, ignore-aware). */
  copyInto(hostPath: string, guestPath: string): void {
    const rc = lib.symbols.cvisor_sandbox_copy_into(this.#ptr, cstr(hostPath), cstr(guestPath));
    if (rc !== 0) throw new Error(`copyInto failed (errno ${-rc})`);
  }

  /** Copy a sandbox file or directory tree out to the host. */
  copyOut(guestPath: string, hostPath: string): void {
    const rc = lib.symbols.cvisor_sandbox_copy_out(this.#ptr, cstr(guestPath), cstr(hostPath));
    if (rc !== 0) throw new Error(`copyOut failed (errno ${-rc})`);
  }

  /** Archive a sandbox directory to the cache under `key`. */
  cacheSave(sandboxPath: string, key: string, options: CacheOptions = {}): void {
    const rc = lib.symbols.cvisor_cache_save(
      this.#ptr, cstr(sandboxPath), cstr(key), cstr(options.backend ?? ""), cstr(options.format ?? "gzip"),
    );
    if (rc !== 0) throw new Error(`cacheSave failed (errno ${-rc})`);
  }

  /** Restore a cached archive (`key`, or its prefix) into a sandbox directory. */
  cacheRestore(sandboxPath: string, key: string, options: CacheOptions = {}): void {
    const rc = lib.symbols.cvisor_cache_restore(
      this.#ptr, cstr(sandboxPath), cstr(key), cstr(options.backend ?? ""), cstr(options.format ?? "gzip"),
    );
    if (rc !== 0) throw new Error(`cacheRestore failed (errno ${-rc})`);
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
