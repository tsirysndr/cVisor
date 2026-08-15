import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { getWsClient } from "../graphql/ws";
import { SESSION_OUTPUT } from "../graphql/queries";
import {
  killSession,
  resizeSession,
  startSession,
  writeSession,
} from "../hooks/useSession";
import { b64ToBytes, strToB64 } from "../lib/base64";

// Bound to a sandbox: start a PTY session, stream sessionOutput -> xterm, and
// pipe keystrokes/resizes back through the session mutations.
export function Terminal({ sandboxId }: { sandboxId: string }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const term = new XTerm({
      convertEol: true,
      fontSize: 13,
      fontFamily: "'Agave Nerd Font', 'Agave', ui-monospace, monospace",
      cursorBlink: true,
      theme: { background: "#171717", foreground: "#FAFAFA" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();

    let sessionId: string | null = null;
    let unsubscribe: (() => void) | null = null;
    let disposed = false;

    const start = async () => {
      sessionId = await startSession(sandboxId, "", true);
      if (disposed) {
        await killSession(sessionId);
        return;
      }
      await resizeSession(sessionId, term.rows, term.cols);

      unsubscribe = getWsClient().subscribe<{ sessionOutput: string }>(
        { query: SESSION_OUTPUT, variables: { id: sessionId } },
        {
          next: ({ data }) => {
            const chunk = data?.sessionOutput;
            if (chunk) term.write(b64ToBytes(chunk));
          },
          error: (err) =>
            term.write(`\r\n\x1b[31m[stream error] ${String(err)}\x1b[0m\r\n`),
          complete: () => {},
        },
      );

      term.onData((data) => {
        if (sessionId) void writeSession(sessionId, strToB64(data));
      });
    };

    void start();

    const onResize = () => {
      fit.fit();
      if (sessionId) void resizeSession(sessionId, term.rows, term.cols);
    };
    window.addEventListener("resize", onResize);
    const ro = new ResizeObserver(onResize);
    ro.observe(el);

    return () => {
      disposed = true;
      window.removeEventListener("resize", onResize);
      ro.disconnect();
      unsubscribe?.();
      if (sessionId) void killSession(sessionId);
      term.dispose();
    };
  }, [sandboxId]);

  return <div ref={ref} className="h-full w-full bg-background p-2" />;
}
