"""An opt-in VRAM ceiling for an inner half, so a 24 GB card can stand in for a 16 GB one.

``forge.toml``'s ``[hardware] tier`` offers a **lean** column for a 16 GB
part, and the numbers in that column can only be honest if somebody measured
them. Nobody here owns a 16 GB card, so the Phase 0 spike measures on the
24 GB one with the inner process held below a ceiling. This module is that
ceiling, and it is **opt-in**: with ``FORGE_VRAM_CAP_GB`` unset — which is
every ordinary run — nothing here changes a byte of behaviour.

Set it and the inner half calls
``torch.cuda.set_per_process_memory_fraction(cap / total)`` **before the
first weight is loaded**, so an allocation past the ceiling raises
``torch.OutOfMemoryError`` the way it would on the smaller card::

    FORGE_VRAM_CAP_GB=16 python3 python/forge_gen mesh ref.png --resolution 512 ...

**What the ceiling is, and is not.** ``set_per_process_memory_fraction``
bounds *torch's caching allocator in this process*. It does not bound the
CUDA context itself, cuBLAS/cuDNN workspaces, nvdiffrast's own device
allocations, or anything a second process holds. So a run under the cap is
an *approximation* of a 16 GB part — good enough to say "this completes and
peaks near here", never a certificate that the part is enough. Every number
measured this way says so where it is written down (``designs/hosting.md``).

Two doors, because an inner half is not always ours:

* ``apply()`` — called from an inner module we own (``forge_gen.mesh``,
  ``forge_gen.motion.sweep``) right after ``import torch`` and before the
  pipeline is built;
* ``python -m forge_gen.vram_cap <script.py> [args…]`` — a wrapper that
  applies the ceiling and then runs somebody else's script under its own
  ``__main__``, for an upstream entry point (SkinTokens' ``demo.py``) whose
  source is not ours to edit.

The companion knob is ``PYTORCH_CUDA_ALLOC_CONF``, which the caller exports;
``expandable_segments:True`` is what keeps a tight ceiling from failing on
fragmentation rather than on size. It is not set here — an allocator policy
belongs to whoever launched the run, and this module only reads.
"""

from __future__ import annotations

import os
import runpy
import sys

#: The environment variable that switches the ceiling on. Unset = no ceiling.
CAP_VAR = "FORGE_VRAM_CAP_GB"

#: The log prefix, so a capped run is obvious in a log tail.
TAG = "vram-cap"


def cap_gb() -> float | None:
    """The ceiling in GB, or ``None`` when the run is not capped.

    A value that is not a positive number is a usage error the caller wants
    to hear about immediately, not a silently ignored cap.
    """
    raw = os.environ.get(CAP_VAR, "").strip()
    if not raw:
        return None
    try:
        value = float(raw)
    except ValueError as err:
        raise ValueError(f"{CAP_VAR}={raw!r} is not a number of gigabytes") from err
    if value <= 0:
        raise ValueError(f"{CAP_VAR}={raw!r} must be a positive number of gigabytes")
    return value


def apply(stream=None) -> dict | None:
    """Hold this process to ``$FORGE_VRAM_CAP_GB`` of VRAM; return what was applied, or ``None``.

    Safe to call when the variable is unset, when torch is not importable and
    when there is no CUDA device: each of those is "no ceiling", reported and
    not raised, because the cap is a measurement aid and never a gate.
    """
    out = stream or sys.stderr
    cap = cap_gb()
    if cap is None:
        return None
    try:
        import torch  # noqa: PLC0415
    except ImportError:
        print(f"{TAG}: {CAP_VAR}={cap} but torch is not importable — no ceiling applied", file=out, flush=True)
        return None
    if not torch.cuda.is_available():
        print(f"{TAG}: {CAP_VAR}={cap} but no CUDA device — no ceiling applied", file=out, flush=True)
        return None
    total_bytes = int(torch.cuda.get_device_properties(0).total_memory)
    total_gb = total_bytes / 1024.0**3
    fraction = min(1.0, (cap * 1024.0**3) / total_bytes)
    torch.cuda.set_per_process_memory_fraction(fraction, 0)
    print(
        f"{TAG}: torch allocator held to {cap:.1f} GB of {total_gb:.1f} GB "
        f"(fraction {fraction:.4f}) — approximate: the CUDA context and any "
        f"non-torch device allocation sit outside it",
        file=out,
        flush=True,
    )
    return {
        "cap_gb": cap,
        "device_total_gb": round(total_gb, 3),
        "fraction": round(fraction, 6),
        "alloc_conf": os.environ.get("PYTORCH_CUDA_ALLOC_CONF"),
        "approximate": True,
    }


def main(argv: list[str] | None = None) -> int:
    """``python -m forge_gen.vram_cap <script.py> [args…]`` — cap, then run somebody else's script."""
    args = list(sys.argv[1:] if argv is None else argv)
    if not args:
        sys.stderr.write(
            "forge_gen.vram_cap: usage: python -m forge_gen.vram_cap <script.py> [args…]\n"
            f"                   with {CAP_VAR} set; without it the script runs uncapped\n"
        )
        return 2
    script = args[0]
    apply()
    sys.argv = args
    # The script's own directory leads sys.path, exactly as `python script.py` does.
    script_dir = os.path.dirname(os.path.abspath(script))
    if script_dir not in sys.path:
        sys.path.insert(0, script_dir)
    runpy.run_path(script, run_name="__main__")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
