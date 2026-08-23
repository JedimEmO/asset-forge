"""review.py's --json shape is a frozen contract; this file is the assertion the docstring asked for.

The docstring says "THE JSON SHAPE IS A CONTRACT. A Rust port of these
metrics must keep it" and lists the keys, their order, the sub-shapes and
the intent rule — and until now nothing on either side asserted any of it.
The fake path is checked with nothing installed; the real path is checked
too when numpy is importable (it is a declared dev dependency).
"""

from __future__ import annotations

import json

import pytest

from forge_gen import cli, npz
from forge_gen.motion import review
from tests.conftest import REPO

#: The contract keys, in the order the docstring lists them.
EXPECTED_KEYS = [
    "frames", "fps", "duration_s",
    "path_len_m", "net_travel_m",
    "avg_speed_mps", "peak_speed_mps",
    "start_ratio", "stop_ratio", "moving_frac",
    "foot_skate_mps",
    "contact_frac", "penetration_m", "lowest_foot_m",
    "jitter_mm", "jitter_pct", "activity_m",
    "head_min_m", "head_max_m", "head_end_m",
    "hand_top_m", "hand_reach_m", "lean_max_deg",
    "loop_gap_m", "loop_vel_gap",
    "drift_deg", "net_turn_deg",
    "travel_dir_deg",
    "turn_deg", "left_steps", "action_beats",
    "speed_curve",
    "flags",
    "suggest",
    "prompt", "path",
]

ONESHOT_SUGGEST_KEYS = ["trim_start_s", "trim_end_s", "kept_s"]
LOOP_SUGGEST_KEYS = ["trim_start_s", "trim_end_s", "kept_s", "seam_gap_m", "mean_speed_mps", "speed_cv"]


def parse(argv: list[str]):
    """The production parser, so the args carry exactly the defaults a run gets."""
    args = cli.build_parser().parse_args(argv)
    return args._module, args


@pytest.fixture
def humanoid(monkeypatch):
    monkeypatch.setenv("FORGE_RIG_PROFILE", str(REPO / "rigs" / "humanoid"))


def test_the_docstring_and_this_test_agree_on_every_key():
    for key in EXPECTED_KEYS:
        assert f'"{key}"' in review.__doc__, f"contract key {key} is not in the docstring"


def test_run_fake_emits_the_contract_shape(humanoid, tmp_path):
    take = npz.write_take(tmp_path / "walk__d2_c2_s0_0.npz", frames=8, fps=20, prompt="a walk")
    module, args = parse(["motion", "review", str(take), "--metrics", str(tmp_path / "m.json"), "--fake", "--json"])
    result = module.run_fake(args)
    doc = result["metrics"]
    assert list(doc) == ["walk__d2_c2_s0_0"]
    entry = doc["walk__d2_c2_s0_0"]
    assert list(entry) == EXPECTED_KEYS, "key order is the contract"
    assert list(entry["suggest"]) == ["oneshot", "loop"]
    assert list(entry["suggest"]["oneshot"]) == ONESHOT_SUGGEST_KEYS
    assert list(entry["suggest"]["loop"]) == LOOP_SUGGEST_KEYS
    assert len(entry["speed_curve"]) == 8
    assert entry["flags"] == [] and entry["path"] == str(take)
    with open(tmp_path / "m.json", encoding="utf-8") as handle:
        assert list(json.load(handle)["walk__d2_c2_s0_0"]) == EXPECTED_KEYS, "the written document is the same shape"
    assert result["record"] is None


def test_the_real_path_emits_the_same_shape(humanoid, tmp_path):
    pytest.importorskip("numpy")
    take = npz.write_take(tmp_path / "walk__d2_c2_s0_0.npz", frames=12, fps=20, prompt="a walk")
    result = review.main_inner([str(take), "--metrics", str(tmp_path / "m.json"), "--profile", str(REPO / "rigs" / "humanoid")])
    entry = result["metrics"]["walk__d2_c2_s0_0"]
    assert list(entry) == EXPECTED_KEYS, "run_fake, main_inner and the docstring must say one shape"
    assert list(entry["suggest"]["oneshot"]) == ONESHOT_SUGGEST_KEYS
    assert list(entry["suggest"]["loop"]) == LOOP_SUGGEST_KEYS
    assert len(entry["speed_curve"]) == 8
    # Strict JSON: the document must serialise (no NaN), and null is the spelling for it.
    json.dumps(result["metrics"], allow_nan=False)


def test_oneshot_drops_exactly_the_loop_only_flags():
    thresholds = {key: 0.0 for key in review.THRESHOLDS}
    metrics = {
        "foot_skate_mps": 1.0, "jitter_pct": 1.0, "penetration_m": 1.0, "lowest_foot_m": 1.0,
        "activity_m": -1.0, "avg_speed_mps": -1.0, "loop_gap_m": 1.0, "loop_vel_gap": 1.0,
        "drift_deg": 1.0, "net_turn_deg": 1.0, "peak_speed_mps": 1.0, "stop_ratio": -1.0,
    }
    every = review.flags_for(metrics, thresholds, "loop")
    assert set(every) == {"SKATE", "JITTER", "SINKS", "FLOATS", "STATIC", "SEAM", "SEAMVEL", "DRIFT", "TURNS", "STOPS"}
    oneshot = review.flags_for(metrics, thresholds, "oneshot")
    assert set(every) - set(oneshot) == review.LOOP_ONLY_FLAGS, "--intent oneshot drops SEAM/SEAMVEL/STOPS/DRIFT and nothing else"
    assert set(oneshot) == set(every) - review.LOOP_ONLY_FLAGS


def test_fake_records_none_and_zero_metrics_are_numbers(humanoid, tmp_path):
    take = npz.write_take(tmp_path / "t.npz", frames=4, fps=20)
    module, args = parse(["motion", "review", str(take), "--fake"])
    entry = module.run_fake(args)["metrics"]["t"]
    numeric = [key for key in EXPECTED_KEYS if key not in ("flags", "suggest", "prompt", "path", "speed_curve")]
    assert all(isinstance(entry[key], (int, float)) for key in numeric)
