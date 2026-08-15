import { arch, platform } from "os";
import { familySync, MUSL } from "detect-libc";
import { External } from "./napi";

if (platform() !== "linux") {
  throw new Error("cVisor only supports Linux");
}

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
  sandboxWriteFile(
    sandbox: External<"Sandbox">,
    path: string,
    data: Uint8Array,
  ): void;
  sandboxReadFile(
    sandbox: External<"Sandbox">,
    path: string,
  ): Uint8Array;
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

const libc = familySync() === MUSL ? "musl" : "gnu";
export const native: NativeModule = require(`@cvisor/linux-${arch()}-${libc}`);
