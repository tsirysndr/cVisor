"""Remote-GraphQL e2e smoke for the Python SDK's pure-stdlib client
(``RemoteSandbox`` over urllib — no libcvisor). Point it at a running cvisord::

    CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... python remote_smoke.py
"""

from __future__ import annotations

import os
import sys

from cvisor import RemoteSandbox


def main() -> None:
    url = os.environ.get("CVISOR_GRAPHQL_URL", "http://127.0.0.1:8080/graphql")
    token = os.environ.get("CVISOR_TOKEN", "")
    remote = RemoteSandbox(url, token)

    health = remote.health()
    assert health.get("ok") is True, f"health not ok: {health!r}"

    out = remote.run("echo hello")
    assert out["stdout"] == "hello\n", f"run stdout: {out!r}"
    assert out["exitCode"] == 0, f"run exit code: {out!r}"

    sb = remote.create_sandbox()
    assert sb.get("id"), f"create_sandbox returned no id: {sb!r}"
    remote.write_file(sb["id"], "/tmp/data.txt", "round-trip\n")
    data = remote.read_file(sb["id"], "/tmp/data.txt")
    assert data == b"round-trip\n", f"read_file round-trip: {data!r}"
    remote.free_sandbox(sb["id"])

    print("PY_GRAPHQL_OK")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # noqa: BLE001 - smoke: any failure exits non-zero
        print(f"remote smoke failed: {e}", file=sys.stderr)
        sys.exit(1)
