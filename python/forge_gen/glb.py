"""Reading a ``.glb`` back as an outside consumer would, and writing the smallest one that passes.

``verify_glb`` is the gate every exported body and prop goes through before
its record is written, ported from the old ``export_character.py``: plain
``struct`` and ``json`` on purpose, because it asks what an outside consumer
would find in the file, and answering it through the exporter that just
wrote it would only prove the exporter agrees with itself.

``placeholder_glb`` is the ``--fake`` stand-in: one triangle with one
embedded 1×1 PNG, self-contained, so a CI path with no Blender and no GPU
still exercises lift → prop → promote on a file the same validator accepts.
"""

from __future__ import annotations

import json
import os
import struct
from pathlib import Path

from forge_gen import png

GLB_MAGIC = b"glTF"
GLB_VERSION = 2
CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942


class GlbError(Exception):
    """The file is not one self-contained glB container."""


def verify_glb(path: str | os.PathLike) -> dict:
    """Read a .glb back and prove it is one self-contained file.

    Exactly one JSON chunk and one BIN chunk, the declared length equal to
    the file's, and nothing under ``buffers`` or ``images`` with a ``uri`` —
    a .glb that references files on the author's disk renders pink
    everywhere else.

    Returns ``{"document": <the glTF JSON>, "bytes": N, "nodes": N, "meshes":
    N, "images": N, "skins": N, "animations": N, "generator": str|None}`` so
    callers can report what actually shipped; raises :class:`GlbError`.
    """
    path = os.fspath(path)
    with open(path, "rb") as handle:
        data = handle.read()
    if len(data) < 12 or data[:4] != GLB_MAGIC:
        raise GlbError(f"{path} is not a .glb container")
    version, length = struct.unpack_from("<II", data, 4)
    if version != GLB_VERSION:
        raise GlbError(f"{path} is glB version {version}")
    if length != len(data):
        raise GlbError(f"{path} declares {length} bytes but is {len(data)} — the file is truncated")

    chunks = []
    offset = 12
    while offset + 8 <= len(data):
        chunk_length, chunk_type = struct.unpack_from("<II", data, offset)
        offset += 8
        if offset + chunk_length > len(data):
            raise GlbError(f"{path} has a chunk of {chunk_length} bytes running past the end of the file")
        chunks.append((chunk_type, data[offset : offset + chunk_length]))
        offset += chunk_length + (-chunk_length % 4)

    json_chunks = [payload for kind, payload in chunks if kind == CHUNK_JSON]
    bin_chunks = [payload for kind, payload in chunks if kind == CHUNK_BIN]
    if len(json_chunks) != 1 or len(bin_chunks) != 1:
        kinds = ", ".join(f"0x{kind:08X}" for kind, _ in chunks) or "none"
        raise GlbError(
            f"{path} holds {len(json_chunks)} JSON and {len(bin_chunks)} BIN chunk(s) "
            f"(types: {kinds}), want exactly one of each"
        )

    try:
        document = json.loads(json_chunks[0].decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as err:
        raise GlbError(f"{path}: the JSON chunk does not parse: {err}") from err
    if not isinstance(document, dict):
        raise GlbError(f"{path}: the JSON chunk is not an object")
    external = [
        f"{section}[{index}] -> {entry['uri']}"
        for section in ("buffers", "images")
        for index, entry in enumerate(document.get(section, []))
        if isinstance(entry, dict) and "uri" in entry
    ]
    if external:
        listed = ", ".join(external)
        raise GlbError(
            f"{path} points outside itself ({listed}) — a .glb that references files "
            "on the author's disk renders pink everywhere else"
        )

    return {
        "document": document,
        "bytes": len(data),
        "nodes": len(document.get("nodes", [])),
        "meshes": len(document.get("meshes", [])),
        "images": len(document.get("images", [])),
        "skins": len(document.get("skins", [])),
        "animations": len(document.get("animations", [])),
        "generator": document.get("asset", {}).get("generator"),
    }


def report(path: str | os.PathLike, info: dict) -> str:
    """Say what shipped, in the vocabulary the consumer side reads it with."""
    document = info["document"]
    mesh_names = [mesh.get("name", "<unnamed>") for mesh in document.get("meshes", [])]
    return (
        f"{os.path.basename(os.fspath(path))} is self-contained — {info['bytes'] / 1024.0:.0f} KiB, "
        f"{info['nodes']} node(s), {info['meshes']} mesh(es), {info['images']} embedded image(s), "
        f"{info['skins']} skin(s); asset.generator = {info['generator'] or '<none>'}; "
        f"mesh names = {', '.join(mesh_names) or '<none>'}"
    )


def _padded(blob: bytes, pad: bytes) -> bytes:
    return blob + pad * (-len(blob) % 4)


def build_glb(document: dict, binary: bytes) -> bytes:
    """Pack a glTF document and its one buffer into glB bytes."""
    json_bytes = _padded(json.dumps(document, separators=(",", ":")).encode("utf-8"), b" ")
    bin_bytes = _padded(binary, b"\x00")
    total = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    return b"".join(
        [
            GLB_MAGIC,
            struct.pack("<II", GLB_VERSION, total),
            struct.pack("<II", len(json_bytes), CHUNK_JSON),
            json_bytes,
            struct.pack("<II", len(bin_bytes), CHUNK_BIN),
            bin_bytes,
        ]
    )


def placeholder_glb(path: str | os.PathLike, *, name: str = "placeholder", generator: str = "forge-gen --fake") -> Path:
    """Write one triangle with one embedded 1×1 PNG: the smallest file ``verify_glb`` accepts.

    Y-up, metres: the triangle lies on the floor inside a 0.5 m square so a
    viewer framing it sees something. It is a placeholder and says so in its
    names; nothing downstream should mistake it for a lift.
    """
    positions = struct.pack("<9f", -0.25, 0.0, -0.25, 0.25, 0.0, -0.25, 0.0, 0.0, 0.25)
    texcoords = struct.pack("<6f", 0.0, 1.0, 1.0, 1.0, 0.5, 0.0)
    indices = struct.pack("<3H", 0, 1, 2)
    image = png.solid_png(1, 1, (128, 128, 128, 255))
    parts = [positions, texcoords, _padded(indices, b"\x00"), image]
    views = []
    offset = 0
    for blob in parts:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob)})
        offset += len(_padded(blob, b"\x00"))
    views[0]["target"] = 34962  # ARRAY_BUFFER
    views[1]["target"] = 34962
    views[2]["target"] = 34963  # ELEMENT_ARRAY_BUFFER
    binary = b"".join(_padded(blob, b"\x00") for blob in parts)
    document = {
        "asset": {"version": "2.0", "generator": generator},
        "scene": 0,
        "scenes": [{"name": name, "nodes": [0]}],
        "nodes": [{"name": name, "mesh": 0}],
        "meshes": [
            {
                "name": name,
                "primitives": [
                    {"attributes": {"POSITION": 0, "TEXCOORD_0": 1}, "indices": 2, "material": 0, "mode": 4}
                ],
            }
        ],
        "materials": [
            {
                "name": name,
                "doubleSided": True,
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": 0},
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.9,
                },
            }
        ],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 10497, "wrapT": 10497}],
        "images": [{"name": name, "mimeType": "image/png", "bufferView": 3}],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "min": [-0.25, 0.0, -0.25],
                "max": [0.25, 0.0, 0.25],
            },
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(build_glb(document, binary))
    return target
