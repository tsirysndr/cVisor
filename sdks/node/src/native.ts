import { arch, platform } from "os";
import { familySync, MUSL } from "detect-libc";
import { External } from "./napi";

/** FFI contract: typed interface for the native module loaded via require(). */
export interface NativeModule {
  createSandbox(): External<"Sandbox">;
  sandboxSetLogLevel(
    sandbox: External<"Sandbox">,
    level: "OFF" | "DEBUG",
  ): void;
  sandboxSetAllowNetwork(
    sandbox: External<"Sandbox">,
    allow: boolean,
  ): void;
  sandboxSetAllowListen(
    sandbox: External<"Sandbox">,
    allow: boolean,
  ): void;
  sandboxSetEnv(
    sandbox: External<"Sandbox">,
    key: string,
    value: string,
  ): void;
  sandboxSetLimits(
    sandbox: External<"Sandbox">,
    memoryMax: number,
    pidsMax: number,
    cpuPercent: number,
  ): void;
  sandboxWriteFile(
    sandbox: External<"Sandbox">,
    path: string,
    data: Uint8Array,
  ): void;
  sandboxReadFile(
    sandbox: External<"Sandbox">,
    path: string,
  ): Uint8Array;
  sandboxCopyInto(
    sandbox: External<"Sandbox">,
    hostPath: string,
    guestPath: string,
  ): void;
  sandboxCopyOut(
    sandbox: External<"Sandbox">,
    guestPath: string,
    hostPath: string,
  ): void;
  cacheSave(
    sandbox: External<"Sandbox">,
    sandboxPath: string,
    key: string,
    backend: string,
    format: string,
  ): void;
  cacheRestore(
    sandbox: External<"Sandbox">,
    sandboxPath: string,
    key: string,
    backend: string,
    format: string,
  ): void;
  sandboxRunCmd(
    sandbox: External<"Sandbox">,
    command: string,
    timeoutMs?: number,
  ): {
    stdout: External<"Stream">;
    stderr: External<"Stream">;
    exitCode: number;
  };
  streamNext(stream: External<"Stream">): Uint8Array | null;
  sessionStart(
    sandbox: External<"Sandbox">,
    cmd: string | undefined,
    pty: boolean,
  ): External<"Session">;
  sessionReadStdout(session: External<"Session">): Uint8Array | null;
  sessionReadStderr(session: External<"Session">): Uint8Array | null;
  sessionWriteStdin(session: External<"Session">, data: Uint8Array): number;
  sessionResize(session: External<"Session">, rows: number, cols: number): void;
  sessionTryWait(session: External<"Session">): number | null;
  sessionKill(session: External<"Session">): void;
}

// The native napi module (libcvisor.node) is Linux-only, so loading it is
// deferred until the FFI `Sandbox` is actually used. Merely importing the
// package — e.g. to use the pure-fetch GraphQLClient / RemoteSandbox — must not
// require it, so it works on macOS. `native` is a lazy proxy that loads (and
// enforces the Linux-only check) on first access.
function loadNative(): NativeModule {
  if (platform() !== "linux") {
    throw new Error(
      "the local FFI sandbox is Linux-only; use the GraphQL client (GraphQLClient / RemoteSandbox) on this platform",
    );
  }
  const libc = familySync() === MUSL ? "musl" : "gnu";
  return require(`@cvisor/linux-${arch()}-${libc}`);
}

let loadedNative: NativeModule | undefined;
export const native: NativeModule = new Proxy({} as NativeModule, {
  get(_target, prop) {
    loadedNative ??= loadNative();
    return (loadedNative as unknown as Record<PropertyKey, unknown>)[prop];
  },
});
