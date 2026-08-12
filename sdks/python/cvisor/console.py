"""Interactive cVisor console.

Drops you into an IPython REPL (falling back to the stdlib REPL if IPython is
not installed) with a live sandbox ready to use:

    sb            # a cvisor.Sandbox instance
    sh("ls /")    # run a command in the sandbox and print its output
    Sandbox       # the class, to create more sandboxes

Launch it with `cvisor` (the installed console script) or `python -m cvisor`.
"""

from __future__ import annotations

import sys

from . import Sandbox

BANNER = (
    "cVisor interactive console\n"
    "  sb          -> a Sandbox instance\n"
    '  sh("cmd")   -> run a shell command in the sandbox, printing stdout/stderr\n'
    "  Sandbox     -> create your own: Sandbox()\n"
)


def _make_namespace() -> dict:
    sb = Sandbox()

    def sh(command: str):
        """Run `command` in the sandbox; print stdout/stderr and return Output."""
        out = sb.run(command)
        if out.stdout:
            sys.stdout.write(out.stdout)
        if out.stderr:
            sys.stderr.write(out.stderr)
        return out

    return {"Sandbox": Sandbox, "sb": sb, "sh": sh}


def main() -> None:
    ns = _make_namespace()
    try:
        from IPython import start_ipython
        from traitlets.config import Config

        cfg = Config()
        cfg.TerminalInteractiveShell.banner1 = BANNER
        start_ipython(argv=[], user_ns=ns, config=cfg)
    except ImportError:
        import code

        code.interact(banner=BANNER, local=ns, exitmsg="")


if __name__ == "__main__":
    main()
