#!/usr/bin/env python3
"""Stage a verified browser distribution without local Forge installations."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

WEB = Path(__file__).resolve().parent
METADATA = WEB.parent / "web-metadata"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verified_inputs():
    receipt = json.loads((WEB / "asset-receipt.json").read_text())
    inventory = json.loads((METADATA / "asset-sha256.json").read_text())
    for relative, expected in receipt["files"].items():
        path = WEB / relative
        if path.is_symlink() or not path.is_file() or digest(path) != expected:
            raise ValueError(f"asset integrity mismatch: {relative}")
        if relative.startswith("assets/"):
            name = relative.removeprefix("assets/")
            if inventory.get(name) != expected:
                raise ValueError(f"browser inventory differs: {name}")
    actual = {
        p.relative_to(WEB).as_posix()
        for folder in ("assets", "licenses")
        for p in (WEB / folder).rglob("*")
        if p.is_file()
    }
    if actual != set(receipt["files"]):
        raise ValueError("asset receipt does not exactly describe assets and licenses")
    provenance = json.loads((METADATA / "PROVENANCE.json").read_text())
    for name in provenance["embedded_metadata"]:
        if digest(METADATA / name) != inventory[name]:
            raise ValueError(f"embedded metadata drift: {name}")
        if (METADATA / name).read_bytes() != (WEB / "assets" / name).read_bytes():
            raise ValueError(f"embedded and hosted metadata differ: {name}")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wasm-dir", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", action="store_true", help="verify committed assets only")
    args = parser.parse_args()
    receipt = verified_inputs()
    if args.check:
        print(f"Verified {len(receipt['files'])} browser asset/notice files and metadata")
        return
    if args.wasm_dir is None or args.output is None:
        parser.error("--wasm-dir and --output are required unless --check is used")
    if args.output.exists():
        parser.error(f"refusing existing output: {args.output}")
    # wasm-bindgen --target web --out-name relay_run produces these two files.
    runtime = ["relay_run.js", "relay_run_bg.wasm"]
    for name in runtime:
        if not (args.wasm_dir / name).is_file():
            parser.error(f"missing wasm-bindgen output: {name}")
    if not (WEB / "index.html").is_file():
        parser.error("missing web/index.html")
    args.output.mkdir(parents=True)
    for relative in receipt["files"]:
        dest = args.output / relative
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(WEB / relative, dest)
    for name in ["index.html", "NOTICES.md", "asset-receipt.json"]:
        shutil.copyfile(WEB / name, args.output / name)
    for name in runtime:
        shutil.copyfile(args.wasm_dir / name, args.output / name)
    # wasm-bindgen may emit JS snippets for inline bindings; those are runtime.
    snippets = args.wasm_dir / "snippets"
    if snippets.is_dir():
        shutil.copytree(snippets, args.output / "snippets")
    (args.output / ".nojekyll").write_text("")
    files = {
        p.relative_to(args.output).as_posix(): digest(p)
        for p in sorted(args.output.rglob("*"))
        if p.is_file()
    }
    (args.output / "game-delivery.json").write_text(json.dumps({
        "name": "Relay Run", "target": "WebGPU browser demo",
        "source_assets": receipt["source_package"],
        "source_game_delivery_sha256": receipt["source_game_delivery_sha256"],
        "files": files,
    }, indent=2) + "\n")
    print(f"Staged {len(files)} verified browser files at {args.output}")


if __name__ == "__main__":
    main()
