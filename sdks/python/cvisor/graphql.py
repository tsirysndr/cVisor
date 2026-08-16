"""Optional GraphQL client for the cVisor daemon's HTTP API.

Pure stdlib (``urllib.request`` + ``json``) — no native library — so it runs on
any platform (macOS included), unlike the ctypes-backed :class:`~cvisor.Sandbox`.

Example:
    from cvisor import GraphQLClient
    gql = GraphQLClient("http://127.0.0.1:8080/graphql", token)
    data = gql.query("{ health { version ok } }")
    print(data["health"])   # {'version': '0.1.0', 'ok': True}
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any

__all__ = ["GraphQLClient", "GraphQLError"]


class GraphQLError(RuntimeError):
    """Raised when the daemon returns a non-empty ``errors`` array."""


class GraphQLClient:
    """A thin client for the cVisor daemon's GraphQL endpoint."""

    def __init__(self, url: str, token: str) -> None:
        self.url = url
        self.token = token

    def query(self, document: str, variables: dict[str, Any] | None = None) -> dict[str, Any]:
        """Run a GraphQL query document; returns the ``data`` object."""
        return self._execute(document, variables)

    def mutate(self, document: str, variables: dict[str, Any] | None = None) -> dict[str, Any]:
        """Run a GraphQL mutation document; returns the ``data`` object."""
        return self._execute(document, variables)

    def _execute(self, document: str, variables: dict[str, Any] | None) -> dict[str, Any]:
        payload = json.dumps({"query": document, "variables": variables or {}}).encode("utf-8")
        req = urllib.request.Request(
            self.url,
            data=payload,
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self.token}",
            },
        )
        try:
            with urllib.request.urlopen(req) as resp:
                body = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise GraphQLError(f"GraphQL request failed: HTTP {e.code} {e.reason}") from e

        errors = body.get("errors")
        if errors:
            raise GraphQLError("; ".join(e.get("message", str(e)) for e in errors))
        return body.get("data") or {}
