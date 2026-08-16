import type { Transport } from "./types";
import { webTransport } from "./web";
import { tauriTransport } from "./tauri";

export type {
  Sandbox,
  Health,
  CacheEntry,
  RunResult,
  RunVars,
  ConfigureInput,
  TerminalSession,
  Transport,
} from "./types";

// True when running inside the Tauri desktop shell. Tauri injects
// `__TAURI_INTERNALS__` on the window before any app code runs.
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// macOS gets native window buttons (overlay title bar); other platforms render
// the custom TitleBar with its own controls.
export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);
}

let impl: Transport | null = null;

// The active transport, chosen once at runtime: gRPC-over-Tauri on the desktop,
// GraphQL in the browser. Every hook/component talks to this, never graphql
// directly.
export function getTransport(): Transport {
  if (!impl) impl = isTauri() ? tauriTransport() : webTransport();
  return impl;
}
