"""Write the fixtures `crates/forge_library/tests/python_records.rs` reads: one record per kind, one take.

Run with the output directory as the only argument. The clock is pinned,
the stand-in payloads are fixed bytes, and every path is relative to a
pretend project, so the files are the same on every machine and a
`git diff` on them means the writer changed. Re-capture with::

    BLESS_PYTHON_FIXTURES=1 cargo test -p forge_library --test python_records

No numpy, no GPU, no Blender: this is the writer's own output, which is the
claim the Rust test makes — "what records.py writes, generator_record.rs
reads, byte for byte".
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from forge_gen import npz, records  # noqa: E402

#: The day every fixture claims.
TODAY = "2026-08-23"

#: Bytes every stand-in file is made of, so the recorded hashes are fixed.
PAYLOAD = b"probe"


def _file(project: Path, relative: str) -> Path:
    path = project / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(PAYLOAD)
    return path


def build(project: Path) -> dict[str, dict]:
    """Every kind, with the knobs the Rust projections read."""
    out: dict[str, dict] = {}

    rec = records.new_record("lift", "trellis2", created_by="human", created=TODAY)
    rec["backend"] = records.backend_block("trellis2", "75fbf0183001ed9876c8dbb35de6b68552ee08bd", "3.11.15", "2.6.0+cu124", "microsoft/TRELLIS.2-4B")
    records.add_input(rec, "image", _file(project, "assets-src/refs/props/barrel.png"), source="grok, edit-chained from the style board")
    rec["params"] = {
        "seed": 42, "resolution": 1024, "pipeline_type": "1024_cascade", "decimation_target_vertices": 6000,
        "texture_size": 1024, "remesh": True, "texture_baker": "nvdiffrast (non-commercial)",
    }
    records.add_output(rec, _file(project, "out/lifts/barrel.glb"))
    rec["measured"] = {"triangles": 11804, "vertices": 5902}
    out["lift"] = rec

    rec = records.new_record("prop", "blender", created_by="agent:claude", created=TODAY)
    rec["backend"] = records.backend_block("blender", None, "3.13.2", None, None)
    records.add_input(rec, "mesh", _file(project, "out/lifts/barrel.glb"))
    rec["params"] = {"height_m": 0.9, "origin": "floor", "metallic": 0.0, "roughness": 0.9, "tri_budget": 12000}
    records.add_output(rec, _file(project, "out/props/barrel.glb"))
    rec["measured"] = {"triangles": 11804, "bounds_m": [0.62, 0.9, 0.62]}
    out["prop"] = rec

    rec = records.new_record("rig", "blender", created_by="human", created=TODAY)
    rec["backend"] = records.backend_block("blender", None, "3.13.2", None, None)
    records.add_input(rec, "mesh", _file(project, "out/lifts/vex_runner.glb"))
    records.add_input(rec, "blend", _file(project, "rigs/humanoid/rig.blend"), source="the profile")
    rec["params"] = {"stature_m": 1.8, "tri_budget": 60000, "shell_fraction": 0.02, "profile": "humanoid"}
    records.add_output(rec, _file(project, "assets-src/blender/vex_runner.blend"))
    rec["measured"] = {"reach": 1.02, "unweighted_fraction": 0.0}
    out["rig"] = rec

    rec = records.new_record("export", "blender", created_by="human", created=TODAY)
    rec["backend"] = records.backend_block("blender", None, "3.13.2", None, None)
    records.add_input(rec, "blend", _file(project, "assets-src/blender/vex_runner.blend"))
    rec["params"] = {"max_influences": 4, "y_up": True, "materials": "EXPORT", "animations": False}
    records.add_output(rec, _file(project, "out/export/vex_runner.glb"))
    rec["measured"] = {"bones": 55, "images": 1}
    out["export"] = rec

    rec = records.new_record("take", "ardy", created_by="agent:claude", created=TODAY)
    rec["backend"] = records.backend_block("ardy", "693f74d13b3d04a0a22ce127ee79c929dd89756b", "3.12.7", "2.13.0", "core")
    records.add_input(rec, "prompt", prompt="a slow walk forward")
    rec["params"] = {"repo": "nv-tlabs/ardy", "seed": 7, "duration_s": 4.0, "cfg": 2.5, "sample": 1, "fps": 20}
    records.add_output(rec, _file(project, "out/sweeps/walk/walk__d4_c2_s7_1.npz"))
    out["take"] = rec

    rec = records.new_record("sfx", "moss_sound_effect", created_by="human", created=TODAY)
    rec["backend"] = records.backend_block("moss_sfx", "58b20a0d5fcc6766658d50967a90a9d890009a46", "3.12.7", "2.9.0+cu128", "OpenMOSS/MOSS-SoundEffect-v2")
    records.add_input(rec, "prompt", prompt="a heavy wooden door slamming")
    rec["params"] = {"seed": 3, "duration_s": 1.5, "steps": 50, "cfg": 4.0}
    records.add_output(rec, _file(project, "out/audio/door_slam.wav"))
    rec["measured"] = {"duration_s": 1.5, "peak_dbfs": -3.1}
    out["sfx"] = rec

    rec = records.new_record("music", "ace_step", created_by="agent:claude", created=TODAY)
    rec["backend"] = records.backend_block("acestep", "82252c2418de6cb8b3ca99b05592aaf539cc7fb3", "3.12.7", "2.10.0+cu128", "acestep-v15-turbo")
    records.add_input(rec, "prompt", prompt="dark ambient boss theme, low strings, taiko")
    rec["params"] = {
        "lm_model": "acestep-5Hz-lm-0.6B", "dit_model": "acestep-v15-turbo", "seed": "1,2", "bpm": 96,
        "keyscale": "F minor", "timesignature": "4", "genres": "N/A", "lyrics": "[instrumental]", "duration_s": 90.0,
    }
    records.add_output(rec, _file(project, "out/audio/boss_theme.wav"))
    out["music"] = rec

    rec = records.new_record("speech", "moss_tts", created_by="human", created=TODAY)
    rec["backend"] = records.backend_block("moss_tts", "58b20a0d5fcc6766658d50967a90a9d890009a46", "3.12.7", "2.9.1+cu128", "OpenMOSS/MOSS-TTS-Local-Transformer-4B")
    records.add_input(rec, "prompt", prompt="Stand down.")
    records.add_input(rec, "reference", _file(project, "assets-src/voices/calm.wav"), source="a recording the user owns")
    rec["params"] = {"seed": 11, "voice": "calm", "reference": "assets-src/voices/calm.wav", "language": "en"}
    records.add_output(rec, _file(project, "out/audio/stand_down.wav"))
    rec["measured"] = {"duration_s": 1.2}
    rec["note"] = "première ligne — the accent is on purpose: ensure_ascii=False"
    out["speech"] = rec

    fake = records.new_record("lift", "trellis2", created_by="unknown", created=TODAY)
    fake["backend"] = records.backend_block("trellis2", "fake", None, None, None)
    records.add_input(fake, "image", _file(project, "assets-src/refs/props/crate.png"))
    fake["params"] = {"seed": None, "resolution": None, "texture_baker": None}
    records.add_output(fake, _file(project, "out/lifts/crate.glb"))
    fake["fake"] = True
    fake["note"] = "placeholder output from a --fake run; nothing about it is a measurement"
    out["fake_lift"] = fake
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: capture_fixtures.py <out_dir>", file=sys.stderr)
        return 2
    out_dir = Path(argv[1])
    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        project = Path(scratch) / "project"
        project.mkdir()
        records.set_project(project)
        for name, rec in build(project).items():
            records.write(rec, out_dir / f"{name}.json")
    npz.write_take(out_dir / "still.npz", frames=8, fps=20, prompt="stand still")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
