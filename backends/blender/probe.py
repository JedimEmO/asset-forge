#!/usr/bin/env python3
"""Is Blender where the launcher will look, and is it new enough? One JSON line.

    python3 backends/blender/probe.py

Runs under any python (stdlib only): Blender has no environment of its own,
so this probe is about the binary — ``$BLENDER_BIN`` or ``blender`` on
PATH — and what ``blender --version`` says. The last stdout line is the
JSON object doctor reads: ``{"tool": "blender", "ok": bool, "bin": ...,
"version": "5.2.0", "build_hash": ..., "min": "4.2", "imports": {},
"notices": [...], "hints": [...]}``. Exit 0 when Blender answers and meets
the floor, 1 when it is older, 6 (the ``missing_tool`` code) when it is not
there at all.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
TIMEOUT_S = 30.0


def floor() -> tuple[int, int]:
    """``blender_min`` from backend.toml beside this file."""
    try:
        with open(HERE / "backend.toml", "rb") as handle:
            text = str(tomllib.load(handle).get("blender_min", "4.2"))
    except (OSError, tomllib.TOMLDecodeError):
        text = "4.2"
    major, minor = (text.split(".") + ["0"])[:2]
    return int(major), int(minor)


def find_binary() -> tuple[str | None, str | None]:
    override = os.environ.get("BLENDER_BIN")
    if override:
        path = Path(override).expanduser()
        if path.is_file():
            return str(path), None
        return None, f"BLENDER_BIN={override} is not a file"
    found = shutil.which("blender")
    if found:
        return found, None
    return None, "blender is not on PATH and BLENDER_BIN is unset"


def main() -> int:
    result: dict = {
        "tool": "blender",
        "ok": False,
        "bin": None,
        "version": None,
        "build_hash": None,
        "min": ".".join(str(v) for v in floor()),
        "imports": {},
        "notices": [],
        "hints": [],
    }
    binary, why = find_binary()
    if binary is None:
        result["error"] = why
        result["hints"].append("install Blender >= %s and put it on PATH, or set BLENDER_BIN" % result["min"])
        print(json.dumps(result))
        return 6
    result["bin"] = binary
    try:
        done = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=TIMEOUT_S, check=False)
    except (OSError, subprocess.TimeoutExpired) as err:
        result["error"] = f"blender --version did not answer: {err}"
        print(json.dumps(result))
        return 6
    first = next((line for line in done.stdout.splitlines() if line.startswith("Blender ")), "")
    if not first:
        result["error"] = "blender --version printed no version line"
        print(json.dumps(result))
        return 6
    words = first.split()
    result["version"] = words[1]
    for line in done.stdout.splitlines():
        # "build hash: fbe6228777e7" on its own line in --version output.
        if line.strip().lower().startswith("build hash:"):
            result["build_hash"] = line.split(":", 1)[1].strip() or None
    try:
        version = tuple(int(v) for v in words[1].split(".")[:2])
    except ValueError:
        version = (0, 0)
    new_enough = version >= floor()
    result["ok"] = new_enough
    if not new_enough:
        result["error"] = f"Blender {words[1]} is older than {result['min']}"
        result["hints"].append(f"install Blender >= {result['min']}; the rig, export and prop modules are written against it")
    result["notices"].append("Blender: GPL-licensed tool; what it writes is the project's own")
    print(json.dumps(result))
    return 0 if new_enough else 1


if __name__ == "__main__":
    sys.exit(main())
