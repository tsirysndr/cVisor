# Wheel build hook: pick the bundled libcvisor build(s) and the wheel tag.
#
# The .so files are musl builds (`cargo xtask ffi`), so tagged wheels use the
# musllinux_1_2 platform (Alpine). force_include bypasses the wheel target's
# exclude of cvisor/_native.
import os

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


class CustomBuildHook(BuildHookInterface):
    def initialize(self, version, build_data):
        arch = os.environ.get("CVISOR_ARCH")
        arches = [arch] if arch else ["aarch64", "x86_64"]
        for a in arches:
            so = f"cvisor/_native/libcvisor-{a}.so"
            path = os.path.join(self.root, so)
            if not os.path.exists(path):
                raise FileNotFoundError(f"{so} missing — run `cargo xtask ffi --arch {a}`")
            build_data["force_include"][path] = so
        if arch:
            build_data["pure_python"] = False
            build_data["tag"] = f"py3-none-musllinux_1_2_{arch}"
