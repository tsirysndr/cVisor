import { External } from "./napi";
import { native } from "./native";
import { buildCommand, createOutput, Output } from "./output";

export type { Output } from "./output";

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

  runCmd(command: string): Output {
    const result = native.sandboxRunCmd(this.ptr, command);
    return createOutput(
      new Stream(result.stdout).toReadableStream(),
      new Stream(result.stderr).toReadableStream(),
    );
  }

  /**
   * Tagged-template command runner: `sb.sh\`ls -l ${dir}\``.
   * Equivalent to `sb.runCmd("ls -l <dir>")`.
   */
  sh(strings: TemplateStringsArray, ...values: unknown[]): Output {
    return this.runCmd(buildCommand(strings, values));
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
