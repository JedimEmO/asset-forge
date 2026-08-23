"""Just enough ``.npz`` to write what ARDY writes, with no numpy in reach.

An ARDY take is a zip of **stored** ``.npy`` members — a header dict and a
byte reinterpret each — which is why ``forge_motion::Take::read`` can read
one without an ndarray stack, and why this module can write one with
``zipfile`` and ``struct``. ``write_take`` is the ``--fake`` stand-in: a
still figure in the profile's rest pose, the exact member names, dtypes and
shapes a real take carries — plus one extra 0-d member ``forge_gen_fake``
that brands it a placeholder — so the CI path sweep → review → promote runs
on a file the Rust reader accepts and no real take is ever mistaken for it.

The member table (from a real take, ``gen_roll.npz``):

=====================  =======  ================
member                 dtype    shape
=====================  =======  ================
local_rot_mats         <f4      (T, 27, 3, 3)
global_rot_mats        <f4      (T, 27, 3, 3)
posed_joints           <f4      (T, 27, 3)
root_positions         <f4      (T, 3)
smooth_root_pos        <f4      (T, 3)
foot_contacts          \\|b1     (T, 4)   columns Left*, Right*
global_root_heading    <f4      (T, 2)
fps                    <i8      ()
text                   <U{n}    ()
=====================  =======  ================

Stdlib only.
"""

from __future__ import annotations

import ast
import os
import struct
import zipfile
from dataclasses import dataclass
from pathlib import Path

MAGIC = b"\x93NUMPY"


@dataclass(frozen=True)
class Array:
    """One ``.npy`` member: a numpy dtype string, a shape and the raw C-order bytes."""

    descr: str
    shape: tuple[int, ...]
    data: bytes

    @property
    def count(self) -> int:
        """Element count (1 for a 0-d scalar)."""
        total = 1
        for dim in self.shape:
            total *= dim
        return total

    def floats(self) -> list[float]:
        """The elements as floats (``<f4``/``<f8`` only)."""
        if self.descr == "<f4":
            return list(struct.unpack(f"<{self.count}f", self.data[: 4 * self.count]))
        if self.descr == "<f8":
            return list(struct.unpack(f"<{self.count}d", self.data[: 8 * self.count]))
        raise ValueError(f"{self.descr} is not a float dtype")

    def bools(self) -> list[bool]:
        """The elements as booleans (``|b1`` only)."""
        if self.descr != "|b1":
            raise ValueError(f"{self.descr} is not |b1")
        return [byte != 0 for byte in self.data[: self.count]]

    def int_scalar(self) -> int:
        """A 0-d ``<i8``/``<i4`` as an int."""
        if self.descr == "<i8":
            return struct.unpack("<q", self.data[:8])[0]
        if self.descr == "<i4":
            return struct.unpack("<i", self.data[:4])[0]
        raise ValueError(f"{self.descr} is not an integer dtype")

    def str_scalar(self) -> str:
        """A 0-d ``<U{n}`` as text, NUL padding stripped."""
        if not self.descr.startswith("<U"):
            raise ValueError(f"{self.descr} is not a unicode dtype")
        return self.data.decode("utf-32-le").rstrip("\x00")


def f32(values, shape: tuple[int, ...]) -> Array:
    """A little-endian float32 array from a flat iterable in C order."""
    values = list(values)
    return Array("<f4", tuple(shape), struct.pack(f"<{len(values)}f", *values))


def bools(values, shape: tuple[int, ...]) -> Array:
    """A ``|b1`` array from a flat iterable."""
    return Array("|b1", tuple(shape), bytes(1 if v else 0 for v in values))


def int_scalar(value: int) -> Array:
    """A 0-d ``<i8``, as ARDY writes ``fps``."""
    return Array("<i8", (), struct.pack("<q", int(value)))


def str_scalar(text: str) -> Array:
    """A 0-d ``<U{n}``, as ARDY writes the prompt. numpy refuses ``<U0``, so an empty string is ``<U1``."""
    width = max(1, len(text))
    return Array(f"<U{width}", (), text.encode("utf-32-le").ljust(4 * width, b"\x00"))


def npy_bytes(array: Array) -> bytes:
    """Serialise one array as a version-1.0 ``.npy``, header padded to 64 bytes as numpy does."""
    shape = "()" if not array.shape else "(" + ", ".join(str(d) for d in array.shape) + ("," if len(array.shape) == 1 else "") + ")"
    header = f"{{'descr': '{array.descr}', 'fortran_order': False, 'shape': {shape}, }}"
    # numpy pads the header with spaces so that magic + version + len + header
    # is a multiple of 64 bytes, and ends it with a newline.
    prefix = len(MAGIC) + 2 + 2
    padding = (-(prefix + len(header) + 1)) % 64
    header_bytes = (header + " " * padding + "\n").encode("latin-1")
    return MAGIC + b"\x01\x00" + struct.pack("<H", len(header_bytes)) + header_bytes + array.data


def parse_npy(blob: bytes) -> Array:
    """Read one ``.npy`` back: dtype, shape, raw bytes. Refuses Fortran order."""
    if len(blob) < 10 or blob[:6] != MAGIC:
        raise ValueError("not a .npy array (bad magic)")
    major = blob[6]
    if major >= 2:
        length = struct.unpack_from("<I", blob, 8)[0]
        start = 12
    else:
        length = struct.unpack_from("<H", blob, 8)[0]
        start = 10
    header = blob[start : start + length].decode("latin-1")
    fields = ast.literal_eval(header.strip())
    if fields.get("fortran_order"):
        raise ValueError("fortran_order=True is not supported")
    shape = tuple(int(d) for d in fields["shape"])
    return Array(str(fields["descr"]), shape, blob[start + length :])


def write_npz(path: str | os.PathLike, arrays: dict[str, Array]) -> Path:
    """Write a stored (uncompressed) zip of ``.npy`` members, as ``numpy.savez`` does."""
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, array in arrays.items():
            # A fixed timestamp so the same arrays are the same bytes: a
            # fixture that changed every capture would be a test that could
            # not tell the writer changed.
            info = zipfile.ZipInfo(f"{name}.npy", date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, npy_bytes(array))
    return target


def read_npz(path: str | os.PathLike) -> dict[str, Array]:
    """Read every member of a ``.npz`` back, keyed by name without the ``.npy``."""
    with zipfile.ZipFile(path) as archive:
        return {
            info.filename.removesuffix(".npy"): parse_npy(archive.read(info.filename))
            for info in archive.infolist()
            if info.filename.endswith(".npy")
        }


def rest_pose_joints(profile=None) -> list[tuple[float, float, float]]:
    """The driven joints' rest positions in take order, chained from the profile's contract.

    The contract's rest pose is the T-pose every body is skinned to; a fake
    take standing in it is a plausible figure and not a heap at the origin.
    """
    from forge_gen import profile as profile_mod

    prof = profile if profile is not None else profile_mod.load_profile()
    world = prof.rest_world()
    return [world[name][0] for name in prof.joints]


def write_take(
    path: str | os.PathLike,
    *,
    frames: int,
    fps: int = 20,
    prompt: str = "",
    profile=None,
    contacts: bool = True,
    heading: tuple[float, float] = (1.0, 0.0),
) -> Path:
    """Write an ARDY-shaped take of a still figure in the profile's rest pose.

    Identity local and global rotations on every joint, the hips at the
    contract's rest height, every foot flagged planted (``contacts=True``;
    ``False`` omits the member, as takes from before contacts were kept do),
    heading +X. ``frames`` at ``fps`` is the only thing that varies.
    """
    if frames < 1:
        raise ValueError("a take has at least one frame")
    joints = rest_pose_joints(profile)
    count = len(joints)
    identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    rotations = f32(identity * (frames * count), (frames, count, 3, 3))
    flat_joints = [component for joint in joints for component in joint]
    posed = f32(flat_joints * frames, (frames, count, 3))
    root = f32(list(joints[0]) * frames, (frames, 3))
    arrays: dict[str, Array] = {
        "local_rot_mats": rotations,
        "global_rot_mats": rotations,
        "posed_joints": posed,
        "root_positions": root,
        "smooth_root_pos": root,
    }
    if contacts:
        arrays["foot_contacts"] = bools([True] * (frames * 4), (frames, 4))
    arrays["global_root_heading"] = f32(list(heading) * frames, (frames, 2))
    arrays["fps"] = int_scalar(fps)
    arrays["text"] = str_scalar(prompt)
    # One member no real take carries, so a placeholder is recognisable as
    # one (`placeholders.is_placeholder`) and `--fake` can refuse to
    # overwrite a real take. Readers pick members by name and never see it.
    arrays["forge_gen_fake"] = int_scalar(1)
    return write_npz(path, arrays)
