"""The one rule the fitted skeleton trades: lengths are the body's, directions are not.

``forge gen export`` is the only door in the toolkit with a rest-*translation*
rule, so it is the only door Phase 2 had to change. It used to hold every
bone within 0.1 mm of the contract's rest pose, which the fitted witch missed
by 252 mm on 55 bones — correctly, under the old rule, and uselessly, under
the new design. What it holds now is the **direction** of each bone's local
rest translation, its **length ratio**, and the position of a segment too
short to have a direction.

The arithmetic runs outside Blender, so it is tested here; the real gate was
run under Blender on both bodies that matter, and both pass: the shipped
``vex_runner``, and the fitted witch whose 55 problems this rule exists to
stop being problems.
"""

from __future__ import annotations

import math

import pytest

from forge_gen import profile as profile_mod
from forge_gen.blender import export
from forge_gen.exit_codes import UsageError


@pytest.fixture
def humanoid(repo_root):
    return profile_mod.load_profile(repo_root / "rigs" / "humanoid")


@pytest.fixture
def spec(humanoid):
    return {
        "rest_tolerance": float(humanoid.section("export")["rest_tolerance_m"]),
        **export._bone_tolerances(humanoid),
    }


def test_the_tolerances_come_from_the_contract_when_it_carries_them(humanoid, monkeypatch):
    """``profile.toml`` is the source and ``contract.json`` its projection; the
    contract is asked first because it is what the Rust half reads."""
    from_profile = export._bone_tolerances(humanoid)
    assert from_profile == {
        "rest_direction_tolerance_deg": 1.0,
        "length_ratio_min": 0.4,
        "length_ratio_max": 2.5,
        "rest_zero_length_m": 0.0001,
    }
    humanoid.contract["rest_direction_tolerance_deg"] = 1.0
    assert export._bone_tolerances(humanoid)["rest_direction_tolerance_deg"] == 1.0
    humanoid.contract["rest_direction_tolerance_deg"] = 3.0
    assert export._bone_tolerances(humanoid)["rest_direction_tolerance_deg"] == 3.0, "the contract wins where it speaks"
    del humanoid.contract["rest_direction_tolerance_deg"]


def test_a_profile_that_names_neither_is_refused_by_name(humanoid):
    """A rule with no number is worse than no rule; it would silently pass everything."""
    kept = humanoid.toml["export"].pop("length_ratio_min")
    from_contract = humanoid.contract.pop("length_ratio_min", None)
    try:
        with pytest.raises(UsageError, match="length_ratio_min"):
            export._bone_tolerances(humanoid)
    finally:
        humanoid.toml["export"]["length_ratio_min"] = kept
        if from_contract is not None:
            humanoid.contract["length_ratio_min"] = from_contract


# --------------------------------------------------------------- one segment --


def _problems(want, got, spec):
    out: list[str] = []
    export._check_one_segment("Head", want=want, got=got, spec=spec, problems=out)
    return out


def test_a_bone_at_the_contract_s_own_length_passes(spec):
    assert _problems((0.0, 0.094, -0.009), (0.0, 0.094, -0.009), spec) == []


def test_a_longer_bone_in_the_same_direction_passes(spec):
    """Bone lengths belong to the body. That is the whole decision."""
    for ratio in (0.45, 0.8, 1.0, 1.6, 2.4):
        scaled = tuple(ratio * component for component in (0.0, 0.094, -0.009))
        assert _problems((0.0, 0.094, -0.009), scaled, spec) == [], ratio


def test_a_rotated_bone_is_refused_and_the_message_says_why(spec):
    """The refusal designs/skin.md writes out, to the tenth of a degree."""
    want = (0.0, 0.094, -0.009)
    length = math.hypot(want[1], want[2])
    turned = math.atan2(want[2], want[1]) + math.radians(2.3)
    got = (0.0, length * math.cos(turned), length * math.sin(turned))
    problems = _problems(want, got, spec)
    assert len(problems) == 1
    message = problems[0]
    assert "points 2.3 deg off the contract's" in message
    assert "+0.000, +0.094, -0.005" in message and "+0.000, +0.094, -0.009" in message
    assert "clips are baked against rest ROTATIONS" in message.replace("Clips", "clips")
    assert "a rotated bone binds perfectly and animates wrongly" in message
    assert "Lengths are yours; directions are not." in message


def test_the_direction_tolerance_is_where_the_profile_says_it_is(spec):
    """1.0 deg is a generous ceiling on a quantity the spike measured at 0.0000."""
    want = (0.0, 0.1, 0.0)
    inside = (math.sin(math.radians(0.9)) * 0.1, math.cos(math.radians(0.9)) * 0.1, 0.0)
    outside = (math.sin(math.radians(1.1)) * 0.1, math.cos(math.radians(1.1)) * 0.1, 0.0)
    assert _problems(want, inside, spec) == []
    assert len(_problems(want, outside, spec)) == 1


def test_a_collapsed_bone_is_refused_and_a_short_one_is_not(spec):
    want = (0.0, 0.4, 0.0)
    assert _problems(want, (0.0, 0.4 * 0.41, 0.0), spec) == [], "a short body is a short body"
    collapsed = _problems(want, (0.0, 0.4 * 0.02, 0.0), spec)
    assert len(collapsed) == 1 and "collapsed skeleton, not a short body" in collapsed[0]
    stretched = _problems(want, (0.0, 0.4 * 3.0, 0.0), spec)
    assert len(stretched) == 1 and "3.00x the contract's length" in stretched[0]


def test_a_zero_length_segment_is_compared_by_position(spec):
    """It has no direction to check, so it keeps the rule it always had."""
    zero = (0.0, 0.0, 0.0)
    assert _problems(zero, (0.0, 0.00005, 0.0), spec) == [], "inside 0.1 mm"
    moved = _problems(zero, (0.0, 0.00005, 0.002), spec)
    assert len(moved) == 1 and "no direction to check" in moved[0]
    # And a bone the contract puts on its parent, that this body moved off it.
    off = _problems(zero, (0.0, 0.05, 0.0), spec)
    assert len(off) == 1 and "sits on its parent in the contract" in off[0]


def test_the_length_rule_and_the_direction_rule_are_reported_together(spec):
    """An artist fixing two things wants two lines, not two round trips."""
    turned = math.radians(3.0)
    problems = _problems((0.0, 0.1, 0.0), (0.03 * math.sin(turned), 0.03 * math.cos(turned), 0.0), spec)
    assert len(problems) == 2
    assert any("deg off" in line for line in problems)
    assert any("contract's length" in line for line in problems)


def test_the_helpers_are_the_plain_arithmetic_they_look_like():
    assert export._subtract((1.0, 2.0, 3.0), (0.5, 0.5, 0.5)) == (0.5, 1.5, 2.5)
    assert export._length((3.0, 4.0, 0.0)) == 5.0
    assert export._angle_deg((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)) == pytest.approx(90.0)
    assert export._angle_deg((1.0, 0.0, 0.0), (1.0, 0.0, 0.0)) == pytest.approx(0.0, abs=1e-9)
    assert export._angle_deg((1.0, 0.0, 0.0), (-1.0, 0.0, 0.0)) == pytest.approx(180.0), "and it does not fall off the arccos"


def test_every_contract_bone_is_a_segment_with_a_direction_or_a_zero(humanoid, spec):
    """On the profile's own skeleton the rule is vacuously true, and says so.

    Which is the point: the contract compared against itself must produce no
    findings at all, or the gate is measuring its own reference wrongly.
    """
    world = humanoid.rest_world()
    zero_length = 0
    problems: list[str] = []
    for bone in humanoid.bones:
        parent = bone.get("parent")
        head = world[bone["name"]][0]
        parent_head = world[humanoid.bones[parent]["name"]][0] if parent is not None else (0.0, 0.0, 0.0)
        segment = export._subtract(head, parent_head)
        if export._length(segment) < spec["rest_zero_length_m"]:
            zero_length += 1
        export._check_one_segment(bone["name"], want=segment, got=segment, spec=spec, problems=problems)
    assert problems == []
    assert zero_length == 0, "no contract bone sits exactly on its parent, so every one has a direction"
