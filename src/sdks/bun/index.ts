// cVisor Bun SDK — Bun FFI (bun:ffi) wrapper over the libcvisor C ABI.
//
//   import { Sandbox } from "./index";
//   const out = new Sandbox().run("echo hello");
//   console.log(out.stdout); // "hello\n"

import { dlopen, FFIType, ptr, CString, toArrayBuffer } from "bun:ffi";
import { arch } from "os";

function libraryPath(): string {
  const override = process.env.CVISOR_LIB;
  if (override) return override;
  const a = arch() === "arm64" ? "aarch64" : "x86_64";
  return new URL(`./native/libcvisor-${a}.so`, import.meta.url).pathname;
}

const { symbols } = dlopen(libraryPath(), {
  cvisor_sandbox_new: { args: [], returns: FFIType.ptr },
  cvisor_sandbox_free: { args: [FFIType.ptr], returns: FFIType.void },
  cvisor_sandbox_set_log_level: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.void,
  },
  cvisor_run: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  cvisor_output_free: { args: [FFIType.ptr], returns: FFIType.void },
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

export interface Output {
  stdout: string;
  stderr: string;
  stdoutBytes: Uint8Array;
  stderrBytes: Uint8Array;
}

function readOutput(out: number, accessor: (o: number, lenPtr: number) => number): Uint8Array {
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
  private ptr: number;

  constructor() {
    this.ptr = symbols.cvisor_sandbox_new() as number;
    if (!this.ptr) throw new Error("failed to create sandbox");
  }

  setLogLevel(level: "OFF" | "DEBUG"): void {
    symbols.cvisor_sandbox_set_log_level(this.ptr, level === "DEBUG" ? 1 : 0);
  }

  run(command: string): Output {
    const cmd = Buffer.from(command + "\0", "utf8");
    const out = symbols.cvisor_run(this.ptr, ptr(cmd)) as number;
    if (!out) throw new Error("sandbox run failed");
    try {
      const stdoutBytes = readOutput(out, symbols.cvisor_output_stdout as any);
      const stderrBytes = readOutput(out, symbols.cvisor_output_stderr as any);
      const dec = new TextDecoder();
      return {
        stdoutBytes,
        stderrBytes,
        stdout: dec.decode(stdoutBytes),
        stderr: dec.decode(stderrBytes),
      };
    } finally {
      symbols.cvisor_output_free(out);
    }
  }

  close(): void {
    if (this.ptr) {
      symbols.cvisor_sandbox_free(this.ptr);
      this.ptr = 0;
    }
  }
}
