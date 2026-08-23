#!/usr/bin/env python
"""Does the moss_sfx env hold what `forge gen sfx` will import? One JSON line.

Run by doctor under the backend's own interpreter, from the checkout (the
``moss_soundeffect_v2`` subdirectory), with the ``[env]`` of ``backend.toml``
exported. Never raises and always exits 0 with a JSON object as its last
line: a missing import is a ``false`` in ``imports`` and a line in ``hints``,
which doctor turns into ``partial``; only a probe that cannot speak at all is
``broken``.

The imports are exactly the inner module's: the pipeline class the package
exposes, ``soundfile`` (the writer — ``pipe.save_audio`` goes through
torchaudio → torchcodec, whose ffmpeg collides with the system glib here),
and torch. ``extras`` carries the versions whose pins are the reason this
backend has its own venv.
"""

from __future__ import annotations

import json
import os
import sys


def _version(dist: str) -> str | None:
    try:
        from importlib.metadata import version

        return version(dist)
    except Exception:  # noqa: BLE001 - a version is a nicety; the import is the test
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
        from moss_soundeffect_v2 import MossSoundEffectPipeline  # noqa: F401

        out["imports"]["moss_soundeffect_v2"] = True
    except Exception as err:  # noqa: BLE001
        out["imports"]["moss_soundeffect_v2"] = False
        out["hints"].append(
            f"moss_soundeffect_v2 does not import ({err.__class__.__name__}: {err}); "
            "the env needs `pip install -e <clone>/moss_soundeffect_v2[torch-cu128]` — bash backends/moss_sfx/install.sh"
        )

    try:
        import soundfile  # noqa: F401

        out["imports"]["soundfile"] = True
    except Exception as err:  # noqa: BLE001
        out["imports"]["soundfile"] = False
        out["hints"].append(f"soundfile does not import ({err}); it is the WAV writer — pip install soundfile into the env")

    for dist in ("transformers", "numpy", "diffusers", "huggingface_hub"):
        version = _version(dist)
        if version:
            out["extras"][dist] = version
    if os.environ.get("TORCHDYNAMO_DISABLE") != "1":
        out["notices"].append(
            "TORCHDYNAMO_DISABLE is not 1: the first call will torch.compile the DiT for minutes and lose the kernel with the process"
        )
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
