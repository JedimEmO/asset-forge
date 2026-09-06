"""Recorded loop derivation, on real PCM without a model."""
import array
import hashlib
import wave
from pathlib import Path

import pytest

from forge_gen.audio import music
from forge_gen.exit_codes import BackendFailed, InputRejected


def pcm(path, frames=4000):
    samples = array.array("h", range(frames))
    with wave.open(str(path), "wb") as handle:
        handle.setparams((1, 2, 1000, frames, "NONE", "not compressed"))
        handle.writeframes(samples.tobytes())


@pytest.mark.parametrize("values", [(None, 2, .1), (0, 2, None), (-1, 2, .1), (0, 0, .1), (0, 2, 2), (0, 2, 0), (0, float("nan"), .1), (0, 2, float("inf")), (9, 2, .1)])
def test_invalid_loop_refused(values):
    with pytest.raises(InputRejected):
        music.check_loop(*values, 10)


def test_period_seam_and_source_integrity(tmp_path):
    source, out = tmp_path / "source.wav", tmp_path / "loop.wav"
    pcm(source)
    before = hashlib.sha256(source.read_bytes()).hexdigest()
    recipe = music.check_loop(.5, 2, .1, 4)
    music.derive_loop(source, out, recipe)
    assert hashlib.sha256(source.read_bytes()).hexdigest() == before
    with wave.open(str(out), "rb") as handle:
        assert handle.getnframes() == 2000
        data = array.array("h", handle.readframes(2000))
    assert data[0] == 2500
    assert data[-1] == 2499
    assert data[50] == 1550
    assert data[100] == 600
    assert music.measure_wav(out)["duration_s"] == 2


def test_actual_source_and_subframe_overlap_refused(tmp_path):
    source, out = tmp_path / "source.wav", tmp_path / "loop.wav"
    pcm(source, 2000)
    for recipe in [music.check_loop(0, 2, .1, 10), music.check_loop(0, 1, .0001, 10)]:
        with pytest.raises(BackendFailed, match="actual source frames"):
            music.derive_loop(source, out, recipe)
    assert not out.exists()


def test_record_hashes_source_and_fake_does_not_claim_transform(tmp_path):
    source, out, record = (tmp_path / name for name in ("source.wav", "loop.wav", "loop.json"))
    pcm(source)
    request = {"loop": music.check_loop(0, 2, .1, 4), "loop_source": source,
               "prompt": "test", "format": "wav", "duration_s": 4}
    music.derive_loop(source, out, request["loop"])
    rec = music.build_record(request, {})
    music.finish(rec, request, out, record)
    assert rec["params"]["loop"]["applied"] is True
    assert next(row for row in rec["inputs"] if row["role"] == "loop_source")["sha256"] == "sha256:" + hashlib.sha256(source.read_bytes()).hexdigest()
    assert music.build_record(request, {}, fake=True)["params"]["loop"]["applied"] is False


def test_cli_validation_and_fake_run(tmp_path):
    import argparse
    import json
    parser = argparse.ArgumentParser()
    music.add_parser(parser.add_subparsers(dest="command"))
    args = parser.parse_args(["music", "--prompt", "steady", "--out", str(tmp_path / "loop.wav"),
        "--record", str(tmp_path / "loop.json"), "--duration", "30",
        "--loop-start", "2", "--loop-duration", "16", "--loop-crossfade", "0.5"])
    assert music.check_inputs(args)["loop"]["duration_s"] == 16
    result = music.run_fake(args)
    record = json.loads(Path(result["record"]).read_text())
    assert record["fake"] is True
    assert record["params"]["loop"]["applied"] is False
    assert not any(row["role"] == "loop_source" for row in record["inputs"])
    assert not list(tmp_path.glob("*.source.*"))
    args.loop_start = float("nan")
    with pytest.raises(InputRejected, match="finite"):
        music.check_inputs(args)
