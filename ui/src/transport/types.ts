import type { CvisorConfig } from "../config";

// Data shapes shared by every transport implementation. Binary payloads cross
// the transport boundary as `Uint8Array`; each implementation handles its own
// base64 (de)coding internally.

export interface Sandbox {
  id: string;
  name: string;
  allowNetwork: boolean;
  allowListen: boolean;
}

export interface Health {
  version: string;
  ok: boolean;
}

export interface CacheEntry {
  name: string;
  size: number;
}

export interface RunResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

export interface RunVars {
  id?: string | null;
  command: string;
  timeoutMs?: number;
}

export interface ConfigureInput {
  id: string;
  allowNetwork?: boolean;
  allowListen?: boolean;
}

// An open interactive terminal. Output is delivered to the callback passed to
// `openTerminal`; the returned handle drives input/resize and teardown.
export interface TerminalSession {
  write(data: Uint8Array): void;
  resize(rows: number, cols: number): void;
  close(): void;
}

// The full operation surface the app uses, independent of GraphQL vs gRPC.
export interface Transport {
  // Push the active connection config to the backend. For the web transport
  // this updates the module-level mirror the graphql clients read; for the
  // desktop transport it forwards to the Tauri `set_config` command.
  setConfig(cfg: CvisorConfig): void | Promise<void>;
  // Validate a candidate config (used by the onboarding/settings form).
  checkConnection(cfg: CvisorConfig): Promise<Health>;

  health(): Promise<Health>;
  listSandboxes(): Promise<Sandbox[]>;
  createSandbox(name?: string | null): Promise<Sandbox>;
  freeSandbox(id: string): Promise<void>;
  configure(input: ConfigureInput): Promise<Sandbox>;
  run(vars: RunVars): Promise<RunResult>;

  snapshot(id: string, snapshotId?: string): Promise<string>;
  rollback(id: string, snapshotId: string): Promise<void>;
  branch(snapshotId: string, name?: string): Promise<Sandbox>;
  fork(id: string, name?: string): Promise<Sandbox>;
  listSnapshots(): Promise<CacheEntry[]>;
  deleteSnapshot(id: string): Promise<void>;

  readFile(id: string, path: string): Promise<Uint8Array>;
  writeFile(id: string, path: string, data: Uint8Array): Promise<void>;

  cacheList(backend?: string): Promise<CacheEntry[]>;
  cacheSave(
    id: string,
    path: string,
    key: string,
    backend?: string,
    format?: string,
  ): Promise<void>;
  cacheRestore(
    id: string,
    path: string,
    key: string,
    backend?: string,
    format?: string,
  ): Promise<void>;
  cacheRemove(key: string, backend?: string, format?: string): Promise<boolean>;
  cacheClear(backend?: string): Promise<number>;

  openTerminal(
    sandboxId: string,
    onOutput: (data: Uint8Array) => void,
    onExit?: (code: number) => void,
  ): Promise<TerminalSession>;
}
