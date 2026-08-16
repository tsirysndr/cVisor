import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { b64ToBytes, bytesToB64 } from "../lib/base64";
import type { CvisorConfig } from "../config";
import type {
  CacheEntry,
  ConfigureInput,
  Health,
  RunResult,
  RunVars,
  Sandbox,
  TerminalSession,
  Transport,
} from "./types";

// In the desktop build the config's `graphqlUrl` field carries the daemon's
// gRPC address (the onboarding form is relabelled accordingly). The Rust side
// prefixes `http://` when no scheme is present.
function daemonUrl(cfg: CvisorConfig): string {
  return cfg.graphqlUrl.trim();
}

// The desktop transport: every unary op is a Tauri command over gRPC, and the
// terminal is driven by the `shell_*` commands plus `shell-output`/`shell-exit`
// events. No GraphQL is involved.
export function tauriTransport(): Transport {
  return {
    async setConfig(cfg) {
      await invoke("set_config", { url: daemonUrl(cfg), token: cfg.token });
    },

    async checkConnection(cfg: CvisorConfig): Promise<Health> {
      await invoke("set_config", { url: daemonUrl(cfg), token: cfg.token });
      const health = await invoke<Health>("health");
      if (!health?.ok) throw new Error("daemon reported not-ok health");
      return health;
    },

    health: () => invoke<Health>("health"),

    listSandboxes: () => invoke<Sandbox[]>("list_sandboxes"),

    createSandbox: (name) =>
      invoke<Sandbox>("create_sandbox", { name: name ?? null }),

    freeSandbox: async (id) => {
      await invoke("free_sandbox", { id });
    },

    configure: (input: ConfigureInput) =>
      invoke<Sandbox>("configure", {
        id: input.id,
        allowNetwork: input.allowNetwork ?? null,
        allowListen: input.allowListen ?? null,
        limits: input.limits ?? null,
      }),

    run: (vars: RunVars) =>
      invoke<RunResult>("run", {
        id: vars.id ?? null,
        command: vars.command,
        timeoutMs: vars.timeoutMs ?? 0,
      }),

    snapshot: (id, snapshotId) =>
      invoke<string>("snapshot", { id, snapshotId: snapshotId ?? null }),

    rollback: async (id, snapshotId) => {
      await invoke("rollback", { id, snapshotId });
    },

    branch: (snapshotId, name) =>
      invoke<Sandbox>("branch", { snapshotId, name: name ?? null }),

    fork: (id, name) => invoke<Sandbox>("fork", { id, name: name ?? null }),

    listSnapshots: () => invoke<CacheEntry[]>("list_snapshots"),

    deleteSnapshot: async (id) => {
      await invoke("delete_snapshot", { id });
    },

    readFile: async (id, path) =>
      b64ToBytes(await invoke<string>("read_file", { id, path })),

    writeFile: async (id, path, data) => {
      await invoke("write_file", { id, path, dataBase64: bytesToB64(data) });
    },

    cacheList: (backend) =>
      invoke<CacheEntry[]>("cache_list", { backend: backend ?? null }),

    cacheSave: async (id, path, key, backend, format) => {
      await invoke("cache_save", {
        id,
        path,
        key,
        backend: backend ?? null,
        format: format ?? null,
      });
    },

    cacheRestore: async (id, path, key, backend, format) => {
      await invoke("cache_restore", {
        id,
        path,
        key,
        backend: backend ?? null,
        format: format ?? null,
      });
    },

    cacheRemove: (key, backend, format) =>
      invoke<boolean>("cache_remove", {
        key,
        backend: backend ?? null,
        format: format ?? null,
      }),

    cacheClear: (backend) =>
      invoke<number>("cache_clear", { backend: backend ?? null }),

    async openTerminal(sandboxId, onOutput, onExit): Promise<TerminalSession> {
      // Attach the listeners before shell_open: the daemon starts streaming as
      // soon as the command runs, and events emitted while the invoke promise
      // resolves (typically the shell prompt) would otherwise be lost. Until
      // the session id is known, events are buffered and replayed after.
      let sessionId: string | null = null;
      const pendingOutput: { sessionId: string; base64: string }[] = [];
      const pendingExit: { sessionId: string; code: number }[] = [];

      const unlisten: UnlistenFn[] = [];
      unlisten.push(
        await listen<{ sessionId: string; base64: string }>(
          "shell-output",
          (e) => {
            if (sessionId === null) pendingOutput.push(e.payload);
            else if (e.payload.sessionId === sessionId)
              onOutput(b64ToBytes(e.payload.base64));
          },
        ),
      );
      unlisten.push(
        await listen<{ sessionId: string; code: number }>(
          "shell-exit",
          (e) => {
            if (sessionId === null) pendingExit.push(e.payload);
            else if (e.payload.sessionId === sessionId) onExit?.(e.payload.code);
          },
        ),
      );

      try {
        sessionId = await invoke<string>("shell_open", { sandboxId });
      } catch (e) {
        unlisten.forEach((fn) => fn());
        throw e;
      }
      for (const p of pendingOutput)
        if (p.sessionId === sessionId) onOutput(b64ToBytes(p.base64));
      for (const p of pendingExit)
        if (p.sessionId === sessionId) onExit?.(p.code);
      pendingOutput.length = 0;
      pendingExit.length = 0;

      return {
        write: (data) => {
          void invoke("shell_write", { sessionId, base64: bytesToB64(data) });
        },
        resize: (rows, cols) => {
          void invoke("shell_resize", { sessionId, rows, cols });
        },
        close: () => {
          unlisten.forEach((fn) => fn());
          void invoke("shell_close", { sessionId });
        },
      };
    },
  };
}
