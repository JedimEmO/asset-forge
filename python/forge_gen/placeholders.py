"""What a ``--fake`` run writes instead of calling a backend.

Every placeholder passes the validator the real output must pass — the
``.glb`` through ``verify_glb``, the take through ``forge_motion::Take::read``,
the WAV through ``forge audio inspect`` (a quiet pluck, not silence, so the
silence gate real audio must pass holds for placeholders too), the PNG
through any decoder — and nothing else about it is true. A record written
beside one says ``"fake": true`` and ``backend.commit = "fake"`` so no
reader mistakes it for a lift.

Every placeholder is also *recognisable* as one: the ``.blend`` by its
header, the ``.glb`` by its generator string, the WAV by a trailing RIFF
chunk, the take by an extra member, the record by ``"fake": true``. That is
what :func:`refuse_real` runs on — a ``--fake`` run may overwrite what an
earlier ``--fake`` run left, and nothing else. ``FORGE_FAKE=1`` is exported
by ``just ci-fake``, so "the flag was in the shell" must never be able to
destroy a real committed file.

``FORGE_FAKE=1`` in the environment implies ``--fake`` on every command.
"""

from __future__ import annotations

import json
import math
import os
import struct
import wave
import zipfile
from pathlib import Path

from forge_gen import glb, npz, png, records
from forge_gen.exit_codes import UsageError

#: The commit a fake record carries in place of a real one.
FAKE_COMMIT = "fake"

#: What every recognisable placeholder is stamped with, one way or another.
FAKE_MARK = b"forge-gen --fake placeholder"

#: Bytes a ``--fake`` ``.blend`` starts with: Blender's own magic, so a sniff
#: says "a .blend" and the rest of the file says "not really".
FAKE_BLEND_HEADER = b"BLENDER-v000RENDH"

#: The RIFF chunk id the placeholder WAV carries after its data chunk.
#: Standard readers walk to ``data`` and stop; the trailing chunk is legal
#: RIFF and only :func:`is_placeholder` looks for it.
_WAV_CHUNK_ID = b"fgen"

#: The placeholder tone: peak amplitude (~-12 dBFS — well above the "very
#: quiet" gate at -18 and nowhere near clipping), pitch, and the envelope's
#: attack/release (seconds). The decay makes it a pluck rather than a steady
#: sine: a constant sine fails the crest-factor ("very compressed") warning.
_TONE_PEAK = 0.25
_TONE_HZ = 440.0
_TONE_ATTACK_S = 0.01
_TONE_RELEASE_S = 0.02


def requested(args=None) -> bool:
    """Whether this run is fake: ``--fake`` on the command line, or ``FORGE_FAKE=1``."""
    flag = bool(getattr(args, "fake", False)) if args is not None else False
    return flag or os.environ.get("FORGE_FAKE", "") == "1"


def placeholder_wav(path: str | os.PathLike, *, seconds: float = 0.5, rate: int = 48000, channels: int = 1) -> Path:
    """A quiet 440 Hz pluck with a lead-in and tail — sound-shaped, obviously not the sound.

    Passes every ``forge audio inspect`` gate silence cannot: not silent,
    no clip run, a crest factor from the decay, the first audible sample
    within milliseconds (a one-shot that fires late warns), a fade-out so
    the tail is not a cliff. A trailing RIFF chunk marks it as a placeholder
    for :func:`is_placeholder`; readers stop at the ``data`` chunk and never
    see it.
    """
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    frames = max(1, int(round(seconds * rate)))
    attack = max(1, min(frames, int(_TONE_ATTACK_S * rate)))
    release = max(1, min(frames, int(_TONE_RELEASE_S * rate)))
    decay = 4.0 / max(seconds, 1e-6)
    samples = bytearray()
    for i in range(frames):
        t = i / rate
        envelope = math.exp(-decay * t)
        if i < attack:
            envelope *= (i + 1) / attack
        if frames - i <= release:
            envelope *= (frames - i) / release
        value = int(round(_TONE_PEAK * envelope * math.sin(2.0 * math.pi * _TONE_HZ * t) * 32767.0))
        samples += struct.pack("<h", value) * channels
    with wave.open(os.fspath(target), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(bytes(samples))
    _append_wav_mark(target)
    return target


def _append_wav_mark(target: Path) -> None:
    """Append the ``fgen`` chunk and patch the RIFF size so the file stays well-formed."""
    payload = FAKE_MARK + (b"\x00" if len(FAKE_MARK) % 2 else b"")
    with open(target, "r+b") as handle:
        handle.seek(0, os.SEEK_END)
        handle.write(_WAV_CHUNK_ID + struct.pack("<I", len(FAKE_MARK)) + payload)
        size = handle.tell()
        handle.seek(4)
        handle.write(struct.pack("<I", size - 8))


def tile_png(path: str | os.PathLike, *, columns: int = 4, rows: int = 2, cell: int = 8) -> Path:
    """Write a checkerboard of ``columns × rows`` cells: a contact sheet with nothing on it."""
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    width, height = columns * cell, rows * cell
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            shade = 200 if ((x // cell) + (y // cell)) % 2 == 0 else 90
            pixels += bytes((shade, shade, shade, 255))
    png.write_png(target, width, height, bytes(pixels))
    return target


def placeholder_glb(path: str | os.PathLike, *, name: str = "placeholder") -> Path:
    """One triangle with an embedded texture; see :func:`forge_gen.glb.placeholder_glb`."""
    return glb.placeholder_glb(path, name=name)


#: How many segments the boxy placeholder body is built from — one box each,
#: twelve triangles a box, so the figure comes out at 192 triangles: enough
#: geometry that a viewer sees a body and few enough that the file stays a
#: placeholder anyone can read in a hex dump.
BODY_SEGMENTS = 16

#: How wide a placeholder segment's box is, as a fraction of its own length.
BODY_SEGMENT_WIDTH = 0.3

#: The twelve triangles of a box over the corner order :func:`boxy_body`
#: writes (bit 0 = +x, bit 1 = +y, bit 2 = +z). Winding is not load-bearing:
#: every placeholder material is double-sided.
_BOX_TRIANGLES = (
    (0, 2, 3), (0, 3, 1),
    (4, 5, 7), (4, 7, 6),
    (0, 1, 5), (0, 5, 4),
    (2, 6, 7), (2, 7, 3),
    (0, 4, 6), (0, 6, 2),
    (1, 3, 7), (1, 7, 5),
)


def contract_bone_nodes(profile, *, mesh_index: int, scale: float = 1.0) -> tuple[list[dict], int, int]:
    """The profile's bones as glTF nodes, in contract order, plus the armature node.

    Returns ``(nodes, root_index, armature_index)`` with the bones occupying
    indices ``0..len(bones) - 1``, so a node index and a contract index are
    the same number — which is what every reader of these files assumes.

    ``scale`` multiplies each bone's local rest **translation** and touches no
    rotation: that is exactly the move a fitted skeleton makes, so a fake
    written at 0.95 exercises a sidecar's ``bones[]`` and its ``motion_scale``
    with no card, and a rotation this function could introduce is the one
    thing the whole design forbids.
    """
    bones = profile.bones
    armature_node = str(profile.section("bones")["armature_node"])
    nodes: list[dict] = []
    for index, bone in enumerate(bones):
        children = [i for i, other in enumerate(bones) if other.get("parent") == index]
        node = {
            "name": bone["name"],
            "translation": [float(v) * scale for v in bone["rest_translation"]],
            "rotation": [float(v) for v in bone["rest_rotation"]],
        }
        if children:
            node["children"] = children
        nodes.append(node)
    root_index = next(i for i, bone in enumerate(bones) if bone.get("parent") is None)
    armature_index = len(nodes)
    nodes.append({"name": armature_node, "children": [root_index, mesh_index]})
    return nodes, root_index, armature_index


def boxy_body(profile, *, scale: float = 1.0) -> tuple[list[tuple], list[int], list[int]]:
    """A figure of boxes standing on the profile's own rest skeleton.

    One axis-aligned box per segment over the longest :data:`BODY_SEGMENTS`
    segments the contract has, so the placeholder is body-shaped on whatever
    profile it is handed rather than a board that happens to be human-sized.
    Returns ``(positions, indices, bone_of_vertex)``; the last is what a
    skinned placeholder weights each vertex to and an unskinned one ignores.
    """
    bones = profile.bones
    rest = profile.rest_world()
    segments = []
    for index, bone in enumerate(bones):
        parent = bone.get("parent")
        if parent is None:
            continue
        head = tuple(v * scale for v in rest[bones[parent]["name"]][0])
        tail = tuple(v * scale for v in rest[bone["name"]][0])
        length = math.dist(head, tail)
        if length > 1e-4:
            segments.append((length, index, head, tail))
    segments.sort(key=lambda row: -row[0])
    segments = sorted(segments[:BODY_SEGMENTS], key=lambda row: row[1])

    positions: list[tuple] = []
    indices: list[int] = []
    bone_of_vertex: list[int] = []
    for length, index, head, tail in segments:
        radius = max(BODY_SEGMENT_WIDTH * length, 0.01)
        low = tuple(min(head[axis], tail[axis]) - radius for axis in range(3))
        high = tuple(max(head[axis], tail[axis]) + radius for axis in range(3))
        base = len(positions)
        for corner in range(8):
            positions.append(
                (
                    high[0] if corner & 1 else low[0],
                    high[1] if corner & 2 else low[1],
                    high[2] if corner & 4 else low[2],
                )
            )
            bone_of_vertex.append(index)
        for a, b, c in _BOX_TRIANGLES:
            indices += [base + a, base + b, base + c]
    return positions, indices, bone_of_vertex


def inverse_rigid(position: tuple, rotation: tuple) -> list[float]:
    """Column-major 4×4 inverse of ``T(position) · R(rotation)``: ``R^T · T(-position)``.

    One copy for both skinned placeholders — the exporter's board and the
    skin door's boxy figure — because an inverse bind matrix written two
    slightly different ways is a bug that shows up as a body inside out.
    """
    x, y, z, w = rotation
    # Rotation matrix rows (R), then transpose by reading columns as rows.
    r = [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]
    rt = [[r[j][i] for j in range(3)] for i in range(3)]
    t = [-sum(rt[i][k] * position[k] for k in range(3)) for i in range(3)]
    # glTF stores column-major: element [row][col] at index col*4 + row.
    out = [0.0] * 16
    for row in range(3):
        for col in range(3):
            out[col * 4 + row] = rt[row][col]
        out[3 * 4 + row] = t[row]
    out[15] = 1.0
    return out


def placeholder_body_glb(path: str | os.PathLike, profile, *, name: str = "Body", scale: float = 1.0) -> Path:
    """What a ``--fake`` ``forge gen skin`` writes: a boxy body skinned to a fitted-looking skeleton.

    Every bone's rest **translation** is multiplied by ``scale`` and every
    rest **rotation** is left alone — the one move a real fit makes — and
    each box is weighted wholly to the bone it was built on, so a reader
    downstream finds 55 joints, a skin over all of them and a
    ``motion_scale`` that is not 1.0. Nothing about it is a measurement.
    """
    positions, indices, bone_of_vertex = boxy_body(profile, scale=scale)
    mesh_index = len(profile.bones) + 1
    nodes, _root, armature_index = contract_bone_nodes(profile, mesh_index=mesh_index, scale=scale)
    nodes.append({"name": name, "mesh": 0, "skin": 0})
    rest = profile.rest_world()
    parts = [
        b"".join(struct.pack("<3f", *point) for point in positions),
        struct.pack("<2f", 0.0, 0.0) * len(positions),
        b"".join(struct.pack("<4H", index, 0, 0, 0) for index in bone_of_vertex),
        struct.pack("<4f", 1.0, 0.0, 0.0, 0.0) * len(positions),
        b"".join(struct.pack("<H", value) for value in indices),
        b"".join(
            struct.pack(
                "<16f",
                *inverse_rigid(tuple(v * scale for v in rest[bone["name"]][0]), rest[bone["name"]][1]),
            )
            for bone in profile.bones
        ),
        png.solid_png(1, 1, (128, 128, 128, 255)),
    ]
    views, offset = [], 0
    for blob in parts:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob)})
        offset += len(blob) + (-len(blob) % 4)
    for index in range(4):
        views[index]["target"] = 34962
    views[4]["target"] = 34963
    binary = b"".join(blob + b"\x00" * (-len(blob) % 4) for blob in parts)
    low = [min(point[axis] for point in positions) for axis in range(3)]
    high = [max(point[axis] for point in positions) for axis in range(3)]
    document = {
        "asset": {"version": "2.0", "generator": "forge-gen --fake"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [armature_index]}],
        "nodes": nodes,
        "skins": [{"inverseBindMatrices": 5, "joints": list(range(len(profile.bones))), "skeleton": _root}],
        "meshes": [
            {
                "name": name,
                "primitives": [
                    {
                        "attributes": {"POSITION": 0, "TEXCOORD_0": 1, "JOINTS_0": 2, "WEIGHTS_0": 3},
                        "indices": 4,
                        "material": 0,
                        "mode": 4,
                    }
                ],
            }
        ],
        "materials": [
            {
                "name": name,
                "doubleSided": True,
                "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.9},
            }
        ],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 10497, "wrapT": 10497}],
        "images": [{"name": name, "mimeType": "image/png", "bufferView": 6}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "min": low, "max": high},
            {"bufferView": 1, "componentType": 5126, "count": len(positions), "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": len(positions), "type": "VEC4"},
            {"bufferView": 3, "componentType": 5126, "count": len(positions), "type": "VEC4"},
            {"bufferView": 4, "componentType": 5123, "count": len(indices), "type": "SCALAR"},
            {"bufferView": 5, "componentType": 5126, "count": len(profile.bones), "type": "MAT4"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(glb.build_glb(document, binary))
    return target


def placeholder_prepared_glb(path: str | os.PathLike, profile, *, name: str = "Body") -> Path:
    """What a ``--fake`` ``forge gen prepare`` writes: bone nodes, a boxy body, no weights.

    The same *shape* a real prepare writes — every contract bone as a node,
    and a mesh that is nobody's skin — so the door downstream is exercised on
    a file with the profile's bones in it and no skin to mistake for one.
    Nothing about the geometry is a measurement.
    """
    positions, indices, _bones = boxy_body(profile)
    mesh_index = len(profile.bones) + 1
    nodes, _root, armature_index = contract_bone_nodes(profile, mesh_index=mesh_index)
    nodes.append({"name": name, "mesh": 0})
    parts = [
        b"".join(struct.pack("<3f", *point) for point in positions),
        struct.pack("<2f", 0.0, 0.0) * len(positions),
        b"".join(struct.pack("<H", value) for value in indices),
        png.solid_png(1, 1, (128, 128, 128, 255)),
    ]
    views, offset = [], 0
    for blob in parts:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob)})
        offset += len(blob) + (-len(blob) % 4)
    views[0]["target"] = 34962
    views[1]["target"] = 34962
    views[2]["target"] = 34963
    binary = b"".join(blob + b"\x00" * (-len(blob) % 4) for blob in parts)
    low = [min(point[axis] for point in positions) for axis in range(3)]
    high = [max(point[axis] for point in positions) for axis in range(3)]
    document = {
        "asset": {"version": "2.0", "generator": "forge-gen --fake"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [armature_index]}],
        "nodes": nodes,
        "meshes": [
            {
                "name": name,
                "primitives": [{"attributes": {"POSITION": 0, "TEXCOORD_0": 1}, "indices": 2, "material": 0, "mode": 4}],
            }
        ],
        "materials": [
            {
                "name": name,
                "doubleSided": True,
                "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.9},
            }
        ],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 10497, "wrapT": 10497}],
        "images": [{"name": name, "mimeType": "image/png", "bufferView": 3}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "min": low, "max": high},
            {"bufferView": 1, "componentType": 5126, "count": len(positions), "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": len(indices), "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(glb.build_glb(document, binary))
    return target


def placeholder_blend(path: str | os.PathLike) -> Path:
    """A file that sniffs as a ``.blend`` and says in its second line that it is not one."""
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(FAKE_BLEND_HEADER + b"\n# " + FAKE_MARK + b"; not a Blender file\n")
    return target


def placeholder_take(path: str | os.PathLike, *, frames: int = 40, fps: int = 20, prompt: str = "") -> Path:
    """A still figure in the rest pose; see :func:`forge_gen.npz.write_take`."""
    return npz.write_take(path, frames=frames, fps=fps, prompt=prompt)


def fake_record(kind: str, tool: str, *, backend: str | None, created_by: str | None = None, model: str | None = None) -> dict:
    """A record whose backend block says what it is: ``commit = "fake"``, ``fake = true``."""
    rec = records.new_record(kind, tool, created_by=created_by)
    rec["backend"] = records.backend_block(name=backend, commit=FAKE_COMMIT, model=model)
    rec["fake"] = True
    rec["note"] = "placeholder output from a --fake run; nothing about it is a measurement"
    return rec


# ------------------------------------------------------- recognising fakes --


def is_placeholder(path: str | os.PathLike) -> bool:
    """Whether the file is something a ``--fake`` run wrote, judged by its own marks.

    Recognised by suffix: ``.blend`` by :data:`FAKE_BLEND_HEADER`, ``.glb``
    by its generator string, ``.wav`` by the trailing ``fgen`` chunk,
    ``.ogg`` by the vorbis comment carrying :data:`FAKE_MARK`, ``.npz`` by
    the ``forge_gen_fake`` member, ``.json`` by a top-level ``"fake": true``
    (a fake generator record, or a fake keys spec). Anything unrecognised —
    another suffix, a truncated file — is **not** a placeholder: the caller
    refuses rather than guesses.
    """
    target = Path(path)
    suffix = target.suffix.lower()
    try:
        if suffix == ".blend":
            with open(target, "rb") as handle:
                return handle.read(len(FAKE_BLEND_HEADER)) == FAKE_BLEND_HEADER
        if suffix == ".glb":
            generator = glb.verify_glb(target).get("generator") or ""
            return str(generator).startswith("forge-gen --fake")
        if suffix == ".wav":
            return _wav_has_mark(target)
        if suffix == ".ogg":
            with open(target, "rb") as handle:
                return FAKE_MARK in handle.read(64 * 1024)
        if suffix == ".npz":
            with zipfile.ZipFile(target) as archive:
                return "forge_gen_fake.npy" in archive.namelist()
        if suffix == ".json":
            with open(target, encoding="utf-8") as handle:
                doc = json.load(handle)
            return isinstance(doc, dict) and doc.get("fake") is True
    except (OSError, ValueError, glb.GlbError, zipfile.BadZipFile, wave.Error):
        return False
    return False


def _wav_has_mark(target: Path) -> bool:
    """Walk the RIFF chunk list for the ``fgen`` chunk the placeholder writer appends."""
    with open(target, "rb") as handle:
        head = handle.read(12)
        if len(head) < 12 or head[:4] != b"RIFF" or head[8:12] != b"WAVE":
            return False
        while True:
            header = handle.read(8)
            if len(header) < 8:
                return False
            chunk_id, size = header[:4], struct.unpack("<I", header[4:])[0]
            if chunk_id == _WAV_CHUNK_ID:
                return handle.read(size) == FAKE_MARK
            handle.seek(size + (size % 2), os.SEEK_CUR)


def refuse_real(*paths: str | os.PathLike | None) -> None:
    """Refuse to let a ``--fake`` run overwrite anything a ``--fake`` run did not write.

    Called by every ``run_fake`` before it touches a caller-named path.
    A target that does not exist, or that :func:`is_placeholder` recognises,
    is fine; anything else raises :class:`UsageError` (exit 2) naming the
    file — ``FORGE_FAKE=1`` left in a shell must not destroy a committed
    ``.blend`` or a recorded WAV.
    """
    for path in paths:
        if path is None:
            continue
        target = Path(path)
        if target.exists() and not is_placeholder(target):
            raise UsageError(
                f"--fake refuses to overwrite {target}: it exists and is not a placeholder from an "
                f"earlier --fake run — move it, or point --out somewhere else"
            )
