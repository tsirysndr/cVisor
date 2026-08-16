import { GraphQLClient } from "graphql-request";
import { graphQLRequest } from "../graphql/client";
import { getWsClient, resetWsClient } from "../graphql/ws";
import {
  BRANCH,
  CACHE_CLEAR,
  CACHE_LIST,
  CACHE_REMOVE,
  CACHE_RESTORE,
  CACHE_SAVE,
  CONFIGURE,
  CREATE_SANDBOX,
  DELETE_SNAPSHOT,
  FORK,
  FREE_SANDBOX,
  HEALTH,
  KILL_SESSION,
  READ_FILE,
  RESIZE_SESSION,
  ROLLBACK,
  RUN,
  SANDBOXES,
  SESSION_OUTPUT,
  SNAPSHOT,
  SNAPSHOTS,
  START_SESSION,
  WRITE_FILE,
  WRITE_SESSION,
} from "../graphql/queries";
import { setActiveConfig, type CvisorConfig } from "../config";
import { b64ToBytes, bytesToB64 } from "../lib/base64";
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

// The browser transport: graphql-request for unary ops, graphql-ws for the
// terminal output subscription. This is the behavior served by `cvisor ui`.
export function webTransport(): Transport {
  return {
    setConfig(cfg) {
      setActiveConfig(cfg);
      resetWsClient();
    },

    async checkConnection(cfg: CvisorConfig): Promise<Health> {
      const client = new GraphQLClient(cfg.graphqlUrl, {
        headers: cfg.token ? { Authorization: `Bearer ${cfg.token}` } : {},
      });
      const d = await client.request<{ health: Health }>(HEALTH);
      if (!d.health?.ok) throw new Error("daemon reported not-ok health");
      return d.health;
    },

    async health() {
      return (await graphQLRequest<{ health: Health }>(HEALTH)).health;
    },

    async listSandboxes() {
      return (await graphQLRequest<{ sandboxes: Sandbox[] }>(SANDBOXES))
        .sandboxes;
    },

    async createSandbox(name) {
      return (
        await graphQLRequest<{ createSandbox: Sandbox }>(CREATE_SANDBOX, {
          name: name || null,
        })
      ).createSandbox;
    },

    async freeSandbox(id) {
      await graphQLRequest(FREE_SANDBOX, { id });
    },

    async configure(input: ConfigureInput) {
      return (
        await graphQLRequest<{ configure: Sandbox }>(CONFIGURE, {
          id: input.id,
          allowNetwork: input.allowNetwork ?? null,
          allowListen: input.allowListen ?? null,
        })
      ).configure;
    },

    async run(vars: RunVars) {
      return (await graphQLRequest<{ run: RunResult }>(RUN, vars)).run;
    },

    async snapshot(id, snapshotId) {
      return (
        await graphQLRequest<{ snapshot: string }>(SNAPSHOT, {
          id,
          snapshotId: snapshotId || null,
        })
      ).snapshot;
    },

    async rollback(id, snapshotId) {
      await graphQLRequest(ROLLBACK, { id, snapshotId });
    },

    async branch(snapshotId, name) {
      return (
        await graphQLRequest<{ branch: Sandbox }>(BRANCH, {
          snapshotId,
          name: name || null,
        })
      ).branch;
    },

    async fork(id, name) {
      return (
        await graphQLRequest<{ fork: Sandbox }>(FORK, {
          id,
          name: name || null,
        })
      ).fork;
    },

    async listSnapshots() {
      return (await graphQLRequest<{ snapshots: CacheEntry[] }>(SNAPSHOTS))
        .snapshots;
    },

    async deleteSnapshot(id) {
      await graphQLRequest(DELETE_SNAPSHOT, { id });
    },

    async readFile(id, path) {
      const d = await graphQLRequest<{ readFile: string }>(READ_FILE, {
        id,
        path,
      });
      return b64ToBytes(d.readFile);
    },

    async writeFile(id, path, data) {
      await graphQLRequest(WRITE_FILE, {
        id,
        path,
        dataBase64: bytesToB64(data),
      });
    },

    async cacheList(backend) {
      return (
        await graphQLRequest<{ cacheList: CacheEntry[] }>(CACHE_LIST, {
          backend: backend || null,
        })
      ).cacheList;
    },

    async cacheSave(id, path, key, backend, format) {
      await graphQLRequest(CACHE_SAVE, {
        id,
        path,
        key,
        backend: backend || null,
        format: format || null,
      });
    },

    async cacheRestore(id, path, key, backend, format) {
      await graphQLRequest(CACHE_RESTORE, {
        id,
        path,
        key,
        backend: backend || null,
        format: format || null,
      });
    },

    async cacheRemove(key, backend, format) {
      return (
        await graphQLRequest<{ cacheRemove: boolean }>(CACHE_REMOVE, {
          key,
          backend: backend || null,
          format: format || null,
        })
      ).cacheRemove;
    },

    async cacheClear(backend) {
      return (
        await graphQLRequest<{ cacheClear: number }>(CACHE_CLEAR, {
          backend: backend || null,
        })
      ).cacheClear;
    },

    async openTerminal(sandboxId, onOutput, onExit): Promise<TerminalSession> {
      const d = await graphQLRequest<{ startSession: { id: string } }>(
        START_SESSION,
        { id: sandboxId, command: "", pty: true },
      );
      const sessionId = d.startSession.id;

      const unsubscribe = getWsClient().subscribe<{ sessionOutput: string }>(
        { query: SESSION_OUTPUT, variables: { id: sessionId } },
        {
          next: ({ data }) => {
            const chunk = data?.sessionOutput;
            if (chunk) onOutput(b64ToBytes(chunk));
          },
          error: () => onExit?.(-1),
          complete: () => onExit?.(0),
        },
      );

      return {
        write: (data) => {
          void graphQLRequest(WRITE_SESSION, {
            id: sessionId,
            dataBase64: bytesToB64(data),
          });
        },
        resize: (rows, cols) => {
          void graphQLRequest(RESIZE_SESSION, { id: sessionId, rows, cols });
        },
        close: () => {
          unsubscribe();
          void graphQLRequest(KILL_SESSION, { id: sessionId });
        },
      };
    },
  };
}
