// A GraphQL-backed high-level API mirroring the FFI `Sandbox`, but driven over
// HTTP against a running cVisor daemon. Pure `fetch` + `JSON` (via
// GraphQLClient), so it works on any platform (macOS included) with no native
// library.

import { GraphQLClient } from "./graphql";

/** A daemon-side sandbox's identity and configuration. */
export interface RemoteSandboxInfo {
  id: string;
  name: string;
  allowNetwork: boolean;
  allowListen: boolean;
  env: Array<{ key: string; value: string }>;
  limits: { memoryMax: number | null; pidsMax: number | null; cpuPercent: number | null };
}

/** The captured result of a remote `run`. */
export interface RemoteRunResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

/** Resource-limit overrides. */
export interface RemoteLimits {
  memoryMax?: number;
  pidsMax?: number;
  cpuPercent?: number;
}

/** Options for `configure`. */
export interface ConfigureOptions {
  allowNetwork?: boolean;
  allowListen?: boolean;
  limits?: RemoteLimits;
  env?: Array<{ key: string; value: string }>;
}

const SANDBOX_FIELDS = `{ id name allowNetwork allowListen env { key value } limits { memoryMax pidsMax cpuPercent } }`;

function b64encode(data: Uint8Array | string): string {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

function b64decode(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/**
 * A remote sandbox controller backed by a daemon's GraphQL API.
 *
 *   const remote = new RemoteSandbox("http://127.0.0.1:8080/graphql", token);
 *   const out = await remote.run("echo hello");   // { stdout: "hello\n", ... }
 */
export class RemoteSandbox {
  private readonly client: GraphQLClient;

  constructor(url: string, token: string) {
    this.client = new GraphQLClient(url, token);
  }

  /** Daemon liveness and build info. */
  async health(): Promise<{ version: string; ok: boolean }> {
    const data = await this.client.query<{ health: { version: string; ok: boolean } }>(
      `{ health { version ok } }`,
    );
    return data.health;
  }

  /** Run a command in a fresh ephemeral sandbox; returns decoded output. */
  async run(command: string, timeoutMs?: number): Promise<RemoteRunResult> {
    const data = await this.client.mutate<{ run: RemoteRunResult }>(
      `mutation($command: String!, $timeoutMs: Int) {
         run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
       }`,
      { command, timeoutMs: timeoutMs ?? 0 },
    );
    return data.run;
  }

  /** Create a sandbox; an empty/undefined name gets a random one. */
  async createSandbox(name?: string): Promise<RemoteSandboxInfo> {
    const data = await this.client.mutate<{ createSandbox: RemoteSandboxInfo }>(
      `mutation($name: String) { createSandbox(name: $name) ${SANDBOX_FIELDS} }`,
      { name: name ?? null },
    );
    return data.createSandbox;
  }

  /** List all live sandboxes. */
  async listSandboxes(): Promise<RemoteSandboxInfo[]> {
    const data = await this.client.query<{ sandboxes: RemoteSandboxInfo[] }>(
      `{ sandboxes ${SANDBOX_FIELDS} }`,
    );
    return data.sandboxes;
  }

  /** Free a sandbox and clean up its overlay. */
  async freeSandbox(id: string): Promise<boolean> {
    const data = await this.client.mutate<{ freeSandbox: boolean }>(
      `mutation($id: String!) { freeSandbox(id: $id) }`,
      { id },
    );
    return data.freeSandbox;
  }

  /** Update a sandbox's network/listen flags, limits, and env. */
  async configure(id: string, options: ConfigureOptions = {}): Promise<RemoteSandboxInfo> {
    const data = await this.client.mutate<{ configure: RemoteSandboxInfo }>(
      `mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean, $limits: LimitsInput, $env: [EnvVarInput!]) {
         configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen, limits: $limits, env: $env) ${SANDBOX_FIELDS}
       }`,
      {
        id,
        allowNetwork: options.allowNetwork ?? null,
        allowListen: options.allowListen ?? null,
        limits: options.limits ?? null,
        env: options.env ?? null,
      },
    );
    return data.configure;
  }

  /** Write bytes (or a string) to a file inside a sandbox (base64 over the wire). */
  async writeFile(id: string, path: string, data: Uint8Array | string): Promise<boolean> {
    const res = await this.client.mutate<{ writeFile: boolean }>(
      `mutation($id: String!, $path: String!, $data: String!) {
         writeFile(id: $id, path: $path, dataBase64: $data)
       }`,
      { id, path, data: b64encode(data) },
    );
    return res.writeFile;
  }

  /** Read a file from a sandbox (base64-decoded from the wire). */
  async readFile(id: string, path: string): Promise<Uint8Array> {
    const data = await this.client.query<{ readFile: string }>(
      `query($id: String!, $path: String!) { readFile(id: $id, path: $path) }`,
      { id, path },
    );
    return b64decode(data.readFile);
  }

  /** Copy a host file into a sandbox. */
  async copyInto(id: string, hostPath: string, guestPath: string): Promise<boolean> {
    const data = await this.client.mutate<{ copyInto: boolean }>(
      `mutation($id: String!, $hostPath: String!, $guestPath: String!) {
         copyInto(id: $id, hostPath: $hostPath, guestPath: $guestPath)
       }`,
      { id, hostPath, guestPath },
    );
    return data.copyInto;
  }

  /** Copy a file out of a sandbox to the host. */
  async copyOut(id: string, guestPath: string, hostPath: string): Promise<boolean> {
    const data = await this.client.mutate<{ copyOut: boolean }>(
      `mutation($id: String!, $guestPath: String!, $hostPath: String!) {
         copyOut(id: $id, guestPath: $guestPath, hostPath: $hostPath)
       }`,
      { id, guestPath, hostPath },
    );
    return data.copyOut;
  }

  /** Archive a sandbox path into the cache under `key`. */
  async cacheSave(
    id: string,
    path: string,
    key: string,
    options: { backend?: string; format?: string } = {},
  ): Promise<boolean> {
    const data = await this.client.mutate<{ cacheSave: boolean }>(
      `mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
         cacheSave(id: $id, path: $path, key: $key, backend: $backend, format: $format)
       }`,
      { id, path, key, backend: options.backend ?? null, format: options.format ?? null },
    );
    return data.cacheSave;
  }

  /** Restore a cached entry into a sandbox path. */
  async cacheRestore(
    id: string,
    path: string,
    key: string,
    options: { backend?: string; format?: string } = {},
  ): Promise<boolean> {
    const data = await this.client.mutate<{ cacheRestore: boolean }>(
      `mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
         cacheRestore(id: $id, path: $path, key: $key, backend: $backend, format: $format)
       }`,
      { id, path, key, backend: options.backend ?? null, format: options.format ?? null },
    );
    return data.cacheRestore;
  }

  /** List entries in a cache backend (default backend when omitted). */
  async cacheList(backend?: string): Promise<Array<{ name: string; size: number }>> {
    const data = await this.client.query<{ cacheList: Array<{ name: string; size: number }> }>(
      `query($backend: String) { cacheList(backend: $backend) { name size } }`,
      { backend: backend ?? null },
    );
    return data.cacheList;
  }

  /** Snapshot a sandbox's overlay; an empty/undefined id gets a generated one. */
  async snapshot(id: string, snapshotId?: string): Promise<string> {
    const data = await this.client.mutate<{ snapshot: string }>(
      `mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }`,
      { id, snapshotId: snapshotId ?? null },
    );
    return data.snapshot;
  }

  /** Replace a sandbox's overlay with a snapshot (discard changes since). */
  async rollback(id: string, snapshotId: string): Promise<boolean> {
    const data = await this.client.mutate<{ rollback: boolean }>(
      `mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }`,
      { id, snapshotId },
    );
    return data.rollback;
  }

  /** Create a new sandbox from a snapshot; empty/undefined name gets a random one. */
  async branch(snapshotId: string, name?: string): Promise<RemoteSandboxInfo> {
    const data = await this.client.mutate<{ branch: RemoteSandboxInfo }>(
      `mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) ${SANDBOX_FIELDS} }`,
      { snapshotId, name: name ?? null },
    );
    return data.branch;
  }

  /** Fork a live sandbox's overlay into a new sandbox; empty/undefined name random. */
  async fork(id: string, name?: string): Promise<RemoteSandboxInfo> {
    const data = await this.client.mutate<{ fork: RemoteSandboxInfo }>(
      `mutation($id: String!, $name: String) { fork(id: $id, name: $name) ${SANDBOX_FIELDS} }`,
      { id, name: name ?? null },
    );
    return data.fork;
  }

  /** List saved overlay snapshots. */
  async snapshots(): Promise<Array<{ name: string; size: number }>> {
    const data = await this.client.query<{ snapshots: Array<{ name: string; size: number }> }>(
      `{ snapshots { name size } }`,
    );
    return data.snapshots;
  }

  /** Delete a snapshot; returns whether it existed. */
  async deleteSnapshot(id: string): Promise<boolean> {
    const data = await this.client.mutate<{ deleteSnapshot: boolean }>(
      `mutation($id: String!) { deleteSnapshot(id: $id) }`,
      { id },
    );
    return data.deleteSnapshot;
  }
}
