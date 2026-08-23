"""verify_glb refuses what an outside consumer would choke on, and the placeholder passes it."""

from __future__ import annotations

import json
import struct

import pytest

from forge_gen import glb


def _container(chunks: list[tuple[int, bytes]], *, declared: int | None = None) -> bytes:
    body = b""
    for kind, payload in chunks:
        padded = payload + (b" " if kind == glb.CHUNK_JSON else b"\x00") * (-len(payload) % 4)
        body += struct.pack("<II", len(padded), kind) + padded
    total = 12 + len(body)
    return glb.GLB_MAGIC + struct.pack("<II", 2, declared if declared is not None else total) + body


def _doc(**extra) -> bytes:
    document = {"asset": {"version": "2.0"}, "buffers": [{"byteLength": 4}]}
    document.update(extra)
    return json.dumps(document).encode()


def test_placeholder_passes_and_reports(tmp_path):
    path = glb.placeholder_glb(tmp_path / "p.glb", name="probe")
    info = glb.verify_glb(path)
    assert info["meshes"] == 1 and info["images"] == 1 and info["nodes"] == 1 and info["skins"] == 0
    assert info["document"]["meshes"][0]["name"] == "probe"
    assert "self-contained" in glb.report(path, info)
    # The embedded PNG is a real one: signature and IEND in place.
    binary_view = info["document"]["bufferViews"][3]
    assert binary_view["byteLength"] > 40
    assert info["document"]["images"][0]["mimeType"] == "image/png"


def test_placeholder_is_parsed_by_an_independent_reader(tmp_path):
    """Only struct and json: the same thing a game engine's loader does first."""
    data = (glb.placeholder_glb(tmp_path / "p.glb")).read_bytes()
    magic, version, length = struct.unpack_from("<4sII", data, 0)
    assert magic == b"glTF" and version == 2 and length == len(data)
    json_len, json_kind = struct.unpack_from("<II", data, 12)
    assert json_kind == glb.CHUNK_JSON
    document = json.loads(data[20 : 20 + json_len])
    bin_len, bin_kind = struct.unpack_from("<II", data, 20 + json_len)
    assert bin_kind == glb.CHUNK_BIN
    assert document["buffers"][0]["byteLength"] <= bin_len
    start = 28 + json_len
    positions = struct.unpack_from("<9f", data, start + document["bufferViews"][0]["byteOffset"])
    assert positions[1] == 0.0 and positions[4] == 0.0 and positions[7] == 0.0, "the triangle lies on the floor"
    png_view = document["bufferViews"][3]
    png = data[start + png_view["byteOffset"] : start + png_view["byteOffset"] + png_view["byteLength"]]
    assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")


def test_truncated_file_is_refused(tmp_path):
    good = _container([(glb.CHUNK_JSON, _doc()), (glb.CHUNK_BIN, b"\x00\x00\x00\x00")])
    path = tmp_path / "t.glb"
    path.write_bytes(good[:-3])
    with pytest.raises(glb.GlbError, match="truncated"):
        glb.verify_glb(path)


def test_two_json_chunks_are_refused(tmp_path):
    path = tmp_path / "two.glb"
    path.write_bytes(_container([(glb.CHUNK_JSON, _doc()), (glb.CHUNK_JSON, _doc()), (glb.CHUNK_BIN, b"\x00" * 4)]))
    with pytest.raises(glb.GlbError, match="2 JSON"):
        glb.verify_glb(path)


def test_missing_bin_chunk_is_refused(tmp_path):
    path = tmp_path / "nobin.glb"
    path.write_bytes(_container([(glb.CHUNK_JSON, _doc())]))
    with pytest.raises(glb.GlbError, match="0 BIN"):
        glb.verify_glb(path)


def test_external_uri_is_refused(tmp_path):
    path = tmp_path / "ext.glb"
    doc = _doc(images=[{"uri": "textures/skin.png"}])
    path.write_bytes(_container([(glb.CHUNK_JSON, doc), (glb.CHUNK_BIN, b"\x00" * 4)]))
    with pytest.raises(glb.GlbError, match=r"images\[0\] -> textures/skin.png"):
        glb.verify_glb(path)
    doc = _doc(buffers=[{"uri": "mesh.bin", "byteLength": 4}])
    path.write_bytes(_container([(glb.CHUNK_JSON, doc), (glb.CHUNK_BIN, b"\x00" * 4)]))
    with pytest.raises(glb.GlbError, match=r"buffers\[0\]"):
        glb.verify_glb(path)


def test_not_a_glb_and_wrong_version(tmp_path):
    path = tmp_path / "x.glb"
    path.write_bytes(b"not a glb at all")
    with pytest.raises(glb.GlbError, match="not a .glb"):
        glb.verify_glb(path)
    data = _container([(glb.CHUNK_JSON, _doc()), (glb.CHUNK_BIN, b"\x00" * 4)])
    path.write_bytes(data[:4] + struct.pack("<I", 1) + data[8:])
    with pytest.raises(glb.GlbError, match="version 1"):
        glb.verify_glb(path)


def test_a_chunk_past_the_end_is_refused(tmp_path):
    path = tmp_path / "over.glb"
    json_bytes = _doc()
    body = struct.pack("<II", 10_000, glb.CHUNK_JSON) + json_bytes
    data = glb.GLB_MAGIC + struct.pack("<II", 2, 12 + len(body)) + body
    path.write_bytes(data)
    with pytest.raises(glb.GlbError, match="past the end"):
        glb.verify_glb(path)
