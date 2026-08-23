"""``python3 python/forge_gen <cmd>`` — the directory is the program.

Run as a directory, Python puts the directory itself on ``sys.path`` and
not its parent, so ``import forge_gen`` would fail; the one line below
fixes that before the package is touched. ``python3 -m forge_gen`` and the
``forge-gen`` console script come in with the package already importable.

The version guard sits before any package import and uses no syntax newer
than the pythons it exists to catch: on a 3.10 host the failure used to be
twelve ``No module named 'tomllib'`` lines and an exit 2 that named
nothing.
"""

import os
import sys

if sys.version_info < (3, 11):
    sys.stderr.write(
        "forge-gen needs python3 >= 3.11; this is %s at %s -- "
        "put a newer python3 first on PATH (tomllib arrived in 3.11)\n"
        % (".".join(str(v) for v in sys.version_info[:3]), sys.executable)
    )
    sys.exit(6)  # MISSING_TOOL: the tool that is missing is the interpreter

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from forge_gen.cli import main  # noqa: E402

sys.exit(main())
