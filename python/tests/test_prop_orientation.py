"""Prop pose corrections are explicit inputs, retained across the Blender door."""
import argparse
import pytest
from forge_gen.blender import prop
from forge_gen.exit_codes import UsageError


def args(repo_root, pitch, heading=0):
    parser = argparse.ArgumentParser()
    prop._add_arguments(parser)
    return parser.parse_args(["input.glb", "--out", "out.glb", "--record", "out.json",
                              "--height", "2", "--yaw-deg", "180", f"--pitch-deg={pitch}", f"--heading-deg={heading}",
                              "--profile", str(repo_root / "rigs/humanoid")])


def test_pitch_is_retained_in_the_normalization_recipe(repo_root):
    params = prop._params(prop._spec(args(repo_root, -25, 27.5)))
    assert params["pitch_deg"] == -25
    assert params["yaw_deg"] == 180
    assert params["heading_deg"] == 27.5
    assert params["placement"] == "floor"


@pytest.mark.parametrize("pitch", ["nan", "inf", "-inf"])
def test_invalid_pitch_is_refused_before_blender(repo_root, pitch):
    with pytest.raises(UsageError, match="pitch-deg must be finite"):
        prop._spec(args(repo_root, pitch))


@pytest.mark.parametrize("heading", ["nan", "inf", "-inf"])
def test_invalid_heading_is_refused_before_blender(repo_root, heading):
    with pytest.raises(UsageError, match="heading-deg must be finite"):
        prop._spec(args(repo_root, 0, heading))
