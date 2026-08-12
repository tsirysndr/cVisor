// bVisor Deno SDK — Deno FFI (Deno.dlopen) wrapper over the libbvisor C ABI.
//
//   import { Sandbox } from "./mod.ts";
//   const out = new Sandbox().run("echo hello");
//   console.log(out.stdout); // "hello\n"
//
// Run with: deno run --allow-ffi --allow-env --unstable-ffi mod.ts

function libraryPath(): string {
  const override = Deno.env.get("BVISOR_LIB");
  if (override) return override;
  const a = Deno.build.arch === "aarch64" ? "aarch64" : "x86_64";
  return new URL(`./native/libbvisor-${a}.so`, import.meta.url).pathname;
}

const lib = Deno.dlopen(libraryPath(), {
  bvisor_sandbox_new: { parameters: [], result: "pointer" },
  bvisor_sandbox_free: { parameters: ["pointer"], result: "void" },
  bvisor_sandbox_set_log_level: {
    parameters: ["pointer", "i32"],
    result: "void",
  },
  bvisor_run: { parameters: ["pointer", "buffer"], result: "pointer" },
  bvisor_output_free: { parameters: ["pointer"], result: "void" },
  bvisor_output_stdout: { parameters: ["pointer", "buffer"], result: "pointer" },
  bvisor_output_stderr: { parameters: ["pointer", "buffer"], result: "pointer" },
  bvisor_bytes_free: { parameters: ["pointer", "usize"], result: "void" },
});

export interface Output {
  stdout: string;
  stderr: string;
  stdoutBytes: Uint8Array;
  stderrBytes: Uint8Array;
}

type Accessor = (o: Deno.PointerValue, lenBuf: Uint8Array) => Deno.PointerValue;

function readOutput(out: Deno.PointerValue, accessor: Accessor): Uint8Array {
  const lenBuf = new Uint8Array(8);
  const p = accessor(out, lenBuf);
  const n = Number(new DataView(lenBuf.buffer).getBigUint64(0, true));
  if (p === null || n === 0) return new Uint8Array(0);
  const view = new Deno.UnsafePointerView(p);
  const copy = new Uint8Array(n);
  view.copyInto(copy);
  lib.symbols.bvisor_bytes_free(p, BigInt(n));
  return copy;
}

export class Sandbox {
  #ptr: Deno.PointerValue;

  constructor() {
    this.#ptr = lib.symbols.bvisor_sandbox_new();
    if (this.#ptr === null) throw new Error("failed to create sandbox");
  }

  setLogLevel(level: "OFF" | "DEBUG"): void {
    lib.symbols.bvisor_sandbox_set_log_level(this.#ptr, level === "DEBUG" ? 1 : 0);
  }

  run(command: string): Output {
    const cmd = new TextEncoder().encode(command + "\0");
    const out = lib.symbols.bvisor_run(this.#ptr, cmd);
    if (out === null) throw new Error("sandbox run failed");
    try {
      const stdoutBytes = readOutput(out, lib.symbols.bvisor_output_stdout);
      const stderrBytes = readOutput(out, lib.symbols.bvisor_output_stderr);
      const dec = new TextDecoder();
      return {
        stdoutBytes,
        stderrBytes,
        stdout: dec.decode(stdoutBytes),
        stderr: dec.decode(stderrBytes),
      };
    } finally {
      lib.symbols.bvisor_output_free(out);
    }
  }

  close(): void {
    if (this.#ptr !== null) {
      lib.symbols.bvisor_sandbox_free(this.#ptr);
      this.#ptr = null;
    }
  }
}
