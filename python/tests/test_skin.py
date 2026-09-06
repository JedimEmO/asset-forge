"""What ``forge gen skin`` decides before it spends a card, and what it writes after.

The five-step loop itself needs a GPU and two runs of Blender, so what is
covered here is everything either side of that: the argv the skinner is
handed, the two refusals that would otherwise make the numbers meaningless,
the alignment call that decides whether the skeleton that came back is the
one we sent, the abort on a skin that came back bare, and the shape of the
``fit`` block a body's record carries — the thing ``promote body`` reads and
a skill prints.
"""

from __future__ import annotations

import argparse
import json
import socket
from pathlib import Path

import pytest

from forge_gen import profile as profile_mod, skin
from forge_gen.exit_codes import BackendFailed, UsageError

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "fit"


@pytest.fixture
def humanoid(repo_root):
    return profile_mod.load_profile(repo_root / "rigs" / "humanoid")


def _args(**overrides):
    base = dict(
        top_k=5,
        top_p=0.95,
        temperature=1.0,
        repetition_penalty=2.0,
        num_beams=10,
        model_ckpt=skin.DEFAULT_MODEL_CKPT,
        hf_path=None,
    )
    base.update(overrides)
    return argparse.Namespace(**base)


# ----------------------------------------------------------------- the argv --


def test_the_demo_argv_states_every_knob(tmp_path):
    """A recipe names every knob; a knob read out of somebody else's default is one nobody wrote down."""
    argv = skin._demo_argv(_args(), tmp_path / "in.glb", tmp_path / "out.glb")
    assert argv[0] == "demo.py"
    for flag in ("--use_skeleton", "--use_transfer", "--use_postprocess"):
        assert flag in argv, flag
    for flag in ("--top_k", "--top_p", "--temperature", "--repetition_penalty", "--num_beams", "--model_ckpt"):
        assert flag in argv, flag
    assert "--hf_path" not in argv, "a flag with nothing to say is not passed"
    assert skin.DEFAULT_MODEL_CKPT in argv
    assert skin._demo_argv(_args(hf_path="/models/x"), tmp_path / "a.glb", tmp_path / "b.glb")[-1] == "/models/x"


def test_the_output_must_have_a_suffix(tmp_path, humanoid):
    """``demo.py`` reads a suffixless ``--output`` as a directory, and then writes nothing there."""
    prepared = tmp_path / "hero.glb"
    prepared.write_bytes(b"glTF")
    with pytest.raises(UsageError, match="no suffix"):
        skin._places(_places_args(prepared, out=tmp_path / "hero", source=prepared), humanoid)


def _places_args(prepared, **overrides):
    base = dict(glb=str(prepared), source=None, out=None, blend=None, record=None, work=None, name=None)
    base.update({key: None if value is None else str(value) for key, value in overrides.items()})
    return argparse.Namespace(**base)


# --------------------------------------------------------------- the source --


def test_the_second_prepare_reads_its_lift_out_of_the_prepare_record(tmp_path, humanoid):
    """The loop prepares the *lift* a second time, so it has to know which lift that was."""
    lift = tmp_path / "hero_lift.glb"
    lift.write_bytes(b"glTF")
    prepared = tmp_path / "hero.glb"
    prepared.write_bytes(b"glTF")
    (tmp_path / "hero.prepare.json").write_text(
        json.dumps({"kind": "prepare", "inputs": [{"role": "mesh", "path": str(lift)}]}), encoding="utf-8"
    )
    places = skin._places(_places_args(prepared), humanoid)
    assert places["lift"] == lift.resolve()
    assert places["name"] == "hero"


def test_a_prepared_glb_with_no_record_beside_it_is_refused_by_name(tmp_path, humanoid):
    """Refused, not guessed at: the lift is a fact the record carries or nobody does."""
    prepared = tmp_path / "hero.glb"
    prepared.write_bytes(b"glTF")
    with pytest.raises(UsageError, match="hero.prepare.json"):
        skin._places(_places_args(prepared), humanoid)

    other = tmp_path / "somewhere.glb"
    other.write_bytes(b"glTF")
    places = skin._places(_places_args(prepared, source=other), humanoid)
    assert places["lift"] == other.resolve(), "--source names it when there is no record"


def test_every_output_lands_where_the_next_door_looks_for_it(tmp_path, humanoid, monkeypatch):
    from forge_gen import records

    lift = tmp_path / "hero_lift.glb"
    lift.write_bytes(b"glTF")
    prepared = tmp_path / "out" / "prepare" / "hero.glb"
    prepared.parent.mkdir(parents=True)
    prepared.write_bytes(b"glTF")
    monkeypatch.setattr(records, "_PROJECT", tmp_path)
    places = skin._places(_places_args(prepared, source=lift), humanoid)
    assert places["skinned"] == (tmp_path / "out" / "skin" / "hero.skinned.glb").resolve()
    assert places["blend"] == (tmp_path / "assets-src" / "blender" / "hero.blend").resolve()
    assert places["record"] == (tmp_path / "assets-src" / "blender" / "hero.rig.json").resolve()
    assert places["fit_report"] == (tmp_path / "assets-src" / "blender" / "hero.fit.json").resolve()
    assert places["work"] == (tmp_path / "out" / "skin" / "hero").resolve()


# ------------------------------------------------------------- the refusals --


def test_a_stale_bpy_server_is_refused_by_pid_and_never_by_pattern():
    """demo.py only *pings* port 59876, so a survivor from a killed run is talked to silently."""
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        listener.bind(("127.0.0.1", skin.BPY_PORT))
    except OSError:
        pytest.skip(f"port {skin.BPY_PORT} is in use by something this test did not start")
    listener.listen(1)
    try:
        with pytest.raises(BackendFailed) as caught:
            skin._refuse_a_stale_server()
        message = caught.value.message
        assert str(skin.BPY_PORT) in message
        assert "ss -lptn" in message and "never pkill -f" in message
        assert caught.value.payload()["port"] == skin.BPY_PORT
    finally:
        listener.close()
    skin._refuse_a_stale_server()


def test_a_busy_card_is_refused_with_the_holders_named(monkeypatch):
    """One reader of the card, doctor's: two generates never co-reside."""
    from forge_gen import doctor

    monkeypatch.setattr(
        doctor,
        "gpu_report",
        lambda: {"ok": True, "name": "RTX 4090", "total_mb": 24576, "used_mb": 22000, "apps": [{"pid": 42, "name": "python", "used_mb": 22000}]},
    )
    with pytest.raises(BackendFailed, match="pid 42"):
        skin._refuse_a_busy_card(14.0, allow_busy=False)
    assert skin._refuse_a_busy_card(14.0, allow_busy=True)["ok"] is True

    monkeypatch.setattr(doctor, "gpu_report", lambda: {"ok": False, "error": "no nvidia-smi"})
    assert skin._refuse_a_busy_card(14.0, allow_busy=False)["ok"] is False, "a card that cannot be read runs blind, and says so"


def test_a_skin_that_came_back_bare_aborts_rather_than_shipping_a_statue(humanoid):
    ceiling = float(humanoid.section("rig")["unweighted_abort_fraction"])
    assert ceiling == 0.2
    skin._abort_on_a_failed_skin({"unweighted_fraction": 0.0, "unweighted_abort_fraction": ceiling}, label="pass 1")
    skin._abort_on_a_failed_skin({"unweighted_fraction": ceiling, "unweighted_abort_fraction": ceiling}, label="pass 1")
    with pytest.raises(BackendFailed, match="do not ship a statue"):
        skin._abort_on_a_failed_skin({"unweighted_fraction": 0.35, "unweighted_abort_fraction": ceiling}, label="pass 2")


# ------------------------------------------------------------ the alignment --


def _document(names, positions, parents):
    """A minimal glTF document with one skin over ``names``."""
    nodes = []
    for index, name in enumerate(names):
        node = {"name": name, "translation": list(positions[index])}
        children = [i for i, parent in enumerate(parents) if parent == index]
        if children:
            node["children"] = children
        nodes.append(node)
    roots = [i for i, parent in enumerate(parents) if parent is None]
    nodes.append({"name": "Armature", "children": roots})
    return {
        "nodes": nodes,
        "scene": 0,
        "scenes": [{"nodes": [len(nodes) - 1]}],
        "skins": [{"joints": list(range(len(names)))}],
    }


NAMES = ["Hips", "Spine", "Head"]
PLACES = [(0.0, 1.0, 0.0), (0.0, 0.2, 0.0), (0.0, 0.4, 0.0)]
PARENTS = [None, 0, 1]


def _skeletons(names, places, parents):
    return skin._skeleton(_document(names, places, parents))


def test_alignment_named_when_the_names_come_back():
    handed = _skeletons(NAMES, PLACES, PARENTS)
    result = skin._alignment(handed, _skeletons(NAMES, PLACES, PARENTS))
    assert result["mode"] == "named"
    assert result["displacement_max_m"] == 0.0


def test_alignment_by_order_when_only_the_names_are_lost():
    """The case the whole re-attach exists for: our skeleton, upstream's labels."""
    handed = _skeletons(NAMES, PLACES, PARENTS)
    moved = [(x, y + 0.01, z) for x, y, z in PLACES]
    result = skin._alignment(handed, _skeletons(["bone_0", "bone_1", "bone_2"], moved, PARENTS))
    assert result["mode"] == "by_order"
    assert result["same_parent_array"] is True
    # Displacement is measured in world space, so a shift at the root moves
    # every joint under it — which is exactly the failure it exists to catch.
    assert result["displacement_max_m"] > 0.009


def test_alignment_none_when_the_hierarchy_is_the_models_own():
    handed = _skeletons(NAMES, PLACES, PARENTS)
    result = skin._alignment(handed, _skeletons(["a", "b", "c"], PLACES, [None, 0, 0]))
    assert result["mode"] == "none"
    assert result["same_parent_array"] is False


# ------------------------------------------------------------- the fit block --


def test_the_fit_block_is_what_a_body_s_record_carries(humanoid):
    """The shape ``promote body`` reads and a skill prints, off a real report."""
    report = json.loads((FIXTURES / "vex_runner_geometry_root.fit.json").read_text(encoding="utf-8"))
    block = skin.fit_block(report, humanoid)

    assert block["passes"] == 1, "one pass is a decision this record states, not a default"
    assert block["motion_scale"] == 1.0
    assert (block["asymmetry_arms"], block["asymmetry_other"]) == (0.35, 0.20)
    assert block["sources"] == {"limbs": "weights", "root": "geometry", "shoulder_line": "geometry", "ground": "geometry"}
    assert block["ratios"]["Spine"] == pytest.approx(0.9053, abs=1e-4)
    assert len(block["ratios"]) == len(humanoid.bones)

    run = next(row for row in block["runs"] if row["run"] == "Hips->Neck")
    assert set(run) == {"run", "reference_length_m", "ratio", "ratio_measured", "support", "off_axis_m", "mirrored"}
    assert run["reference_length_m"] == pytest.approx(0.60272, abs=1e-5)
    assert run["mirrored"] is False, "Hips->Neck has no mirror to average with"
    assert next(row for row in block["runs"] if row["run"] == "LeftArm->LeftForeArm")["mirrored"] is True

    assert block["grounding"] == {"left": pytest.approx(1.179, abs=0.01), "right": pytest.approx(1.179, abs=0.01)}
    assert block["symmetry_worst"]["gap"] == pytest.approx(0.254, abs=0.002)
    assert block["symmetry_worst"]["run"] in {"LeftArm->LeftForeArm", "LeftForeArm->LeftHand"}


def test_the_raw_asymmetry_keeps_the_measurement_symmetrising_hides(humanoid):
    """The gate reads the raw numbers, so the record carries the raw numbers."""
    report = json.loads((FIXTURES / "drow_warlock.fit.json").read_text(encoding="utf-8"))
    block = skin.fit_block(report, humanoid)
    assert block["raw_asymmetry"]["ForeArm->Hand"] == pytest.approx(0.287, abs=0.002)
    assert max(block["raw_asymmetry"].values()) == block["symmetry_worst"]["gap"]
    # And the used ratio is the mean, which is not either measurement.
    left = next(row for row in block["runs"] if row["run"] == "LeftForeArm->LeftHand")
    right = next(row for row in block["runs"] if row["run"] == "RightForeArm->RightHand")
    assert left["ratio"] == right["ratio"] != left["ratio_measured"]


def test_the_skinner_names_itself_and_its_licence_question(humanoid):
    """A licence fact that lives only in an installer is one nobody reading a record can see."""
    assert "MIT" in skin.SKINNER and "SkinTokens" in skin.SKINNER
    assert "issue #9" in skin.SKINNER_NOTE and "Michelangelo" in skin.SKINNER_NOTE


def test_a_rig_claims_integrity_and_never_reproduction(tmp_path, humanoid):
    """``seed`` is null because it is unknown: demo.py samples and takes no seed."""
    from forge_gen import records

    report = json.loads((FIXTURES / "vex_runner_geometry_root.fit.json").read_text(encoding="utf-8"))
    source = tmp_path / "hero.glb"
    source.write_bytes(b"glTF")
    out = tmp_path / "hero.skinned.glb"
    out.write_bytes(b"glTF")
    spec = {
        "name": "hero",
        "profile": humanoid,
        "profile_sha256": "sha256:0",
        "commit": "273b691d",
        "python": "3.11.9",
        "torch": "2.6.0",
        "fit": skin.fit_block(report, humanoid),
    }
    written = skin._record(_args(created_by="human"), spec, {"vertices": 100}, source, [out], tmp_path / "hero.rig.json")
    rec = json.loads(written.read_text(encoding="utf-8"))
    assert rec["kind"] == "rig" and rec["tool"] == "skintokens"
    assert rec["params"]["seed"] is None
    assert rec["params"]["skinner"]["commit"] == "273b691d"
    assert rec["params"]["fit"]["passes"] == 1
    assert list(rec) == list(records.KEYS)


def test_the_fake_ratio_is_not_one(humanoid):
    """A placeholder skeleton at the contract's own lengths exercises nothing."""
    assert skin.FAKE_FIT_RATIO != 1.0
    assert 0.4 < skin.FAKE_FIT_RATIO < 2.5, "and it is inside the band the export gate holds a real one to"

def test_second_prepare_replays_normalization_instead_of_refitting_a_different_mesh(tmp_path):
    """A backpack correction and a non-default stature must survive both skinning passes."""
    prepared = tmp_path / "hero.glb"
    prepared.write_bytes(b"glTF")
    prepared.with_suffix(".prepare.json").write_text(json.dumps({
        "params": {"stature_m": 1.65, "yaw_deg": 180.0, "depth_offset_m": -0.12, "tri_budget": 42000},
    }))
    assert skin._prepare_options(prepared) == {
        "stature": 1.65, "yaw_deg": 180.0, "depth_offset": -0.12, "budget": 42000,
    }


def test_legacy_preparation_keeps_its_recorded_scale_without_inventing_an_offset(tmp_path):
    prepared = tmp_path / "hero.glb"
    prepared.with_suffix(".prepare.json").write_text(json.dumps({
        "params": {"stature_m": 1.7, "yaw_deg": 90.0, "tri_budget": 50000},
    }))
    assert skin._prepare_options(prepared) == {
        "stature": 1.7, "yaw_deg": 90.0, "depth_offset": 0.0, "budget": 50000,
    }
