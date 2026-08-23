"""Shared fixtures: the package on sys.path, a scratch backends tree, a stub interpreter."""

from __future__ import annotations

import os
import stat
import sys
import textwrap
from pathlib import Path

import pytest

PYTHON_DIR = Path(__file__).resolve().parent.parent
if str(PYTHON_DIR) not in sys.path:
    sys.path.insert(0, str(PYTHON_DIR))

REPO = PYTHON_DIR.parent


@pytest.fixture
def repo_root() -> Path:
    """The toolkit checkout."""
    return REPO


@pytest.fixture(autouse=True)
def _forget_project():
    """records.set_project is process-global; a test that sets it must not leak into the next."""
    yield
    from forge_gen import records

    records.set_project(None)


#: A backend.toml that parses, with one model per store and one notice.
GOOD_TOML = textwrap.dedent(
    """
    name = "ardy"
    role = "motion"
    upstream = "https://github.com/nv-tlabs/ardy"
    commit = "693f74d13b3d04a0a22ce127ee79c929dd89756b"
    license = "Apache-2.0"
    env_kind = "venv"
    python = "3.12"
    cuda = "12.8"
    vram_gb = 16
    entry = "motion.session"
    cwd = "checkout"
    resident = false

    [env]
    PYTHONNOUSERSITE = "1"
    TEXT_ENCODERS_DIR = "${TEXT_ENCODERS}"
    ARDY_HOME = "${CHECKOUT}"
    HF_HUB_OFFLINE = "1"

    [[models]]
    id = "nvidia/ARDY-Core-RP-20FPS-Horizon40"
    store = "hf"
    license = "NVIDIA Open Model License"

    [[models]]
    id = "llama3-llm2vec-merged"
    store = "text_encoders_dir"
    license = "Llama 3 Community License"

    [[models]]
    id = "facebook/dinov3-vitl16-pretrain-lvd1689m"
    store = "hf"
    gated = true
    accept_url = "https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m"

    [[notices]]
    title = "Llama 3"
    text = "Built with Meta Llama 3"
    """
)


def write_stub_python(path: Path, *, version: str = "3.12.7", probe_json: str | None = None) -> Path:
    """A shell script that answers like an interpreter for what doctor and the launcher ask.

    ``-c`` with a version expression prints ``version``; ``-m forge_gen.x
    --inner`` echoes its argv as progress and a JSON line; a ``probe.py``
    argument prints ``probe_json``.
    """
    probe_json = probe_json or '{"torch": "2.13.0", "cuda_available": true, "torch_cuda": "12.8", "imports": {"ardy": true}, "extras": {}}'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        textwrap.dedent(
            f"""\
            #!/bin/sh
            # A stand-in interpreter: answers the three questions the launcher and doctor ask.
            case "$1" in
              -c) echo "{version}"; exit 0 ;;
              -m)
                shift; module="$1"; shift
                echo "stub: module=$module args=$*"
                echo "stub: PYTHONPATH=$PYTHONPATH"
                echo "stub: cwd=$(pwd)"
                echo "stub: FORGE_TEST_VAR=$FORGE_TEST_VAR TEXT_ENCODERS_DIR=$TEXT_ENCODERS_DIR"
                case "$*" in
                  *--exit-4*) echo '{{"ok": false, "error": "input_rejected", "reason": "stub refused"}}'; exit 4 ;;
                  *--exit-7*) echo "something went wrong" >&2; exit 7 ;;
                esac
                echo '{{"ok": true, "module": "'"$module"'"}}'
                exit 0 ;;
              *probe.py) echo "probe: warming up"; echo '{probe_json}'; exit 0 ;;
            esac
            echo "stub python: unexpected argv $*" >&2
            exit 1
            """
        )
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return path


def make_prefix(root: Path, **kwargs) -> Path:
    """A fake env prefix with ``bin/python`` as the stub."""
    prefix = root / "venv"
    write_stub_python(prefix / "bin" / "python", **kwargs)
    return prefix


@pytest.fixture
def backends_tree(tmp_path, monkeypatch) -> Path:
    """A backends dir with one described backend (ardy), not installed; ``FORGE_BACKENDS`` points at it."""
    root = tmp_path / "backends"
    ardy = root / "ardy"
    ardy.mkdir(parents=True)
    (ardy / "backend.toml").write_text(GOOD_TOML)
    monkeypatch.setenv("FORGE_BACKENDS", str(root))
    for name in ("FORGE_BACKEND_ARDY_PYTHON", "FORGE_FAKE", "HF_HUB_CACHE", "HF_HOME", "HF_TOKEN"):
        monkeypatch.delenv(name, raising=False)
    return root


@pytest.fixture
def installed_tree(backends_tree, tmp_path) -> Path:
    """The same tree with ardy installed: ``.env`` → a stub prefix, ``.checkout`` → a git repo at the pinned commit, a probe."""
    ardy = backends_tree / "ardy"
    prefix = make_prefix(tmp_path)
    os.symlink(prefix, ardy / ".env")
    checkout = tmp_path / "checkout"
    checkout.mkdir()
    os.symlink(checkout, ardy / ".checkout")
    encoders = tmp_path / "text-encoders" / "llama3-llm2vec-merged"
    encoders.mkdir(parents=True)
    (encoders / "config.json").write_text("{}")
    os.symlink(encoders.parent, ardy / ".text-encoders")
    (ardy / "probe.py").write_text("print('never run: the stub answers for me')\n")
    return backends_tree
