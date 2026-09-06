"""Keyframe requests fail before model loading when their constraints cannot run."""
import argparse
import json
import pytest
from forge_gen.motion import keys
from forge_gen.exit_codes import InputRejected


def test_unsupported_bone_refused_before_backend_load(tmp_path, monkeypatch):
    base = tmp_path / "base.npz"
    base.write_bytes(b"base is not read before validating the keys")
    source = tmp_path / "keys.json"
    source.write_text(json.dumps({"joints": ["Head"], "keys": [{"frame": 0}]}))
    from forge_gen import backends
    monkeypatch.setattr(backends, "load_backend", lambda *_: pytest.fail("loaded backend before refusing input"))
    args = argparse.Namespace(keys=str(source), preset=None, prompt="idle", samples=1,
        duration=4, base=str(base), out_dir=str(tmp_path / "out"), name=None)
    with pytest.raises(InputRejected, match="unsupported constraint joint 'Head'.*Hips"):
        keys.run(args)
    assert not (tmp_path / "out").exists()


@pytest.mark.parametrize("spec", [
    {"joints": ["Hips", "Hips"], "keys": [{"frame": 0}]},
    {"joints": ["Hips"], "keys": [{"frame": True}]},
    {"joints": ["Hips"], "keys": [{"frame": 0, "pos": {"Hips": [0, True, 0]}}]},
    {"joints": ["Hips"], "keys": [{"frame": 0, "pos": {"Hips": [0, float("nan"), 0]}}]},
    {"joints": ["Hips"], "keys": [{"frame": 0, "aim": {"Hips": [0, 0, 0]}}]},
])
def test_invalid_constraints_are_rejected(spec):
    with pytest.raises(InputRejected):
        keys.check_keys(spec)


def test_supported_sparse_constraints_pass():
    keys.check_keys({"joints": list(keys.CONSTRAINT_JOINTS), "keys": [
        {"frame": 0, "pose_frame": 10, "pos": {"Hips": [0.03, 0.95, 0]}, "aim": {"RightHand": [0, 0, 1]}},
        {"frame": 20},
    ]})
