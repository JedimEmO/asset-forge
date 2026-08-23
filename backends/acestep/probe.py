#!/usr/bin/env python
"""What doctor runs inside the ACE-Step env: does the server import, and are the weights there?

    <env python> backends/acestep/probe.py

Prints progress to stderr and one JSON line to stdout::

    {"torch": "2.10.0+cu128", "torch_cuda": "12.8", "cuda_available": true,
     "imports": {"acestep.api_server": true, "soundfile": true, "torch": true},
     "extras": {"transformers": "4.57.6", "checkpoints": "/…", "lm": "acestep-5Hz-lm-0.6B"},
     "models": {"acestep-v15-turbo": true, …}, "notices": [], "hints": []}

Imports are attempted one by one so a broken one names itself rather than
taking the probe down; ``acestep.api_server`` is the expensive one (it pulls
in torch, transformers and the FastAPI tree) and is what the music command
actually starts, so if it imports here it starts there. The checkpoint
directories are read from ``ACESTEP_CHECKPOINTS_DIR`` — the launcher exports
it from ``backend.toml`` — and listed by name; doctor's own ``[[models]]``
check is the one that decides, this is the view from inside the env.

Doctor runs this from the upstream checkout (``cwd = "checkout"``), and the
working directory goes on ``sys.path`` first — the same view ``python -m
acestep.api_server`` gets when the music command starts the server. That
matters for an adopted venv whose editable install points at a path the
clone has since moved away from: the server still starts because every
launch stands in the checkout, and the probe must see what the server sees.
``extras.acestep`` says which tree answered.

Runs under the backend's python, never the system one. Exit 0 when the JSON
line was printed, whatever it says; exit 1 only when even that failed.
"""

from __future__ import annotations

import importlib
import json
import os
import sys
from pathlib import Path

#: The subdirectories the minimal install guarantees; mirrors backend.toml's [[models]].
REQUIRED = ("acestep-v15-turbo", "acestep-5Hz-lm-0.6B", "Qwen3-Embedding-0.6B", "vae")

#: The optional ones --all-models fetches; listed so doctor's extras show what else is there.
OPTIONAL = ("acestep-v15-xl-base", "acestep-v15-xl-sft", "acestep-5Hz-lm-1.7B", "acestep-5Hz-lm-4B")

#: What the music command needs importable, in the order the failures are most informative.
IMPORTS = ("torch", "soundfile", "transformers", "acestep.api_server")


def _try_import(name: str, imports: dict, hints: list[str]) -> object | None:
    try:
        module = importlib.import_module(name)
    except Exception as err:  # noqa: BLE001 - the probe reports, it does not crash
        imports[name] = False
        hints.append(f"{name} does not import: {err.__class__.__name__}: {str(err).splitlines()[0] if str(err) else ''}")
        print(f"probe: {name}: {err.__class__.__name__}: {err}", file=sys.stderr)
        return None
    imports[name] = True
    return module


def _has_weights(directory: Path) -> bool:
    """A checkpoint directory is present when it holds at least one weights file."""
    if not directory.is_dir():
        return False
    try:
        return any(
            entry.suffix in (".safetensors", ".bin", ".pt", ".pth", ".ckpt") and entry.stat().st_size > 0
            for entry in directory.iterdir()
        )
    except OSError:
        return False


def main() -> int:
    out: dict = {
        "torch": None,
        "torch_cuda": None,
        "cuda_available": False,
        "imports": {},
        "extras": {},
        "models": {},
        "notices": [],
        "hints": [],
    }
    imports: dict = out["imports"]
    hints: list[str] = out["hints"]
    extras: dict = out["extras"]

    torch = _try_import("torch", imports, hints)
    if torch is not None:
        out["torch"] = getattr(torch, "__version__", None)
        try:
            out["torch_cuda"] = getattr(getattr(torch, "version", None), "cuda", None)
            out["cuda_available"] = bool(torch.cuda.is_available())
            if out["cuda_available"]:
                extras["gpu"] = torch.cuda.get_device_name(0)
        except Exception as err:  # noqa: BLE001
            hints.append(f"torch.cuda did not answer: {err}")

    soundfile = _try_import("soundfile", imports, hints)
    if soundfile is not None:
        extras["soundfile"] = getattr(soundfile, "__version__", "?")
    else:
        hints.append("the soundfile patch routes every WAV save through soundfile; without it the server cannot write audio")

    transformers = _try_import("transformers", imports, hints)
    if transformers is not None:
        extras["transformers"] = getattr(transformers, "__version__", "?")

    # The server module is what `forge gen music` starts. Importing it is the
    # proof the env is whole: torch, the FastAPI tree, loguru, the handlers.
    # It prints a torchao warning on this torch; that is noise, not a fault.
    cwd = os.getcwd()
    if cwd not in sys.path:
        sys.path.insert(0, cwd)
    server = _try_import("acestep.api_server", imports, hints)
    if server is not None:
        package = sys.modules.get("acestep")
        location = getattr(package, "__file__", None)
        extras["acestep"] = str(Path(location).resolve().parent) if location else "namespace (no __init__)"
        if location and Path(location).resolve().parent.parent == Path(cwd).resolve():
            # Resolved through the working directory, not the env: an editable
            # install pointing elsewhere (a clone that moved). The server
            # starts all the same; the install receipt is the place to fix it.
            try:
                import importlib.metadata as metadata

                origin = json.loads(metadata.distribution("ace-step").read_text("direct_url.json") or "{}").get("url", "")
            except Exception:  # noqa: BLE001
                origin = ""
            if origin.startswith("file://") and not Path(origin[7:]).is_dir():
                hints.append(f"the env's editable ace-step points at {origin[7:]} (gone); imports work only from the checkout — re-run install.sh, or pip install -e the clone")

    checkpoints = os.environ.get("ACESTEP_CHECKPOINTS_DIR")
    if checkpoints:
        root = Path(checkpoints).expanduser()
        extras["checkpoints"] = str(root)
        for name in REQUIRED:
            out["models"][name] = _has_weights(root / name)
        for name in OPTIONAL:
            if (root / name).is_dir():
                out["models"][name] = _has_weights(root / name)
        missing = [name for name in REQUIRED if not out["models"].get(name)]
        if missing:
            hints.append(
                "checkpoints missing under "
                + str(root)
                + ": "
                + ", ".join(missing)
                + " — bash backends/acestep/install.sh (or --adopt-checkpoints DIR)"
            )
    else:
        hints.append("ACESTEP_CHECKPOINTS_DIR is not set: the launcher exports it from backend.toml, so this probe was run by hand")

    lm = os.environ.get("ACESTEP_LM_MODEL_PATH")
    if lm:
        extras["lm"] = lm

    print(json.dumps(out, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as err:  # noqa: BLE001 - still say something parseable
        print(json.dumps({"torch": None, "cuda_available": False, "imports": {}, "hints": [f"probe crashed: {err.__class__.__name__}: {err}"]}))
        sys.exit(1)
