"""End-to-end tests for the cVisor Python SDK.

Requires a built libcvisor.so; point CVISOR_LIB at it (the xtask copies one into
cvisor/_native/ for packaging). Linux-only — skips elsewhere.
"""

import os
import platform

import pytest

pytestmark = pytest.mark.skipif(
    platform.system() != "Linux", reason="cVisor runs on Linux only"
)


def test_echo():
    from cvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("echo hello from python")
        assert out.stdout == "hello from python\n"


def test_tmp_roundtrip():
    from cvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("echo data > /tmp/f.txt; grep data /tmp/f.txt")
        assert out.stdout == "data\n"


def test_pipeline():
    from cvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("printf 'a\\nb\\nc\\n' | grep b")
        assert out.stdout == "b\n"
