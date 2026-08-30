"""records.py writes the Rust field order, sorted free-form keys, and lands atomically."""

from __future__ import annotations

import json
import os

import pytest

from forge_gen import records

#: The lift record transcribed in generator_record.rs's unit test, byte for byte.
GOLDEN = """{
  "forge_record": 2,
  "kind": "lift",
  "tool": "trellis2",
  "created": "2026-08-23",
  "created_by": "human",
  "backend": {
    "name": "trellis2",
    "commit": "75fbf0183001ed9876c8dbb35de6b68552ee08bd",
    "python": "3.11.9",
    "torch": "2.6.0+cu124",
    "model": "microsoft/TRELLIS.2-4B",
    "model_revision": null,
    "executor": "env",
    "comfyui_commit": null,
    "workflow_sha256": null,
    "packs": null
  },
  "inputs": [
    {
      "role": "image",
      "path": "assets-src/refs/props/barrel.png",
      "sha256": "sha256:cbaf",
      "source": "the user",
      "prompt": null
    }
  ],
  "params": {
    "decimation_target_vertices": 6000,
    "pipeline_type": "1024_cascade",
    "remesh": true,
    "resolution": 1024,
    "seed": 42,
    "texture_baker": "nvdiffrast (non-commercial)",
    "texture_size": 1024
  },
  "outputs": [
    {
      "path": "out/lifts/barrel.glb",
      "sha256": "sha256:5833",
      "bytes": 1234
    }
  ],
  "measured": {},
  "fake": false,
  "note": null
}
"""


def test_golden_bytes_from_a_record_built_out_of_order():
    rec = records.new_record("lift", "trellis2", created_by="human", created="2026-08-23")
    rec["backend"] = records.backend_block(
        name="trellis2", commit="75fbf0183001ed9876c8dbb35de6b68552ee08bd", python="3.11.9",
        torch="2.6.0+cu124", model="microsoft/TRELLIS.2-4B",
    )
    rec["inputs"].append({"role": "image", "path": "assets-src/refs/props/barrel.png", "sha256": "sha256:cbaf", "source": "the user", "prompt": None})
    # Deliberately unsorted: the writer sorts, because the Rust reader would.
    rec["params"] = {"texture_size": 1024, "seed": 42, "resolution": 1024, "remesh": True, "pipeline_type": "1024_cascade", "decimation_target_vertices": 6000, "texture_baker": "nvdiffrast (non-commercial)"}
    rec["outputs"].append({"path": "out/lifts/barrel.glb", "sha256": "sha256:5833", "bytes": 1234})
    assert records.dumps(rec) == GOLDEN
    assert list(json.loads(records.dumps(rec))) == list(records.KEYS)


def test_inputs_and_outputs_are_hashed_and_relative_to_the_project(tmp_path):
    project = tmp_path / "proj"
    (project / "refs").mkdir(parents=True)
    image = project / "refs" / "x.png"
    image.write_bytes(b"png?")
    outside = tmp_path / "elsewhere.glb"
    outside.write_bytes(b"glb")
    records.set_project(None)
    rec = records.new_record("rig", "blender")
    records.add_input(rec, "image", image, source="grok", project_root=project)
    records.add_input(rec, "prompt", prompt="a barrel")
    records.add_output(rec, outside, project_root=project)
    assert rec["inputs"][0]["path"] == "refs/x.png"
    assert rec["inputs"][0]["sha256"] == "sha256:" + __import__("hashlib").sha256(b"png?").hexdigest()
    assert rec["inputs"][1] == {"role": "prompt", "path": None, "sha256": None, "source": None, "prompt": "a barrel"}
    assert rec["outputs"][0]["path"] == str(outside.resolve()), "outside the project stays absolute"
    assert rec["outputs"][0]["bytes"] == 3
    # set_project makes the default.
    records.set_project(project)
    try:
        assert records.record_path(image) == "refs/x.png"
    finally:
        records.set_project(None)


def test_write_is_atomic_and_round_trips(tmp_path, monkeypatch):
    rec = records.new_record("sfx", "moss_sound_effect", created_by="agent:claude")
    rec["params"] = {"seed": 7, "duration_s": 1.5}
    target = tmp_path / "deep" / "x.sfx.json"
    records.write(rec, target)
    assert target.read_text().endswith("}\n")
    assert records.load(target)["params"] == {"duration_s": 1.5, "seed": 7}
    assert not [p for p in target.parent.iterdir() if p.name.startswith(".")], "no temp file left behind"

    # A failure mid-write leaves the previous file intact and no temp sibling.
    before = target.read_bytes()
    real_replace = os.replace

    def explode(src, dst):
        raise OSError("disk on fire")

    monkeypatch.setattr(os, "replace", explode)
    with pytest.raises(OSError):
        records.write(rec, target)
    monkeypatch.setattr(os, "replace", real_replace)
    assert target.read_bytes() == before
    assert not [p for p in target.parent.iterdir() if p.name.startswith(".")]


def test_unknown_keys_and_kinds_are_refused():
    rec = records.new_record("take", "ardy")
    rec["extra"] = 1
    with pytest.raises(ValueError, match="extra"):
        records.dumps(rec)
    with pytest.raises(ValueError, match="kind"):
        records.new_record("sidecar", "x")
    rec = records.new_record("take", "ardy")
    rec["backend"]["vram"] = 16
    with pytest.raises(ValueError, match="vram"):
        records.dumps(rec)


def test_actor_and_today():
    assert records.actor(None) == "unknown"
    assert records.actor("human") == "human"
    assert records.actor("agent:claude") == "agent:claude"
    assert records.actor("claude") == "agent:claude"
    assert records.actor("  ") == "unknown"
    assert len(records.today()) == 10 and records.today()[4] == "-"


def test_load_refuses_other_schemas(tmp_path):
    path = tmp_path / "r.json"
    path.write_text('{"forge_record": 3}')
    with pytest.raises(ValueError, match="forge_record 3"):
        records.load(path)
    path.write_text('{"forge_record": 0}')
    with pytest.raises(ValueError, match="forge_record 0"):
        records.load(path)
    path.write_text('{"kind": "lift"}')
    with pytest.raises(ValueError, match="no forge_record"):
        records.load(path)


def test_record_v2_reads_v1_and_writes_v2(tmp_path):
    """Both schemas read; only 2 is written; the four new keys come last.

    Nothing under ``assets/`` was migrated when the schema went to 2, so a
    reader that refused a 1 would refuse every record this repository has
    ever shipped. A 1 read here stays a 1 — a reader that promoted one would
    be claiming the four keys were absent on purpose.
    """
    assert records.SCHEMA == 2 and records.SCHEMA_MIN == 1
    assert records.BACKEND_KEYS[:6] == ("name", "commit", "python", "torch", "model", "model_revision")
    assert records.BACKEND_KEYS[6:] == ("executor", "comfyui_commit", "workflow_sha256", "packs")

    v1 = tmp_path / "old.json"
    v1.write_text(
        '{"forge_record": 1, "kind": "lift", "tool": "trellis2", "created": "2026-08-23",\n'
        ' "created_by": "human",\n'
        ' "backend": {"name": "trellis2", "commit": "75fbf018", "python": "3.11.9",\n'
        '             "torch": "2.6.0+cu124", "model": "microsoft/TRELLIS.2-4B", "model_revision": null},\n'
        ' "inputs": [], "params": {"seed": 42}, "outputs": [], "measured": {}, "fake": false, "note": null}\n'
    )
    old = records.load(v1)
    assert old["forge_record"] == 1, "read, not promoted in place"
    assert "executor" not in old["backend"], "a v1 record predates the question"
    assert old["params"]["seed"] == 42

    # The writer only ever writes 2, and the block is complete either way.
    fresh = records.new_record("lift", "trellis2", created="2026-08-23")
    assert fresh["forge_record"] == 2
    assert list(fresh["backend"]) == list(records.BACKEND_KEYS)
    assert fresh["backend"]["executor"] == "env", "written for every record, env ones included"
    old["forge_record"] = 1
    with pytest.raises(ValueError, match="this writer is 2"):
        records.dumps(old)


def test_a_comfy_backend_block_says_what_ran_and_sorts_its_packs():
    rec = records.new_record("sfx", "moss_sound_effect", created="2026-08-30")
    rec["backend"] = records.backend_block(
        "moss_sfx", None, model="OpenMOSS-Team/MOSS-SoundEffect-v2.0",
        executor="comfy", comfyui_commit="169fcf35", workflow_sha256="sha256:9f1c",
        packs={"https://b": "2", "https://a": "1"},
    )
    rec["params"] = {"workflow": "sfx.api.json", "seed": 815273}
    written = json.loads(records.dumps(rec))
    assert written["backend"]["commit"] is None, "a comfy backend has no checkout of its own"
    assert written["backend"]["executor"] == "comfy"
    assert list(written["backend"]["packs"]) == ["https://a", "https://b"], "free-form maps are sorted"
    assert written["params"]["workflow"] == "sfx.api.json", "the patch is knobs; knobs live in params"
