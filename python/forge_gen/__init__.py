"""forge_gen: the generator launcher of asset-forge.

Stdlib-only at import time, by rule. The outer half of every command runs
under the system python and resolves a backend's interpreter before any GPU
work; the inner half runs under that interpreter and may import torch, bpy
and numpy — inside functions, never at module level. ``cli.py`` is the
tree, ``launcher.py`` the seam, ``records.py`` the contract with Rust.
"""

__version__ = "0.1.0"
