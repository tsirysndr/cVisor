import { External } from "./napi";
import { native } from "./native";
import { buildCommand, CacheOptions, createOutput, Output, RunOptions } from "./output";
import {
  makeShell,
  runStreaming as runStreamingImpl,
  SessionNative,
  Shell,
  ShellOptions,
  StreamOptions,
} from "./session";

export type { Output, RunOptions, CacheOptions } from "./output";
export type { Shell, ShellOptions, StreamOptions } from "./session";

/** Bind a napi session handle to the runtime-agnostic SessionNative shape. */
function napiSession(ptr: External<"Session">): SessionNative {
  return {
    readStdout: () => native.sessionReadStdout(ptr),
    readStderr: () => native.sessionReadStderr(ptr),
    writeStdin: (data) => native.sessionWriteStdin(ptr, data),
    resize: (rows, cols) => native.sessionResize(ptr, rows, cols),
    tryWait: () => native.sessionTryWait(ptr),
    kill: () => native.sessionKill(ptr),
    // napi has no explicit free; the handle's native Drop kills + joins on GC.
    // kill() stops a still-running guest deterministically.
    close: () => native.sessionKill(ptr),
  };
}

class Stream {
  private ptr: External<"Stream">;

  constructor(ptr: External<"Stream">) {
    this.ptr = ptr;
  }

  toReadableStream(): ReadableStream<Uint8Array> {
    const self = this;
    return new ReadableStream({
      async pull(controller) {
        // TODO: make streamNext return a promise
        const chunk = native.streamNext(self.ptr);
        if (chunk) {
          controller.enqueue(chunk);
        } else {
          controller.close();
        }
      },
    });
  }
}

export class Sandbox {
  private ptr: External<"Sandbox">;

  constructor() {
    this.ptr = native.createSandbox();
  }

  setLogLevel(level: "OFF" | "DEBUG") {
    native.sandboxSetLogLevel(this.ptr, level);
  }

  /** Enable or disable outbound INET/INET6 networking (default on). */
  setAllowNetwork(allow: boolean) {
    native.sandboxSetAllowNetwork(this.ptr, allow);
  }

  /** Enable or disable inbound TCP servers — bind fixed port, listen (default off). */
  setAllowListen(allow: boolean) {
    native.sandboxSetAllowListen(this.ptr, allow);
  }

  /** Write a file into the sandbox overlay (visible to later runs of this sandbox). */
  writeFile(path: string, data: Uint8Array | string) {
    native.sandboxWriteFile(this.ptr, path, typeof data === "string" ? new TextEncoder().encode(data) : data);
  }

  /** Read a file from the sandbox overlay (the guest's view). */
  readFile(path: string): Uint8Array {
    return native.sandboxReadFile(this.ptr, path);
  }

  /** Copy a host file or directory tree into the sandbox (recursive, ignore-aware). */
  copyInto(hostPath: string, guestPath: string) {
    native.sandboxCopyInto(this.ptr, hostPath, guestPath);
  }

  /** Copy a sandbox file or directory tree out to the host. */
  copyOut(guestPath: string, hostPath: string) {
    native.sandboxCopyOut(this.ptr, guestPath, hostPath);
  }

  /** Archive a sandbox directory to the cache under `key`. */
  cacheSave(sandboxPath: string, key: string, options: CacheOptions = {}) {
    native.cacheSave(this.ptr, sandboxPath, key, options.backend ?? "", options.format ?? "gzip");
  }

  /** Restore a cached archive (`key`, or its prefix) into a sandbox directory. */
  cacheRestore(sandboxPath: string, key: string, options: CacheOptions = {}) {
    native.cacheRestore(this.ptr, sandboxPath, key, options.backend ?? "", options.format ?? "gzip");
  }

  runCmd(command: string, options: RunOptions = {}): Output {
    const result = native.sandboxRunCmd(this.ptr, command, options.timeoutMs);
    return createOutput(
      new Stream(result.stdout).toReadableStream(),
      new Stream(result.stderr).toReadableStream(),
      result.exitCode,
    );
  }

  /**
   * Tagged-template command runner: `sb.sh\`ls -l ${dir}\``.
   * Equivalent to `sb.runCmd("ls -l <dir>")`.
   */
  sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
    return this.runCmd(buildCommand(strings, values));
  }

  /**
   * Run a command in the background, streaming its output to `onStdout` /
   * `onStderr` as it arrives. Resolves with the exit code.
   */
  runStreaming(command: string, options: StreamOptions = {}): Promise<number> {
    const s = native.sessionStart(this.ptr, command, false);
    return runStreamingImpl(napiSession(s), options);
  }

  /**
   * Start an interactive `/bin/sh` on a PTY. Feed it with `shell.write(...)`,
   * observe merged output via `onOutput`, and await `shell.wait()`.
   */
  shell(options: ShellOptions = {}): Shell {
    const s = native.sessionStart(this.ptr, undefined, true);
    return makeShell(napiSession(s), options);
  }
}

let defaultSandbox: Sandbox | undefined;

/**
 * Run a command in a shared, lazily-created sandbox via a tagged template:
 *
 *   import { sh } from "cvisor";
 *   const files = await sh`ls -l`.stdout();
 *
 * For an isolated sandbox, construct your own and use `sb.sh` / `sb.runCmd`.
 */
export function sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
  defaultSandbox ??= new Sandbox();
  return defaultSandbox.sh(strings, ...values);
}
