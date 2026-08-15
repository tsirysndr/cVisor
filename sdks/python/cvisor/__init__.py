"""cVisor Python SDK — a thin ctypes wrapper over the libcvisor C ABI.

Example:
    from cvisor import Sandbox
    out = Sandbox().run("echo hello")
    print(out.stdout)  # "hello\n"
"""

from __future__ import annotations

import ctypes
import os
import platform
import threading
import time
from ctypes import (
    c_char_p,
    c_int,
    c_long,
    c_size_t,
    c_uint8,
    c_uint16,
    c_uint32,
    c_uint64,
    c_void_p,
    POINTER,
)

__all__ = ["Sandbox", "Output", "Session", "load_library"]


def _default_library_path() -> str:
    """Resolve libcvisor.so: CVISOR_LIB env override, else a bundled copy."""
    override = os.environ.get("CVISOR_LIB")
    if override:
        return override
    here = os.path.dirname(os.path.abspath(__file__))
    arch = "aarch64" if platform.machine() in ("aarch64", "arm64") else "x86_64"
    candidates = [
        os.path.join(here, "_native", f"libcvisor-{arch}.so"),
        os.path.join(here, "_native", "libcvisor.so"),
        os.path.join(here, "libcvisor.so"),
    ]
    for c in candidates:
        if os.path.exists(c):
            return c
    # Fall back to the first candidate so the error names a concrete path.
    return candidates[0]


def load_library(path: str | None = None) -> ctypes.CDLL:
    lib = ctypes.CDLL(path or _default_library_path())

    lib.cvisor_sandbox_new.restype = c_void_p
    lib.cvisor_sandbox_new.argtypes = []

    lib.cvisor_sandbox_free.restype = None
    lib.cvisor_sandbox_free.argtypes = [c_void_p]

    lib.cvisor_sandbox_set_log_level.restype = None
    lib.cvisor_sandbox_set_log_level.argtypes = [c_void_p, c_int]

    lib.cvisor_sandbox_set_allow_network.restype = None
    lib.cvisor_sandbox_set_allow_network.argtypes = [c_void_p, c_int]

    lib.cvisor_sandbox_set_allow_listen.restype = None
    lib.cvisor_sandbox_set_allow_listen.argtypes = [c_void_p, c_int]

    lib.cvisor_sandbox_set_env.restype = None
    lib.cvisor_sandbox_set_env.argtypes = [c_void_p, c_char_p, c_char_p]

    lib.cvisor_sandbox_set_limits.restype = None
    lib.cvisor_sandbox_set_limits.argtypes = [c_void_p, c_uint64, c_uint64, c_uint32]

    lib.cvisor_sandbox_write_file.restype = c_int
    lib.cvisor_sandbox_write_file.argtypes = [c_void_p, c_char_p, POINTER(c_uint8), c_size_t]

    lib.cvisor_sandbox_read_file.restype = POINTER(c_uint8)
    lib.cvisor_sandbox_read_file.argtypes = [c_void_p, c_char_p, POINTER(c_size_t)]

    lib.cvisor_sandbox_copy_into.restype = c_int
    lib.cvisor_sandbox_copy_into.argtypes = [c_void_p, c_char_p, c_char_p]

    lib.cvisor_sandbox_copy_out.restype = c_int
    lib.cvisor_sandbox_copy_out.argtypes = [c_void_p, c_char_p, c_char_p]

    lib.cvisor_cache_save.restype = c_int
    lib.cvisor_cache_save.argtypes = [c_void_p, c_char_p, c_char_p, c_char_p, c_char_p]

    lib.cvisor_cache_restore.restype = c_int
    lib.cvisor_cache_restore.argtypes = [c_void_p, c_char_p, c_char_p, c_char_p, c_char_p]

    lib.cvisor_run.restype = c_void_p
    lib.cvisor_run.argtypes = [c_void_p, c_char_p]

    lib.cvisor_run_timeout.restype = c_void_p
    lib.cvisor_run_timeout.argtypes = [c_void_p, c_char_p, c_uint64]

    lib.cvisor_output_free.restype = None
    lib.cvisor_output_free.argtypes = [c_void_p]

    lib.cvisor_output_exit_code.restype = c_int
    lib.cvisor_output_exit_code.argtypes = [c_void_p]

    for fn in ("cvisor_output_stdout", "cvisor_output_stderr"):
        f = getattr(lib, fn)
        f.restype = POINTER(c_uint8)
        f.argtypes = [c_void_p, POINTER(c_size_t)]

    lib.cvisor_bytes_free.restype = None
    lib.cvisor_bytes_free.argtypes = [POINTER(c_uint8), c_size_t]

    lib.cvisor_session_start.restype = c_void_p
    lib.cvisor_session_start.argtypes = [c_void_p, c_char_p, c_int]

    for fn in ("cvisor_session_read_stdout", "cvisor_session_read_stderr"):
        f = getattr(lib, fn)
        f.restype = POINTER(c_uint8)
        f.argtypes = [c_void_p, POINTER(c_size_t)]

    lib.cvisor_session_write_stdin.restype = c_long
    lib.cvisor_session_write_stdin.argtypes = [c_void_p, POINTER(c_uint8), c_size_t]

    lib.cvisor_session_resize.restype = None
    lib.cvisor_session_resize.argtypes = [c_void_p, c_uint16, c_uint16]

    lib.cvisor_session_try_wait.restype = c_int
    lib.cvisor_session_try_wait.argtypes = [c_void_p, POINTER(c_int)]

    lib.cvisor_session_wait.restype = c_int
    lib.cvisor_session_wait.argtypes = [c_void_p]

    lib.cvisor_session_kill.restype = None
    lib.cvisor_session_kill.argtypes = [c_void_p]

    lib.cvisor_session_free.restype = None
    lib.cvisor_session_free.argtypes = [c_void_p]

    return lib


_LIB: ctypes.CDLL | None = None


def _lib() -> ctypes.CDLL:
    global _LIB
    if _LIB is None:
        _LIB = load_library()
    return _LIB


class Output:
    """Captured output of one sandbox run."""

    def __init__(self, stdout: bytes, stderr: bytes, exit_code: int) -> None:
        self.stdout_bytes = stdout
        self.stderr_bytes = stderr
        self.exit_code = exit_code

    @property
    def stdout(self) -> str:
        return self.stdout_bytes.decode("utf-8", "replace")

    @property
    def stderr(self) -> str:
        return self.stderr_bytes.decode("utf-8", "replace")


class Session:
    """A live, streaming sandbox session.

    Wraps an opaque ``CvisorSession*``. For a PTY session stdout and stderr are
    merged (drain stdout; stderr is empty) and stdin can be written; for a plain
    session the two streams are separate and stdin is unavailable.
    """

    def __init__(self, lib: ctypes.CDLL, ptr: int) -> None:
        self._lib = lib
        self._ptr = ptr

    def _read(self, accessor) -> bytes:
        n = c_size_t(0)
        ptr = accessor(self._ptr, ctypes.byref(n))
        if not ptr or n.value == 0:
            return b""
        try:
            return bytes(ctypes.cast(ptr, POINTER(c_uint8 * n.value)).contents)
        finally:
            self._lib.cvisor_bytes_free(ptr, n.value)

    def read_stdout(self) -> bytes:
        return self._read(self._lib.cvisor_session_read_stdout)

    def read_stderr(self) -> bytes:
        return self._read(self._lib.cvisor_session_read_stderr)

    def write_stdin(self, data: bytes | str) -> int:
        if isinstance(data, str):
            data = data.encode("utf-8")
        buf = (c_uint8 * len(data)).from_buffer_copy(data)
        return self._lib.cvisor_session_write_stdin(self._ptr, buf, len(data))

    def resize(self, rows: int, cols: int) -> None:
        self._lib.cvisor_session_resize(self._ptr, rows, cols)

    def exit_code(self) -> int | None:
        """Return the exit code if the session has finished, else None."""
        done = c_int(0)
        code = self._lib.cvisor_session_try_wait(self._ptr, ctypes.byref(done))
        return code if done.value else None

    def wait(self) -> int:
        return self._lib.cvisor_session_wait(self._ptr)

    def kill(self) -> None:
        self._lib.cvisor_session_kill(self._ptr)

    def close(self) -> None:
        if getattr(self, "_ptr", None):
            self._lib.cvisor_session_free(self._ptr)
            self._ptr = None

    def __del__(self) -> None:
        self.close()

    def __enter__(self) -> "Session":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


class Sandbox:
    def __init__(self) -> None:
        self._lib = _lib()
        self._ptr = self._lib.cvisor_sandbox_new()
        if not self._ptr:
            raise RuntimeError("failed to create sandbox")

    def set_log_level(self, level: str) -> None:
        self._lib.cvisor_sandbox_set_log_level(self._ptr, 1 if level == "DEBUG" else 0)

    def set_allow_network(self, allow: bool) -> None:
        """Allow (default) or deny outbound INET/INET6 networking."""
        self._lib.cvisor_sandbox_set_allow_network(self._ptr, 1 if allow else 0)

    def set_allow_listen(self, allow: bool) -> None:
        """Allow or deny (default) inbound TCP servers (listen/accept)."""
        self._lib.cvisor_sandbox_set_allow_listen(self._ptr, 1 if allow else 0)

    def set_env(self, key: str, value: str) -> None:
        """Set an environment variable for the guest (applies to later runs)."""
        self._lib.cvisor_sandbox_set_env(
            self._ptr, key.encode("utf-8"), value.encode("utf-8")
        )

    def set_limits(
        self,
        memory_max: int = 0,
        pids_max: int = 0,
        cpu_percent: int = 0,
    ) -> None:
        """Cap guest resources via cgroup v2 (applies to later runs).

        ``memory_max`` is bytes, ``pids_max`` a process count, ``cpu_percent``
        percent of one core (50 = half a core). 0 leaves that limit unset."""
        self._lib.cvisor_sandbox_set_limits(
            self._ptr, memory_max, pids_max, cpu_percent
        )

    def write_file(self, path: str, data: bytes | str) -> None:
        """Write ``data`` to ``path`` in the sandbox's persistent overlay.

        ``path`` must be absolute; ``/proc`` and passthrough paths are not
        writable. The file is visible to later ``run`` calls of this same
        Sandbox instance. Raises ``OSError`` on failure."""
        if isinstance(data, str):
            data = data.encode("utf-8")
        buf = (c_uint8 * len(data)).from_buffer_copy(data)
        rc = self._lib.cvisor_sandbox_write_file(
            self._ptr, path.encode("utf-8"), buf, len(data)
        )
        if rc != 0:
            raise OSError(-rc, os.strerror(-rc), path)

    def read_file(self, path: str) -> bytes:
        """Read ``path`` as the guest sees it (overlay copy, else the real host
        file for cow paths) and return its bytes.

        ``path`` must be absolute. An empty file returns ``b""``; a missing or
        unreadable path raises ``FileNotFoundError``."""
        n = c_size_t(0)
        ptr = self._lib.cvisor_sandbox_read_file(
            self._ptr, path.encode("utf-8"), ctypes.byref(n)
        )
        if not ptr:
            if n.value == 0:
                return b""
            raise FileNotFoundError(path)
        try:
            return bytes(ctypes.cast(ptr, POINTER(c_uint8 * n.value)).contents)
        finally:
            self._lib.cvisor_bytes_free(ptr, n.value)

    def copy_into(self, host_path: str, guest_path: str) -> None:
        """Copy a host file or directory tree into the sandbox overlay.

        ``host_path`` may be a single file or a directory; directories are
        copied recursively and respect ``.gitignore``/``.dockerignore``. The
        copied files land at ``guest_path`` in the sandbox's persistent overlay
        and are visible to later ``run`` calls. Raises ``OSError`` on failure."""
        rc = self._lib.cvisor_sandbox_copy_into(
            self._ptr, host_path.encode("utf-8"), guest_path.encode("utf-8")
        )
        if rc != 0:
            raise OSError(-rc, os.strerror(-rc), host_path)

    def copy_out(self, guest_path: str, host_path: str) -> None:
        """Copy a file or directory tree out of the sandbox overlay to the host.

        ``guest_path`` is resolved as the guest sees it and written to
        ``host_path`` on the real filesystem. Raises ``OSError`` on failure."""
        rc = self._lib.cvisor_sandbox_copy_out(
            self._ptr, guest_path.encode("utf-8"), host_path.encode("utf-8")
        )
        if rc != 0:
            raise OSError(-rc, os.strerror(-rc), host_path)

    def cache_save(
        self, sandbox_path: str, key: str, backend: str = "", format: str = "gzip"
    ) -> None:
        """Archive a sandbox directory under ``key`` to a cache backend.

        ``sandbox_path`` is a directory in the sandbox; its contents (respecting
        ``.gitignore``/``.dockerignore``) are archived under ``key``.

        ``backend`` selects where the archive is stored: ``""`` or ``"disk"``
        (host disk, the default), ``"disk:/path"``, or
        ``"s3://bucket/prefix?region=..&endpoint=.."``. S3 requires the library
        to be built with the ``s3`` feature; the bundled ``libcvisor.so`` is not,
        so ``disk`` is the working default.

        ``format`` selects the archive format: ``""`` or ``"gzip"`` (the
        default), ``"estargz"``, ``"none"`` (uncompressed), or ``"zstd"``
        (only if the library is built with zstd; the bundled lib is not).

        Raises ``OSError`` on failure."""
        rc = self._lib.cvisor_cache_save(
            self._ptr,
            sandbox_path.encode("utf-8"),
            key.encode("utf-8"),
            backend.encode("utf-8"),
            format.encode("utf-8"),
        )
        if rc != 0:
            raise OSError(-rc, os.strerror(-rc), key)

    def cache_restore(
        self, sandbox_path: str, key: str, backend: str = "", format: str = "gzip"
    ) -> None:
        """Restore a cached archive stored under ``key`` back into the overlay.

        Unpacks the archive saved by :meth:`cache_save` into ``sandbox_path`` in
        the sandbox's persistent overlay, so its files are visible to later
        ``run`` calls. ``backend`` and ``format`` must match what was used to
        save (see :meth:`cache_save` for the accepted values and the disk-only
        note for the bundled library). Raises ``OSError`` on failure."""
        rc = self._lib.cvisor_cache_restore(
            self._ptr,
            sandbox_path.encode("utf-8"),
            key.encode("utf-8"),
            backend.encode("utf-8"),
            format.encode("utf-8"),
        )
        if rc != 0:
            raise OSError(-rc, os.strerror(-rc), key)

    def run(self, command: str, timeout_ms: int | None = None) -> Output:
        """Run a shell command; if timeout_ms is set, SIGKILL the guest after
        that many milliseconds (a timed-out run reports exit code 137)."""
        cmd = command.encode("utf-8")
        if timeout_ms is not None and timeout_ms > 0:
            out = self._lib.cvisor_run_timeout(self._ptr, cmd, timeout_ms)
        else:
            out = self._lib.cvisor_run(self._ptr, cmd)
        if not out:
            raise RuntimeError("sandbox run failed")
        try:
            return Output(self._read(out, self._lib.cvisor_output_stdout),
                          self._read(out, self._lib.cvisor_output_stderr),
                          self._lib.cvisor_output_exit_code(out))
        finally:
            self._lib.cvisor_output_free(out)

    def _read(self, out: int, accessor) -> bytes:
        n = c_size_t(0)
        ptr = accessor(out, ctypes.byref(n))
        if not ptr or n.value == 0:
            return b""
        try:
            return bytes(ctypes.cast(ptr, POINTER(c_uint8 * n.value)).contents)
        finally:
            self._lib.cvisor_bytes_free(ptr, n.value)

    def run_streaming(self, command: str, on_stdout=None, on_stderr=None,
                      poll_ms: int = 15) -> int:
        """Run a command, streaming output to callbacks as it is produced.

        Starts a non-PTY session and polls it: whenever new stdout/stderr bytes
        arrive they are decoded (utf-8, errors="replace") and passed to the
        matching callback. Returns the command's exit code."""
        ptr = self._lib.cvisor_session_start(self._ptr, command.encode("utf-8"), 0)
        if not ptr:
            raise RuntimeError("failed to start session")
        session = Session(self._lib, ptr)
        try:
            while True:
                out = session.read_stdout()
                if out and on_stdout is not None:
                    on_stdout(out.decode("utf-8", "replace"))
                err = session.read_stderr()
                if err and on_stderr is not None:
                    on_stderr(err.decode("utf-8", "replace"))
                code = session.exit_code()
                if code is not None:
                    out = session.read_stdout()
                    if out and on_stdout is not None:
                        on_stdout(out.decode("utf-8", "replace"))
                    err = session.read_stderr()
                    if err and on_stderr is not None:
                        on_stderr(err.decode("utf-8", "replace"))
                    return code
                time.sleep(poll_ms / 1000)
        finally:
            session.close()

    def shell(self, on_output=None, poll_ms: int = 15) -> Session:
        """Start an interactive PTY shell (/bin/sh -i) as a streaming session.

        If ``on_output`` is given, a daemon thread drains merged output and
        passes each decoded chunk (utf-8, errors="replace") to it until the
        shell exits. Returns the Session so the caller can write_stdin/resize/
        wait/close it."""
        ptr = self._lib.cvisor_session_start(self._ptr, None, 1)
        if not ptr:
            raise RuntimeError("failed to start session")
        session = Session(self._lib, ptr)
        if on_output is not None:
            def pump() -> None:
                while True:
                    out = session.read_stdout()
                    if out:
                        on_output(out.decode("utf-8", "replace"))
                    if session.exit_code() is not None:
                        out = session.read_stdout()
                        if out:
                            on_output(out.decode("utf-8", "replace"))
                        return
                    time.sleep(poll_ms / 1000)

            threading.Thread(target=pump, daemon=True).start()
        return session

    def close(self) -> None:
        if getattr(self, "_ptr", None):
            self._lib.cvisor_sandbox_free(self._ptr)
            self._ptr = None

    def __del__(self) -> None:
        self.close()

    def __enter__(self) -> "Sandbox":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
