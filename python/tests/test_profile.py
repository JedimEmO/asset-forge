"""profile.py loads the shipped humanoid profile and checks what it must."""

from __future__ import annotations

import shutil

import pytest

from forge_gen import profile


def test_humanoid_loads(repo_root):
    prof = profile.load_profile(repo_root / "rigs" / "humanoid")
    assert prof.name == "humanoid"
    assert len(prof.bones) == 55
    assert prof.root == "Hips"
    assert [b["name"] for b in prof.bones if b["parent"] is None] == ["Hips"]
    assert len(prof.joints) == 27 and prof.joints[0] == "Hips"
    assert prof.rig_glb.is_file() and prof.rig_blend.is_file()
    assert prof.fixture_clip is not None and prof.fixture_clip.is_file()
    assert prof.section("review")["foot_skate_mps"] == 0.25
    assert prof.section("rig")["tri_budget"] == 60000
    assert prof.section("fingers")["chain"][0]["name"] == "Index"
    with pytest.raises(profile.ProfileError):
        prof.section("nope")


def test_sockets_resolve_to_contract_bones(repo_root):
    prof = profile.load_profile(repo_root / "rigs" / "humanoid")
    sockets = prof.sockets["sockets"]
    assert sockets, "the profile ships sockets"
    for socket in sockets:
        bone = prof.bone(socket["bone"])
        assert bone["name"] == socket["bone"]
        assert len(socket["translation"]) == 3 and len(socket["rotation"]) == 4
    assert prof.socket("hand_r")["bone"] == "RightHand"
    with pytest.raises(profile.ProfileError):
        prof.socket("tail")
    with pytest.raises(profile.ProfileError):
        prof.bone("Tail")


def test_rest_pose_is_a_standing_figure(repo_root):
    prof = profile.load_profile(repo_root / "rigs" / "humanoid")
    world = prof.rest_world()
    hips = world["Hips"][0]
    head = world["Head"][0]
    left_toe = world["LeftToeBase"][0]
    assert 0.8 < hips[1] < 1.1
    assert head[1] > hips[1] + 0.4
    assert abs(left_toe[1]) < 0.1, "the toes are near the floor"
    assert left_toe[2] > 0.1, "the profile faces +Z: toes in front of the ankles"
    assert world["LeftHand"][0][0] > 0.5 and world["RightHand"][0][0] < -0.5, "T-pose: hands out to the sides"


def test_default_dir_and_env_override(monkeypatch, repo_root, tmp_path):
    monkeypatch.delenv("FORGE_RIG_PROFILE", raising=False)
    assert profile.default_dir() == repo_root / "rigs" / "humanoid"
    assert profile.load_profile().name == "humanoid"
    monkeypatch.setenv("FORGE_RIG_PROFILE", str(tmp_path))
    assert profile.default_dir() == tmp_path
    with pytest.raises(profile.ProfileError, match="no profile.toml"):
        profile.load_profile()


def test_disagreements_are_refused(repo_root, tmp_path):
    copy = tmp_path / "humanoid"
    shutil.copytree(repo_root / "rigs" / "humanoid", copy)
    toml = copy / "profile.toml"
    text = toml.read_text()
    toml.write_text(text.replace("count = 55", "count = 54"))
    with pytest.raises(profile.ProfileError, match="54 bones"):
        profile.load_profile(copy)
    toml.write_text(text)
    motion = copy / "motion_skeleton.json"
    motion.write_text(motion.read_text().replace('"Hips",', '"Pelvis",', 1))
    with pytest.raises(profile.ProfileError, match="Pelvis"):
        profile.load_profile(copy)
