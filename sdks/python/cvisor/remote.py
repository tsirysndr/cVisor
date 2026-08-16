"""A GraphQL-backed high-level API mirroring the FFI :class:`~cvisor.Sandbox`,
but driven over HTTP against a running cVisor daemon.

Pure stdlib (via :class:`~cvisor.GraphQLClient`) — no native library — so it
runs on any platform (macOS included). This is what makes the SDK usable off
Linux, where the FFI :class:`~cvisor.Sandbox` is unavailable.

Example:
    from cvisor import RemoteSandbox
    remote = RemoteSandbox("http://127.0.0.1:8080/graphql", token)
    out = remote.run("echo hello")
    print(out["stdout"])   # "hello\\n"
"""

from __future__ import annotations

import base64
from typing import Any

from .graphql import GraphQLClient

__all__ = ["RemoteSandbox"]

_SANDBOX_FIELDS = (
    "{ id name allowNetwork allowListen env { key value } "
    "limits { memoryMax pidsMax cpuPercent } }"
)


class RemoteSandbox:
    """Control a cVisor daemon over its GraphQL API."""

    def __init__(self, url: str, token: str) -> None:
        self._client = GraphQLClient(url, token)

    def health(self) -> dict[str, Any]:
        """Daemon liveness and build info."""
        return self._client.query("{ health { version ok } }")["health"]

    def run(self, command: str, timeout_ms: int | None = None) -> dict[str, Any]:
        """Run a command in a fresh ephemeral sandbox.

        Returns a dict with ``stdout``/``stderr`` (str) and ``exitCode`` (int).
        """
        data = self._client.mutate(
            """mutation($command: String!, $timeoutMs: Int) {
                 run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
               }""",
            {"command": command, "timeoutMs": timeout_ms or 0},
        )
        return data["run"]

    def create_sandbox(self, name: str | None = None) -> dict[str, Any]:
        """Create a sandbox; a null/empty name gets a random docker-style name."""
        data = self._client.mutate(
            "mutation($name: String) { createSandbox(name: $name) " + _SANDBOX_FIELDS + " }",
            {"name": name},
        )
        return data["createSandbox"]

    def list_sandboxes(self) -> list[dict[str, Any]]:
        """All live sandboxes."""
        return self._client.query("{ sandboxes " + _SANDBOX_FIELDS + " }")["sandboxes"]

    def free_sandbox(self, id: str) -> bool:
        """Free a sandbox and clean up its overlay."""
        return self._client.mutate(
            "mutation($id: String!) { freeSandbox(id: $id) }", {"id": id}
        )["freeSandbox"]

    def configure(
        self,
        id: str,
        allow_network: bool | None = None,
        allow_listen: bool | None = None,
        limits: dict[str, int] | None = None,
        env: list[dict[str, str]] | None = None,
    ) -> dict[str, Any]:
        """Update a sandbox's network/listen flags, limits, and env."""
        data = self._client.mutate(
            """mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean,
                        $limits: LimitsInput, $env: [EnvVarInput!]) {
                 configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen,
                           limits: $limits, env: $env) """
            + _SANDBOX_FIELDS
            + " }",
            {
                "id": id,
                "allowNetwork": allow_network,
                "allowListen": allow_listen,
                "limits": limits,
                "env": env,
            },
        )
        return data["configure"]

    def write_file(self, id: str, path: str, data: bytes | str) -> bool:
        """Write bytes (or str) to a file inside a sandbox (base64 over the wire)."""
        if isinstance(data, str):
            data = data.encode("utf-8")
        res = self._client.mutate(
            """mutation($id: String!, $path: String!, $data: String!) {
                 writeFile(id: $id, path: $path, dataBase64: $data)
               }""",
            {"id": id, "path": path, "data": base64.b64encode(data).decode("ascii")},
        )
        return res["writeFile"]

    def read_file(self, id: str, path: str) -> bytes:
        """Read a file from a sandbox (base64-decoded from the wire)."""
        data = self._client.query(
            "query($id: String!, $path: String!) { readFile(id: $id, path: $path) }",
            {"id": id, "path": path},
        )
        return base64.b64decode(data["readFile"])

    def cache_save(
        self, id: str, path: str, key: str, backend: str = "", format: str = "gzip"
    ) -> bool:
        """Archive a sandbox path into the cache under ``key``."""
        res = self._client.mutate(
            """mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
                 cacheSave(id: $id, path: $path, key: $key, backend: $backend, format: $format)
               }""",
            {"id": id, "path": path, "key": key, "backend": backend, "format": format},
        )
        return res["cacheSave"]

    def cache_restore(
        self, id: str, path: str, key: str, backend: str = "", format: str = "gzip"
    ) -> bool:
        """Restore a cached entry into a sandbox path."""
        res = self._client.mutate(
            """mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
                 cacheRestore(id: $id, path: $path, key: $key, backend: $backend, format: $format)
               }""",
            {"id": id, "path": path, "key": key, "backend": backend, "format": format},
        )
        return res["cacheRestore"]

    def cache_list(self, backend: str | None = None) -> list[dict[str, Any]]:
        """List entries in a cache backend (default backend when None)."""
        return self._client.query(
            "query($backend: String) { cacheList(backend: $backend) { name size } }",
            {"backend": backend},
        )["cacheList"]

    def snapshot(self, id: str, snapshot_id: str | None = None) -> str:
        """Snapshot a sandbox's overlay; returns the snapshot id."""
        return self._client.mutate(
            "mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }",
            {"id": id, "snapshotId": snapshot_id},
        )["snapshot"]

    def rollback(self, id: str, snapshot_id: str) -> bool:
        """Replace a sandbox's overlay with a snapshot (discard changes since)."""
        return self._client.mutate(
            "mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }",
            {"id": id, "snapshotId": snapshot_id},
        )["rollback"]

    def branch(self, snapshot_id: str, name: str | None = None) -> dict[str, Any]:
        """Create a new sandbox from a snapshot."""
        data = self._client.mutate(
            "mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) "
            + _SANDBOX_FIELDS
            + " }",
            {"snapshotId": snapshot_id, "name": name},
        )
        return data["branch"]

    def fork(self, id: str, name: str | None = None) -> dict[str, Any]:
        """Fork a live sandbox's overlay into a new sandbox."""
        data = self._client.mutate(
            "mutation($id: String!, $name: String) { fork(id: $id, name: $name) "
            + _SANDBOX_FIELDS
            + " }",
            {"id": id, "name": name},
        )
        return data["fork"]

    def snapshots(self) -> list[dict[str, Any]]:
        """List saved overlay snapshots."""
        return self._client.query("{ snapshots { name size } }")["snapshots"]

    def delete_snapshot(self, id: str) -> bool:
        """Delete a snapshot; returns whether it existed."""
        return self._client.mutate(
            "mutation($id: String!) { deleteSnapshot(id: $id) }", {"id": id}
        )["deleteSnapshot"]
