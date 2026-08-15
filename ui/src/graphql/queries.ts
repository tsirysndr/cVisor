import { gql } from "graphql-request";

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

export const HEALTH = gql`
  query Health {
    health {
      version
      ok
    }
  }
`;

export const SANDBOXES = gql`
  query Sandboxes {
    sandboxes {
      id
      name
      allowNetwork
      allowListen
    }
  }
`;

export const CACHE_LIST = gql`
  query CacheList($backend: String) {
    cacheList(backend: $backend) {
      name
      size
    }
  }
`;

export const CREATE_SANDBOX = gql`
  mutation CreateSandbox($name: String) {
    createSandbox(name: $name) {
      id
      name
      allowNetwork
      allowListen
    }
  }
`;

export const FREE_SANDBOX = gql`
  mutation FreeSandbox($id: String!) {
    freeSandbox(id: $id)
  }
`;

export const RUN = gql`
  mutation Run($id: String, $command: String!, $timeoutMs: Int) {
    run(id: $id, command: $command, timeoutMs: $timeoutMs) {
      stdout
      stderr
      exitCode
    }
  }
`;

export const START_SESSION = gql`
  mutation StartSession($id: String, $command: String, $pty: Boolean) {
    startSession(id: $id, command: $command, pty: $pty) {
      id
    }
  }
`;

export const WRITE_SESSION = gql`
  mutation WriteSession($id: String!, $dataBase64: String!) {
    writeSession(id: $id, dataBase64: $dataBase64)
  }
`;

export const RESIZE_SESSION = gql`
  mutation ResizeSession($id: String!, $rows: Int!, $cols: Int!) {
    resizeSession(id: $id, rows: $rows, cols: $cols)
  }
`;

export const KILL_SESSION = gql`
  mutation KillSession($id: String!) {
    killSession(id: $id)
  }
`;

// graphql-ws subscription: streams base64 terminal output chunks.
export const SESSION_OUTPUT = gql`
  subscription SessionOutput($id: String!) {
    sessionOutput(id: $id)
  }
`;
