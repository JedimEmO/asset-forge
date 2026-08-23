"""launcher.py: env var > .env link > MissingBackend, and the inner exec really runs the way the contract says."""

from __future__ import annotations

import os
import sys

import pytest

from forge_gen import backends, launcher
from forge_gen.exit_codes import BackendFailed, InputRejected, MissingBackend, MissingTool
from tests.conftest import make_prefix, write_stub_python


def test_missing_when_nothing_is_installed(backends_tree):
    backend = backends.load_backend("ardy")
    with pytest.raises(MissingBackend) as caught:
        launcher.resolve_interpreter(backend)
    assert caught.value.code == 3
    payload = caught.value.payload()
    assert payload["error"] == "missing_backend" and payload["backend"] == "ardy"
    assert "install.sh" in payload["hint"]


def test_env_link_resolves_and_a_dead_link_is_named(backends_tree, tmp_path):
    backend = backends.load_backend("ardy")
    prefix = make_prefix(tmp_path)
    os.symlink(prefix, backend.env_link)
    assert launcher.resolve_interpreter(backend) == backend.env_link / "bin" / "python"
    assert launcher.prefix_of(backend) == prefix.resolve()
    # A prefix with no bin/python is an install that did not finish.
    os.remove(backend.env_link)
    empty = tmp_path / "empty"
    empty.mkdir()
    os.symlink(empty, backend.env_link)
    with pytest.raises(MissingBackend, match="no bin/python"):
        launcher.resolve_interpreter(backend)


def test_env_var_wins_over_the_link(backends_tree, tmp_path, monkeypatch):
    backend = backends.load_backend("ardy")
    os.symlink(make_prefix(tmp_path / "linked"), backend.env_link)
    other = write_stub_python(tmp_path / "other" / "bin" / "python")
    monkeypatch.setenv("FORGE_BACKEND_ARDY_PYTHON", str(other))
    assert launcher.resolve_interpreter(backend) == other
    # The override may also be a prefix.
    monkeypatch.setenv("FORGE_BACKEND_ARDY_PYTHON", str(other.parent.parent))
    assert launcher.resolve_interpreter(backend) == other
    monkeypatch.setenv("FORGE_BACKEND_ARDY_PYTHON", str(tmp_path / "nowhere"))
    with pytest.raises(MissingBackend, match="FORGE_BACKEND_ARDY_PYTHON"):
        launcher.resolve_interpreter(backend)


def test_inner_env_expands_and_defers_to_the_user(installed_tree, monkeypatch):
    backend = backends.load_backend("ardy")
    monkeypatch.setenv("HF_HUB_OFFLINE", "0")
    monkeypatch.delenv("PYTHONPATH", raising=False)
    env = launcher.inner_env(backend)
    assert env["PYTHONPATH"] == str(launcher.python_dir())
    assert env["TEXT_ENCODERS_DIR"] == str(backend.text_encoders.resolve())
    assert env["ARDY_HOME"] == str(backend.checkout.resolve())
    assert env["HF_HUB_OFFLINE"] == "0", "the user's value wins"
    assert env["PYTHONNOUSERSITE"] == "1"
    assert env["FORGE_BACKEND"] == "ardy"
    monkeypatch.setenv("PYTHONPATH", "/x")
    assert launcher.inner_env(backend)["PYTHONPATH"].split(os.pathsep) == [str(launcher.python_dir()), "/x"]


def test_run_inner_streams_and_relays_the_code(installed_tree, monkeypatch, capfd):
    backend = backends.load_backend("ardy")
    monkeypatch.setenv("FORGE_TEST_VAR", "seen")
    code = launcher.run_inner(backend, "motion.session", ["--prompt", "walk"])
    out, err = capfd.readouterr()
    assert code == 0
    assert "stub: module=forge_gen.motion.session args=--inner --prompt walk" in out
    assert f"stub: cwd={backend.checkout.resolve()}" in out, "cwd = checkout runs from the clone"
    assert "FORGE_TEST_VAR=seen" in out
    assert '{"ok": true' in out, "run_inner streams everything through, the JSON line included"


def test_run_inner_checked_holds_the_json_line_and_relays_refusals(installed_tree, capfd):
    backend = backends.load_backend("ardy")
    result = launcher.run_inner_checked(backend, "motion.session", ["--x"])
    out, _ = capfd.readouterr()
    assert result == {"ok": True, "module": "forge_gen.motion.session"}
    assert '{"ok": true' not in out, "the JSON line is data for the outer, not a stray line"
    assert "stub: module=" in out, "progress lines still stream"

    with pytest.raises(InputRejected) as caught:
        launcher.run_inner_checked(backend, "motion.session", ["--exit-4"])
    assert caught.value.reason == "stub refused" and caught.value.code == 4

    with pytest.raises(BackendFailed) as caught:
        launcher.run_inner_checked(backend, "motion.session", ["--exit-7"])
    assert caught.value.code == 5
    assert any("something went wrong" in line for line in caught.value.log_tail)


def test_run_inner_without_checkout_is_missing(installed_tree):
    backend = backends.load_backend("ardy")
    os.remove(backend.checkout)
    with pytest.raises(MissingBackend, match="upstream checkout"):
        launcher.run_inner(backend, "motion.session", [])


def test_stream_timeout_kills(tmp_path):
    with pytest.raises(BackendFailed, match="did not finish"):
        launcher.stream([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.5)


def test_blender_resolution(monkeypatch, tmp_path):
    monkeypatch.setenv("BLENDER_BIN", str(tmp_path / "no-blender"))
    with pytest.raises(MissingTool) as caught:
        launcher.blender_bin()
    assert caught.value.code == 6 and caught.value.payload()["tool"] == "blender"
    monkeypatch.delenv("BLENDER_BIN")
    monkeypatch.setenv("PATH", str(tmp_path))
    with pytest.raises(MissingTool, match="not on PATH"):
        launcher.blender_bin()
    fake = tmp_path / "blender"
    fake.write_text("#!/bin/sh\necho 'Blender 4.2.0'\n")
    fake.chmod(0o755)
    assert launcher.blender_bin() == fake
    assert launcher.blender_version(fake) == (4, 2, 0)
    command = launcher.blender_command("/tmp/mod.py", ["--out", "x"], binary=fake)
    assert command[1:] == ["--background", "--factory-startup", "--python-exit-code", "5", "--python", "/tmp/mod.py", "--", "--out", "x"]


def test_run_blender_relays_the_script_exit_code(monkeypatch, tmp_path, capfd):
    fake = tmp_path / "blender"
    fake.write_text('#!/bin/sh\necho "Blender 4.2.0 (stub)"\necho \'{"ok": false, "error": "input_rejected", "reason": "not a T-pose"}\'\nexit 4\n')
    fake.chmod(0o755)
    monkeypatch.setenv("BLENDER_BIN", str(fake))
    assert launcher.run_blender(tmp_path / "mod.py", []) == 4
    with pytest.raises(InputRejected, match="not a T-pose"):
        launcher.run_blender_checked(tmp_path / "mod.py", [])
