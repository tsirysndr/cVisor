"""End-to-end tests for the cVisor Python SDK.

Requires a built libcvisor.so; point CVISOR_LIB at it (the xtask copies one into
cvisor/_native/ for packaging). Linux-only — skips elsewhere.
"""

import os
import platform
import tempfile
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


def test_file_io():
    from cvisor import Sandbox

    with Sandbox() as sb:
        sb.write_file("/tmp/data.txt", "seeded\n")
        assert sb.run("grep seeded /tmp/data.txt").stdout == "seeded\n"

        sb.run("echo from-run > /tmp/out.txt")
        assert sb.read_file("/tmp/out.txt") == b"from-run\n"


def test_cache_roundtrip_across_sandboxes():
    from cvisor import Sandbox

    with Sandbox() as sb:
        sb.write_file("/tmp/proj/a.txt", "alpha\n")
        sb.write_file("/tmp/proj/b.txt", "beta\n")
        sb.cache_save("/tmp/proj", "k1")

    with Sandbox() as sb2:
        sb2.cache_restore("/tmp/proj", "k1")
        assert sb2.run("grep alpha /tmp/proj/a.txt").stdout == "alpha\n"
        assert sb2.run("grep beta /tmp/proj/b.txt").stdout == "beta\n"


def test_copy_into_copy_out_dir():
    from cvisor import Sandbox

    with tempfile.TemporaryDirectory() as src:
        with open(os.path.join(src, "one.txt"), "w") as f:
            f.write("one\n")
        with open(os.path.join(src, "two.txt"), "w") as f:
            f.write("two\n")

        with Sandbox() as sb:
            sb.copy_into(src, "/tmp/incoming")
            assert sb.run("grep one /tmp/incoming/one.txt").stdout == "one\n"
            assert sb.run("grep two /tmp/incoming/two.txt").stdout == "two\n"

            sb.run("echo three > /tmp/incoming/three.txt")
            with tempfile.TemporaryDirectory() as dst:
                out_dir = os.path.join(dst, "out")
                sb.copy_out("/tmp/incoming", out_dir)
                with open(os.path.join(out_dir, "three.txt")) as f:
                    assert f.read() == "three\n"


def test_allow_listen():
    from cvisor import Sandbox

    with Sandbox() as sb:
        sb.set_allow_listen(True)


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


def test_set_env():
    from cvisor import Sandbox

    with Sandbox() as sb:
        sb.set_env("FOO", "bar")
        sb.set_env("GREETING", "hi there")
        assert sb.run("echo $FOO-$GREETING").stdout == "bar-hi there\n"


def test_set_limits():
    # No writable cgroup v2 in the test container, so limits gracefully no-op;
    # a limited run must still succeed.
    from cvisor import Sandbox

    with Sandbox() as sb:
        sb.set_limits(memory_max=256 * 1024 * 1024, pids_max=128, cpu_percent=50)
        assert sb.run("echo limited").stdout == "limited\n"
