export interface Output {
  stdoutStream: ReadableStream<Uint8Array>;
  stderrStream: ReadableStream<Uint8Array>;
  stdout: () => Promise<string>;
  stderr: () => Promise<string>;
  /** The guest's exit code (shell convention: status, or 128 + signal). */
  exitCode: number;
}

export function createOutput(
  stdoutStream: ReadableStream<Uint8Array>,
  stderrStream: ReadableStream<Uint8Array>,
  exitCode: number,
): Output {
  return {
    stdoutStream,
    stderrStream,
    stdout: () => new Response(stdoutStream).text(),
    stderr: () => new Response(stderrStream).text(),
    exitCode,
  };
}

/** Optional per-run controls shared by every SDK entry. */
export interface RunOptions {
  /** SIGKILL the guest after this many milliseconds (exit code 137). */
  timeoutMs?: number;
}

/** Wrap an already-materialized buffer as a single-chunk stream, so the FFI
 * backends (which run a command to completion) share the napi Output shape. */
export function bytesToStream(bytes: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      if (bytes.length > 0) {
        controller.enqueue(bytes);
      }
      controller.close();
    },
  });
}

/** Reconstruct the command string from a tagged template's parts and values. */
export function buildCommand(
  strings: TemplateStringsArray,
  values: unknown[],
): string {
  let command = strings[0];
  for (let i = 0; i < values.length; i++) {
    command += String(values[i]) + strings[i + 1];
  }
  return command;
}
