"""Exercise the staged runtime from outside the checkout, with no toolkit override."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/debug/forge"


@pytest.mark.skipif(not BINARY.is_file(), reason="build forge before testing the distribution")
def test_unpacked_toolkit_initializes_and_generates_for_two_games(tmp_path):
    install = tmp_path / "shared toolkit"
    subprocess.run([
        "python3", str(ROOT / "release/package.py"), "--binary", str(BINARY),
        "--output", str(install),
    ], check=True, capture_output=True, text=True)
    manifest = json.loads((install / "distribution.json").read_text())
    assert (install / "rigs/humanoid/profile.toml").is_file()
    assert (install / "backends/trellis2/install_runtime.py").is_file()
    assert not (install / "forge.toml").exists()
    assert not (install / "assets").exists()
    assert not any("installed.json" in name or ".env" in name or ".toolchain" in name or "__pycache__" in name
                   for name in manifest["files"])
    for name, expected in manifest["files"].items():
        with (install / name).open("rb") as stream:
            assert hashlib.file_digest(stream, "sha256").hexdigest() == expected
    executable = install / "bin/forge"
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("FORGE_") and key != "PYTHONPATH"}
    games = [tmp_path / "game one", tmp_path / "game two"]

    def run(game, *args):
        result = subprocess.run([str(executable), "--project", str(game), *args],
                                cwd=tmp_path, env=env, capture_output=True, text=True,
                                timeout=60)
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    # The full guide travels inside the binary, before any game is initialized.
    guide = run(games[0], "guide")
    version = manifest["binary_version"].removeprefix("forge ")
    assert guide == (f"Toolkit version: {version}\n\n"
                     + (ROOT / "crates/forge_mcp/guides/workflow.md").read_text())
    assert not games[0].exists()

    for game in games:
        run(game, "init", "--name", game.name, "--tier", "fake", "--make", "none", "--yes")
        config = json.loads((game / ".forge/mcp.json").read_text())
        server = config["mcpServers"]["asset-forge"]
        assert server["command"] == str(executable)
        assert server["env"]["FORGE_HOME"] == str(install)
        run(game, "doctor", "--quick")
        run(game, "gen", "sfx", "--prompt", "a short impact", "--out", "out/audio/impact.wav")
        assert (game / "out/audio/impact.wav").is_file()
        run(game, "verify")
        run(game, "manifest", "--check")
    # Same output names are separate files, and no sample library was created.
    assert not (install / "out").exists()
    assert not (install / "assets").exists()
    refused = subprocess.run([
        "python3", str(ROOT / "release/package.py"), "--binary", str(BINARY),
        "--output", str(install),
    ], capture_output=True, text=True)
    assert refused.returncode != 0
    assert "destination already exists" in refused.stderr
