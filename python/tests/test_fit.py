"""The fit, against the five reports the real runs wrote.

``python/tests/fixtures/fit/`` holds fit reports from three bodies, copied
out of ``out/spike_fit/`` and ``out/fit_warlock/`` unedited — the skinned
glbs they were measured from are 3 MB each and are not committed, so the
reports are the evidence. Every calibrated number in ``profile.toml``'s
``[fit]`` came from these files, and these tests are what stops one of them
drifting away from what it was measured on:

``vex_runner.fit.json``
    the shipped body, fitted with the **weights** placing the root. It asks
    for a ``motion_scale`` of 1.0605 and is why the root now comes from
    geometry, and its arms disagree left to right by 24 %, which is why a
    10 % symmetry rule refuses the body that ships.
``vex_runner_geometry_root.fit.json``
    the same weights re-read by the door as it stands. ``motion_scale``
    1.0000.
``moss_witch_v4_pass1.fit.json`` / ``_pass2.fit.json``
    the four-head witch, fitted and then fitted again after a re-skin. The
    second pass is the reason the door fits **once**.
``drow_warlock.fit.json``
    the widest left/right arm disagreement measured on a body that walks:
    28.7 %, which is what ``asymmetry_arms = 0.35`` is calibrated against.
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np
import pytest

from forge_gen import fit as fit_mod
from forge_gen import profile as profile_mod
from forge_gen.exit_codes import InputRejected, UsageError

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "fit"


def _report(name: str) -> dict:
    return json.loads((FIXTURES / f"{name}.fit.json").read_text(encoding="utf-8"))


@pytest.fixture
def humanoid(repo_root):
    return profile_mod.load_profile(repo_root / "rigs" / "humanoid")


# ------------------------------------------------------- the frozen reports --


@pytest.mark.parametrize(
    "name,expected",
    [
        ("vex_runner", {"Hips->Neck": 0.8096, "LeftArm->LeftForeArm": 0.8774, "LeftUpLeg->LeftLeg": 1.2327}),
        ("vex_runner_geometry_root", {"Hips->Neck": 0.9053, "LeftForeArm->LeftHand": 1.1561}),
        ("moss_witch_v4_pass1", {"Hips->Neck": 0.7311, "LeftArm->LeftForeArm": 0.8497, "LeftFoot->LeftToeBase": 0.8182}),
        ("drow_warlock", {"Hips->Neck": 0.7789, "LeftForeArm->LeftHand": 0.9928}),
    ],
)
def test_the_frozen_reports_still_say_what_they_said(name, expected):
    """The ratios these reports carry are the calibration; nothing may edit them quietly."""
    runs = {run["run"]: run for run in _report(name)["runs"]}
    for run, ratio in expected.items():
        assert runs[run]["ratio"] == pytest.approx(ratio, abs=1e-4), run


def test_a_ratio_and_the_joints_it_placed_agree():
    """Re-derive every fitted joint from the run ratios and land on the report's own.

    The report states both the per-bone ratio and the joint it put each bone
    at; if they ever disagree, some consumer is reading a number that was
    never used. ``fit_rig`` scales the segments rather than reading the
    joints, so this is the check that the two descriptions are one.
    """
    report = _report("vex_runner_geometry_root")
    ratios = {row["bone"]: row["ratio"] for row in report["bones"]}
    parent_of = {row["bone"]: row.get("parent") for row in report["bones"]}
    reference = {name: np.array(value) for name, value in report["reference_joints"].items()}
    fitted = {report["root"]: np.array(report["fitted_joints"][report["root"]])}
    pending = [row["bone"] for row in report["bones"] if parent_of[row["bone"]] is not None]
    while pending:
        placed = [bone for bone in pending if parent_of[bone] in fitted]
        assert placed, f"{len(pending)} bone(s) hang off nothing: {pending[:5]}"
        for bone in placed:
            parent = parent_of[bone]
            fitted[bone] = fitted[parent] + ratios[bone] * (reference[bone] - reference[parent])
            pending.remove(bone)
    for bone, place in fitted.items():
        assert place == pytest.approx(report["fitted_joints"][bone], abs=2e-5), bone


# ------------------------------------------------------------- the two bands --


def test_the_symmetry_gate_refuses_040_on_an_arm_and_passes_0287(humanoid):
    """0.287 is a body that walks; 0.40 is not a body, it is a bad measurement."""
    bands = fit_mod.symmetry_bands(humanoid)
    assert bands == {"asymmetry_arms": 0.35, "asymmetry_other": 0.20}

    passes = _synthetic_pair(1.0, gap=0.287)
    problems, _warnings = fit_mod.gate(passes, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
    assert problems == [], "the warlock's forearm is 28.7% apart and he walks"

    refuses = _synthetic_pair(1.0, gap=0.40)
    problems, _warnings = fit_mod.gate(refuses, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
    assert len(problems) == 1 and "40.0% apart" in problems[0] and "past the 35%" in problems[0]


def test_the_wider_band_is_the_arms_only(humanoid):
    """A hip 28.7% apart is refused where a forearm 28.7% apart is not."""
    bands = fit_mod.symmetry_bands(humanoid)
    assert fit_mod.asymmetry_band("LeftForeArm", **bands) == 0.35
    assert fit_mod.asymmetry_band("LeftArm", **bands) == 0.20
    assert fit_mod.asymmetry_band("LeftUpLeg", **bands) == 0.20

    hips = _synthetic_pair(1.0, gap=0.287, end="LeftUpLeg", start="Hips")
    problems, _warnings = fit_mod.gate(hips, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
    assert len(problems) == 1 and "past the 20%" in problems[0]


def test_the_measured_worst_on_three_real_bodies_is_inside_the_two_bands(humanoid):
    """Where 0.35 and 0.20 came from, held to the files they came from."""
    bands = fit_mod.symmetry_bands(humanoid)
    worst = {"arms": 0.0, "other": 0.0}
    for name in ("vex_runner_geometry_root", "moss_witch_v4_pass1", "drow_warlock"):
        report = _report(name)
        problems, _warnings = fit_mod.gate(report, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
        assert problems == [], f"{name} is a body that walks and the gate must not refuse it: {problems}"
        for gap, kind in _gaps(report):
            worst[kind] = max(worst[kind], gap)
    assert worst["arms"] == pytest.approx(0.287, abs=0.002), "the warlock's forearm is the widest arm measured"
    assert worst["other"] < bands["asymmetry_other"], "and the widest hip is inside 20%"
    assert worst["arms"] > bands["asymmetry_other"], "which is exactly why the arms need their own band"


def _gaps(report: dict):
    ends = {run["end"]: run for run in report["runs"]}
    for end, run in sorted(ends.items()):
        if not end.startswith("Left"):
            continue
        mirror = ends.get("Right" + end[len("Left") :])
        if mirror is None:
            continue
        mean = 0.5 * (run["ratio_measured"] + mirror["ratio_measured"])
        if abs(mean) <= 1e-6:
            continue
        gap = abs(run["ratio_measured"] - mirror["ratio_measured"]) / abs(mean)
        yield gap, "arms" if end in fit_mod.ARM_LANDMARKS else "other"


def _synthetic_pair(base: float, *, gap: float, end: str = "LeftForeArm", start: str = "LeftArm") -> dict:
    """A two-run report whose left/right pair disagrees by exactly ``gap``."""
    mirror_end = "Right" + end[len("Left") :]
    mirror_start = start if not start.startswith("Left") else "Right" + start[len("Left") :]
    left = base * (1.0 + gap / 2.0)
    right = base * (1.0 - gap / 2.0)
    return {
        "runs": [
            {"run": f"{start}->{end}", "end": end, "reference_length_m": 0.3, "ratio": base, "ratio_measured": left, "support": 100.0, "off_axis_m": 0.0},
            {"run": f"{mirror_start}->{mirror_end}", "end": mirror_end, "reference_length_m": 0.3, "ratio": base, "ratio_measured": right, "support": 100.0, "off_axis_m": 0.0},
        ],
        "grounding": {},
    }


# --------------------------------------------------------------- the root --


def test_the_root_comes_from_geometry_not_from_the_weights():
    """The whole of the 2026-08-30 correction, in one file's numbers.

    The weight band puts ``vex_runner``'s hip line at 0.9845 m against the
    contract's 0.9267 — 5.8 cm high — which is a ``motion_scale`` of 1.0605
    for a body that already ships. Read off the body's own floor and stature
    it lands on 1.0000, and the weight band is still in the report as the
    cross-check it now is.
    """
    weights = _report("vex_runner")
    geometry = _report("vex_runner_geometry_root")
    assert weights["motion_scale"] == 1.0605
    assert geometry["motion_scale"] == 1.0

    row = next(r for r in geometry["bones"] if r["bone"] == geometry["root"])
    assert row["source_of_height"] == "geometry"
    assert row["hip_line_weights_m"] == 0.9845, "still measured, still reported, no longer used"
    assert row["hip_line_weights_m"] - row["hip_line_contract_m"] == pytest.approx(0.058, abs=0.001)
    assert geometry["sources"] == {"limbs": "weights", "root": "geometry", "shoulder_line": "geometry", "ground": "geometry"}


def test_the_root_is_read_off_the_body_that_arrives(humanoid):
    """A body prepared at four fifths of the reference stature gets a root to match.

    Run against a synthetic skin whose weights are deliberately useless for
    the root: every vertex belongs wholly to ``Hips``, so the weight band has
    nothing to say, and the answer still comes out right because it never
    asked the weights.
    """
    reference = float(humanoid.section("bones")["reference_stature_m"])
    for stature in (reference, 0.8 * reference):
        points, dense = _one_bone_body(humanoid, stature)
        report = fit_mod.fit(points, dense, humanoid, min_support=8.0, symmetry=True, ground=False)
        assert report["motion_scale"] == pytest.approx(stature / reference, abs=1e-3)
        row = next(r for r in report["bones"] if r["bone"] == report["root"])
        assert row["measured_height_m"] == pytest.approx(stature, abs=1e-3)


def _one_bone_body(prof, stature: float):
    """A cloud spanning ``stature``, every vertex weighted wholly to the root."""
    rng = np.random.default_rng(7)
    points = rng.uniform(-0.4, 0.4, size=(600, 3))
    points[:, 1] = rng.uniform(0.0, stature, size=600)
    points[0, 1], points[1, 1] = 0.0, stature
    dense = np.zeros((len(points), len(prof.bones)))
    dense[:, prof.bone_index(prof.root)] = 1.0
    return points, dense


# ----------------------------------------------------------- the landmarks --


def test_the_landmarks_come_from_the_profile(humanoid, tmp_path):
    assert fit_mod.landmarks_of(humanoid) == tuple(humanoid.section("fit")["landmarks"])
    assert "Neck" in fit_mod.landmarks_of(humanoid)
    assert "Head" not in fit_mod.landmarks_of(humanoid), "the contract's Head joint is inside the skull; the weights' boundary is not it"

    stripped = _profile_without(humanoid, tmp_path, "landmarks")
    with pytest.raises(UsageError, match="landmarks"):
        fit_mod.landmarks_of(stripped)


def _profile_without(prof, tmp_path, key: str):
    import copy

    other = copy.copy(prof)
    other.toml = json.loads(json.dumps({k: v for k, v in prof.toml.items()}))
    other.toml["fit"] = {k: v for k, v in prof.toml["fit"].items() if k != key}
    return other


# ------------------------------------------------------------ convergence --


def test_convergence_measures_the_downhill_walk_that_killed_the_second_pass():
    """Why there is no ``--passes``: the second fit does not converge, it drifts.

    ``convergence()`` has no caller in the door and this is what keeps it
    honest — the numbers it reports are the argument for fitting once.
    """
    pass1 = _report("moss_witch_v4_pass1")
    pass2 = _report("moss_witch_v4_pass2")
    moves = fit_mod.convergence(pass2, pass1)
    assert moves["joints"] == 55
    assert moves["mean_move_mm"] == pytest.approx(73.5, abs=0.5), "a converging second pass would move nothing"
    assert moves["max_move_mm"] == pytest.approx(118.1, abs=0.5)
    assert moves["max_move_bone"] == "Head", "and what walks is the torso, carrying the head with it"
    assert moves["per_bone_mm"]["Spine3"] > 80.0 and moves["per_bone_mm"]["Hips"] > 55.0
    assert pass2["motion_scale"] < pass1["motion_scale"], "each pass shortens the body it just measured"


def test_convergence_of_a_report_against_itself_is_zero():
    report = _report("moss_witch_v4_pass1")
    moves = fit_mod.convergence(report, report)
    assert moves["max_move_mm"] == 0.0 and moves["mean_move_mm"] == 0.0
    assert moves["ratio_changes"] == {run["run"]: 0.0 for run in report["runs"]}


# ------------------------------------------------------- the rest of the gate --


def test_the_gate_refuses_a_collapsed_run_and_a_thin_band(humanoid):
    bands = fit_mod.symmetry_bands(humanoid)
    report = _synthetic_pair(1.0, gap=0.0)
    report["runs"][0]["ratio"] = 0.02
    report["runs"][1]["support"] = 3.0
    problems, _warnings = fit_mod.gate(report, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
    assert any("0.02x its reference length" in line for line in problems)
    assert any("3.0 effective vertices" in line for line in problems)


def test_an_off_axis_landmark_warns_and_never_refuses(humanoid):
    """No length fixes a direction — and saying so is not the same as refusing."""
    bands = fit_mod.symmetry_bands(humanoid)
    report = _synthetic_pair(1.0, gap=0.0)
    report["runs"][0]["off_axis_m"] = 0.9 * report["runs"][0]["reference_length_m"]
    problems, warnings = fit_mod.gate(report, ratio_min=0.4, ratio_max=2.5, min_support=8.0, **bands)
    assert problems == []
    assert len(warnings) == 1 and "off the run's frozen axis" in warnings[0]


def test_the_grounding_factors_of_three_real_bodies_are_inside_the_warning_band():
    """0.7–1.4 was chosen to hold every body measured; it does."""
    for name in ("vex_runner_geometry_root", "moss_witch_v4_pass1", "drow_warlock"):
        for side, values in _report(name)["grounding"].items():
            assert 0.7 <= values["factor"] <= 1.4, f"{name} {side} {values['factor']}"


def test_a_fit_report_names_every_bone_the_contract_has(humanoid):
    for name in ("vex_runner_geometry_root", "moss_witch_v4_pass1", "drow_warlock"):
        report = _report(name)
        assert {row["bone"] for row in report["bones"]} == {bone["name"] for bone in humanoid.bones}
        assert set(report["fitted_joints"]) == set(report["reference_joints"])


def test_symmetrise_takes_the_mean_and_leaves_the_raw_measurement_alone():
    """Symmetrising must hide nothing: the gate reads ``ratio_measured``."""
    runs = [
        {"run": "LeftArm->LeftForeArm", "end": "LeftForeArm", "ratio": 0.9, "ratio_measured": 0.9},
        {"run": "RightArm->RightForeArm", "end": "RightForeArm", "ratio": 0.7, "ratio_measured": 0.7},
    ]
    fit_mod._symmetrise(runs)
    assert runs[0]["ratio"] == runs[1]["ratio"] == pytest.approx(0.8)
    assert (runs[0]["ratio_measured"], runs[1]["ratio_measured"]) == (0.9, 0.7)


def test_the_centroid_rests_on_an_effective_sample_size_not_a_count():
    """One vertex carrying all the weight is a support of 1, however many are in the cloud."""
    points = np.array([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]])
    own = np.array([1.0, 1e-9, 1e-9])
    child = np.array([1.0, 1e-9, 1e-9])
    centroid, support, squareness = fit_mod._centroid(points, own, child)
    assert support == pytest.approx(1.0, abs=1e-6)
    assert centroid[1] == pytest.approx(0.0, abs=1e-6)
    assert squareness == pytest.approx(1.0, abs=1e-6)
    assert fit_mod._centroid(points, np.zeros(3), np.zeros(3)) == (None, 0.0, 0.0)


def test_a_fit_that_is_not_trustworthy_is_an_input_rejection(humanoid, tmp_path):
    """Exit 4, with the report named: the numbers are written before the refusal."""
    err = InputRejected("the fit is not trustworthy (1 problem(s))", report=str(tmp_path / "x.fit.json"))
    assert err.code == 4 and err.payload()["report"].endswith("x.fit.json")


def test_the_landmark_set_and_the_run_table_read_the_same_report():
    report = _report("vex_runner_geometry_root")
    table = fit_mod.run_table(report).splitlines()
    assert table[0].startswith("run")
    assert len(table) == len(report["runs"]) + 1
    assert math.isclose(report["runs"][0]["reference_length_m"], 0.60272, abs_tol=1e-5)
