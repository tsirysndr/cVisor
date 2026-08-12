# Wheel build hook: pick the bundled libcvisor build(s) and the wheel tag.
#
# The .so files are musl builds (`cargo xtask ffi`), so tagged wheels use the
# musllinux_1_2 platform (Alpine). force_include bypasses the wheel target's
# exclude of cvisor/_native.
#
# CVISOR_ARCH=aarch64|x86_64 builds a platform wheel with exactly that arch's
# .so (missing = hard error; publishing flow). Unset, it builds an untagged
# fat wheel with whichever arches are present (dev/CI convenience — CI only
# builds its own runner's arch).
import os

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


class CustomBuildHook(BuildHookInterface):
    def initialize(self, version, build_data):
        if version == "editable":
            return
        arch = os.environ.get("CVISOR_ARCH")
        arches = [arch] if arch else ["aarch64", "x86_64"]
        included = []
        for a in arches:
            so = f"cvisor/_native/libcvisor-{a}.so"
            path = os.path.join(self.root, so)
            if os.path.exists(path):
                build_data["force_include"][path] = so
                included.append(a)
            elif arch:
                raise FileNotFoundError(f"{so} missing — run `cargo xtask ffi --arch {a}`")
        if not included:
            raise FileNotFoundError(
                "no cvisor/_native/libcvisor-*.so found — run `cargo xtask ffi`"
            )
        if arch:
            build_data["pure_python"] = False
            build_data["tag"] = f"py3-none-musllinux_1_2_{arch}"
