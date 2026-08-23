"""``python3 python/forge_gen <cmd>`` — the directory is the program.

Run as a directory, Python puts the directory itself on ``sys.path`` and
not its parent, so ``import forge_gen`` would fail; the one line below
fixes that before the package is touched. ``python3 -m forge_gen`` and the
``forge-gen`` console script come in with the package already importable.
"""

import os
import sys

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from forge_gen.cli import main  # noqa: E402

sys.exit(main())
