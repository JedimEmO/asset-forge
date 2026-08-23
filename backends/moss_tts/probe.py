#!/usr/bin/env python
"""Does the moss_tts env hold what `forge gen speech` will import? One JSON line.

Run by doctor under the backend's own interpreter with ``backend.toml``'s
``[env]`` exported. Never raises and always exits 0 with a JSON object as
its last line: a failed import is ``false`` in ``imports`` plus a hint, which
doctor reports as ``partial``.

The imports are the inner module's: ``transformers.AutoModel`` and
``AutoProcessor`` (the model code itself arrives with the weights through
``trust_remote_code``, so nothing of the clone is imported), ``soundfile``
(the writer; torchaudio's save goes through torchcodec, whose ffmpeg
collides with the system glib here) and torch. ``extras`` says which
attention implementation the inner module will pick — flash-attn when it is
installed and the card is Ampere or newer, else SDPA — so a slow run has an
explanation in the doctor table.
"""

from __future__ import annotations

import importlib.util
import json
import sys


def _version(dist: str) -> str | None:
    try:
        from importlib.metadata import version

        return version(dist)
    except Exception:  # noqa: BLE001
        return None


def main() -> int:
    out: dict = {
        "torch": None,
        "cuda_available": False,
        "torch_cuda": None,
        "imports": {},
        "extras": {},
        "notices": [],
        "hints": [],
    }
    torch = None
    try:
        import torch

        out["torch"] = torch.__version__
        out["torch_cuda"] = torch.version.cuda
        out["cuda_available"] = bool(torch.cuda.is_available())
        out["imports"]["torch"] = True
        if out["cuda_available"]:
            out["extras"]["gpu"] = torch.cuda.get_device_name(0)
    except Exception as err:  # noqa: BLE001
        out["imports"]["torch"] = False
        out["hints"].append(f"torch does not import: {err.__class__.__name__}: {err}")

    try:
        from transformers import AutoModel, AutoProcessor  # noqa: F401

        out["imports"]["transformers"] = True
    except Exception as err:  # noqa: BLE001
        out["imports"]["transformers"] = False
        out["hints"].append(
            f"transformers does not import ({err.__class__.__name__}: {err}); "
            "the env needs `pip install -e <clone>[torch-runtime]` — bash backends/moss_tts/install.sh"
        )

    try:
        import soundfile  # noqa: F401

        out["imports"]["soundfile"] = True
    except Exception as err:  # noqa: BLE001
        out["imports"]["soundfile"] = False
        out["hints"].append(f"soundfile does not import ({err}); it is the WAV writer — pip install soundfile into the env")

    for dist in ("transformers", "numpy", "huggingface_hub"):
        version = _version(dist)
        if version:
            out["extras"][dist] = version

    # The same choice the inner module makes, reported so a slow line has a reason.
    attn = "eager"
    if torch is not None and out["cuda_available"]:
        attn = "sdpa"
        try:
            if importlib.util.find_spec("flash_attn") is not None and torch.cuda.get_device_capability()[0] >= 8:
                attn = "flash_attention_2"
        except Exception:  # noqa: BLE001
            pass
    out["extras"]["attn"] = attn
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
