"""``forge-gen ref-import``: a drawn PNG becomes a checked, recorded reference.

    forge-gen ref-import out/refs_grok/ember_knight_v3.png \\
        --name ember_knight_v3 --kind character \\
        --source "xAI Grok, image_edit from a style board"

The one way a PNG gets under ``assets-src/refs/``. No image model ships in
this toolkit — the reference is *brought*, not made here
(``designs/decisions.md``, "The reference image stays brought", 2026-08-30)
— so what this door owns is everything between "a picture exists" and "a
picture this pipeline can account for": the format, the keyer, the gates the
2026-08-30 spike proved ride through to a lift, the stored bytes, the record
and the ledger row.

**Everything here happens before a GPU minute.** A lift is four minutes and
22 GB; a redraw is a sentence. Each of the four keyer pre-checks below is a
refusal and not a warning for exactly that reason: the spike watched a drawn
ground plane lift as a slab, a faint contact shadow key as a detached island
above the dust threshold and ride a foot bone, and a border flood punch
through a cream jacket — all three passed every gate that existed and all
three cost a lift.

Order of operations:

1. **format** — one PNG, 1024 px or more on the long side;
2. **key** — :func:`forge_gen.mesh.keyed`, the actual keyer at its own
   tolerance, imported and not re-implemented, so what this door measures is
   what ``forge gen mesh`` will see;
3. **keyer pre-checks** — floor band, contact shadow, flood-through holes,
   retained alpha;
4. **geometry pre-checks** on the keyed alpha — span over height, heads,
   subject fill, exactly one island above the dust fraction; for a prop, the
   whole object inside the frame with a margin;
5. **write** — ``assets-src/refs/<kind>s/<name>.png`` holding **the original
   bytes**, then ``<name>.ref.json`` beside it and the ``SOURCES.md`` row.

**The stored PNG is the original file, byte for byte, never the keyed one.**
``mesh.py`` keys again at lift time, so a keyed PNG in the source tree would
be a derived artefact whose sha256 and whose ledger row describe something
the user never drew — and a derived artefact in ``assets-src/`` is the one
thing nobody can re-derive.

**The sliver check is not here.** A picture cannot be measured for volume;
only a mesh can. That is the whole of the 2026-08-30 "a reference that passes
every gate can still lift to junk" lesson, and the check lives at
``forge gen prepare``.

Two layers, like every command in this package. The outer half runs under
the system python and is stdlib-only: arguments, the format check, the
refusals, the record, the ledger row. The inner half re-execs under the
**trellis2** backend's interpreter — the env that already holds Pillow,
numpy and OpenCV, because the keyer is TRELLIS's own — opens the image, keys
it, and hands back one JSON object of measurements. The analysis in between
(:func:`analyse`) is stdlib and shared by both halves, so a gate can be
tested with no backend, no card and no third-party wheel.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import traceback
from dataclasses import dataclass
from pathlib import Path

if __package__ in (None, ""):
    # Run as a file rather than as `-m forge_gen.reference`: put the
    # package's parent on the path so the "run me through forge-gen" line
    # below can print instead of an import error.
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from forge_gen import exit_codes, launcher, records  # noqa: E402
from forge_gen.backends import load_backend  # noqa: E402
from forge_gen.exit_codes import ForgeGenError, InputRejected, UsageError  # noqa: E402

#: Whose interpreter keys the image when this one cannot. Not because a
#: reference is lifted by TRELLIS.2 (it is), but because
#: :func:`forge_gen.mesh.keyed` is the keyer and that env is where its
#: Pillow, numpy and OpenCV live. See :func:`measure_image`: the door keys
#: in this process when the three are importable here and borrows that
#: interpreter when they are not, and the answer is the same either way
#: because it is the same module doing the work.
BACKEND = "trellis2"

#: The record's ``tool``. Nothing generated this image; it was brought.
TOOL = "imported"

#: The record kind. ``records.KINDS`` carries it, and so does the Rust
#: reader's ``RecordKind``.
KIND = "ref"

#: What ``--kind`` takes, and the directory each writes into (``<kind>s``).
KINDS = ("character", "prop")

#: The eight bytes every PNG starts with.
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

#: The format's own floor: under this on the long side there is not enough
#: picture for a 1024³ lift to have anything to lift.
MIN_LONG_SIDE = 1024

# --------------------------------------------------------------- the gates --
#
# Every number below is either **measured**, and says on which pictures, or
# marked **budget**, and says what would pin it. The pictures are the
# twenty-one on this machine on 2026-08-30: `out/refs_grok/` (eleven Grok
# references), `out/spike/refs/` (two couriers), `out/spike/reference_v2/`
# (five v2 seeds) and the three shipped references under `assets-src/refs/`.
# Seventeen key — fifteen characters and two props; the four `contact*.png`
# are refused by mesh.py's own border-flatness check before this file sees
# them. Every reference that ever lifted passes every gate below, and the
# three the gates refuse are `ember_knight.png` (superseded by v3, for the
# reason the span gate names) and the two drawn contact shadows.

#: The bottom slice of the frame the floor-band check looks at.
FLOOR_BAND_ROWS = 0.02

#: Refuse when more than this fraction of the columns in that slice carry an
#: opaque pixel. **Budget.** The good side is measured — the worst of the
#: seventeen keyable references reads 0.093 (`courier_v2_11.png`, a figure
#: whose boots touch the bottom row) — but the bad side is not: a drawn
#: ground plane dark enough to matter fails mesh.py's border-flatness check
#: before it reaches here, so no picture on disk exercises this refusal. It
#: is pinned by the test's generated PNG and by 2.7x of headroom over the
#: worst good picture.
FLOOR_BAND_MAX = 0.25

#: A detached island is a contact shadow when it sits under the ankle, is
#: flatter than this fraction of the subject's height, and is wider than
#: :data:`SHADOW_MIN_WIDTH_OVER_FOOT` of the stance. **Measured**: the two
#: drawn shadows on disk read 0.0356 (`courier_v2_7.png`) and 0.0584
#: (`courier_v2_1234.png`) tall and 3.16 and 3.41 wide, and no other
#: secondary island above the dust fraction on any of the seventeen comes
#: near either bound.
SHADOW_MAX_HEIGHT_FRACTION = 0.08
SHADOW_MIN_WIDTH_OVER_FOOT = 1.4

#: The lowest slice of the subject the stance is measured across, and how far
#: up from its lowest row "under the ankle" reaches.
FOOT_ROWS = 0.05
ANKLE_FRACTION = 0.10

#: Refuse when the border flood punched through into the subject: interior
#: holes over this fraction of the subject's own area. **Budget.** Every one
#: of the seventeen reads 0.0 — including `courier_flux.png`, the cream jacket
#: the spike watched the flood eat on another draw of the same prompt — so
#: the number that separates is not on disk. Half a percent of a subject is
#: a hole a reviewer sees.
HOLE_MAX_FRACTION = 0.005

#: An island smaller than this fraction of the subject is dust, not a second
#: object. **Measured**: the real strays on disk (`sword.png`'s two rivets,
#: `courier_v2_42.png`'s four) read 0.0003-0.0022, and the smallest thing
#: anyone would call a second object is `courier_v2_7.png`'s shadow at
#: 0.0428. The same half-percent as :data:`HOLE_MAX_FRACTION`, deliberately:
#: it is one idea — half a percent of the subject is the smallest mark worth
#: refusing over.
DUST_FRACTION = 0.005

#: What fraction of the frame the subject may keep after the key.
#: **Measured**, and *wider at the bottom than designs/skin.md proposed*: the
#: seventeen read 0.123 (`courier_v2_42.png`) to 0.278 (`barrel.png`), so the
#: 0.15 the design proposed refuses two references that lifted. 0.10 sits 19 %
#: under the thinnest measured subject; 0.85 is three times the fattest.
#: mesh.py's own band (0.05-0.95) still runs first and catches the collapse.
RETAINED_ALPHA_BAND = (0.10, 0.85)

#: Head-to-toe against fingertip-to-fingertip, for a character. **Measured**:
#: the characters read 0.772-1.125, and the one picture this band
#: refuses is `ember_knight.png` at 1.321 — superseded by `ember_knight_v3`
#: at 0.983 for exactly this reason. The fit scales a bone's length; it
#: cannot rotate an arm.
SPAN_BAND = (0.7, 1.3)

#: Heads, crown to the arm line (see :func:`analyse`). **Measured, and the
#: number moved**: the twelve characters that reach this check read 3.37
#: (`moss_witch_v5.png`) to 7.54 (`courier_flux.png`). skin.md proposed 4.0
#: "because the four-head witch now ships" — and the witch measures 3.37, so
#: 4.0 refuses the body Phase 2 exists to ship. 3.0 sits 12 % under her, which
#: is the same headroom the sliver gate ships with, and still refuses a
#: picture whose arms begin a third of the way down the frame.
HEADS_REFUSE_BELOW = 3.0

#: Between this and :data:`HEADS_REFUSE_BELOW` the door prints a note and
#: goes on. The format asks for seven; the fitted skeleton made that a
#: preference rather than a rule.
HEADS_NOTE_BELOW = 7.0

#: Under this the door says the subject is small in its frame — a **note**,
#: never a refusal. The measured spread (0.63 for `barrel.png`, 0.69 for a
#: character that lifted, 0.95 for two couriers) does not separate good from
#: bad, and designs/skin.md's own rule is that a gate nobody can calibrate
#: ships as a printed note.
SUBJECT_FILL_NOTE_BELOW = 0.60

#: A prop must sit inside its frame with this much of each dimension clear.
#: The three-quarter view is the whole point: an object cropped at the frame
#: edge lifts with the crop in it.
PROP_MARGIN = 0.02


# ------------------------------------------------------------- the format --
#
# ONE HOME. Verbatim from designs/forge2.md § The reference door, with one
# dated amendment. Every other copy of this text is *generated* from here —
# `crates/forge_mcp/build.rs` reads these two constants at build time for the
# `import_reference` description, and `just ref-format markdown` writes the
# block the forge-character skill includes — because three hand-maintained
# copies held to byte equality is a test that fails on a rewrap and teaches
# people to edit the fixture. There is no third generator of the Rust copy:
# the build step IS that copy, and it cannot go stale.

#: The reference format, as the door, the tool description and the skill all
#: state it. Amended 2026-08-30; see :data:`FORMAT_AMENDMENT`.
FORMAT = """\
A reference is one PNG, 1024 px or more on its long side, of one subject on
a flat, uniform background: no floor, no shadow, no gradient, nothing behind
it. The subject fills about nine tenths of the height, and is exactly as
tall as it is wide: head-to-toe equals fingertip-to-fingertip.

CHARACTER: the front view, facing the camera, in a strict T-pose — arms
straight out and horizontal, palms down, legs slightly apart, feet flat —
holding nothing, with no hair, cloth or gear crossing the silhouette of the
arms or legs. "Chunky" is volume, never proportion: a large head, big hands
and boots, limbs as wide as the neck, a baked key with occlusion painted
into the pits. A picture that passes every gate here can still lift to a
sliver, because no picture can be measured for volume — the sliver check is
on the prepared mesh, at `forge gen prepare`.

PROP: a three-quarter view that shows the top and one side, the whole object
inside the frame, resting the way it will rest in the game.

The importer keys the background to alpha, hashes the file, writes the
SOURCES.md row from the source you state, and runs the silhouette
pre-checks the fit gate would otherwise fail after a lift."""

#: What changed, and when, and why — carried with the text everywhere it is
#: generated, because the amendment is the interesting part.
FORMAT_AMENDMENT = """\
Amended 2026-08-30: the format used to say "at seven heads or more, because
the fit gate measures reach against wrist span and arm height against the
wrists". Phase 2 fitted the skeleton to the body and deleted that gate, so
proportion is no longer a rule. The door refuses below three heads and notes
anything under seven; the four-head witch the old gate refused five times is
the body Phase 2 exists to ship."""


def format_text() -> str:
    """The format and its amendment, as a person reads them."""
    return f"{FORMAT}\n\n{FORMAT_AMENDMENT}\n"


def format_markdown() -> str:
    """The same text as a markdown block, for the skills. ``just ref-format markdown``."""
    quoted = "\n".join(f"> {line}".rstrip() for line in format_text().rstrip("\n").splitlines())
    return (
        "<!-- GENERATED by `just ref-format markdown` from python/forge_gen/reference.py's\n"
        "     FORMAT. The format text has one home; do not edit this block, regenerate it. -->\n"
        f"{quoted}\n"
    )


RENDERERS = {"text": format_text, "markdown": format_markdown}


# ------------------------------------------------------------------ parser --


def add_parser(subparsers) -> None:
    """Register ``ref-import``."""
    parser = subparsers.add_parser(
        "ref-import",
        help="A drawn PNG -> a checked, recorded reference under assets-src/refs/",
        description=__doc__,
    )
    parser.add_argument("image", nargs="?", help="the PNG as it was drawn (anywhere; it is copied, not moved)")
    parser.add_argument("--name", metavar="NAME", help="the library name: assets-src/refs/<kind>s/<name>.png")
    parser.add_argument("--kind", choices=KINDS, help="which register the picture is for")
    parser.add_argument("--source", metavar="TEXT", help="where it came from, in your words — the ledger row and the record's stated_source")
    parser.add_argument("--sources", metavar="DIR", help="the project's sources directory (default <project>/assets-src)")
    parser.add_argument("--overwrite", action="store_true", help="replace a reference of this name; the record it replaces is echoed")
    parser.add_argument("--print-format", choices=sorted(RENDERERS), metavar="text|markdown", help="print the reference format and exit — the one home of that text")


# --------------------------------------------------------------- settling --


@dataclass(frozen=True)
class Settled:
    """Everything resolved before anything is read or written."""

    image: Path
    name: str
    kind: str
    source: str
    sources: Path
    png: Path
    record: Path
    ledger: Path
    overwrite: bool


def check_png(path: Path) -> None:
    """Exists, is a file, and starts with the PNG signature — not the suffix."""
    if not path.exists():
        raise InputRejected(f"{path} does not exist", image=str(path))
    if not path.is_file():
        raise InputRejected(f"{path} is not a file — a reference is one PNG", image=str(path))
    with open(path, "rb") as handle:
        head = handle.read(len(PNG_SIGNATURE))
    if head != PNG_SIGNATURE:
        raise InputRejected(
            f"{path} is not a PNG (the file does not start with the PNG signature). "
            "A reference is one PNG; export it again rather than renaming it.",
            image=str(path),
        )


def _valid_name(name: str) -> bool:
    return bool(name) and all(character.isalnum() or character in "_-" for character in name)


def settle(args) -> Settled:
    """Validate and resolve every path; nothing is read yet."""
    if not args.image:
        raise UsageError("ref-import needs the PNG to import (or --print-format)")
    if not args.name:
        raise UsageError("--name is required: it is the name the library will know this reference by")
    if not args.kind:
        raise UsageError(f"--kind is required: {' or '.join(KINDS)}")
    source = (args.source or "").strip()
    if not source:
        raise UsageError(
            "--source is required: a reference claims a ledger row, and the row is where its "
            'origin and its licence live (e.g. --source "xAI Grok, image_edit from a style board")'
        )
    name = args.name.strip()
    if not _valid_name(name):
        raise InputRejected(f"--name {name!r} is not a library name (letters, digits, '_' and '-')")
    image = Path(args.image).expanduser().resolve()
    check_png(image)
    root = records.project() or Path.cwd()
    sources = Path(args.sources).expanduser().resolve() if args.sources else Path(root) / "assets-src"
    directory = sources / "refs" / f"{args.kind}s"
    return Settled(
        image=image,
        name=name,
        kind=args.kind,
        source=source,
        sources=sources,
        png=directory / f"{name}.png",
        record=directory / f"{name}.ref.json",
        ledger=sources / "SOURCES.md",
        overwrite=bool(getattr(args, "overwrite", False)),
    )


def refuse_taken(settled: Settled) -> None:
    """A taken name is refused and names what it would replace, like every other door."""
    if settled.png.exists() and not settled.overwrite:
        raise InputRejected(
            f"{settled.png} already exists — a reference is never quietly replaced, because "
            f"everything lifted from it hashes these bytes. Pass --overwrite to replace it "
            f"(the record it replaces is echoed), or import under another --name.",
            existing=str(settled.png),
        )


# --------------------------------------------------------------- analysis --
#
# Stdlib, on a plain alpha bitmap, so every gate below is testable with no
# backend, no card and no third-party wheel. `flags` is one byte per pixel,
# row-major, 1 where the pixel is opaque.


def _components(width: int, height: int, flags, *, connectivity: int = 8) -> list[dict]:
    """Connected components of the set pixels, by scanline runs and union-find.

    Runs rather than pixels: a reference is a megapixel, and the components
    of a silhouette are a few thousand runs. Each component carries its area,
    its bounding box and its runs, which is everything the gates ask.
    """
    parent: list[int] = []

    def find(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    def union(a: int, b: int) -> None:
        root_a, root_b = find(a), find(b)
        if root_a != root_b:
            parent[max(root_a, root_b)] = min(root_a, root_b)

    runs: list[tuple[int, int, int]] = []
    previous: list[int] = []
    slack = 1 if connectivity == 8 else 0
    for y in range(height):
        base = y * width
        row: list[int] = []
        x = 0
        while x < width:
            if flags[base + x]:
                start = x
                while x < width and flags[base + x]:
                    x += 1
                runs.append((y, start, x - 1))
                parent.append(len(runs) - 1)
                row.append(len(runs) - 1)
            else:
                x += 1
        for index in row:
            _, a0, a1 = runs[index]
            for other in previous:
                _, b0, b1 = runs[other]
                if b0 <= a1 + slack and a0 <= b1 + slack:
                    union(index, other)
        previous = row

    grouped: dict[int, dict] = {}
    for index, (y, x0, x1) in enumerate(runs):
        root = find(index)
        entry = grouped.get(root)
        if entry is None:
            grouped[root] = {"area": x1 - x0 + 1, "x0": x0, "x1": x1, "y0": y, "y1": y, "runs": [(y, x0, x1)]}
        else:
            entry["area"] += x1 - x0 + 1
            entry["x0"] = min(entry["x0"], x0)
            entry["x1"] = max(entry["x1"], x1)
            entry["y1"] = y
            entry["runs"].append((y, x0, x1))
    components = list(grouped.values())
    components.sort(key=lambda entry: -entry["area"])
    return components


def _row_widths(component: dict) -> list[int]:
    """The subject's width on every one of its own rows, top to bottom."""
    y0, y1 = component["y0"], component["y1"]
    left = [None] * (y1 - y0 + 1)
    right = [None] * (y1 - y0 + 1)
    for y, x0, x1 in component["runs"]:
        index = y - y0
        left[index] = x0 if left[index] is None else min(left[index], x0)
        right[index] = x1 if right[index] is None else max(right[index], x1)
    return [0 if left[i] is None else right[i] - left[i] + 1 for i in range(len(left))]


def heads_from(widths: list[int]) -> tuple[float | None, int | None]:
    """Heads, crown to the arm line, and the row the arms start on.

    Not the artist's head count and it does not pretend to be. A T-posed
    silhouette has one landmark a machine can find without argument: the row
    where the width first reaches half the widest row, which is where the
    arms come out. Everything above it is head, headgear and neck, and its
    reciprocal is what "heads" means here.

    Three estimators were tried on the seventeen references on disk
    (2026-08-30). A neck-pinch estimator disagreed with itself by a factor of
    two on the same picture (`ember_knight_v3.png` read 6.58 or 10.74) and
    returned nothing at all on six of them; this one is defined everywhere
    and monotone in the thing it measures. It counts a hat: the witch's
    pointed hat is a quarter of her height and she reads 3.37 rather than
    4.5. That is the honest answer to the question that matters downstream —
    how much of the picture is not body.
    """
    if not widths:
        return None, None
    height = len(widths)
    widest = max(widths)
    if widest <= 0:
        return None, None
    arm_line = next((index for index, width in enumerate(widths) if width >= 0.5 * widest), None)
    if arm_line is None or arm_line < 1:
        return None, arm_line
    return round(height / arm_line, 2), arm_line


def analyse(width: int, height: int, flags) -> dict:
    """Every measurement the gates read, from the keyed alpha alone.

    ``flags`` is one byte per pixel, row-major, 1 where opaque. Returns the
    ``measured`` block's geometry half; the caller adds what only the keyer
    knows (the backdrop it voted for, the tolerance it used).
    """
    subject_area = sum(1 for value in flags if value)
    measured: dict = {
        "width": width,
        "height": height,
        "alpha_fraction": round(subject_area / (width * height), 4) if width and height else 0.0,
        "span_over_height": None,
        "heads": None,
        "arm_line_fraction": None,
        "subject_fill": None,
        "islands": 0,
        "islands_all": 0,
        "floor_band": 0.0,
        "contact_shadow_px": 0,
        "interior_hole_fraction": 0.0,
        "foot_width_px": None,
        "margins": None,
        "shadows": [],
    }
    if subject_area == 0:
        return measured

    # The floor band: how much of the bottom slice of the FRAME carries the
    # subject. A drawn ground plane spans it; two boots do not.
    band = max(1, int(round(height * FLOOR_BAND_ROWS)))
    columns = bytearray(width)
    for y in range(height - band, height):
        base = y * width
        for x in range(width):
            if flags[base + x]:
                columns[x] = 1
    measured["floor_band"] = round(sum(columns) / width, 4)

    components = _components(width, height, flags, connectivity=8)
    measured["islands_all"] = len(components)
    dust = DUST_FRACTION * subject_area
    kept = [entry for entry in components if entry["area"] > dust]
    measured["islands"] = len(kept)
    main = components[0]
    subject_height = main["y1"] - main["y0"] + 1
    subject_width = main["x1"] - main["x0"] + 1
    measured["span_over_height"] = round(subject_width / subject_height, 3)
    measured["subject_fill"] = round(subject_height / height, 3)

    widths = _row_widths(main)
    heads, arm_line = heads_from(widths)
    measured["heads"] = heads
    measured["arm_line_fraction"] = round(arm_line / subject_height, 4) if arm_line else None

    # The stance: how wide the subject is across its own lowest rows. A
    # contact shadow is measured against it, because a shadow is drawn under
    # the feet and is wider than they are.
    foot_rows = max(1, int(subject_height * FOOT_ROWS))
    foot_left, foot_right = None, None
    for y, x0, x1 in main["runs"]:
        if y > main["y1"] - foot_rows:
            foot_left = x0 if foot_left is None else min(foot_left, x0)
            foot_right = x1 if foot_right is None else max(foot_right, x1)
    foot_width = (foot_right - foot_left + 1) if foot_left is not None else subject_width
    measured["foot_width_px"] = foot_width

    ankle_y = main["y1"] - ANKLE_FRACTION * subject_height
    shadows = []
    for entry in kept[1:]:
        shadows.append(
            {
                "area_fraction": round(entry["area"] / subject_area, 4),
                "width_px": entry["x1"] - entry["x0"] + 1,
                "height_px": entry["y1"] - entry["y0"] + 1,
                "height_over_subject": round((entry["y1"] - entry["y0"] + 1) / subject_height, 4),
                "width_over_stance": round((entry["x1"] - entry["x0"] + 1) / max(foot_width, 1), 3),
                "under_ankle": bool(entry["y0"] > ankle_y),
            }
        )
    measured["shadows"] = shadows

    # Interior holes: transparent pixels the border flood could not reach.
    # A hole is where the key ate the subject.
    holes = bytearray(width * height)
    for index, value in enumerate(flags):
        if not value:
            holes[index] = 1
    background = _components(width, height, holes, connectivity=4)
    hole_area = 0
    for entry in background:
        touches = entry["x0"] == 0 or entry["y0"] == 0 or entry["x1"] == width - 1 or entry["y1"] == height - 1
        if not touches:
            hole_area += entry["area"]
    measured["interior_hole_fraction"] = round(hole_area / subject_area, 5)
    measured["contact_shadow_px"] = sum(entry["width_px"] * entry["height_px"] for entry in shadows if entry["under_ankle"])

    measured["margins"] = {
        "left": round(main["x0"] / width, 4),
        "right": round((width - 1 - main["x1"]) / width, 4),
        "top": round(main["y0"] / height, 4),
        "bottom": round((height - 1 - main["y1"]) / height, 4),
    }
    return measured


# ----------------------------------------------------------- the pre-checks --


def check_keyer(measured: dict, *, image: str) -> None:
    """The four the spike proved ride through to the lift. Each is a refusal.

    Order matters: the contact-shadow refusal has to come before the island
    count, or a drawn shadow is refused as "two objects" and the person
    reading the message goes looking for a second object.
    """
    floor = measured.get("floor_band") or 0.0
    if floor > FLOOR_BAND_MAX:
        raise InputRejected(
            f"{image}: the subject spans {floor:.0%} of the bottom {FLOOR_BAND_ROWS:.0%} of the frame "
            f"after the key, past {FLOOR_BAND_MAX:.0%} — that is a drawn ground plane, not a pair of "
            "feet, and it lifts as a slab welded to the boots. Redraw on a flat background with no "
            "floor under the subject.",
            floor_band=floor,
        )
    for shadow in measured.get("shadows") or []:
        if (
            shadow["under_ankle"]
            and shadow["height_over_subject"] < SHADOW_MAX_HEIGHT_FRACTION
            and shadow["width_over_stance"] > SHADOW_MIN_WIDTH_OVER_FOOT
        ):
            raise InputRejected(
                f"{image}: a detached island under the ankle is {shadow['height_over_subject']:.1%} of "
                f"the subject's height and {shadow['width_over_stance']:.1f}x the stance wide "
                f"({shadow['width_px']}x{shadow['height_px']} px) — that is a contact shadow. It keys as "
                "an island above the dust threshold and rides a foot bone through the whole pipeline. "
                "Redraw with no shadow under the subject.",
                shadow=shadow,
            )
    holes = measured.get("interior_hole_fraction") or 0.0
    if holes > HOLE_MAX_FRACTION:
        raise InputRejected(
            f"{image}: the key punched through the subject — interior holes are {holes:.2%} of it, past "
            f"{HOLE_MAX_FRACTION:.2%}. Somewhere inside the silhouette matches the backdrop within the "
            "keyer's tolerance and is connected to the border through a gap. Redraw with the backdrop "
            "further from the subject's own colours, or close the gap the flood came through.",
            interior_hole_fraction=holes,
        )
    fraction = measured.get("alpha_fraction") or 0.0
    low, high = RETAINED_ALPHA_BAND
    if not low <= fraction <= high:
        raise InputRejected(
            f"{image}: the key kept {fraction:.1%} of the frame as subject, outside {low:.0%}-{high:.0%}. "
            "Below the band the subject is a speck in its frame; above it the key kept the backdrop. "
            "Reframe the drawing rather than loosening the keyer.",
            alpha_fraction=fraction,
        )


def check_geometry(measured: dict, kind: str, *, image: str) -> list[str]:
    """Span, heads, fill and islands. Returns the notes; refusals are raised.

    A note is not a soft refusal — it is a measurement the door could not
    turn into a rule, said out loud so the person looking at the picture can.
    """
    notes: list[str] = []
    if kind == "character":
        span = measured.get("span_over_height")
        low, high = SPAN_BAND
        if span is not None and not low <= span <= high:
            raise InputRejected(
                f"{image}: the subject is {span:.3f} as wide as it is tall, outside {low}-{high}. "
                "Head-to-toe equals fingertip-to-fingertip: the fit scales a bone's length to this "
                "body, it cannot rotate an arm. Redraw with the arms straight out and horizontal.",
                span_over_height=span,
            )
        heads = measured.get("heads")
        if heads is not None and heads < HEADS_REFUSE_BELOW:
            raise InputRejected(
                f"{image}: {heads:.2f} heads — the arms come out {measured.get('arm_line_fraction', 0):.0%} of "
                f"the way down the subject, under {HEADS_REFUSE_BELOW:g}. There is no torso between the "
                "head and the shoulders for a skeleton to sit in. Redraw taller, or crop less of the legs.",
                heads=heads,
            )
        if heads is not None and heads < HEADS_NOTE_BELOW:
            notes.append(
                f"{heads:.2f} heads (crown to the arm line; a hat counts) — under the format's seven. "
                "That is allowed since the skeleton fits the body: judge it on the strip, not here."
            )
        if heads is None:
            notes.append("heads could not be measured: no arm line in the silhouette. A T-pose has one.")
    else:
        margins = measured.get("margins") or {}
        tight = sorted(side for side, value in margins.items() if value < PROP_MARGIN)
        if tight:
            raise InputRejected(
                f"{image}: the object touches the frame at the {', '.join(tight)} "
                f"(margins {', '.join(f'{side} {margins[side]:.1%}' for side in tight)}, under "
                f"{PROP_MARGIN:.0%}). A cropped prop lifts with the crop in it. Reframe with the whole "
                "object inside the picture.",
                margins=margins,
            )

    islands = measured.get("islands") or 0
    if islands > 1:
        raise InputRejected(
            f"{image}: {islands} islands survive the dust threshold ({DUST_FRACTION:.1%} of the subject) "
            "— a reference is one subject alone. Whatever the second object is, it keys as its own "
            "island and binds to whichever bone is nearest.",
            islands=islands,
        )
    if islands == 0:
        raise InputRejected(f"{image}: nothing survived the key — there is no subject in this picture.")

    fill = measured.get("subject_fill")
    if fill is not None and fill < SUBJECT_FILL_NOTE_BELOW:
        notes.append(
            f"the subject fills {fill:.0%} of the frame height; the format asks for about nine tenths. "
            "Fewer pixels of the thing you want is fewer pixels for TRELLIS.2 to lift it from."
        )
    strays = (measured.get("islands_all") or 0) - islands
    if strays:
        notes.append(f"{strays} island(s) under the dust threshold were measured and ignored.")
    return notes


# ----------------------------------------------------------------- record --


def build_record(settled: Settled, *, measured: dict, created_by: str | None, fake: bool) -> dict:
    """The ``ref`` record: what was brought, what it measured, what was stored.

    ``backend`` is all-null and stays that way. Nothing generated this
    image; the toolkit ships no image model, and a backend block filled in
    with the keyer's env would say a thing that is not true.
    """
    rec = records.new_record(KIND, TOOL, created_by=created_by)
    # Every field null, `executor` included: `backend_block` defaults it to
    # "env" for a generator that ran in one, and nothing ran here.
    rec["backend"] = records.backend_block(executor=None)
    records.add_input(rec, "image", settled.image, source=settled.source)
    rec["params"] = {
        "kind": settled.kind,
        "long_side_px": max(measured.get("width") or 0, measured.get("height") or 0) or None,
        "stated_source": settled.source,
    }
    rec["measured"] = {key: value for key, value in measured.items() if key not in ("shadows", "margins")}
    if settled.kind != "character":
        # `heads` is a T-pose measurement: the height above the row where the
        # arms come out. A prop has no arm line, so the number the analysis
        # returns is a true statement about a silhouette and a meaningless one
        # about a barrel (54.08 of them). `null` is what a record says for a
        # quantity that is not defined here, and no gate reads it for a prop.
        rec["measured"]["heads"] = None
        rec["measured"]["arm_line_fraction"] = None
    rec["fake"] = bool(fake)
    if fake:
        rec["note"] = "placeholder run: the picture was stored and hashed, but nothing about it was measured"
    return rec


# ----------------------------------------------------------------- ledger --

#: The header cells that identify the reference table in ``SOURCES.md``.
LEDGER_HEADER = ("File", "Origin", "For", "Date")


#: The "For" cell the door writes when the ledger has nothing to say yet.
DEFAULT_FOR = "a {kind} reference, imported by `forge ref import`"


def ledger_row(settled: Settled, *, when: str, keep_for: str | None = None) -> str:
    """The row this reference gets, exactly as the ledger's table spells one.

    ``keep_for`` is the "For" cell an existing row already carried. The door
    owns Origin (that is what ``--source`` states) and the date; it does
    **not** own what somebody wrote about what was made from the picture, so
    a re-import restates the origin and leaves that sentence standing.
    """
    where = f"{settled.kind}s/{settled.name}.png"
    what = keep_for or DEFAULT_FOR.format(kind=settled.kind)
    return f"| `{where}` | {_cell(settled.source)} | {what} | {when} |"


def _cell(text: str) -> str:
    """A table cell: one line, pipes escaped, because a row is a row."""
    return " ".join(text.split()).replace("|", "\\|")


def existing_for(path: Path, key: str) -> str | None:
    """The "For" cell of this reference's row, when the ledger already has one."""
    if not path.exists():
        return None
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith(f"| `{key}` |"):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) >= 3 and cells[2]:
                return cells[2]
    return None


def write_ledger_row(path: Path, row: str, *, key: str) -> str:
    """Add or replace this reference's row in ``SOURCES.md``, in the reference table.

    Written by the door and never by hand: a PNG without a row fails
    ``forge verify``, and a row typed by a person is one nobody re-derived.
    Returns ``"added"`` or ``"replaced"``.
    """
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    lines = text.splitlines()
    marker = f"| `{key}` |"
    for index, line in enumerate(lines):
        if line.startswith(marker):
            lines[index] = row
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("\n".join(lines) + "\n", encoding="utf-8")
            return "replaced"

    # Find the reference table: the header row naming File and Origin, then
    # the last row under it. A row appended to the end of the file would be
    # a row `forge verify` accepts and nobody can find.
    insert = None
    for index, line in enumerate(lines):
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) >= len(LEDGER_HEADER) and cells[: len(LEDGER_HEADER)] == list(LEDGER_HEADER):
            insert = index + 2  # past the separator row
            while insert < len(lines) and lines[insert].lstrip().startswith("|"):
                insert += 1
            break
    if insert is None:
        lines.extend(["", row])
    else:
        lines.insert(insert, row)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return "added"


# -------------------------------------------------------------- the doors --


def _summary(settled: Settled, measured: dict, notes: list[str], *, fake: bool) -> str:
    head = "imported (placeholder run)" if fake else "imported"
    lines = [f"{head} {settled.image.name} -> {settled.png}"]
    if measured.get("width"):
        lines.append(
            f"  {measured['width']}x{measured['height']}, subject {measured.get('alpha_fraction')}, "
            f"span/height {measured.get('span_over_height')}, heads {measured.get('heads')}, "
            f"fill {measured.get('subject_fill')}, islands {measured.get('islands')}"
        )
    else:
        lines.append("  nothing measured: the bytes were stored and hashed, and that is all this run claims")
    lines.append(f"  record {settled.record}")
    lines.append(f"  ledger {settled.ledger}")
    lines.extend(f"  note: {note}" for note in notes)
    return "\n".join(lines)


def print_format(kind: str) -> dict:
    """Write the format on stdout and claim nothing.

    Not a ``_text`` payload: a payload is rendered by whoever called the
    door, and `forge gen` renders one as an aligned key table after the
    child has exited — so a page of prose returned that way reaches a person
    running `python -m forge_gen` and nobody running `just ref-format`. Printed
    here it travels through the job log like every other line the door writes,
    and the empty payload keeps the JSON contract (the last stdout line under
    ``--json`` is still the object).
    """
    sys.stdout.write(RENDERERS[kind]())
    sys.stdout.flush()
    return {}


def run(args) -> dict:
    """Key, check, store the original bytes, record, and write the ledger row."""
    if args.print_format:
        return print_format(args.print_format)
    settled = settle(args)
    refuse_taken(settled)
    measured = measure_image(settled.image)

    check_keyer(measured, image=settled.image.name)
    notes = check_geometry(measured, settled.kind, image=settled.image.name)

    return _store(settled, measured, notes, created_by=getattr(args, "created_by", None), fake=False)


def run_fake(args) -> dict:
    """Store and record the picture with nothing measured; no backend, no keyer.

    A ``--fake`` reference import is the one placeholder in this toolkit that
    is not a placeholder file: the bytes stored are the caller's own PNG,
    because copying a file needs no model. What is fake is the *measurement*
    — and so every measured field is ``null``, which is what ``null`` means
    everywhere else in a record.
    """
    if args.print_format:
        return print_format(args.print_format)
    settled = settle(args)
    refuse_taken(settled)
    from forge_gen import placeholders  # noqa: PLC0415 - only the fake path needs it

    placeholders.refuse_real(settled.record)
    measured = {
        "width": None,
        "height": None,
        "alpha_fraction": None,
        "span_over_height": None,
        "heads": None,
        "arm_line_fraction": None,
        "subject_fill": None,
        "islands": None,
        "islands_all": None,
        "floor_band": None,
        "contact_shadow_px": None,
        "interior_hole_fraction": None,
        "foot_width_px": None,
        "backdrop_rgb": None,
        "keyer_tolerance": None,
    }
    return _store(settled, measured, [], created_by=getattr(args, "created_by", None), fake=True)


def _store(settled: Settled, measured: dict, notes: list[str], *, created_by: str | None, fake: bool) -> dict:
    """The three files, in the order that leaves nothing half-written.

    The PNG first (the record hashes it), the record second, the ledger row
    last — so a ledger row never names a file that is not there.
    """
    replaced = None
    if settled.png.exists() and settled.record.exists():
        try:
            replaced = records.load(settled.record)
        except (OSError, ValueError):
            replaced = None
    settled.png.parent.mkdir(parents=True, exist_ok=True)
    # The ORIGINAL bytes. `copyfile` and not the keyed image: mesh.py keys
    # again at lift time, and a keyed PNG here would be a derived artefact
    # whose hash and ledger row describe something nobody drew. Importing a
    # picture that is already at its destination — which is how a reference
    # that predates this door gets its record — copies nothing and changes
    # no byte of it.
    if not (settled.png.exists() and settled.png.samefile(settled.image)):
        shutil.copyfile(settled.image, settled.png)
    rec = build_record(settled, measured=measured, created_by=created_by, fake=fake)
    records.add_output(rec, settled.png)
    records.write(rec, settled.record)
    key = f"{settled.kind}s/{settled.name}.png"
    row = ledger_row(settled, when=rec["created"], keep_for=existing_for(settled.ledger, key))
    ledger = write_ledger_row(settled.ledger, row, key=key)
    payload = {
        "name": settled.name,
        "kind": settled.kind,
        "outputs": [str(settled.png)],
        "record": str(settled.record),
        "ledger": str(settled.ledger),
        "ledger_row": ledger,
        "measured": rec["measured"],
        "notes": notes,
        # The record's measured block, not the analysis's: what is printed and
        # what is filed have to be the same numbers.
        "_text": _summary(settled, rec["measured"], notes, fake=fake),
    }
    if replaced is not None:
        payload["replaced"] = {
            "created": replaced.get("created"),
            "sha256": (replaced.get("outputs") or [{}])[0].get("sha256"),
        }
    return payload


# ------------------------------------------------------------- inner layer --


def measure_image(path: Path) -> dict:
    """Key and measure, here or under the backend's interpreter — same answer.

    ``python/forge_gen`` is stdlib-only on purpose: a runtime dependency here
    would have to be installed into the system python before a single backend
    could be probed. :func:`forge_gen.mesh.keyed` needs Pillow, numpy and
    OpenCV. So this asks whether the three are importable in *this*
    interpreter and, when they are not, re-execs the inner half under the
    backend that already has them (exit 3 in ~100 ms if it is not installed,
    before anything is written). It is the same module either way — this is a
    question about which interpreter, not about which code.
    """
    try:
        import cv2  # noqa: F401, PLC0415
        import numpy  # noqa: F401, PLC0415
        from PIL import Image  # noqa: F401, PLC0415
    except ImportError:
        backend = load_backend(BACKEND)
        launcher.resolve_interpreter(backend)
        result = launcher.run_inner_checked(backend, "reference", [str(path)])
        return dict(result.get("measured") or {})
    return keyed_alpha(path)


def keyed_alpha(path: Path) -> dict:
    """Open, key, measure. Runs under the backend's interpreter, never here.

    The key is :func:`forge_gen.mesh.keyed` and nothing else — the same call
    ``forge gen mesh`` makes on the same file, so what this door refuses is
    what the lift would have seen. The backdrop colour is read afterwards
    for the record; reading it is a report, not a second key.
    """
    import numpy  # noqa: PLC0415 - inner only
    from PIL import Image  # noqa: PLC0415

    from forge_gen import mesh  # noqa: PLC0415

    image = Image.open(path)
    width, height = image.size
    if max(width, height) < MIN_LONG_SIDE:
        raise InputRejected(
            f"{path.name} is {width}x{height}: under {MIN_LONG_SIDE} px on the long side. "
            "A 1024-cubed lift has nothing to lift from a smaller picture; draw it again larger "
            "rather than upscaling this one.",
            width=width,
            height=height,
        )
    rgba = numpy.array(image.convert("RGBA"))
    had_alpha = not bool(numpy.all(rgba[:, :, 3] == 255))
    keyed, _fraction = mesh.keyed(image)
    alpha = numpy.array(keyed.convert("RGBA"))[:, :, 3]
    flags = (alpha > 0).astype(numpy.uint8).tobytes()
    measured = analyse(width, height, flags)
    if had_alpha:
        # The picture arrived with its own alpha and mesh.keyed trusted it.
        # Nothing was keyed, so there is no backdrop to name and no tolerance
        # to report — null means unknown, and it is unknown.
        measured["backdrop_rgb"] = None
        measured["keyer_tolerance"] = None
    else:
        rgb = rgba[:, :, :3].astype(numpy.int16)
        border = numpy.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]])
        measured["backdrop_rgb"] = [int(value) for value in numpy.median(border, axis=0)]
        measured["keyer_tolerance"] = mesh.KEY_TOLERANCE
    return measured


def _inner_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="forge_gen.reference --inner", add_help=True)
    parser.add_argument("image")
    return parser


def main_inner(argv: list[str]) -> int:
    """The inner entry: keyed under the backend's interpreter, one JSON line out."""
    namespace = _inner_parser().parse_args(argv)
    try:
        measured = keyed_alpha(Path(namespace.image).resolve())
    except ForgeGenError as err:
        sys.stderr.write(f"ref-import: {err.error}: {err.message}\n")
        print(json.dumps(err.payload()), flush=True)
        return err.code
    except Exception as err:  # noqa: BLE001 - the last line must still be JSON
        traceback.print_exc()
        failure = exit_codes.BackendFailed(f"{err.__class__.__name__}: {err}")
        print(json.dumps(failure.payload()), flush=True)
        return failure.code
    print(json.dumps({"measured": measured}), flush=True)
    return exit_codes.OK


if __name__ == "__main__":
    _argv = sys.argv[1:]
    if _argv and _argv[0] == "--inner":
        sys.exit(main_inner(_argv[1:]))
    sys.stderr.write(
        "forge_gen.reference: run me through forge-gen "
        "(python3 python/forge_gen ref-import <png> --name N --kind character --source '…')\n"
    )
    sys.exit(exit_codes.USAGE)
