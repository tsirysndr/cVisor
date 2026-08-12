// cVisor Deno SDK — Deno FFI (Deno.dlopen) wrapper over the libcvisor C ABI.
//
//   import { Sandbox } from "./mod.ts";
//   const out = new Sandbox().run("echo hello");
//   console.log(out.stdout); // "hello\n"
//
// Run with: deno run --allow-ffi --allow-env --unstable-ffi mod.ts

function libraryPath(): string {
  const override = Deno.env.get("CVISOR_LIB");
  if (override) return override;
  const a = Deno.build.arch === "aarch64" ? "aarch64" : "x86_64";
  return new URL(`./native/libcvisor-${a}.so`, import.meta.url).pathname;
}

const lib = Deno.dlopen(libraryPath(), {
  cvisor_sandbox_new: { parameters: [], result: "pointer" },
  cvisor_sandbox_free: { parameters: ["pointer"], result: "void" },
  cvisor_sandbox_set_log_level: {
    parameters: ["pointer", "i32"],
    result: "void",
  },
  cvisor_run: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_output_free: { parameters: ["pointer"], result: "void" },
  cvisor_output_stdout: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_output_stderr: { parameters: ["pointer", "buffer"], result: "pointer" },
  cvisor_bytes_free: { parameters: ["pointer", "usize"], result: "void" },
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
  lib.symbols.cvisor_bytes_free(p, BigInt(n));
  return copy;
}

export class Sandbox {
  #ptr: Deno.PointerValue;

  constructor() {
    this.#ptr = lib.symbols.cvisor_sandbox_new();
    if (this.#ptr === null) throw new Error("failed to create sandbox");
  }

  setLogLevel(level: "OFF" | "DEBUG"): void {
    lib.symbols.cvisor_sandbox_set_log_level(this.#ptr, level === "DEBUG" ? 1 : 0);
  }

  run(command: string): Output {
    const cmd = new TextEncoder().encode(command + "\0");
    const out = lib.symbols.cvisor_run(this.#ptr, cmd);
    if (out === null) throw new Error("sandbox run failed");
    try {
      const stdoutBytes = readOutput(out, lib.symbols.cvisor_output_stdout);
      const stderrBytes = readOutput(out, lib.symbols.cvisor_output_stderr);
      const dec = new TextDecoder();
      return {
        stdoutBytes,
        stderrBytes,
        stdout: dec.decode(stdoutBytes),
        stderr: dec.decode(stderrBytes),
      };
    } finally {
      lib.symbols.cvisor_output_free(out);
    }
  }

  close(): void {
    if (this.#ptr !== null) {
      lib.symbols.cvisor_sandbox_free(this.#ptr);
      this.#ptr = null;
    }
  }
}
