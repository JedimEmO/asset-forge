#!/usr/bin/env python3
"""Build a local toolkit directory from an explicit runtime-file allowlist.

This makes a testable candidate, not a claim that backend quality or release
acceptance passed. No weights, backend receipts, game assets or credentials
are copied. Existing destinations are refused.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = (
    "release/consumer/*.py", "release/consumer/*.md", "release/consumer/Cargo.toml", "release/consumer/Cargo.lock", "release/consumer/src/*.rs",
    "designs/consumer-contract.md",
    "LICENSE-MIT", "LICENSE-APACHE", "release/README.md",
    "python/pyproject.toml", "python/README.md", "python/forge_gen/**/*.py",
    "rigs/humanoid/*.toml", "rigs/humanoid/*.json", "rigs/humanoid/*.glb", "rigs/humanoid/*.blend",
    "rigs/humanoid/README.md", "rigs/humanoid/fixture/*.json",
    "rigs/humanoid/fixture/*.glb",
    "backends/README.md", "backends/_lib/*.sh", "backends/_lib/*.py",
    "backends/*/backend.toml", "backends/*/install.sh", "backends/*/probe.py",
    "backends/*/patches/*.patch", "backends/*/workflows/*.api.json",
    "backends/trellis2/install_runtime.py",
    "backends/ardy/assemble_text_encoder.py", "backends/acestep/fetch_models.py",
    "backends/comfy/snapshot.json", "backends/comfy/extra_model_paths.yaml",
    "backends/comfy/forge-comfy.service", "backends/comfy/README.md",
)


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def package(binary: Path, output: Path) -> Path:
    """Stage a candidate next to its final destination; never replace an install."""
    binary = binary.resolve(strict=True)
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise FileExistsError(f"destination already exists: {output}")
    version = subprocess.check_output([str(binary), "--version"], text=True).strip()
    output.parent.mkdir(parents=True, exist_ok=True)
    files = sorted({path for pattern in PATTERNS for path in ROOT.glob(pattern)})
    with tempfile.TemporaryDirectory(prefix=".forge-package-", dir=output.parent) as temp:
        stage = Path(temp) / "toolkit"
        stage.mkdir()
        for source in files:
            if source.is_symlink() or not source.is_file():
                raise ValueError(f"runtime input must be a regular file: {source}")
            relative = source.relative_to(ROOT)
            target = stage / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        (stage / "bin").mkdir()
        shutil.copy2(binary, stage / "bin/forge")
        manifest = {
            "format": 1,
            "binary_version": version,
            "status": "development candidate; release gates not yet passed",
            "files": {
                path.relative_to(stage).as_posix(): digest(path)
                for path in sorted(stage.rglob("*")) if path.is_file()
            },
        }
        (stage / "distribution.json").write_text(json.dumps(manifest, indent=2) + "\n")
        # Fail closed if a destination appeared while staging.
        output.mkdir()
        try:
            for child in stage.iterdir():
                child.rename(output / child.name)
        except BaseException:
            shutil.rmtree(output)
            raise
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/forge")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(package(args.binary, args.output))


if __name__ == "__main__":
    main()
