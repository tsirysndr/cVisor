import { atom } from "jotai";
import { atomWithStorage, createJSONStorage } from "jotai/utils";

// Runtime connection config. Resolution order:
//   1. window.__CVISOR_CONFIG__ injected by the CLI (wins, may prefill),
//   2. values persisted in localStorage (key `cvisor.config`),
//   3. otherwise null -> the app shows the first-run Setup screen.
// The config is a Jotai atom so the whole app re-renders when it changes.

export interface CvisorConfig {
  graphqlUrl: string;
  wsUrl: string;
  token: string;
}

export const CONFIG_STORAGE_KEY = "cvisor.config";

// Derive the ws endpoint from the http one: http->ws and /graphql -> /graphql/ws.
export function deriveWsUrl(graphqlUrl: string): string {
  const ws = graphqlUrl.replace(/^http/i, "ws");
  return /\/graphql(\/ws)?\/?$/.test(ws)
    ? ws.replace(/\/graphql(\/ws)?\/?$/, "/graphql/ws")
    : ws.replace(/\/?$/, "/graphql/ws");
}

// The desktop build talks gRPC directly to the daemon (default port 50051);
// the browser build talks GraphQL over HTTP/WS.
const desktop =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const CONFIG_DEFAULTS: CvisorConfig = desktop
  ? {
      graphqlUrl:
        import.meta.env.VITE_CVISOR_GRPC_URL || "http://localhost:50051",
      wsUrl: "",
      token: import.meta.env.VITE_CVISOR_TOKEN || "",
    }
  : {
      graphqlUrl:
        import.meta.env.VITE_CVISOR_GRAPHQL_URL ||
        "http://localhost:8080/graphql",
      wsUrl:
        import.meta.env.VITE_CVISOR_WS_URL || "ws://localhost:8080/graphql/ws",
      token: import.meta.env.VITE_CVISOR_TOKEN || "",
    };

declare global {
  interface Window {
    __CVISOR_CONFIG__?: Partial<CvisorConfig>;
  }
}

function injectedConfig(): CvisorConfig | null {
  const inj = (typeof window !== "undefined" && window.__CVISOR_CONFIG__) || {};
  if (!inj.graphqlUrl) return null;
  return {
    graphqlUrl: inj.graphqlUrl,
    wsUrl: inj.wsUrl || deriveWsUrl(inj.graphqlUrl),
    token: inj.token ?? "",
  };
}

const injected = injectedConfig();
export const isInjected = injected !== null;

const storage = createJSONStorage<CvisorConfig | null>(() => localStorage);
// getOnInit reads localStorage synchronously so we don't flash Setup on reload.
const storedConfigAtom = atomWithStorage<CvisorConfig | null>(
  CONFIG_STORAGE_KEY,
  null,
  storage,
  { getOnInit: true },
);

// Effective config: injected wins; else persisted; else null (show Setup).
export const configAtom = atom(
  (get) => injected ?? get(storedConfigAtom),
  (_get, set, next: CvisorConfig | null) => set(storedConfigAtom, next),
);

// Module-level mirror so the non-React graphql clients read live config.
let active: CvisorConfig = injected ?? CONFIG_DEFAULTS;
export function activeConfig(): CvisorConfig {
  return active;
}
export function setActiveConfig(c: CvisorConfig | null) {
  active = c ?? CONFIG_DEFAULTS;
}
export function authHeaders(): Record<string, string> {
  return active.token ? { Authorization: `Bearer ${active.token}` } : {};
}
