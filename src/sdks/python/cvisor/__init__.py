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
from ctypes import c_char_p, c_int, c_size_t, c_uint8, c_void_p, POINTER

__all__ = ["Sandbox", "Output", "load_library"]


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

    lib.cvisor_run.restype = c_void_p
    lib.cvisor_run.argtypes = [c_void_p, c_char_p]

    lib.cvisor_output_free.restype = None
    lib.cvisor_output_free.argtypes = [c_void_p]

    for fn in ("cvisor_output_stdout", "cvisor_output_stderr"):
        f = getattr(lib, fn)
        f.restype = POINTER(c_uint8)
        f.argtypes = [c_void_p, POINTER(c_size_t)]

    lib.cvisor_bytes_free.restype = None
    lib.cvisor_bytes_free.argtypes = [POINTER(c_uint8), c_size_t]

    return lib


_LIB: ctypes.CDLL | None = None


def _lib() -> ctypes.CDLL:
    global _LIB
    if _LIB is None:
        _LIB = load_library()
    return _LIB


class Output:
    """Captured output of one sandbox run."""

    def __init__(self, stdout: bytes, stderr: bytes) -> None:
        self.stdout_bytes = stdout
        self.stderr_bytes = stderr

    @property
    def stdout(self) -> str:
        return self.stdout_bytes.decode("utf-8", "replace")

    @property
    def stderr(self) -> str:
        return self.stderr_bytes.decode("utf-8", "replace")


class Sandbox:
    def __init__(self) -> None:
        self._lib = _lib()
        self._ptr = self._lib.cvisor_sandbox_new()
        if not self._ptr:
            raise RuntimeError("failed to create sandbox")

    def set_log_level(self, level: str) -> None:
        self._lib.cvisor_sandbox_set_log_level(self._ptr, 1 if level == "DEBUG" else 0)

    def run(self, command: str) -> Output:
        out = self._lib.cvisor_run(self._ptr, command.encode("utf-8"))
        if not out:
            raise RuntimeError("sandbox run failed")
        try:
            return Output(self._read(out, self._lib.cvisor_output_stdout),
                          self._read(out, self._lib.cvisor_output_stderr))
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
