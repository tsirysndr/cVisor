import { gql } from "graphql-request";

// GraphQL documents for the web transport. Types live in transport/types.ts.

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
      limits {
        memoryMax
        pidsMax
        cpuPercent
      }
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

export const SNAPSHOTS = gql`
  query Snapshots {
    snapshots {
      name
      size
    }
  }
`;

export const READ_FILE = gql`
  query ReadFile($id: String!, $path: String!) {
    readFile(id: $id, path: $path)
  }
`;

export const CREATE_SANDBOX = gql`
  mutation CreateSandbox($name: String) {
    createSandbox(name: $name) {
      id
      name
      allowNetwork
      allowListen
      limits {
        memoryMax
        pidsMax
        cpuPercent
      }
    }
  }
`;

export const FREE_SANDBOX = gql`
  mutation FreeSandbox($id: String!) {
    freeSandbox(id: $id)
  }
`;

export const CONFIGURE = gql`
  mutation Configure(
    $id: String!
    $allowNetwork: Boolean
    $allowListen: Boolean
    $limits: LimitsInput
  ) {
    configure(
      id: $id
      allowNetwork: $allowNetwork
      allowListen: $allowListen
      limits: $limits
    ) {
      id
      name
      allowNetwork
      allowListen
      limits {
        memoryMax
        pidsMax
        cpuPercent
      }
    }
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

export const WRITE_FILE = gql`
  mutation WriteFile($id: String!, $path: String!, $dataBase64: String!) {
    writeFile(id: $id, path: $path, dataBase64: $dataBase64)
  }
`;

export const SNAPSHOT = gql`
  mutation Snapshot($id: String!, $snapshotId: String) {
    snapshot(id: $id, snapshotId: $snapshotId)
  }
`;

export const ROLLBACK = gql`
  mutation Rollback($id: String!, $snapshotId: String!) {
    rollback(id: $id, snapshotId: $snapshotId)
  }
`;

export const BRANCH = gql`
  mutation Branch($snapshotId: String!, $name: String) {
    branch(snapshotId: $snapshotId, name: $name) {
      id
      name
      allowNetwork
      allowListen
      limits {
        memoryMax
        pidsMax
        cpuPercent
      }
    }
  }
`;

export const FORK = gql`
  mutation Fork($id: String!, $name: String) {
    fork(id: $id, name: $name) {
      id
      name
      allowNetwork
      allowListen
      limits {
        memoryMax
        pidsMax
        cpuPercent
      }
    }
  }
`;

export const DELETE_SNAPSHOT = gql`
  mutation DeleteSnapshot($id: String!) {
    deleteSnapshot(id: $id)
  }
`;

export const CACHE_SAVE = gql`
  mutation CacheSave(
    $id: String!
    $path: String!
    $key: String!
    $backend: String
    $format: String
  ) {
    cacheSave(
      id: $id
      path: $path
      key: $key
      backend: $backend
      format: $format
    )
  }
`;

export const CACHE_RESTORE = gql`
  mutation CacheRestore(
    $id: String!
    $path: String!
    $key: String!
    $backend: String
    $format: String
  ) {
    cacheRestore(
      id: $id
      path: $path
      key: $key
      backend: $backend
      format: $format
    )
  }
`;

export const CACHE_REMOVE = gql`
  mutation CacheRemove($key: String!, $backend: String, $format: String) {
    cacheRemove(key: $key, backend: $backend, format: $format)
  }
`;

export const CACHE_CLEAR = gql`
  mutation CacheClear($backend: String) {
    cacheClear(backend: $backend)
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
