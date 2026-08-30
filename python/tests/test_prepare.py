"""The two gates ``forge gen prepare`` refuses on, on geometry built to trip them.

Neither gate needs Blender: both read an ``(N, 3)`` array through
``forge_gen.fitgeom``, which is the whole reason that module exists — the
door calls it inside Blender on ``foreach_get`` arrays and ``fit.py`` calls
it on the arrays it pulls out of a glb, so the gate and the fit cannot
disagree about where a shoulder is. Here it is called on figures assembled
in numpy, and on the prepared glbs still on disk when there are any.

The numbers asserted below are the ones ``[fit] limb_radius_min_fraction``
and ``[fit] arm_height_tolerance_m`` were calibrated against; see
``fitgeom``'s own table for the four bodies and what each was judged to be.
"""

from __future__ import annotations

import numpy as np
import pytest

from forge_gen import fitgeom
from forge_gen import profile as profile_mod
from forge_gen.blender import prepare


@pytest.fixture
def humanoid(repo_root):
    return profile_mod.load_profile(repo_root / "rigs" / "humanoid")


# ------------------------------------------------------------ the T-pose --


def _t_pose(*, shoulder_y: float, arm_y: float, stature: float = 1.8, half_span: float = 0.9) -> np.ndarray:
    """A torso column plus two arm tubes, the arms leaving at ``shoulder_y`` and ending at ``arm_y``.

    The arm tube is the only geometry outboard of 0.55 of the half-span, so
    it is exactly what the shoulder line measures — which is the point: on a
    T-pose the shoulder line and the arm tips are one measurement seen twice,
    and the gate is the gap between them.
    """
    rng = np.random.default_rng(11)
    torso = np.column_stack(
        (
            rng.uniform(-0.18, 0.18, 400),
            rng.uniform(0.0, stature, 400),
            rng.uniform(-0.12, 0.12, 400),
        )
    )
    arms = []
    for sign in (1.0, -1.0):
        along = rng.uniform(0.2, half_span, 400)
        drop = (along - 0.2) / (half_span - 0.2)
        arms.append(
            np.column_stack(
                (
                    sign * along,
                    shoulder_y + drop * (arm_y - shoulder_y) + rng.uniform(-0.04, 0.04, 400),
                    rng.uniform(-0.06, 0.06, 400),
                )
            )
        )
    return np.vstack([torso, *arms])


def test_the_shoulder_line_is_the_body_s_own_arm_tube():
    """A body whose arms leave at 1.31 m reads 1.31 m, whatever the skeleton says.

    1.311 is the witch's own number, and the shipped ``fitgeom`` reads
    1.3143 off her prepared glb: the measurement that cross-checked her.
    """
    geometry = fitgeom.measure(_t_pose(shoulder_y=1.311, arm_y=1.311))
    assert geometry["shoulder_y"] == pytest.approx(1.311, abs=0.02)
    assert geometry["half_span"] == pytest.approx(0.9, abs=0.01)
    assert geometry["arm_tip_y"] == pytest.approx(geometry["shoulder_y"], abs=0.01), "a horizontal arm reads the same at both ends"


def test_a_body_with_no_arms_has_no_shoulder_line_and_says_so():
    """``None``, not a guess: a column has no arm tube to take a median of."""
    column = np.column_stack((np.zeros(200), np.linspace(0.0, 1.8, 200), np.zeros(200)))
    assert fitgeom.measure(column)["shoulder_y"] is None


def test_the_arm_height_gate_refuses_arms_that_droop_and_passes_arms_that_do_not(humanoid):
    """The gate measures a pose, and says nothing about stature.

    It also **under-reads by a factor of four**, and this test is where that
    is written down. The shoulder line is the median of the same tube whose
    far end is falling, so the gap between the tips and that median comes out
    at a stable 0.23 of the arm's real fall — measured here over a metre of
    it. ``arm_height_tolerance_m = 0.15`` therefore refuses an arm that has
    fallen about **0.65 m**, which is an arm most of the way to the hip; it
    is a budget, exactly as it has always been described, and not a
    measurement of where bodies break. The acceptance run is what re-pins it
    on a real body with hanging arms, and 0.05 would put the refusal at a
    fall of 0.22 m.
    """
    tolerance = float(humanoid.section("fit")["arm_height_tolerance_m"])
    fraction = float(humanoid.section("fit")["arm_tip_fraction"])
    assert (tolerance, fraction) == (0.15, 0.9)

    def gap(fall):
        geometry = fitgeom.measure(_t_pose(shoulder_y=1.31, arm_y=1.31 - fall), arm_tip_fraction=fraction)
        return abs(geometry["arm_tip_y"] - geometry["shoulder_y"])

    assert gap(0.0) <= 0.02, "level arms read level"
    for fall in (0.11, 0.29, 0.46, 0.60, 0.81, 1.01):
        assert gap(fall) / fall == pytest.approx(0.23, abs=0.01), f"a {fall:.2f} m fall reads {gap(fall):.3f} m"
    assert gap(0.29) < tolerance, "a 29 cm fall reads 6.6 cm and passes"
    assert gap(0.60) < tolerance, "and so does a 60 cm fall, at 14.0 cm"
    assert gap(0.81) > tolerance, "an arm 81 cm down does not"


def test_a_four_head_body_is_not_this_refusal():
    """The witch the old gate turned away five times passes the new one.

    Her arms are horizontal; what she is not is seven heads tall. The old
    gate read her 16-23 cm under the *skeleton's* wrists, which was a
    statement about the skeleton and not about her pose.
    """
    witch = fitgeom.measure(_t_pose(shoulder_y=1.311, arm_y=1.311, stature=1.8))
    assert abs(witch["arm_tip_y"] - witch["shoulder_y"]) < 0.15
    assert witch["heads"] < 5.0, "and she is four heads tall, which the record notes and no gate refuses"


# ------------------------------------------------------------- the sliver --


def _limb(length: float, radius: float, *, origin=(0.2, 1.3, 0.0), axis=(1.0, 0.0, 0.0), noise: float = 0.0) -> np.ndarray:
    """A cylinder of ``radius`` along ``axis`` for ``length`` metres, densely sampled."""
    rng = np.random.default_rng(5)
    count = 4000
    origin = np.asarray(origin, dtype=float)
    axis = np.asarray(axis, dtype=float)
    axis = axis / np.linalg.norm(axis)
    perpendicular = np.cross(axis, (0.0, 0.0, 1.0))
    perpendicular = perpendicular / np.linalg.norm(perpendicular)
    other = np.cross(axis, perpendicular)
    along = rng.uniform(0.0, length, count)
    angle = rng.uniform(0.0, 2.0 * np.pi, count)
    r = radius * (1.0 + rng.uniform(-noise, noise, count))
    return origin + along[:, None] * axis + (np.cos(angle) * r)[:, None] * perpendicular + (np.sin(angle) * r)[:, None] * other


def test_the_sliver_check_tells_a_sliver_from_a_limb(humanoid):
    """The measurement is good; what it cannot do is separate the bodies. See below."""
    floor = float(humanoid.section("fit")["limb_radius_min_fraction"])
    assert floor == 0.22

    length = 0.30
    sliver = fitgeom.limb_sections(_limb(length, 0.06), (0.2, 1.3, 0.0), (0.2 + length, 1.3, 0.0))
    good = fitgeom.limb_sections(_limb(length, 0.15), (0.2, 1.3, 0.0), (0.2 + length, 1.3, 0.0))
    assert sliver["ratio"] == pytest.approx(0.06 / length, abs=0.01)
    assert good["ratio"] == pytest.approx(0.15 / length, abs=0.01)
    assert sliver["ratio"] < floor < good["ratio"]
    assert sliver["off_axis_m"] == pytest.approx(0.0, abs=0.005), "a limb on its axis reads no offset"


def test_a_limb_that_points_somewhere_else_reads_as_off_axis_not_as_thin():
    """Two different failures, and only one of them is thinness.

    The sliver body's right arm sat a median 1.47 m off the contract's axis.
    That is a direction failure — no length fixes a direction — and the
    record prints it beside the radius rather than folding it in.
    """
    length = 0.30
    aside = _limb(length, 0.15, origin=(0.2, 1.3, 0.0), axis=(0.0, -1.0, 0.0))
    section = fitgeom.limb_sections(aside, (0.2, 1.3, 0.0), (0.2 + length, 1.3, 0.0))
    assert section["ratio"] is None or section["off_axis_m"] > 0.1


def test_the_cluster_is_what_keeps_the_far_leg_out_of_the_measurement(humanoid):
    """Without clustering, a slab at mid-thigh holds the far leg and the crotch."""
    world = humanoid.rest_world()
    origin, tip = world["LeftUpLeg"][0], world["LeftLeg"][0]
    axis = np.asarray(tip) - np.asarray(origin)
    length = float(np.linalg.norm(axis))
    near = _limb(length, 0.05, origin=origin, axis=tuple(axis / length))
    far = _limb(length, 0.05, origin=(-origin[0], origin[1], origin[2]), axis=tuple(axis / length))
    alone = fitgeom.limb_sections(near, origin, tip)
    together = fitgeom.limb_sections(np.vstack([near, far]), origin, tip)
    assert together["ratio"] == pytest.approx(alone["ratio"], abs=0.01), "the far leg is 19 cm away, and 9 cm of clear air is more than a 3.5 cm link"


def test_a_run_no_station_can_measure_is_null_and_refuses_nothing():
    """Unknown is not thin. A gate that read ``None`` as zero would refuse a hole."""
    empty = np.zeros((3, 3))
    section = fitgeom.limb_sections(empty, (0.0, 1.0, 0.0), (0.3, 1.0, 0.0))
    assert section["ratio"] is None and section["radius_m"] is None
    assert all(station["ratio"] is None for station in section["stations"])


#: Every arm run measured 2026-08-30 on the prepared glbs still on disk, and
#: whether the body it belongs to walks. This is the whole calibration, and
#: the reason the number ships as a note: no threshold cuts the walking
#: bodies from the sliver, because the witch measures below every arm of it.
MEASURED_ARMS = {
    "courier_qwen": ([0.200, 0.206, 0.214, 0.212], False),
    "courier_v2": ([0.505, 0.545, 0.669, 0.674], True),
    "vex_runner": ([0.239, 0.311, 0.608, 0.610], True),
    "drow_warlock": ([0.549, 0.545, 0.682, 0.568], True),
    "moss_witch_v4": ([0.218, 0.160, 0.872, 0.844], True),
}


def test_no_threshold_separates_a_body_that_walks_from_the_one_that_did_not():
    """Why ``limb_radius_min_fraction`` is a printed note and refuses nothing.

    A threshold would have to sit above every arm of ``courier_qwen``, which
    walked as a sliver, and below every arm of the five bodies that walk. The
    witch's right upper arm at 0.160 is below the sliver's thinnest arm at
    0.200, so no such number exists — she is a thin arm inside a wide sleeve,
    and her 1.04 m half-span puts the contract's upper-arm run across the
    sleeve. ``designs/skin.md`` said this is what would happen if a good arm
    landed under 0.22; it did, and this is the measurement that says so.
    """
    sliver = max(MEASURED_ARMS["courier_qwen"][0])
    walking = min(min(arms) for arms, walks in MEASURED_ARMS.values() if walks)
    assert walking < sliver, "a threshold above the sliver would refuse the witch, who ships"
    assert MEASURED_ARMS["moss_witch_v4"][0][1] == 0.160
    # 0.22 does separate the four bodies it was calibrated on, which is why
    # the number stays in the profile and stays in the record.
    without_the_witch = {name: arms for name, (arms, walks) in MEASURED_ARMS.items() if walks and name != "moss_witch_v4"}
    assert all(min(arms) > 0.22 for arms in without_the_witch.values())
    assert sliver < 0.22


def test_the_note_covers_the_arms_and_the_legs_are_measured_beside_them():
    """Neither set gates. ``vex_runner``'s thighs read 0.15 and the sliver's shins 0.30."""
    assert set(prepare.ARM_RUNS) == {
        ("LeftArm", "LeftForeArm"),
        ("RightArm", "RightForeArm"),
        ("LeftForeArm", "LeftHand"),
        ("RightForeArm", "RightHand"),
    }
    assert ("LeftUpLeg", "LeftLeg") in prepare.LEG_RUNS
    assert not set(prepare.ARM_RUNS) & set(prepare.LEG_RUNS)
    assert not hasattr(prepare, "_limb_radius_or_die"), "the sliver check refuses nothing; it prints"


# ------------------------------------------------- what arrives at the door --


class _Body:
    """The two attributes ``_refuse_any_skin`` reads, and nothing else."""

    def __init__(self, groups=(), modifiers=(), parent=None):
        self.name = "Body"
        self.vertex_groups = list(groups)
        self.modifiers = list(modifiers)
        self.parent = parent


class _Group:
    def __init__(self, name):
        self.name = name


class _Modifier:
    def __init__(self, kind):
        self.type = kind


def test_a_skinned_file_arrives_and_is_refused_by_name():
    """The one invariant of this step: the mesh reaches the skinner bare.

    A file that arrived carrying weights would let a run "succeed" on
    weights nobody generated, and the failure would be invisible.
    """
    from forge_gen.exit_codes import BackendFailed

    armature = _Group("Armature")
    with pytest.raises(BackendFailed, match="vertex group"):
        prepare._refuse_any_skin(_Body(groups=[_Group("Hips"), _Group("Spine")]), armature)
    with pytest.raises(BackendFailed, match="Armature modifier"):
        prepare._refuse_any_skin(_Body(modifiers=[_Modifier("ARMATURE")]), armature)
    with pytest.raises(BackendFailed, match="sibling"):
        prepare._refuse_any_skin(_Body(parent=armature), armature)
    prepare._refuse_any_skin(_Body(), armature)


def test_a_dropped_armature_is_refused_before_anything_is_skinned(humanoid):
    """The failure this catches is invisible: a mesh-only glb skins to nothing."""
    from forge_gen.exit_codes import BackendFailed

    every = {"document": {"nodes": [{"name": bone["name"]} for bone in humanoid.bones]}}
    assert prepare._check_armature_survived(every, humanoid) == len(humanoid.bones)

    short = {"document": {"nodes": [{"name": bone["name"]} for bone in humanoid.bones[:-3]]}}
    with pytest.raises(BackendFailed, match="predicted its own skeleton"):
        prepare._check_armature_survived(short, humanoid)


def test_the_frame_conversion_is_the_one_prepare_needs():
    """Blender's +Z up to the frame the contract is stated in."""
    converted = fitgeom.from_blender(np.array([[1.0, 2.0, 3.0]]))
    assert converted.tolist() == [[1.0, 3.0, -2.0]]


# ---------------------------------------------------------------- the fake --


def test_the_fake_writes_bone_nodes_and_no_skin(humanoid, tmp_path):
    from forge_gen import glb, placeholders

    out = tmp_path / "figure.glb"
    placeholders.placeholder_prepared_glb(out, humanoid)
    info = glb.verify_glb(out)
    assert info["skins"] == 0
    names = {node.get("name") for node in info["document"]["nodes"]}
    assert {bone["name"] for bone in humanoid.bones} <= names
    assert info["document"]["accessors"][2]["count"] // 3 == 192, "a box per body segment"
    assert placeholders.is_placeholder(out)
