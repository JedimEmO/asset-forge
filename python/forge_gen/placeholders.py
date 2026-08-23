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
