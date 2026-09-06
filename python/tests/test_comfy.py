"""comfy.py finds the patch points inside a template, refuses what it cannot run, and reads a /history entry.

No ComfyUI here, ever: the graph half is exercised on the **tracked
templates themselves** — if a shipped template loses a marker a verb needs,
these fail — and the wire half against a stub HTTP server inside the test.
The two endpoints this module must never touch (``/system_stats``,
``POST /free``) are the card's, and the stub refuses them so a future edit
that reaches for one fails here rather than on a shared machine.
"""

from __future__ import annotations

import json
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

from forge_gen import backends, comfy

#: A minimal API-format graph with two knobs and one wire, the shape every
#: tracked template has.
GRAPH = {
    "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "x.safetensors"}},
    "5": {
        "class_type": "TextEncode",
        "inputs": {"text": "a placeholder", "clip": ["1", 1]},
        "_meta": {"title": "PATCH:prompt — what the sound is"},
    },
    "11": {
        "class_type": "KSampler",
        "inputs": {"seed": 0, "steps": 100, "cfg": 4.0, "model": ["1", 0], "positive": ["5", 0]},
        "_meta": {"title": "PATCH:seed"},
    },
    "13": {
        "class_type": "SaveAudio",
        "inputs": {"filename_prefix": "forge/out", "audio": ["11", 0]},
        "_meta": {"title": "PATCH:filename_prefix"},
    },
}


def _backend(tmp_path: Path, name: str = "moss_sfx", *, workflows=("sfx.api.json",)) -> backends.Backend:
    """A parsed comfy backend with a tracked template beside it."""
    directory = tmp_path / name
    (directory / "workflows").mkdir(parents=True)
    listed = ", ".join(f'"{w}"' for w in workflows)
    (directory / "backend.toml").write_text(
        f'name = "{name}"\nrole = "sfx"\nexecutor = "comfy"\nhost = "comfy"\n'
        f'upstream = "https://example.invalid/x"\nentry = "audio.sfx"\nvram_gb = 8\n'
        f"[comfy]\nworkflows = [{listed}]\nnodes = [\"KSampler\"]\n"
        f'[[comfy.packs]]\nrepo = "https://github.com/diodiogod/TTS-Audio-Suite"\n'
        f'commit = "b7e41a2c"\ndir = "TTS-Audio-Suite"\n'
    )
    (directory / "workflows" / workflows[0]).write_text(json.dumps(GRAPH, indent=2))
    return backends.load_backend(name, tmp_path)


# ---------------------------------------------------------- the tracked files --


def _tracked_templates(repo_root: Path):
    return sorted(repo_root.glob("backends/*/workflows/*.api.json"))


def test_every_tracked_template_is_a_graph_whose_patch_points_are_findable(repo_root):
    """The marker convention, held against the files this repository actually ships."""
    templates = _tracked_templates(repo_root)
    assert templates, "the toolkit tracks at least one API-format workflow"
    for path in templates:
        graph = json.loads(path.read_text(encoding="utf-8"))
        points = comfy.patch_points(graph, str(path))
        assert points, f"{path} marks no PATCH: point — nothing about it is patchable"
        for key, (node_id, field) in points.items():
            assert node_id in graph, f"{path}: {key} names node {node_id}"
            assert field in graph[node_id]["inputs"], f"{path}: {key} names input {field}"
            assert not comfy._is_link(graph[node_id]["inputs"][field]), f"{path}: {key} patches a wire"


def test_load_template_hashes_the_file_on_disk_not_the_patched_graph(tmp_path):
    backend = _backend(tmp_path)
    graph, digest = comfy.load_template(backend, "sfx.api.json")
    raw = (backend.workflow("sfx.api.json")).read_bytes()
    import hashlib

    assert digest == "sha256:" + hashlib.sha256(raw).hexdigest()
    patched = comfy.patch(graph, {"prompt": "a door", "seed": 7, "filename_prefix": "forge/x"})
    _, again = comfy.load_template(backend, "sfx.api.json")
    assert again == digest, "patching does not change what the record claims"
    assert graph["11"]["inputs"]["seed"] == 0, "the template in hand is not mutated"
    assert patched["11"]["inputs"]["seed"] == 7
    assert patched["5"]["inputs"]["text"] == "a door", "the named input wins over the only-knob rule"
    assert patched["11"]["inputs"]["steps"] == 100, "an unmarked knob keeps the template's value"


def test_a_workflow_the_backend_does_not_list_or_does_not_have_is_refused(tmp_path):
    backend = _backend(tmp_path)
    with pytest.raises(comfy.TemplateError, match="has no workflow"):
        comfy.load_template(backend, "voice.api.json")
    (backend.workflow("sfx.api.json")).unlink()
    with pytest.raises(comfy.TemplateError, match="cannot be read"):
        comfy.load_template(backend, "sfx.api.json")
    assert comfy.TemplateError("x").code == 3, "a template that cannot run is exit 3, before the GPU"
    assert comfy.TemplateError("x").payload()["error"] == "broken_backend"


# ------------------------------------------------------------------ patching --


def test_an_unknown_input_key_is_refused_and_names_what_the_template_does_patch():
    with pytest.raises(comfy.TemplateError) as caught:
        comfy.patch(GRAPH, {"prompt": "a door", "seed": 7, "filename_prefix": "p", "steps": 4}, "sfx.api.json")
    message = str(caught.value)
    assert "steps" in message and "sfx.api.json" in message
    assert "prompt" in message and "seed" in message, "the refusal lists what it does patch"


def test_a_template_missing_a_key_the_verb_requires_is_refused_before_the_gpu():
    lean = {node: value for node, value in GRAPH.items() if node != "11"}
    with pytest.raises(comfy.TemplateError) as caught:
        comfy.patch(lean, {"prompt": "a door", "seed": 7, "filename_prefix": "p"}, "sfx.api.json")
    assert "seed" in str(caught.value) and "sfx.api.json" in str(caught.value)
    # The other direction: a marker nobody fills.
    with pytest.raises(comfy.TemplateError, match="marks .*seed"):
        comfy.patch(GRAPH, {"prompt": "a door", "filename_prefix": "p"}, "sfx.api.json")


def test_a_marker_that_cannot_name_its_input_is_a_template_defect():
    ambiguous = json.loads(json.dumps(GRAPH))
    ambiguous["11"]["_meta"]["title"] = "PATCH:temperature"
    with pytest.raises(comfy.TemplateError, match="several: cfg, seed, steps"):
        comfy.patch_points(ambiguous, "sfx.api.json")
    twice = json.loads(json.dumps(GRAPH))
    twice["1"]["_meta"] = {"title": "PATCH:seed"}
    with pytest.raises(comfy.TemplateError, match="one knob, one place"):
        comfy.patch_points(twice, "sfx.api.json")
    empty = json.loads(json.dumps(GRAPH))
    empty["11"]["_meta"]["title"] = "PATCH:"
    with pytest.raises(comfy.TemplateError, match="no key after"):
        comfy.patch_points(empty, "sfx.api.json")


# -------------------------------------------------------------- the record's --


def test_packs_block_is_repo_to_commit_and_empty_for_native_nodes(tmp_path):
    backend = _backend(tmp_path)
    assert comfy.packs_block(backend) == {"https://github.com/diodiogod/TTS-Audio-Suite": "b7e41a2c"}
    native = _backend(tmp_path, "acestep")
    native.comfy.packs.clear()
    assert comfy.packs_block(native) == {}, "native nodes are an empty object, not a null"


def test_output_prefix_names_the_job_so_two_runs_cannot_collide(monkeypatch):
    monkeypatch.setenv("FORGE_JOB_ID", "j-20260830-141207-3f9a")
    assert comfy.output_prefix("door blast/2") == "forge/j-20260830-141207-3f9a/door_blast_2"
    monkeypatch.delenv("FORGE_JOB_ID")
    assert comfy.output_prefix("door").startswith("forge/local-")


#: A ``/history`` entry as ComfyUI answers with one, trimmed to what is read.
HISTORY = {
    "status": {
        "status_str": "success",
        "completed": True,
        "messages": [
            ["execution_start", {"prompt_id": "b1f0"}],
            ["execution_cached", {"nodes": ["1", "5"], "prompt_id": "b1f0"}],
            ["execution_success", {"prompt_id": "b1f0"}],
        ],
    },
    "outputs": {"13": {"audio": [{"filename": "door_00001_.flac", "subfolder": "forge/j-1", "type": "output"}]}},
}


def test_was_cached_is_observed_on_the_history_entry_never_inferred():
    assert comfy.was_cached(HISTORY, "5") is True
    assert comfy.was_cached(HISTORY, "13") is False, "the save node ran; only the loaders were cached"
    assert comfy.was_cached({"status": {}}, "13") is False
    assert comfy.was_cached({}, "13") is False


def test_outputs_reads_every_saved_file_in_node_order():
    assert comfy.outputs(HISTORY) == [
        {"filename": "door_00001_.flac", "subfolder": "forge/j-1", "type": "output"}
    ]
    assert comfy.outputs({"outputs": {"9": {"images": [{"filename": "a.png"}]}}}) == [
        {"filename": "a.png", "subfolder": "", "type": "output"}
    ]
    assert comfy.outputs({}) == []


# ---------------------------------------------------------------- the client --


class _Host(BaseHTTPRequestHandler):
    """Enough ComfyUI to answer this module, and nothing the card owns."""

    seen: list[str] = []

    def log_message(self, *_args):  # noqa: A003 - quiet under pytest
        pass

    def _json(self, payload, code=200):
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):  # noqa: N802
        type(self).seen.append(f"GET {self.path.split('?')[0]}")
        if self.path.startswith("/history/"):
            self._json({self.path.rsplit("/", 1)[1]: HISTORY})
        elif self.path.startswith("/view"):
            body = b"RIFF....WAVEfmt "
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path.startswith("/object_info"):
            self._json({"KSampler": {"input": {"required": {"seed": [["0"]]}}}})
        else:
            self._json({"error": "the card's endpoints are not this module's"}, code=404)

    def do_POST(self):  # noqa: N802
        type(self).seen.append(f"POST {self.path}")
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        if self.path == "/prompt":
            self._json({"prompt_id": "b1f0", "number": 1})
        elif self.path == "/upload/image":
            assert b"filename=\"ref.wav\"" in body
            self._json({"name": "ref.wav", "subfolder": "", "type": "input"})
        else:
            self._json({"error": "no"}, code=404)


@pytest.fixture
def host():
    _Host.seen = []
    server = HTTPServer(("127.0.0.1", 0), _Host)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()


def test_submit_wait_and_fetch_against_a_stub_host(host, tmp_path):
    prompt_id = comfy.submit(host, GRAPH, comfy.client_id())
    assert prompt_id == "b1f0"
    seen_progress = []
    entry = comfy.wait_for(host, prompt_id, timeout=10, poll=0.01, on_progress=lambda s, e: seen_progress.append(s))
    assert entry["status"]["status_str"] == "success"
    out = tmp_path / "door.wav"
    assert comfy.fetch(host, entry, out) == [out]
    assert out.read_bytes().startswith(b"RIFF")
    # Into a directory that exists, each file keeps its own name.
    (tmp_path / "many").mkdir()
    written = comfy.fetch(host, entry, tmp_path / "many")
    assert [p.name for p in written] == ["door_00001_.flac"]
    assert comfy.upload_image(host, out, "ref.wav") == "ref.wav"
    assert "KSampler" in comfy.object_info(host)
    assert not [line for line in _Host.seen if "system_stats" in line or "free" in line], (
        "the card is the lease holder's business, in Rust, with no Python alive"
    )


def test_a_graph_that_saves_nothing_is_a_backend_failure(host, tmp_path):
    from forge_gen.exit_codes import BackendFailed

    with pytest.raises(BackendFailed, match="saved nothing"):
        comfy.fetch(host, {"outputs": {}}, tmp_path / "x.wav")


def test_a_host_that_does_not_answer_names_the_unit(tmp_path):
    from forge_gen.exit_codes import BackendFailed

    dead = "http://127.0.0.1:1"
    with pytest.raises(BackendFailed, match="did not answer") as caught:
        comfy.submit(dead, GRAPH, "x")
    assert "forge-comfy" in caught.value.payload()["hint"]


def test_base_url_prefers_the_environment_then_the_project_then_the_host(tmp_path, monkeypatch):
    backend = _backend(tmp_path)
    (tmp_path / "comfy" / "workflows").mkdir(parents=True)
    (tmp_path / "comfy" / "backend.toml").write_text(
        'name = "comfy"\nrole = "host"\nexecutor = "tool"\nenv_kind = "none"\n'
        'upstream = "https://github.com/comfyanonymous/ComfyUI"\ncommit = "169fcf35"\nentry = "comfy"\n'
        '[server]\nhost = "127.0.0.1"\nport = 8188\n'
    )
    monkeypatch.delenv("FORGE_COMFY_URL", raising=False)
    assert comfy.base_url(backend, root=tmp_path) == "http://127.0.0.1:8188"

    project = tmp_path / "project"
    project.mkdir()
    (project / "forge.toml").write_text('[hardware]\ncomfy_url = "http://127.0.0.1:9999/"\n')
    assert comfy.base_url(backend, project, root=tmp_path) == "http://127.0.0.1:9999"

    monkeypatch.setenv("FORGE_COMFY_URL", "http://box:8188/")
    assert comfy.base_url(backend, project, root=tmp_path) == "http://box:8188"

    # A forge.toml that does not parse is doctor's complaint, not a reason
    # to refuse a generate over it.
    monkeypatch.delenv("FORGE_COMFY_URL")
    (project / "forge.toml").write_text("[hardware\n")
    assert comfy.base_url(backend, project, root=tmp_path) == "http://127.0.0.1:8188"
