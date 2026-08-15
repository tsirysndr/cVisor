"""End-to-end tests for the cVisor Python SDK.

Requires a built libcvisor.so; point CVISOR_LIB at it (the xtask copies one into
cvisor/_native/ for packaging). Linux-only — skips elsewhere.
"""

import os
import platform
import time

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


def test_run_streaming():
    from cvisor import Sandbox

    with Sandbox() as sb:
        chunks = []
        code = sb.run_streaming(
            "for i in 1 2 3; do echo line$i; sleep 0.1; done",
            on_stdout=chunks.append,
        )
        assert code == 0
        assert "".join(chunks) == "line1\nline2\nline3\n"


def test_shell_pty():
    from cvisor import Sandbox

    with Sandbox() as sb:
        buf = []
        sh = sb.shell(on_output=lambda s: buf.append(s))
        sh.write_stdin("echo SHELL_OK\n")
        sh.write_stdin("test -t 1 && echo IS_TTY\n")
        sh.write_stdin("exit 4\n")
        assert sh.wait() == 4
        time.sleep(0.05)
        text = "".join(buf)
        assert "SHELL_OK" in text
        assert "IS_TTY" in text
        sh.close()
