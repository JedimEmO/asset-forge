"""What doctor runs inside the ARDY env: can the inner modules import what they need, and is the text encoder laid out?

Run under ``backends/ardy/.env/bin/python`` with the ``[env]`` from
``backend.toml`` exported (``TEXT_ENCODERS_DIR`` above all). The last
stdout line is one JSON object in the shape ``forge_gen.doctor`` reads::

    {"torch": "2.13.0+cu130", "cuda_available": true, "torch_cuda": "13.0",
     "imports": {"ardy.model": true, ...}, "extras": {...},
     "notices": [...], "hints": [...]}

Every import is attempted, none is fatal: a missing ``matplotlib`` makes
the review lane fail and is reported as exactly that, not as a broken
backend. Exit 0 whenever the JSON line could be written.
"""

from __future__ import annotations

import importlib
import json
import os
import sys

#: What the inner modules import, by lane. ``ardy.model`` is the sweep and
#: keys lane (it carries ``load_model``); ``peft`` is the text encoder;
#: ``matplotlib`` the review sheets.
IMPORTS = (
    "torch",
    "numpy",
    "ardy",
    "ardy.model",
    "ardy.model.registry",
    "ardy.constraints",
    "ardy.postprocess",
    "ardy.skeleton.registry",
    "peft",
    "transformers",
    "matplotlib",
)

#: The two directories ARDY joins onto ``TEXT_ENCODERS_DIR`` (see
#: ``ardy/model/load_model.py`` TEXT_ENCODER_PRESETS), and what each must hold.
MERGED = "McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp"
ADAPTER = "McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp-supervised"
MERGED_FILES = ("config.json", "tokenizer.json", "modeling_llama_encoder.py")
ADAPTER_FILES = ("adapter_config.json", "adapter_model.safetensors")


def _try_import(name: str) -> tuple[bool, str | None]:
    try:
        importlib.import_module(name)
        return True, None
    except Exception as err:  # noqa: BLE001 - any failure is the answer
        return False, f"{err.__class__.__name__}: {err}"


def _has_weights(directory: str) -> bool:
    """A full model: one ``model.safetensors`` or a sharded index."""
    return os.path.isfile(os.path.join(directory, "model.safetensors")) or os.path.isfile(
        os.path.join(directory, "model.safetensors.index.json")
    )


def text_encoder_layout(root: str | None) -> tuple[dict, list[str]]:
    """Check the layout ``assemble_text_encoder.py`` writes; ``(extras, hints)``."""
    extras: dict = {}
    hints: list[str] = []
    if not root:
        extras["text_encoders"] = "unset"
        hints.append("TEXT_ENCODERS_DIR is unset: no backends/ardy/.text-encoders link — bash backends/ardy/install.sh (or --adopt-text-encoders DIR)")
        return extras, hints
    if not os.path.isdir(root):
        extras["text_encoders"] = "absent"
        hints.append(f"TEXT_ENCODERS_DIR={root} is not a directory")
        return extras, hints
    merged = os.path.join(root, MERGED)
    adapter = os.path.join(root, ADAPTER)
    missing = [f for f in MERGED_FILES if not os.path.isfile(os.path.join(merged, f))]
    if not _has_weights(merged):
        missing.append("model.safetensors")
    missing_adapter = [f for f in ADAPTER_FILES if not os.path.isfile(os.path.join(adapter, f))]
    if missing or missing_adapter:
        extras["text_encoders"] = "incomplete"
        if missing:
            hints.append(f"{merged} lacks {', '.join(missing)} — the MNTP merge did not finish: python backends/ardy/assemble_text_encoder.py --dest {root}")
        if missing_adapter:
            hints.append(f"{adapter} lacks {', '.join(missing_adapter)}")
    else:
        extras["text_encoders"] = "ok"
        # The adapter's base path must name the merged model, or a PEFT
        # load that trusts it would pull 15 GB from the hub.
        try:
            with open(os.path.join(adapter, "adapter_config.json"), encoding="utf-8") as handle:
                base = json.load(handle).get("base_model_name_or_path")
            if not (isinstance(base, str) and os.path.isdir(base)):
                hints.append(f"{adapter}/adapter_config.json base_model_name_or_path={base!r} is not a local directory; assemble_text_encoder.py rewrites it")
        except (OSError, json.JSONDecodeError) as err:
            hints.append(f"{adapter}/adapter_config.json does not read: {err}")
    return extras, hints


def main() -> int:
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
    for module, key in (("transformers", "transformers"), ("numpy", "numpy"), ("peft", "peft"), ("matplotlib", "matplotlib")):
        if out["imports"].get(module):
            mod = sys.modules.get(module) or importlib.import_module(module)
            out["extras"][key] = getattr(mod, "__version__", "?")
    if out["imports"].get("ardy.model"):
        import ardy.model as ardy_model

        out["extras"]["load_model"] = hasattr(ardy_model, "load_model")
        if not hasattr(ardy_model, "load_model"):
            out["hints"].append("ardy.model has no load_model — the checkout is not the pinned commit")
    for name, why in failures.items():
        out["hints"].append(f"{name} does not import: {why}")
    extras, hints = text_encoder_layout(os.environ.get("TEXT_ENCODERS_DIR"))
    out["extras"].update(extras)
    out["hints"].extend(hints)
    sys.stdout.write(json.dumps(out) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
