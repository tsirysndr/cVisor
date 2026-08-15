// Bun entry point (the "bun" export condition): a bun:ffi wrapper over the
// libcvisor C ABI, exposing the same API as the napi entry (index.ts).

import { dlopen, FFIType, ptr, toArrayBuffer, type Pointer } from "bun:ffi";
import { libraryPath } from "./libpath";
import { buildCommand, bytesToStream, CacheOptions, createOutput, Output, RunOptions } from "./output";
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
  cvisor_sandbox_set_allow_listen: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.void,
  },
  cvisor_sandbox_set_env: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring],
    returns: FFIType.void,
  },
  cvisor_sandbox_write_file: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.ptr, FFIType.u64],
    returns: FFIType.i32,
  },
  cvisor_sandbox_read_file: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.ptr],
    returns: FFIType.ptr,
  },
  cvisor_sandbox_copy_into: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
  },
  cvisor_sandbox_copy_out: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
  },
  cvisor_cache_save: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
  },
  cvisor_cache_restore: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
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
  cvisor_session_start: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.i32],
    returns: FFIType.ptr,
  },
  cvisor_session_read_stdout: {
    args: [FFIType.ptr, FFIType.ptr],
    returns: FFIType.ptr,
  },
  cvisor_session_read_stderr: {
    args: [FFIType.ptr, FFIType.ptr],
    returns: FFIType.ptr,
  },
  cvisor_session_write_stdin: {
    args: [FFIType.ptr, FFIType.ptr, FFIType.u64],
    returns: FFIType.i64,
  },
  cvisor_session_resize: {
    args: [FFIType.ptr, FFIType.u16, FFIType.u16],
    returns: FFIType.void,
  },
  cvisor_session_try_wait: {
    args: [FFIType.ptr, FFIType.ptr],
    returns: FFIType.i32,
  },
  cvisor_session_kill: { args: [FFIType.ptr], returns: FFIType.void },
  cvisor_session_free: { args: [FFIType.ptr], returns: FFIType.void },
});

/** A NUL-terminated C string pointer for an FFI cstring arg. */
function cstr(s: string): Pointer {
  return ptr(Buffer.from(s + "\0", "utf8"));
}

/** Bind a bun:ffi session pointer to the runtime-agnostic SessionNative shape. */
function bunSession(sess: Pointer): SessionNative {
  const drain = (accessor: (o: Pointer, lenPtr: Pointer) => Pointer | null) => {
    const bytes = readOutput(sess, accessor);
    return bytes.length > 0 ? bytes : null;
  };
  return {
    readStdout: () => drain(symbols.cvisor_session_read_stdout),
    readStderr: () => drain(symbols.cvisor_session_read_stderr),
    writeStdin: (data) => {
      const buf = Buffer.from(data);
      return Number(symbols.cvisor_session_write_stdin(sess, ptr(buf), BigInt(buf.length)));
    },
    resize: (rows, cols) => symbols.cvisor_session_resize(sess, rows, cols),
    tryWait: () => {
      const done = new Int32Array(1);
      const code = symbols.cvisor_session_try_wait(sess, ptr(done));
      return done[0] ? code : null;
    },
    kill: () => symbols.cvisor_session_kill(sess),
    close: () => symbols.cvisor_session_free(sess),
  };
}

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

  /** Enable or disable inbound TCP servers — bind fixed port, listen (default off). */
  setAllowListen(allow: boolean): void {
    symbols.cvisor_sandbox_set_allow_listen(this.ptr, allow ? 1 : 0);
  }

  /** Set an environment variable for the guest (applies to subsequent runs). */
  setEnv(key: string, value: string): void {
    symbols.cvisor_sandbox_set_env(this.ptr, cstr(key), cstr(value));
  }

  /** Write a file into the sandbox overlay (visible to later runs of this sandbox). */
  writeFile(path: string, data: Uint8Array | string): void {
    const p = Buffer.from(path + "\0", "utf8");
    const bytes = typeof data === "string" ? Buffer.from(data, "utf8") : Buffer.from(data);
    const rc = symbols.cvisor_sandbox_write_file(this.ptr, ptr(p), ptr(bytes), BigInt(bytes.length));
    if (rc !== 0) throw new Error(`writeFile failed (errno ${-rc})`);
  }

  /** Read a file from the sandbox overlay (the guest's view). */
  readFile(path: string): Uint8Array {
    const p = Buffer.from(path + "\0", "utf8");
    const len = new BigUint64Array(1);
    const dataPtr = symbols.cvisor_sandbox_read_file(this.ptr, ptr(p), ptr(len));
    const n = Number(len[0]);
    if (!dataPtr || n === 0) return new Uint8Array(0);
    const copy = new Uint8Array(toArrayBuffer(dataPtr, 0, n)).slice();
    symbols.cvisor_bytes_free(dataPtr, BigInt(n));
    return copy;
  }

  /** Copy a host file or directory tree into the sandbox (recursive, ignore-aware). */
  copyInto(hostPath: string, guestPath: string): void {
    const rc = symbols.cvisor_sandbox_copy_into(this.ptr, cstr(hostPath), cstr(guestPath));
    if (rc !== 0) throw new Error(`copyInto failed (errno ${-rc})`);
  }

  /** Copy a sandbox file or directory tree out to the host. */
  copyOut(guestPath: string, hostPath: string): void {
    const rc = symbols.cvisor_sandbox_copy_out(this.ptr, cstr(guestPath), cstr(hostPath));
    if (rc !== 0) throw new Error(`copyOut failed (errno ${-rc})`);
  }

  /** Archive a sandbox directory to the cache under `key`. */
  cacheSave(sandboxPath: string, key: string, options: CacheOptions = {}): void {
    const rc = symbols.cvisor_cache_save(
      this.ptr, cstr(sandboxPath), cstr(key), cstr(options.backend ?? ""), cstr(options.format ?? "gzip"),
    );
    if (rc !== 0) throw new Error(`cacheSave failed (errno ${-rc})`);
  }

  /** Restore a cached archive (`key`, or its prefix) into a sandbox directory. */
  cacheRestore(sandboxPath: string, key: string, options: CacheOptions = {}): void {
    const rc = symbols.cvisor_cache_restore(
      this.ptr, cstr(sandboxPath), cstr(key), cstr(options.backend ?? ""), cstr(options.format ?? "gzip"),
    );
    if (rc !== 0) throw new Error(`cacheRestore failed (errno ${-rc})`);
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

  /** Run a command in the background, streaming output to the callbacks. */
  runStreaming(command: string, options: StreamOptions = {}): Promise<number> {
    const cmd = Buffer.from(command + "\0", "utf8");
    const sess = symbols.cvisor_session_start(this.ptr, ptr(cmd), 0);
    if (!sess) throw new Error("session start failed");
    return runStreamingImpl(bunSession(sess), options);
  }

  /** Start an interactive `/bin/sh` on a PTY. */
  shell(options: ShellOptions = {}): Shell {
    const sess = symbols.cvisor_session_start(this.ptr, null, 1);
    if (!sess) throw new Error("session start failed");
    return makeShell(bunSession(sess), options);
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
