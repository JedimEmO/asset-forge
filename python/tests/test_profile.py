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


def test_default_dir_honours_the_project_named_by_forge_toml(monkeypatch, repo_root, tmp_path):
    """`python3 python/forge_gen` run bare must read the PROJECT's profile, not the toolkit's."""
    from forge_gen import records

    monkeypatch.delenv("FORGE_RIG_PROFILE", raising=False)
    project = tmp_path / "game"
    (project / "assets-src" / "rigs" / "humanoid").mkdir(parents=True)
    (project / "forge.toml").write_text('[project]\nname = "game"\nrig = "humanoid"\n\n[paths]\nrigs = "assets-src/rigs"\n')
    # Via --project (records.set_project is what cli.main calls).
    records.set_project(project)
    try:
        assert profile.default_dir() == (project / "assets-src" / "rigs" / "humanoid").resolve()
    finally:
        records.set_project(None)
    # Via the working directory, from anywhere inside the project.
    monkeypatch.chdir(project / "assets-src")
    assert profile.default_dir() == (project / "assets-src" / "rigs" / "humanoid").resolve()
    # A project naming a profile it does not have still gets the project
    # path — load_profile then fails loudly instead of the toolkit's copy
    # passing the wrong gates.
    (project / "forge.toml").write_text('[project]\nrig = "other"\n\n[paths]\nrigs = "assets-src/rigs"\n')
    assert profile.default_dir() == (project / "assets-src" / "rigs" / "other").resolve()
    # $FORGE_RIG_PROFILE still wins over everything.
    monkeypatch.setenv("FORGE_RIG_PROFILE", str(repo_root / "rigs" / "humanoid"))
    assert profile.default_dir() == repo_root / "rigs" / "humanoid"


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


def test_a_profile_that_still_names_a_retired_gate_is_refused(repo_root, tmp_path):
    """A knob nothing reads is worse than a missing one.

    ``reach_min``/``reach_max`` measured a body's half-span against the
    *skeleton's* wrists, and the fitted skeleton moves those wrists to the
    body — the gate was measuring its own output. Deleting the code is not
    enough: a hand-written or hand-copied profile that keeps the line goes on
    claiming a rule the pipeline stopped enforcing, so the reader refuses it
    by name.
    """
    copy = tmp_path / "humanoid"
    shutil.copytree(repo_root / "rigs" / "humanoid", copy)
    toml = copy / "profile.toml"
    text = toml.read_text()
    lines = [line.split("=")[0].strip() for line in text.splitlines() if "=" in line and not line.lstrip().startswith("#")]
    assert "reach_min" not in lines and "reach_max" not in lines, "the shipped profile has no dead gate in it"
    assert profile.load_profile(copy).name == "humanoid"

    for key in ("reach_min", "reach_max"):
        toml.write_text(text.replace("[fit]\n", f"[fit]\n{key} = 0.8\n", 1))
        with pytest.raises(profile.ProfileError, match=key):
            profile.load_profile(copy)
    toml.write_text(text)


def test_the_new_fit_and_export_numbers_are_where_the_gates_read_them(repo_root):
    """Every scalar designs/skin.md froze, in the file that is its source."""
    prof = profile.load_profile(repo_root / "rigs" / "humanoid")
    fit = prof.section("fit")
    assert fit["limb_radius_min_fraction"] == 0.22
    assert fit["asymmetry_arms"] == 0.35
    assert fit["asymmetry_other"] == 0.20
    assert fit["arm_height_tolerance_m"] == 0.15
    assert len(fit["landmarks"]) == 15
    assert set(fit["landmarks"]) <= {bone["name"] for bone in prof.bones}, "a landmark is a contract bone"
    export = prof.section("export")
    assert export["rest_direction_tolerance_deg"] == 1.0
    assert export["length_ratio_min"] == 0.4
    assert export["length_ratio_max"] == 2.5
    assert export["rest_zero_length_m"] == 0.0001
    assert prof.section("bones")["contact_foot_tolerance_m"] == 0.05
    # And the knobs that went with the bind half are gone.
    assert "shell_fraction" not in prof.section("rig")
    assert prof.section("rig")["unweighted_abort_fraction"] == 0.2, "the shell-abort gate stays; it is on the skinner's output now"


#: Every scalar name that appears in both ``profile.toml`` and
#: ``contract.json``, with the ``[section]`` it lives in on the profile side.
#: ``contract.json`` is a projection written by ``forge rig export-contract``,
#: and nothing held the two equal until this test: a regeneration that quietly
#: dropped a number, or a profile edited without one, would leave the Rust and
#: Python halves of the same gate reading different values.
SHARED_SCALARS = {
    "foot_tolerance_m": "bones",
    "rest_rotation_tolerance": "bones",
    "contact_foot_tolerance_m": "bones",
    "rest_direction_tolerance_deg": "export",
    "length_ratio_min": "export",
    "length_ratio_max": "export",
    "rest_zero_length_m": "export",
}


def test_every_scalar_profile_toml_and_contract_json_share_is_equal(repo_root):
    prof = profile.load_profile(repo_root / "rigs" / "humanoid")
    shared = 0
    for key, section in SHARED_SCALARS.items():
        if key not in prof.contract:
            continue
        shared += 1
        assert prof.contract[key] == prof.section(section)[key], f"{key}: contract.json and profile.toml disagree"
    assert shared >= 2, "at least the two that have always been in both are still in both"

    # The ones the contract restates under its own names.
    bones = prof.section("bones")
    assert prof.contract["stature_m"] == {
        "reference": bones["reference_stature_m"],
        "min": bones["min_stature_m"],
        "max": bones["max_stature_m"],
    }
    assert prof.contract["root"] == bones["root"]
    assert prof.contract["front"] == bones["front"]
    assert prof.contract["name"] == prof.toml["profile"]["name"]
    assert prof.contract["version"] == prof.toml["profile"]["version"]
    assert prof.contract["reference_clip"] == bones["reference_clip"]
    assert len(prof.contract["bones"]) == bones["count"]
