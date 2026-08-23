"""doctor --json against a fake backends tree: schema, statuses, and the gated-model hint."""

from __future__ import annotations

import json
import os
import subprocess
import sys


from forge_gen import doctor
from tests.conftest import PYTHON_DIR, make_prefix


def _schema_ok(report: dict) -> None:
    assert report["schema"] == 1
    for key in ("host", "blender", "ffmpeg", "backends", "backends_dir", "ok"):
        assert key in report
    for name, entry in report["backends"].items():
        assert entry["status"] in doctor.STATUSES, name
        for check in entry["checks"]:
            assert set(check) == {"name", "ok", "detail"}
            assert isinstance(check["ok"], bool)
        assert isinstance(entry["notices"], list) and isinstance(entry["hints"], list)


def test_described_but_not_installed_is_missing(backends_tree):
    report = doctor.diagnose(host=False)
    _schema_ok(report)
    assert list(report["backends"]) == ["trellis2", "ardy", "acestep", "moss_sfx", "moss_tts"]
    ardy = report["backends"]["ardy"]
    assert ardy["status"] == "missing"
    names = {check["name"]: check for check in ardy["checks"]}
    assert names["toml"]["ok"]
    assert not names["python"]["ok"] and "not installed" in names["python"]["detail"]
    assert any("install.sh" in hint for hint in ardy["hints"])
    assert ardy["notices"] == ["Llama 3: Built with Meta Llama 3"]
    assert report["exit_code"] == 1 and report["ok"] is False


def test_installed_with_a_stub_probe_is_partial_until_the_weights_arrive(installed_tree, tmp_path, monkeypatch):
    monkeypatch.setenv("HF_HUB_CACHE", str(tmp_path / "hub"))
    report = doctor.diagnose(only="ardy", host=False)
    _schema_ok(report)
    ardy = report["backends"]["ardy"]
    names = {check["name"]: check for check in ardy["checks"]}
    assert names["python"]["ok"] and names["python"]["detail"].startswith("3.12.7")
    assert names["probe"]["ok"] and "torch 2.13.0 cu12.8, cuda yes, imports 1/1" in names["probe"]["detail"]
    assert ardy["probe"]["imports"] == {"ardy": True}
    assert names["model:llama3-llm2vec-merged"]["ok"], "the text encoder is under .text-encoders"
    assert not names["model:nvidia/ARDY-Core-RP-20FPS-Horizon40"]["ok"]
    gated = names["model:facebook/dinov3-vitl16-pretrain-lvd1689m"]
    assert not gated["ok"] and "gated" in gated["detail"]
    assert any("huggingface.co/facebook/dinov3" in hint for hint in ardy["hints"])
    assert any(hint.startswith("hf auth login --token") for hint in ardy["hints"])
    assert ardy["status"] == "partial"

    # Drop the weights into the cache and the backend is ok — but the checkout is not a git repo, so broken.
    for model in ("nvidia--ARDY-Core-RP-20FPS-Horizon40", "facebook--dinov3-vitl16-pretrain-lvd1689m"):
        snap = tmp_path / "hub" / f"models--{model}" / "snapshots" / "abc"
        snap.mkdir(parents=True)
        (snap / "config.json").write_text("{}")
    report = doctor.diagnose(only="ardy", host=False)
    ardy = report["backends"]["ardy"]
    names = {check["name"]: check for check in ardy["checks"]}
    assert all(check["ok"] for name, check in names.items() if name.startswith("model:"))
    assert not names["checkout"]["ok"] and "not a git checkout" in names["checkout"]["detail"]
    assert ardy["status"] == "partial"


def test_checkout_at_another_commit_warns_and_dirty_is_noted(installed_tree, tmp_path, monkeypatch):
    checkout = (installed_tree / "ardy" / ".checkout").resolve()
    env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
    subprocess.run(["git", "init", "-q", str(checkout)], check=True, env=env)
    (checkout / "a").write_text("a")
    subprocess.run(["git", "-C", str(checkout), "add", "a"], check=True, env=env)
    subprocess.run(["git", "-C", str(checkout), "commit", "-q", "-m", "a"], check=True, env=env)
    (checkout / "a").write_text("b")
    monkeypatch.setenv("HF_HUB_CACHE", str(tmp_path / "hub"))
    report = doctor.diagnose(only="ardy", host=False)
    checks = {check["name"]: check for check in report["backends"]["ardy"]["checks"]}
    assert checks["checkout"]["ok"]
    assert checks["checkout"]["detail"].startswith("warn: HEAD")
    assert "dirty (1 tracked file modified)" in checks["checkout"]["detail"]
    assert any("git -C" in hint and "checkout 693f74d" in hint for hint in report["backends"]["ardy"]["hints"])


def test_no_probe_is_broken(installed_tree):
    os.remove(installed_tree / "ardy" / "probe.py")
    report = doctor.diagnose(only="ardy", host=False)
    ardy = report["backends"]["ardy"]
    assert ardy["status"] == "broken"
    assert {c["name"]: c["detail"] for c in ardy["checks"]}["probe"] == "no probe"


def test_probe_that_fails_is_broken_and_cuda_off_is_partial(installed_tree, tmp_path, monkeypatch):
    ardy = installed_tree / "ardy"
    os.remove(ardy / ".env")
    os.symlink(make_prefix(tmp_path / "nocuda", probe_json='{"torch": "2.13.0", "cuda_available": false, "imports": {"ardy": true, "peft": false}}'), ardy / ".env")
    report = doctor.diagnose(only="ardy", host=False)
    entry = report["backends"]["ardy"]
    names = {c["name"]: c for c in entry["checks"]}
    assert not names["cuda"]["ok"] and not names["import:peft"]["ok"]
    assert entry["status"] == "partial"


def test_bad_toml_is_broken_and_no_dir_exits_3(backends_tree, tmp_path, monkeypatch):
    (backends_tree / "ardy" / "backend.toml").write_text("name = [")
    report = doctor.diagnose(host=False)
    assert report["backends"]["ardy"]["status"] == "broken"
    monkeypatch.setenv("FORGE_BACKENDS", str(tmp_path / "absent"))
    report = doctor.diagnose(host=False)
    assert report["exit_code"] == 3 and "no backends directory" in report["error"]
    report = doctor.diagnose(only="gpt", root=backends_tree, host=False)
    assert report["exit_code"] == 2


def test_cli_json_is_the_last_line_and_the_table_is_readable(installed_tree, tmp_path, monkeypatch):
    monkeypatch.setenv("HF_HUB_CACHE", str(tmp_path / "hub"))
    env = dict(os.environ)
    done = subprocess.run(
        [sys.executable, str(PYTHON_DIR / "forge_gen"), "doctor", "--backend", "ardy", "--no-host", "--json"],
        capture_output=True, text=True, env=env, check=False,
    )
    assert done.returncode == 1, done.stderr
    report = json.loads(done.stdout.strip().splitlines()[-1])
    _schema_ok(report)
    assert "_text" not in report and "_exit" not in report
    done = subprocess.run(
        [sys.executable, str(PYTHON_DIR / "forge_gen"), "doctor", "--backend", "ardy", "--no-host"],
        capture_output=True, text=True, env=env, check=False,
    )
    assert done.returncode == 1
    assert "ardy       partial" in done.stdout
    assert "warn notice: Llama 3" in done.stdout
    assert "doctor: not every backend is ok (exit 1)" in done.stdout


def test_host_report_never_raises():
    """Whatever the host has, the report is a dict with the keys the Rust reads."""
    host = doctor.host_report()
    assert set(host) >= {"gpu", "conda", "python3"}
    assert isinstance(host["gpu"].get("busy"), bool)
    blender = doctor.blender_report()
    assert "ok" in blender
    assert "ok" in doctor.tool_report("ffmpeg", ["-version"])
