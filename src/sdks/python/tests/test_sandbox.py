"""End-to-end tests for the bVisor Python SDK.

Requires a built libbvisor.so; point BVISOR_LIB at it (the xtask copies one into
bvisor/_native/ for packaging). Linux-only — skips elsewhere.
"""

import os
import platform

import pytest

pytestmark = pytest.mark.skipif(
    platform.system() != "Linux", reason="bVisor runs on Linux only"
)


def test_echo():
    from bvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("echo hello from python")
        assert out.stdout == "hello from python\n"


def test_tmp_roundtrip():
    from bvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("echo data > /tmp/f.txt; grep data /tmp/f.txt")
        assert out.stdout == "data\n"


def test_pipeline():
    from bvisor import Sandbox

    with Sandbox() as sb:
        out = sb.run("printf 'a\\nb\\nc\\n' | grep b")
        assert out.stdout == "b\n"
