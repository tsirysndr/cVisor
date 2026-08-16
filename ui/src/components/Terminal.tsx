import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { getTransport, type TerminalSession } from "../transport";

// Bound to a sandbox: open an interactive PTY session through the active
// transport, stream its output into xterm, and pipe keystrokes/resizes back.
export function Terminal({ sandboxId }: { sandboxId: string }) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new XTerm({
      convertEol: true,
      fontSize: 13,
      fontFamily: "'Agave Nerd Font', 'Agave', ui-monospace, monospace",
      cursorBlink: true,
      theme: {
        background: "#0D0221",
        foreground: "#F5F5FF",
        cursor: "#FF2A6D",
        cursorAccent: "#0D0221",
        selectionBackground: "#B026FF66",
      },
    });
    const fit = new FitAddon();
    // Load the addon before open() so it can attach to the terminal element.
    term.loadAddon(fit);
    term.open(container);

    let session: TerminalSession | null = null;
    let disposed = false;

    // Only fit once the terminal is attached AND the container has real layout;
    // fitting a 0-size element throws inside FitAddon (reads `.dimensions`).
    const doFit = () => {
      if (!term.element) return;
      if (container.clientWidth <= 0 || container.clientHeight <= 0) return;
      try {
        fit.fit();
        session?.resize(term.rows, term.cols);
      } catch {
        // Layout not ready yet; a later ResizeObserver tick will retry.
      }
    };

    let raf = requestAnimationFrame(doFit);

    // Keystrokes -> session stdin (buffered until the session is ready).
    term.onData((data) => session?.write(new TextEncoder().encode(data)));

    getTransport()
      .openTerminal(sandboxId, (bytes) => term.write(bytes))
      .then((s) => {
        if (disposed) {
          s.close();
          return;
        }
        session = s;
        raf = requestAnimationFrame(doFit);
      })
      .catch((e) => {
        term.write(`\r\n\x1b[31m[session error] ${String(e)}\x1b[0m\r\n`);
      });

    let debounce: number | undefined;
    const ro = new ResizeObserver(() => {
      if (debounce) window.clearTimeout(debounce);
      debounce = window.setTimeout(() => {
        raf = requestAnimationFrame(doFit);
      }, 80);
    });
    ro.observe(container);

    return () => {
      disposed = true;
      cancelAnimationFrame(raf);
      if (debounce) window.clearTimeout(debounce);
      ro.disconnect();
      session?.close();
      term.dispose();
    };
  }, [sandboxId]);

  return <div ref={containerRef} className="h-full w-full bg-background p-2" />;
}
