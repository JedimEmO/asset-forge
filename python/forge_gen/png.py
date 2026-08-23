"""A PNG encoder small enough to not need one: ``zlib`` + ``struct``.

Two places need to write an image with no imaging library in reach — the
placeholder ``.glb`` embeds a 1×1 texture, and ``--fake`` runs write a
contact-sheet stand-in — and both run under the system interpreter, where
Pillow is not a thing the launcher may assume.
"""

from __future__ import annotations

import os
import struct
import zlib

#: The eight bytes every PNG starts with.
SIGNATURE = b"\x89PNG\r\n\x1a\n"


def _chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def encode_png(width: int, height: int, pixels: bytes, *, channels: int = 4) -> bytes:
    """Encode 8-bit RGB (``channels=3``) or RGBA (``channels=4``) rows into PNG bytes.

    ``pixels`` is ``height`` rows of ``width * channels`` bytes, top row
    first, no padding — the layout a list comprehension produces.
    """
    if channels not in (3, 4):
        raise ValueError("channels must be 3 (RGB) or 4 (RGBA)")
    stride = width * channels
    if len(pixels) != stride * height:
        raise ValueError(f"expected {stride * height} pixel bytes for {width}x{height}x{channels}, got {len(pixels)}")
    color_type = 6 if channels == 4 else 2
    header = struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0)
    # Filter byte 0 (None) in front of every scanline.
    raw = b"".join(b"\x00" + pixels[row * stride : (row + 1) * stride] for row in range(height))
    return SIGNATURE + _chunk(b"IHDR", header) + _chunk(b"IDAT", zlib.compress(raw, 9)) + _chunk(b"IEND", b"")


def solid_png(width: int, height: int, rgba: tuple[int, int, int, int]) -> bytes:
    """A PNG of one colour."""
    return encode_png(width, height, bytes(rgba) * (width * height))


def write_png(path: str | os.PathLike, width: int, height: int, pixels: bytes, *, channels: int = 4) -> None:
    """Encode and write."""
    with open(path, "wb") as handle:
        handle.write(encode_png(width, height, pixels, channels=channels))
