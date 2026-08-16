# A GraphQL-backed high-level API mirroring the FFI Cvisor::Sandbox, but driven
# over HTTP against a running cVisor daemon. Pure net/http + json (via
# Cvisor::GraphQL) — no native library — so it runs on any platform (macOS
# included). This is what makes the SDK usable off Linux.
#
#   remote = Cvisor::RemoteSandbox.new("http://127.0.0.1:8080/graphql", token)
#   out = remote.run("echo hello")
#   out["stdout"]   # => "hello\n"

require "base64"

require_relative "graphql"

module Cvisor
  # Control a cVisor daemon over its GraphQL API.
  class RemoteSandbox
    SANDBOX_FIELDS =
      "{ id name allowNetwork allowListen env { key value } " \
      "limits { memoryMax pidsMax cpuPercent } }".freeze

    def initialize(url, token)
      @client = Cvisor::GraphQL.new(url, token)
    end

    # Daemon liveness and build info.
    def health
      @client.query("{ health { version ok } }")["health"]
    end

    # Run a command in a fresh ephemeral sandbox. Returns a Hash with
    # "stdout"/"stderr" (String) and "exitCode" (Integer).
    def run(command, timeout_ms: nil)
      @client.mutate(
        "mutation($command: String!, $timeoutMs: Int) {
           run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
         }",
        { command: command, timeoutMs: timeout_ms || 0 }
      )["run"]
    end

    # Create a sandbox; a nil/empty name gets a random docker-style name.
    def create_sandbox(name = nil)
      @client.mutate(
        "mutation($name: String) { createSandbox(name: $name) #{SANDBOX_FIELDS} }",
        { name: name }
      )["createSandbox"]
    end

    # All live sandboxes.
    def list_sandboxes
      @client.query("{ sandboxes #{SANDBOX_FIELDS} }")["sandboxes"]
    end

    # Free a sandbox and clean up its overlay.
    def free_sandbox(id)
      @client.mutate("mutation($id: String!) { freeSandbox(id: $id) }", { id: id })["freeSandbox"]
    end

    # Update a sandbox's network/listen flags, limits, and env.
    def configure(id, allow_network: nil, allow_listen: nil, limits: nil, env: nil)
      @client.mutate(
        "mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean,
                  $limits: LimitsInput, $env: [EnvVarInput!]) {
           configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen,
                     limits: $limits, env: $env) #{SANDBOX_FIELDS}
         }",
        { id: id, allowNetwork: allow_network, allowListen: allow_listen, limits: limits, env: env }
      )["configure"]
    end

    # Write bytes (or a String) to a file inside a sandbox (base64 over the wire).
    def write_file(id, path, data)
      encoded = Base64.strict_encode64(data.to_s.b)
      @client.mutate(
        "mutation($id: String!, $path: String!, $data: String!) {
           writeFile(id: $id, path: $path, dataBase64: $data)
         }",
        { id: id, path: path, data: encoded }
      )["writeFile"]
    end

    # Read a file from a sandbox (base64-decoded from the wire); returns a binary String.
    def read_file(id, path)
      encoded = @client.query(
        "query($id: String!, $path: String!) { readFile(id: $id, path: $path) }",
        { id: id, path: path }
      )["readFile"]
      Base64.decode64(encoded)
    end

    # Archive a sandbox path into the cache under +key+.
    def cache_save(id, path, key, backend: "", format: "gzip")
      @client.mutate(
        "mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
           cacheSave(id: $id, path: $path, key: $key, backend: $backend, format: $format)
         }",
        { id: id, path: path, key: key, backend: backend, format: format }
      )["cacheSave"]
    end

    # Restore a cached entry into a sandbox path.
    def cache_restore(id, path, key, backend: "", format: "gzip")
      @client.mutate(
        "mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
           cacheRestore(id: $id, path: $path, key: $key, backend: $backend, format: $format)
         }",
        { id: id, path: path, key: key, backend: backend, format: format }
      )["cacheRestore"]
    end

    # List entries in a cache backend (default backend when nil).
    def cache_list(backend = nil)
      @client.query(
        "query($backend: String) { cacheList(backend: $backend) { name size } }",
        { backend: backend }
      )["cacheList"]
    end

    # Snapshot a sandbox's overlay; returns the snapshot id.
    def snapshot(id, snapshot_id = nil)
      @client.mutate(
        "mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }",
        { id: id, snapshotId: snapshot_id }
      )["snapshot"]
    end

    # Replace a sandbox's overlay with a snapshot (discard changes since).
    def rollback(id, snapshot_id)
      @client.mutate(
        "mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }",
        { id: id, snapshotId: snapshot_id }
      )["rollback"]
    end

    # Create a new sandbox from a snapshot.
    def branch(snapshot_id, name = nil)
      @client.mutate(
        "mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) #{SANDBOX_FIELDS} }",
        { snapshotId: snapshot_id, name: name }
      )["branch"]
    end

    # Fork a live sandbox's overlay into a new sandbox.
    def fork(id, name = nil)
      @client.mutate(
        "mutation($id: String!, $name: String) { fork(id: $id, name: $name) #{SANDBOX_FIELDS} }",
        { id: id, name: name }
      )["fork"]
    end

    # List saved overlay snapshots.
    def snapshots
      @client.query("{ snapshots { name size } }")["snapshots"]
    end

    # Delete a snapshot; returns whether it existed.
    def delete_snapshot(id)
      @client.mutate("mutation($id: String!) { deleteSnapshot(id: $id) }", { id: id })["deleteSnapshot"]
    end
  end
end
