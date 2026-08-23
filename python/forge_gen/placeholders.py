"""What a ``--fake`` run writes instead of calling a backend.

Every placeholder passes the validator the real output must pass — the
``.glb`` through ``verify_glb``, the take through ``forge_motion::Take::read``,
the WAV through any PCM reader, the PNG through any decoder — and nothing
else about it is true. A record written beside one says ``"fake": true`` and
``backend.commit = "fake"`` so no reader mistakes it for a lift.

``FORGE_FAKE=1`` in the environment implies ``--fake`` on every command.
"""

from __future__ import annotations

import os
import struct
import wave
from pathlib import Path

from forge_gen import glb, npz, png, records

#: The commit a fake record carries in place of a real one.
FAKE_COMMIT = "fake"


def requested(args=None) -> bool:
    """Whether this run is fake: ``--fake`` on the command line, or ``FORGE_FAKE=1``."""
    flag = bool(getattr(args, "fake", False)) if args is not None else False
    return flag or os.environ.get("FORGE_FAKE", "") == "1"


def silence_wav(path: str | os.PathLike, *, seconds: float = 0.5, rate: int = 48000, channels: int = 1) -> Path:
    """Write 16-bit PCM silence — what a sound generator would have produced, minus the sound."""
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    frames = max(1, int(round(seconds * rate)))
    with wave.open(os.fspath(target), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(struct.pack("<h", 0) * (frames * channels))
    return target


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
