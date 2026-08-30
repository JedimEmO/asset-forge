"""The music gain: a stated knob in the graph, refused rather than rounded.

ACE-Step 1.5 turbo normalises to peak. Nine renders off this host on
2026-08-30 came back pinned at 0.0 dBFS with runs of 10 to 186 full-scale
samples, and the clipping gate refused every one of them. The gate is right;
the fix is upstream of the file, which here means a gain node inside the
tracked graph whose value the record states — never a normalise applied to a
shipped WAV, which would be the hand-repair of a derived artefact.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from forge_gen import comfy
from forge_gen.audio import music
from forge_gen.exit_codes import InputRejected

WORKFLOW = Path("backends/acestep/workflows/music.api.json")


def graph(repo_root: Path) -> dict:
    return json.loads((repo_root / WORKFLOW).read_text(encoding="utf-8"))


def test_the_gain_node_sits_between_the_decode_and_the_save(repo_root: Path):
    """12 (VAEDecodeAudio) -> 14 (AudioAdjustVolume) -> 13 (SaveAudio)."""
    nodes = graph(repo_root)
    assert nodes["14"]["class_type"] == "AudioAdjustVolume"
    assert nodes["14"]["inputs"]["audio"] == ["12", 0]
    assert nodes["13"]["inputs"]["audio"] == ["14", 0]
    assert nodes["12"]["class_type"] == "VAEDecodeAudio"
    assert nodes["13"]["class_type"] == "SaveAudio"


def test_the_gain_is_a_patch_point_named_gain_db(repo_root: Path):
    points = comfy.patch_points(graph(repo_root), str(WORKFLOW))
    assert points["gain_db"] == ("14", "volume")


def test_the_template_ships_the_default_the_door_ships(repo_root: Path):
    """A template saved at another value would render at it whenever a run is cached."""
    assert graph(repo_root)["14"]["inputs"]["volume"] == music.DEFAULT_GAIN_DB


def test_an_unstated_gain_is_the_default_and_lands_in_the_graph_and_the_record():
    request = {"gain_db": music.check_gain_db(None), "prompt": "p", "lyrics": music.INSTRUMENTAL,
               "duration_s": 30.0, "seed": 1, "bpm": None, "keyscale": None, "timesignature": None,
               "thinking": True, "format": "wav"}
    inputs = music.template_inputs(request, seed=1, prefix="audio/x")
    assert inputs["gain_db"] == music.DEFAULT_GAIN_DB
    assert isinstance(inputs["gain_db"], int)
    record = music.build_record(request, {"metas": {}})
    assert record["params"]["gain_db"] == music.DEFAULT_GAIN_DB


def test_a_fractional_gain_is_refused_by_name_and_never_rounded():
    with pytest.raises(InputRejected) as caught:
        music.check_gain_db("-2.5")
    message = caught.value.message
    assert "AudioAdjustVolume" in message
    assert "integer volume" in message
    assert "rounded to -2" in message


def test_a_gain_outside_the_node_s_range_is_refused():
    with pytest.raises(InputRejected):
        music.check_gain_db(-120)
    with pytest.raises(InputRejected):
        music.check_gain_db("loud")


def test_a_whole_gain_passes_however_it_is_written():
    assert music.check_gain_db("-6") == -6
    assert music.check_gain_db("-6.0") == -6
    assert music.check_gain_db(0) == 0
