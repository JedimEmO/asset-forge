"""doctor --json against a fake backends tree: schema, the five statuses, the
gated-model hint, and the comfy ladder against a service that answers."""

from __future__ import annotations

import contextlib
import http.server
import json
import os
import subprocess
import sys
import threading
import types

import pytest

from forge_gen import doctor
from tests.conftest import PYTHON_DIR, make_prefix


def _schema_ok(report: dict) -> None:
    assert report["schema"] == 1
    for key in ("host", "blender", "ffmpeg", "backends", "backends_dir", "ok"):
        assert key in report
    for name, entry in report["backends"].items():
        assert entry["status"] in doctor.STATUSES, name
        assert isinstance(entry["chosen"], bool), name
        assert entry["executor"] in ("env", "comfy", "tool", None), name
        for check in entry["checks"]:
            assert set(check) == {"name", "ok", "detail"}
            assert isinstance(check["ok"], bool)
        assert isinstance(entry["notices"], list) and isinstance(entry["hints"], list)


def test_described_but_not_installed_is_missing(backends_tree):
    report = doctor.diagnose(host=False)
    _schema_ok(report)
    assert list(report["backends"]) == ["trellis2", "ardy", "acestep", "moss_sfx", "moss_tts", "moss_speech"]
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


def test_ambient_shadow_is_a_warn_row_and_hints_are_deduplicated(installed_tree, tmp_path, monkeypatch):
    monkeypatch.setenv("HF_HUB_CACHE", str(tmp_path / "hub"))
    monkeypatch.setenv("HF_HUB_OFFLINE", "0")
    report = doctor.diagnose(only="ardy", host=False)
    ardy = report["backends"]["ardy"]
    names = {check["name"]: check for check in ardy["checks"]}
    row = names["env:HF_HUB_OFFLINE"]
    assert row["ok"], "a shadow warns; it does not fail the backend"
    assert row["detail"].startswith("warn:") and "HF_HUB_OFFLINE=0" in row["detail"] and "shadows" in row["detail"]
    assert len(ardy["hints"]) == len(set(ardy["hints"])), "one hint each, not once per FAIL"
    monkeypatch.setenv("HF_HUB_OFFLINE", "1")
    report = doctor.diagnose(only="ardy", host=False)
    checks = report["backends"]["ardy"]["checks"]
    assert not any(c["name"] == "env:HF_HUB_OFFLINE" for c in checks), "an equal ambient value is not a shadow"


def test_missing_backend_hint_is_an_absolute_path(backends_tree):
    report = doctor.diagnose(only="ardy", host=False)
    entry = report["backends"]["ardy"]
    assert entry["status"] == "missing"
    install_line = str(backends_tree / "ardy" / "install.sh")
    assert any(install_line in hint for hint in entry["hints"]), (
        "the hint must name the resolved script — `bash backends/ardy/install.sh` does not exist from a user project"
    )


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
    assert "doctor: not every chosen backend is ok: ardy (exit 1)" in done.stdout


def test_host_report_never_raises():
    """Whatever the host has, the report is a dict with the keys the Rust reads."""
    host = doctor.host_report()
    assert set(host) >= {"gpu", "conda", "python3"}
    assert isinstance(host["gpu"].get("busy"), bool)
    blender = doctor.blender_report()
    assert "ok" in blender
    assert "ok" in doctor.tool_report("ffmpeg", ["-version"])


# ------------------------------------------------------------------- off --


def test_doctor_says_off_for_an_unchosen_kind_and_exits_zero(backends_tree):
    """`off` is not a probe result: it is `[make]` not having chosen the kind.

    The row is never probed — which is what makes doctor fast on a
    props-only project — it carries the project's own words for why, and it
    never votes on the exit code. `--make none` chooses nothing, so every
    row reads `off` and doctor exits 0: that is how the gate stays green on
    a machine with no card.
    """
    report = doctor.diagnose(host=False, chosen=[], off_reasons=["ardy=[make] clips = false"])
    _schema_ok(report)
    ardy = report["backends"]["ardy"]
    assert ardy["status"] == "off"
    assert ardy["chosen"] is False
    assert ardy["executor"] == "env", "the toml is read — that is a file read, not a probe"
    assert ardy["reason"] == "[make] clips = false"
    assert ardy["checks"] == [], "nothing was probed"
    assert report["exit_code"] == 0 and report["ok"] is True
    assert report["chosen"] == []

    table = doctor.render(report)
    assert "off — [make] clips = false" in table
    assert "nothing is chosen" in table

    # Choose it and the same tree is not ok: an uninstalled backend a kind
    # needs is exactly what doctor exists to say.
    report = doctor.diagnose(host=False, chosen="ardy")
    assert report["backends"]["ardy"]["status"] == "missing"
    assert report["backends"]["ardy"]["chosen"] is True
    assert report["exit_code"] == 1

    # An unstated --chosen is every backend, so a project with no [make]
    # loses nothing.
    report = doctor.diagnose(host=False)
    assert report["chosen"] is None
    assert report["backends"]["ardy"]["chosen"] is True
    assert report["exit_code"] == 1


# ------------------------------------------------------------- comfy host --


# A comfy backend, in `backend.toml`'s second form: no interpreter, no
# `env_kind`, no checkout of its own. Saying `python` here is refused at
# parse time by `backends.py` (designs/serve.md §4), which is why this
# fixture cannot carry the env keys it used to.
COMFY_TOML = """
name = "tts"
role = "speech"
upstream = "https://github.com/OpenMOSS/MOSS-TTS"
commit = "58b20a0d35989d71cd17ff2895fdc735097b92d1"
license = "Apache-2.0"
executor = "comfy"
host = "comfy"
entry = "speech"
resident = false

[comfy]
nodes = ["MossTTSNode"]
workflows = ["speech.api.json"]

[[comfy.models]]
repo = "OpenMOSS-Team/MOSS-TTS"
file = "moss_tts_4b.safetensors"
folder = "tts"
gb = 8.1
"""

COMFY_HOST_TOML = """
name = "comfy"
role = "host"
upstream = "https://github.com/comfyanonymous/ComfyUI"
commit = "{commit}"
license = "GPL-3.0-or-later"
executor = "tool"
env_kind = "none"
python = "3.12"
entry = "comfy"
cwd = "none"
resident = true

[server]
host = "127.0.0.1"
port = {port}
unit = "forge-comfy.service"

[comfy]
base_directory = "data"

[[comfy.packs]]
repo = "https://github.com/diodiogod/TTS-Audio-Suite"
commit = "{pack}"
dir = "TTS-Audio-Suite"
"""


class _Host(http.server.BaseHTTPRequestHandler):
    """A stand-in ComfyUI: the two GETs doctor makes, and nothing else."""

    stats: dict = {}
    info: dict = {}

    def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler's spelling
        body = self.stats if self.path.startswith("/system_stats") else self.info
        payload = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


@contextlib.contextmanager
def _comfy_service(stats: dict, info: dict):
    """A ComfyUI that answers on a real socket, for the length of one test."""
    _Host.stats, _Host.info = stats, info
    server = http.server.HTTPServer(("127.0.0.1", 0), _Host)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_address[1]
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def _git_init(path, commit_message="pinned"):
    """A real one-commit git repo, and its HEAD."""
    path.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
    subprocess.run(["git", "init", "-q"], cwd=path, check=True, env=env)
    (path / "README").write_text(commit_message)
    subprocess.run(["git", "add", "-A"], cwd=path, check=True, env=env)
    subprocess.run(["git", "commit", "-qm", commit_message], cwd=path, check=True, env=env)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=path, check=True, capture_output=True, text=True, env=env)
    return head.stdout.strip()


@pytest.fixture
def comfy_tree(tmp_path, monkeypatch):
    """A backends tree holding the host and one backend the comfy executor runs.

    Builds the pieces the ladder is judged on — a clone at a known commit, a
    node pack at a known commit, a base directory with the weight in it, a
    tracked workflow — and hands back a callable that starts a service
    saying whatever the test wants it to say.
    """
    root = tmp_path / "backends"
    host_dir, tts_dir = root / "comfy", root / "tts"
    host_dir.mkdir(parents=True)
    tts_dir.mkdir(parents=True)

    clone = tmp_path / "ComfyUI"
    commit = _git_init(clone)
    os.symlink(clone, host_dir / ".checkout")

    base = tmp_path / "prefix" / "data"
    pack = base / "custom_nodes" / "TTS-Audio-Suite"
    pack_commit = _git_init(pack)
    os.symlink(tmp_path / "prefix", host_dir / ".env")

    (tts_dir / "workflows").mkdir()
    (tts_dir / "workflows" / "speech.api.json").write_text(
        json.dumps({"1": {"class_type": "MossTTSNode"}, "2": {"class_type": "SaveAudio"}})
    )
    (tts_dir / "backend.toml").write_text(COMFY_TOML)
    monkeypatch.setenv("FORGE_BACKENDS", str(root))

    def describe(port, *, commit_override=None, pack_override=None):
        (host_dir / "backend.toml").write_text(
            COMFY_HOST_TOML.format(
                commit=commit_override or commit,
                pack=pack_override or pack_commit,
                port=port,
            )
        )

    return types.SimpleNamespace(
        root=root, base=base, describe=describe, commit=commit, pack_commit=pack_commit, weights=base / "models" / "tts"
    )


def _stats(base):
    return {
        "system": {
            "comfyui_version": "0.34.2",
            "argv": ["main.py", "--base-directory", str(base), "--disable-api-nodes"],
        }
    }


def test_the_comfy_ladder_reads_missing_partial_broken_and_ok(comfy_tree):
    """The five words for a backend the comfy executor hosts.

    `missing` nothing is listening; `partial` it answers and the packs are
    right but a class or a weight is absent; `broken` it answers as another
    commit than pinned; `ok` everything the description names is there.
    """
    info = {"MossTTSNode": {}, "SaveAudio": {}}

    # missing: nothing is listening on that port at all.
    with _comfy_service(_stats(comfy_tree.base), info) as port:
        pass
    comfy_tree.describe(port)
    report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
    tts = report["backends"]["tts"]
    assert tts["status"] == "missing"
    assert tts["executor"] == "comfy"
    assert any("systemctl --user status forge-comfy.service" in hint for hint in tts["hints"])
    assert report["exit_code"] == 1

    # partial: it answers, the pin and the pack are right, the weight is not there.
    with _comfy_service(_stats(comfy_tree.base), info) as port:
        comfy_tree.describe(port)
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        tts = report["backends"]["tts"]
        names = {check["name"]: check for check in tts["checks"]}
        assert tts["status"] == "partial", names
        assert names["commit"]["ok"] and names["pack:TTS-Audio-Suite"]["ok"]
        assert names["nodes"]["ok"] and names["workflow:speech.api.json"]["ok"]
        weight = names["model:OpenMOSS-Team/MOSS-TTS/moss_tts_4b.safetensors"]
        assert not weight["ok"] and "8.1 GB to fetch" in weight["detail"]

        # ok: put the weight where ComfyUI reads it.
        comfy_tree.weights.mkdir(parents=True)
        (comfy_tree.weights / "moss_tts_4b.safetensors").write_text("weights")
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        assert report["backends"]["tts"]["status"] == "ok"
        assert report["exit_code"] == 0

        # partial again: the node class the workflow names is gone from the
        # service — named, so the fix is a pack and not a guess.
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        assert report["backends"]["tts"]["status"] == "ok"

    # broken: it answers as a commit that is not the pinned one.
    with _comfy_service(_stats(comfy_tree.base), info) as port:
        comfy_tree.describe(port, commit_override="deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        tts = report["backends"]["tts"]
        assert tts["status"] == "broken"
        assert any("not the pinned deadbeefdead" in check["detail"] for check in tts["checks"])

    # broken: a pack off its pin is the same defect one level down.
    with _comfy_service(_stats(comfy_tree.base), info) as port:
        comfy_tree.describe(port, pack_override="0123456789abcdef0123456789abcdef01234567")
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        assert report["backends"]["tts"]["status"] == "broken"

    # broken: a tracked workflow naming a class the service does not list.
    with _comfy_service(_stats(comfy_tree.base), {"SaveAudio": {}}) as port:
        comfy_tree.describe(port)
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        tts = report["backends"]["tts"]
        assert tts["status"] == "broken"
        assert any(
            check["name"] == "workflow:speech.api.json" and "MossTTSNode" in check["detail"]
            for check in tts["checks"]
        )


def test_object_info_is_fetched_once_per_run_and_shared(comfy_tree, monkeypatch):
    """Six comfy backends must not be six fetches of the whole node surface."""
    calls: list[str] = []
    real = doctor._get_json

    def counting(url, *, timeout):
        calls.append(url)
        return real(url, timeout=timeout)

    monkeypatch.setattr(doctor, "_get_json", counting)
    for name in ("tts2", "tts3"):
        directory = comfy_tree.root / name
        directory.mkdir()
        (directory / "backend.toml").write_text(COMFY_TOML.replace('name = "tts"', f'name = "{name}"', 1))
        (directory / "workflows").mkdir()
        (directory / "workflows" / "speech.api.json").write_text(json.dumps({"1": {"class_type": "MossTTSNode"}}))
    with _comfy_service(_stats(comfy_tree.base), {"MossTTSNode": {}, "SaveAudio": {}}) as port:
        comfy_tree.describe(port)
        report = doctor.diagnose(host=False, chosen=["tts", "tts2", "tts3"])
    assert len([url for url in calls if url.endswith("/object_info")]) == 1, calls
    assert len([url for url in calls if url.endswith("/system_stats")]) == 1, calls
    assert {report["backends"][name]["status"] for name in ("tts", "tts2", "tts3")} == {"partial"}


# A comfy backend whose weights are a whole hub snapshot rather than one
# file, in the `[[models]]` form with a `comfy:` store: what TTS-Audio-Suite
# writes for the three MOSS models.
COMFY_DIR_TOML = """
name = "tts"
role = "speech"
upstream = "https://github.com/OpenMOSS/MOSS-TTS"
commit = "58b20a0d35989d71cd17ff2895fdc735097b92d1"
license = "Apache-2.0"
executor = "comfy"
host = "comfy"
entry = "speech"
resident = false

[comfy]
nodes = ["MossTTSNode"]
workflows = ["speech.api.json"]

[[models]]
id = "OpenMOSS-Team/MOSS-VoiceGenerator"
store = "comfy:models/TTS/moss_tts"
local = "MOSS-VoiceGenerator"
license = "Apache-2.0"
gb = 3.95
"""


def test_a_comfy_model_may_be_a_directory(comfy_tree):
    """A weight a node pack downloads is a snapshot, not a file.

    TTS-Audio-Suite writes `models/TTS/moss_tts/MOSS-VoiceGenerator/` — a
    directory — and a file check read that as absent for ever: the row said
    `partial` on a machine where the voice designer had just spoken, and
    doctor offered a download the installer cannot do (2026-08-30). Empty is
    still absent, because a directory the download half-made is not weights.
    """
    (comfy_tree.root / "tts" / "backend.toml").write_text(COMFY_DIR_TOML)
    snapshot = comfy_tree.base / "models" / "TTS" / "moss_tts" / "MOSS-VoiceGenerator"
    info = {"MossTTSNode": {}, "SaveAudio": {}}
    with _comfy_service(_stats(comfy_tree.base), info) as port:
        comfy_tree.describe(port)
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        weight = next(
            check for check in report["backends"]["tts"]["checks"] if check["name"].startswith("model:")
        )
        assert not weight["ok"]
        assert "3.95 GB to fetch" in weight["detail"]

        # An empty directory is not weights either.
        snapshot.mkdir(parents=True)
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        assert report["backends"]["tts"]["status"] == "partial"

        # With the snapshot in it, the row is ok and names the directory.
        (snapshot / "model.safetensors").write_text("weights")
        report = doctor.diagnose(host=False, chosen=["tts"], only="tts")
        weight = next(
            check for check in report["backends"]["tts"]["checks"] if check["name"].startswith("model:")
        )
        assert weight["ok"], weight
        assert str(snapshot) in weight["detail"]
        assert report["backends"]["tts"]["status"] == "ok"
