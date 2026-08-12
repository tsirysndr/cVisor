import { External } from "./napi";
import { native } from "./native";

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

/** Reconstruct the command string from a tagged template's parts and values. */
function buildCommand(strings: TemplateStringsArray, values: unknown[]): string {
  let command = strings[0];
  for (let i = 0; i < values.length; i++) {
    command += String(values[i]) + strings[i + 1];
  }
  return command;
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

export interface Output {
  stdoutStream: ReadableStream<Uint8Array>;
  stderrStream: ReadableStream<Uint8Array>;
  stdout: () => Promise<string>;
  stderr: () => Promise<string>;
}

function createOutput(
  stdoutStream: ReadableStream<Uint8Array>,
  stderrStream: ReadableStream<Uint8Array>,
): Output {
  return {
    stdoutStream,
    stderrStream,
    stdout: () => new Response(stdoutStream).text(),
    stderr: () => new Response(stderrStream).text(),
  };
}
