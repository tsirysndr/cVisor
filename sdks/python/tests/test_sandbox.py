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


def test_exit_codes():
    from cvisor import Sandbox

    with Sandbox() as sb:
        assert sb.run("exit 7").exit_code == 7
        assert sb.run("false").exit_code == 1
        assert sb.run("true").exit_code == 0


def test_atomic_rename():
    from cvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a")
        assert out.stdout == "hi\n"


def test_timeout():
    from cvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("sleep 30", timeout_ms=300)
        assert out.exit_code == 137
