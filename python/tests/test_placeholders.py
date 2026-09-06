"""The --fake outputs pass the validators their real counterparts must — and are recognisable as fakes."""

from __future__ import annotations

import json
import struct
import wave
import zlib

import pytest

from forge_gen import glb, npz, placeholders, records
from forge_gen.exit_codes import UsageError
from tests.conftest import REPO


def test_placeholder_wav_is_pcm16_audible_and_marked(tmp_path):
    path = placeholders.placeholder_wav(tmp_path / "s.wav", seconds=0.5, rate=48000)
    with wave.open(str(path)) as handle:
        assert handle.getnchannels() == 1 and handle.getsampwidth() == 2 and handle.getframerate() == 48000
        assert handle.getnframes() == 24000
        frames = handle.readframes(24000)
    samples = struct.unpack("<24000h", frames)
    peak = max(abs(s) for s in samples)
    # Not silence (forge audio inspect calls a silent file defective), not
    # "very quiet" (< -18 dBFS warns), nowhere near full scale.
    assert 0.1 * 32767 < peak < 0.5 * 32767
    # The first audible sample arrives within 50 ms, or a one-shot "feels late".
    floor = int(0.001 * 32767) + 1
    first = next(i for i, s in enumerate(samples) if abs(s) >= floor)
    assert first < 0.05 * 48000
    # The tail fades out rather than ending on a cliff.
    assert abs(samples[-1]) < floor
    assert placeholders.is_placeholder(path)


def test_placeholder_wav_mark_survives_stdlib_reading_and_channels(tmp_path):
    stereo = placeholders.placeholder_wav(tmp_path / "st.wav", seconds=0.25, rate=24000, channels=2)
    with wave.open(str(stereo)) as handle:
        assert handle.getnchannels() == 2 and handle.getnframes() == 6000
    assert placeholders.is_placeholder(stereo)


def test_tile_png_decodes(tmp_path):
    path = placeholders.tile_png(tmp_path / "sheet.png", columns=4, rows=2, cell=3)
    data = path.read_bytes()
    assert data.startswith(b"\x89PNG\r\n\x1a\n")
    width = int.from_bytes(data[16:20], "big")
    height = int.from_bytes(data[20:24], "big")
    assert (width, height) == (12, 6)
    idat_at = data.index(b"IDAT")
    length = int.from_bytes(data[idat_at - 4 : idat_at], "big")
    raw = zlib.decompress(data[idat_at + 4 : idat_at + 4 + length])
    assert len(raw) == height * (1 + width * 4)


def test_placeholder_glb_verifies(tmp_path):
    path = placeholders.placeholder_glb(tmp_path / "p.glb", name="barrel")
    assert glb.verify_glb(path)["meshes"] == 1
    assert placeholders.is_placeholder(path)


def test_fake_record_says_so():
    rec = placeholders.fake_record("lift", "trellis2", backend="trellis2", created_by="human", model="microsoft/TRELLIS.2-4B")
    assert rec["fake"] is True and rec["backend"]["commit"] == "fake" and rec["backend"]["name"] == "trellis2"
    assert rec["created_by"] == "human"
    records.normalize(rec)


def test_requested_reads_the_flag_or_the_environment(monkeypatch):
    class Args:
        fake = False

    monkeypatch.delenv("FORGE_FAKE", raising=False)
    assert not placeholders.requested(Args())
    monkeypatch.setenv("FORGE_FAKE", "1")
    assert placeholders.requested(Args())
    monkeypatch.setenv("FORGE_FAKE", "0")
    assert not placeholders.requested(Args())
    Args.fake = True
    assert placeholders.requested(Args())


def test_is_placeholder_recognises_each_kind_and_nothing_else(tmp_path, monkeypatch):
    monkeypatch.setenv("FORGE_RIG_PROFILE", str(REPO / "rigs" / "humanoid"))
    blend = placeholders.placeholder_blend(tmp_path / "p.blend")
    take = npz.write_take(tmp_path / "p.npz", frames=4, fps=20)
    fake_json = tmp_path / "p.json"
    records.write(placeholders.fake_record("sfx", "moss_sound_effect", backend="moss_sfx"), fake_json)
    for path in (blend, take, fake_json):
        assert placeholders.is_placeholder(path), path

    real_blend = tmp_path / "r.blend"
    real_blend.write_bytes(b"BLENDER-v405RENDH" + b"\x00" * 64)
    real_wav = tmp_path / "r.wav"
    with wave.open(str(real_wav), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(8000)
        handle.writeframes(b"\x00\x01" * 800)
    real_json = tmp_path / "r.json"
    real_json.write_text(json.dumps({"fake": False}))
    unknown = tmp_path / "r.bin"
    unknown.write_bytes(b"???")
    truncated = tmp_path / "t.glb"
    truncated.write_bytes(b"glTF")
    for path in (real_blend, real_wav, real_json, unknown, truncated):
        assert not placeholders.is_placeholder(path), path


def test_refuse_real_guards_and_lets_placeholders_by(tmp_path):
    fake = placeholders.placeholder_wav(tmp_path / "f.wav")
    placeholders.refuse_real(fake, tmp_path / "not-there.wav", None)  # no complaint
    real = tmp_path / "real.wav"
    with wave.open(str(real), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(8000)
        handle.writeframes(b"\x00\x01" * 800)
    with pytest.raises(UsageError, match="real.wav"):
        placeholders.refuse_real(fake, real)


def _humanoid(repo_root):
    from forge_gen import profile as profile_mod

    return profile_mod.load_profile(repo_root / "rigs" / "humanoid")


def test_the_prepared_placeholder_is_a_body_with_bones_and_no_skin(repo_root, tmp_path):
    """What ``forge gen prepare --fake`` writes: the shape a real one writes."""
    prof = _humanoid(repo_root)
    out = placeholders.placeholder_prepared_glb(tmp_path / "figure.glb", prof)
    info = glb.verify_glb(out)
    assert info["skins"] == 0, "the weights are what the skinner is being asked for"
    assert info["meshes"] == 1 and info["images"] == 1
    names = {node.get("name") for node in info["document"]["nodes"]}
    assert {bone["name"] for bone in prof.bones} <= names
    assert placeholders.is_placeholder(out)


def test_the_skinned_placeholder_carries_a_fitted_looking_skeleton(repo_root, tmp_path):
    """Bones scaled, rotations untouched — the one move a real fit makes.

    A placeholder at exactly the contract's lengths would exercise none of
    what the fitted skeleton exists to fill: a sidecar's ``bones[]`` and its
    ``motion_scale`` would both be the contract's own numbers.
    """
    prof = _humanoid(repo_root)
    plain = glb.verify_glb(placeholders.placeholder_body_glb(tmp_path / "one.glb", prof))["document"]
    fitted = glb.verify_glb(placeholders.placeholder_body_glb(tmp_path / "fit.glb", prof, scale=0.95))["document"]
    assert len(fitted["skins"][0]["joints"]) == len(prof.bones)

    for index, bone in enumerate(prof.bones):
        want = [value * 0.95 for value in bone["rest_translation"]]
        assert fitted["nodes"][index]["translation"] == pytest.approx(want, abs=1e-9), bone["name"]
        assert fitted["nodes"][index]["rotation"] == plain["nodes"][index]["rotation"], "a fit may scale a bone and may not turn one"
    assert placeholders.is_placeholder(tmp_path / "fit.glb")


def test_the_boxy_body_is_one_box_per_segment(repo_root):
    prof = _humanoid(repo_root)
    positions, indices, bones = placeholders.boxy_body(prof)
    assert len(positions) == 8 * placeholders.BODY_SEGMENTS
    assert len(indices) == 3 * 12 * placeholders.BODY_SEGMENTS
    assert len(bones) == len(positions)
    assert len(set(bones)) == placeholders.BODY_SEGMENTS, "each box belongs wholly to one bone"
    assert max(point[1] for point in positions) > 1.0, "and the figure stands up"


def test_the_boxy_body_scales_with_the_skeleton_it_is_built_on(repo_root):
    prof = _humanoid(repo_root)
    plain, _indices, _bones = placeholders.boxy_body(prof)
    small, _indices, _bones = placeholders.boxy_body(prof, scale=0.5)
    assert max(point[1] for point in small) == pytest.approx(0.5 * max(point[1] for point in plain), rel=0.02)


def test_the_inverse_bind_matrix_is_the_inverse_of_the_rest_transform():
    """One copy of the arithmetic; a matrix written two ways is a body inside out."""
    position, rotation = (0.1, 0.9, -0.2), (0.0, 0.0, 0.3826834, 0.9238795)
    inverse = placeholders.inverse_rigid(position, rotation)
    # Applying the inverse to the rest position must land on the origin.
    columns = [[inverse[column * 4 + row] for column in range(4)] for row in range(4)]
    moved = [sum(columns[row][k] * value for k, value in enumerate((*position, 1.0))) for row in range(3)]
    assert moved == pytest.approx([0.0, 0.0, 0.0], abs=1e-6)
    assert inverse[15] == 1.0
