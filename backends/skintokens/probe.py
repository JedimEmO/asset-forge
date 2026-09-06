"""What doctor runs inside the SkinTokens env: does the model's own package import, is bpy there, are the checkpoints on disk?

Run under ``backends/skintokens/.env/bin/python`` from the upstream clone,
with the ``[env]`` from ``backend.toml`` exported (``SKINTOKENS_CHECKOUT``
and ``SKINTOKENS_WEIGHTS`` above all). The last stdout line is one JSON
object in the shape ``forge_gen.doctor`` reads::

    {"torch": "2.7.0+cu128", "cuda_available": true, "torch_cuda": "12.8",
     "imports": {"src.model.tokenrig": true, ...}, "extras": {...},
     "notices": [...], "hints": [...]}

``src`` is a plain directory in the clone, not an installed distribution,
so the checkout goes on ``sys.path`` first — from ``SKINTOKENS_CHECKOUT``,
falling back to the working directory, which is the checkout when the
launcher runs this.

Every import is attempted and none is fatal: the failure is reported as
the import that failed, not as a traceback. Exit 0 whenever the JSON line
could be written.
"""

from __future__ import annotations

import importlib
import json
import os
import sys

#: What the inner half imports. ``src.model.tokenrig`` is the whole stack —
#: transformers, diffusers, lightning and the patched flash-attn fallback —
#: and ``src.rig_package.parser.bpy`` is bpy plus the file issue #8 is in.
IMPORTS = (
    "torch",
    "numpy",
    "transformers",
    "diffusers",
    "lightning",
    "trimesh",
    "open3d",
    "fast_simplification",
    "bottle",
    "tornado",
    "bpy",
    "src",
    "src.model.tokenrig",
    "src.rig_package.parser.bpy",
    "src.server.spec",
)

#: What ``download.py --model`` leaves under the weights directory, and the
#: one file each must hold. Directory names are upstream's; the checkpoint
#: loader joins them itself, so they are not ours to rename.
WEIGHTS = (
    ("experiments/skin_vae_2_10_32768", "last.ckpt"),
    ("experiments/articulation_xl_quantization_256_token_4", "grpo_1400.ckpt"),
    ("models/Qwen3-0.6B", "config.json"),
)


def checkout_dir() -> str:
    """The clone: ``$SKINTOKENS_CHECKOUT``, else the working directory."""
    root = os.environ.get("SKINTOKENS_CHECKOUT") or os.getcwd()
    return os.path.realpath(root)


def _try_import(name: str) -> tuple[bool, str | None]:
    try:
        importlib.import_module(name)
        return True, None
    except Exception as err:  # noqa: BLE001 - any failure is the answer
        return False, f"{err.__class__.__name__}: {err}"


def patch_state(checkout: str) -> tuple[dict, list[str]]:
    """Are ``patches/0001`` (SDPA) and ``patches/0002`` (issue #8) in the tree?

    Read from the source text rather than from ``git apply -R --check``, so
    an adopted clone that carries the same fix written another way still
    reads as patched. Both are reported, never fatal: 0001's absence shows
    up anyway as ``src.model.tokenrig`` failing to import.
    """
    extras: dict = {}
    hints: list[str] = []

    def _read(rel: str) -> str | None:
        try:
            with open(os.path.join(checkout, rel), encoding="utf-8") as handle:
                return handle.read()
        except OSError:
            return None

    tokenrig = _read("src/model/tokenrig.py")
    if tokenrig is None:
        extras["patch:sdpa"] = "no checkout"
        hints.append(f"{checkout}/src/model/tokenrig.py is not readable — is .checkout the SkinTokens clone?")
    elif 'attn_implementation="flash_attention_2"' in tokenrig:
        extras["patch:sdpa"] = "missing"
        hints.append(
            "src/model/tokenrig.py still asks for flash_attention_2: "
            "git -C %s apply %s/patches/0001-sdpa-instead-of-flash-attn.patch" % (checkout, os.path.dirname(os.path.abspath(__file__)))
        )
    else:
        extras["patch:sdpa"] = "applied"

    parser = _read("src/rig_package/parser/bpy.py")
    if parser is None:
        extras["patch:issue8"] = "no checkout"
    elif parser.count("sons[p].append(i)") > 1:
        extras["patch:issue8"] = "missing"
        hints.append(
            "make_asset() still appends each child twice (upstream issue #8; bone tails wrong on export): "
            "git -C %s apply %s/patches/0002-make_asset-sons-counted-once.patch" % (checkout, os.path.dirname(os.path.abspath(__file__)))
        )
    else:
        extras["patch:issue8"] = "applied"
    return extras, hints


def weights_layout(root: str | None) -> tuple[dict, list[str]]:
    """Check what ``download.py --model`` writes; ``(extras, hints)``."""
    extras: dict = {}
    hints: list[str] = []
    if not root:
        extras["weights"] = "unset"
        hints.append("SKINTOKENS_WEIGHTS is unset: no backends/skintokens/.checkpoints link — bash backends/skintokens/install.sh")
        return extras, hints
    if not os.path.isdir(root):
        extras["weights"] = "absent"
        hints.append(f"SKINTOKENS_WEIGHTS={root} is not a directory")
        return extras, hints
    missing = [
        os.path.join(directory, name)
        for directory, name in WEIGHTS
        if not os.path.isfile(os.path.join(root, directory, name))
    ]
    if missing:
        extras["weights"] = "incomplete"
        hints.append(f"{root} lacks {', '.join(missing)} — (cd {root} && python {os.environ.get('SKINTOKENS_CHECKOUT', '<checkout>')}/download.py --model)")
    else:
        extras["weights"] = "ok"
    # The clone joins "models/Qwen3-0.6B" onto its own directory, so the
    # install links the two names into it; say so when the link is gone.
    checkout = os.environ.get("SKINTOKENS_CHECKOUT")
    if checkout:
        for name in ("experiments", "models"):
            if not os.path.isdir(os.path.join(checkout, name)):
                hints.append(f"{checkout}/{name} is missing — src/model/tokenrig.py resolves models/Qwen3-0.6B against the checkout; re-run install.sh to relink")
    return extras, hints


def main() -> int:
    checkout = checkout_dir()
    if os.path.isdir(os.path.join(checkout, "src")) and checkout not in sys.path:
        sys.path.insert(0, checkout)

    out: dict = {
        "python": ".".join(map(str, sys.version_info[:3])),
        "torch": None,
        "cuda_available": False,
        "torch_cuda": None,
        "imports": {},
        "extras": {},
        "notices": [],
        "hints": [],
    }
    failures: dict[str, str] = {}
    for name in IMPORTS:
        ok, why = _try_import(name)
        out["imports"][name] = ok
        if not ok and why:
            failures[name] = why

    if out["imports"].get("torch"):
        import torch

        out["torch"] = torch.__version__
        out["torch_cuda"] = torch.version.cuda
        try:
            out["cuda_available"] = bool(torch.cuda.is_available())
        except Exception:  # noqa: BLE001
            out["cuda_available"] = False

    for module, key in (("transformers", "transformers"), ("diffusers", "diffusers"), ("bpy", "bpy"), ("open3d", "open3d")):
        if out["imports"].get(module):
            mod = sys.modules.get(module) or importlib.import_module(module)
            version = getattr(mod, "__version__", None)
            if module == "bpy" and version is None:
                version = ".".join(str(part) for part in getattr(mod, "app", None).version) if getattr(mod, "app", None) else "?"
            out["extras"][key] = version or "?"

    # flash-attn is deliberately absent; say which attention the run will
    # use so a machine that happens to have the wheel is not a mystery.
    out["extras"]["attention"] = "flash-attn" if _try_import("flash_attn_interface")[0] or _try_import("flash_attn")[0] else "sdpa"

    extras, hints = patch_state(checkout)
    out["extras"].update(extras)
    out["hints"].extend(hints)

    extras, hints = weights_layout(os.environ.get("SKINTOKENS_WEIGHTS"))
    out["extras"].update(extras)
    out["hints"].extend(hints)

    for name, why in failures.items():
        out["hints"].append(f"{name} does not import: {why}")

    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
