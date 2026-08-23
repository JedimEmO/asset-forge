"""The --fake outputs pass the validators their real counterparts must."""

from __future__ import annotations

import wave
import zlib

from forge_gen import glb, placeholders, records


def test_silence_is_pcm16_of_the_asked_length(tmp_path):
    path = placeholders.silence_wav(tmp_path / "s.wav", seconds=0.5, rate=48000)
    with wave.open(str(path)) as handle:
        assert handle.getnchannels() == 1 and handle.getsampwidth() == 2 and handle.getframerate() == 48000
        assert handle.getnframes() == 24000
        assert handle.readframes(10) == b"\x00" * 20


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
