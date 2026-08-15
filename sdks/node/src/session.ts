// Shared session logic for all three runtimes. Each entry (napi/bun/deno)
// supplies a `SessionNative` that binds these calls to its native layer; the
// streaming/shell control flow (drain-poll loops) lives here so the three stay
// in sync.

/** The native session operations each runtime binds. */
export interface SessionNative {
  /** Drain newly-available stdout (merged terminal output for a PTY), or null. */
  readStdout(): Uint8Array | null;
  /** Drain newly-available stderr (empty for a PTY), or null. */
  readStderr(): Uint8Array | null;
  /** Write to the guest's stdin (PTY sessions only); returns bytes written. */
  writeStdin(data: Uint8Array): number;
  /** Set the PTY window size. */
  resize(rows: number, cols: number): void;
  /** The exit code once the guest has finished, else null. */
  tryWait(): number | null;
  /** SIGKILL the guest process group. */
  kill(): void;
  /** Release the session (frees native resources). */
  close(): void;
}

export interface StreamOptions {
  onStdout?: (chunk: string) => void;
  onStderr?: (chunk: string) => void;
  /** Drain interval in ms (default 15). */
  pollMs?: number;
}

export interface ShellOptions {
  onOutput?: (chunk: string) => void;
  pollMs?: number;
}

/** A running interactive PTY shell. */
export interface Shell {
  /** Feed the shell's stdin. */
  write(data: string | Uint8Array): void;
  /** Resize the PTY. */
  resize(rows: number, cols: number): void;
  /** The exit code once the shell has finished, else null. */
  exitCode(): number | null;
  /** Resolve with the exit code once the shell exits. */
  wait(): Promise<number>;
  /** SIGKILL the shell. */
  kill(): void;
  /** Release the session. */
  close(): void;
}

const decoder = new TextDecoder();
const encoder = new TextEncoder();
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Run a session to completion, streaming output to the callbacks. Resolves
 * with the exit code. Releases the session when done. */
export async function runStreaming(
  s: SessionNative,
  opts: StreamOptions = {},
): Promise<number> {
  const pollMs = opts.pollMs ?? 15;
  const drain = () => {
    if (opts.onStdout) {
      const o = s.readStdout();
      if (o) opts.onStdout(decoder.decode(o));
    }
    if (opts.onStderr) {
      const e = s.readStderr();
      if (e) opts.onStderr(decoder.decode(e));
    }
  };
  try {
    for (;;) {
      drain();
      const code = s.tryWait();
      if (code !== null) {
        drain(); // final bytes emitted after exit
        return code;
      }
      await sleep(pollMs);
    }
  } finally {
    s.close();
  }
}

/** Wrap a started PTY session as a `Shell`, pumping output to `onOutput`. */
export function makeShell(s: SessionNative, opts: ShellOptions = {}): Shell {
  const pollMs = opts.pollMs ?? 15;
  if (opts.onOutput) {
    const emit = () => {
      const o = s.readStdout();
      if (o) opts.onOutput!(decoder.decode(o));
    };
    void (async () => {
      for (;;) {
        emit();
        if (s.tryWait() !== null) {
          emit(); // final drain
          break;
        }
        await sleep(pollMs);
      }
    })();
  }
  return {
    write(data) {
      s.writeStdin(typeof data === "string" ? encoder.encode(data) : data);
    },
    resize(rows, cols) {
      s.resize(rows, cols);
    },
    exitCode() {
      return s.tryWait();
    },
    async wait() {
      for (;;) {
        const code = s.tryWait();
        if (code !== null) return code;
        await sleep(pollMs);
      }
    },
    kill() {
      s.kill();
    },
    close() {
      s.close();
    },
  };
}
