#!/usr/bin/env python
"""Fetch the ACE-Step checkpoints the backend needs into one directory.

    <env python> backends/acestep/fetch_models.py --dir DIR [--all]

The minimal set (~7.3 GB) is four subdirectories the server finds by name
under ``ACESTEP_CHECKPOINTS_DIR``: ``acestep-v15-turbo``, ``vae`` and
``Qwen3-Embedding-0.6B`` from the main repo ``ACE-Step/Ace-Step1.5``, and
the planner ``acestep-5Hz-lm-0.6B`` from its own repo. ``--all`` adds the
two XL DiTs (``acestep-v15-xl-base``, ``acestep-v15-xl-sft``, ~19 GB each),
off by default because nothing in the toolkit asks for them.

Why not upstream's ``acestep-download``: its "main model" is the whole
``Ace-Step1.5`` repo, which carries the 1.7B planner (3.5 GB) that the
server would then prefer over the 0.6B the toolkit pins. This fetches only
the named subdirectories through ``huggingface_hub.snapshot_download`` with
``allow_patterns`` — the env's own hub client, so the token and cache the
user already set up apply — and then runs upstream's own code sync
(``acestep.model_downloader._sync_model_code_files``) so the model ``.py``
files beside the weights match the checkout, exactly as upstream's
downloader would have left them.

Idempotent: a subdirectory that already holds a weights file is skipped;
``--force`` re-fetches. Runs under the backend's python, never the system
one; the hub client is imported inside ``main`` so ``--help`` works without
it.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

#: The main repo and the subdirectories of it the server needs.
MAIN_REPO = "ACE-Step/Ace-Step1.5"
MAIN_SUBDIRS = ("acestep-v15-turbo", "vae", "Qwen3-Embedding-0.6B")

#: Sub-models with a repo each: name → repo id. The names are the directory
#: names the server looks up, and mirror upstream's SUBMODEL_REGISTRY.
SUBMODELS = {
    "acestep-5Hz-lm-0.6B": "ACE-Step/acestep-5Hz-lm-0.6B",
    "acestep-v15-xl-base": "ACE-Step/acestep-v15-xl-base",
    "acestep-v15-xl-sft": "ACE-Step/acestep-v15-xl-sft",
}

#: What --all adds on top of the minimal set.
OPTIONAL = ("acestep-v15-xl-base", "acestep-v15-xl-sft")

WEIGHT_SUFFIXES = (".safetensors", ".bin", ".pt", ".pth", ".ckpt")


def has_weights(directory: Path) -> bool:
    """Whether a checkpoint directory holds at least one non-empty weights file."""
    if not directory.is_dir():
        return False
    try:
        return any(entry.suffix in WEIGHT_SUFFIXES and entry.stat().st_size > 0 for entry in directory.iterdir())
    except OSError:
        return False


def sync_code(name: str, root: Path) -> list[str]:
    """Upstream's code sync for one checkpoint, when the package is importable; else nothing."""
    try:
        from acestep.model_downloader import _sync_model_code_files
    except Exception:  # noqa: BLE001 - fetching weights must not depend on the package importing
        return []
    try:
        return list(_sync_model_code_files(name, root) or [])
    except Exception as err:  # noqa: BLE001
        print(f"fetch_models: code sync for {name} failed: {err}", file=sys.stderr)
        return []


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dir", required=True, metavar="DIR", help="the checkpoints directory (ACESTEP_CHECKPOINTS_DIR)")
    parser.add_argument("--all", action="store_true", help="also fetch the XL DiTs (~38 GB more)")
    parser.add_argument("--force", action="store_true", help="re-fetch subdirectories that already hold weights")
    args = parser.parse_args(argv)

    try:
        from huggingface_hub import snapshot_download
    except ImportError as err:
        print(f"fetch_models: huggingface_hub is not importable here ({err}); run me with the backend's python", file=sys.stderr)
        return 2

    # The code sync imports the upstream package; when the env's editable
    # install points at a moved clone it resolves only through the working
    # directory, which install.sh makes the checkout.
    if os.getcwd() not in sys.path:
        sys.path.insert(0, os.getcwd())

    root = Path(args.dir).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    fetched: list[str] = []

    main_needed = [name for name in MAIN_SUBDIRS if args.force or not has_weights(root / name)]
    if main_needed:
        print(f"fetch_models: {MAIN_REPO} -> {root} ({', '.join(main_needed)})", file=sys.stderr)
        snapshot_download(
            MAIN_REPO,
            local_dir=str(root),
            allow_patterns=[f"{name}/*" for name in main_needed],
        )
        fetched.extend(main_needed)
    else:
        print(f"fetch_models: main set present under {root}", file=sys.stderr)

    wanted = ["acestep-5Hz-lm-0.6B"] + (list(OPTIONAL) if args.all else [])
    for name in wanted:
        target = root / name
        if not args.force and has_weights(target):
            print(f"fetch_models: {name} present", file=sys.stderr)
            continue
        print(f"fetch_models: {SUBMODELS[name]} -> {target}", file=sys.stderr)
        snapshot_download(SUBMODELS[name], local_dir=str(target))
        fetched.append(name)

    # Only what this run fetched: a re-run over an adopted directory must
    # not rewrite files it did not put there.
    for name in fetched:
        synced = sync_code(name, root)
        if synced:
            print(f"fetch_models: synced model code for {name}: {', '.join(synced)}", file=sys.stderr)

    missing = [name for name in list(MAIN_SUBDIRS) + wanted if not has_weights(root / name)]
    if missing:
        print(f"fetch_models: still missing after download: {', '.join(missing)}", file=sys.stderr)
        return 1
    print(f"fetch_models: ok — {', '.join(list(MAIN_SUBDIRS) + wanted)} under {root}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
