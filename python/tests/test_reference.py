"""The reference door: the gates, the stored bytes, the record, the ledger row.

Every gate here is exercised from a **generated picture**, not from a
hand-written measurement dict, because the thing that has to hold is that a
drawing with a floor under it is refused — not that a function refuses a
number. The pictures are RGBA PNGs written with :mod:`forge_gen.png`, which
carry their own alpha, so :func:`forge_gen.mesh.keyed` passes them through
untouched and the analysis runs on exactly the mask the test drew. That also
keeps the whole file stdlib: no numpy, no OpenCV, no Pillow, so these gates
run on a bare runner rather than skipping there.

The one thing that needs the third-party half — opening a real PNG and
keying a flat backdrop away — is :func:`forge_gen.reference.keyed_alpha`,
and it is tested behind an ``importorskip``.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from forge_gen import png, records, reference
from forge_gen.exit_codes import InputRejected, UsageError

SIZE = 1024


# ----------------------------------------------------------------- drawing --


class Canvas:
    """A transparent frame you can paint opaque rectangles into."""

    def __init__(self, width: int = SIZE, height: int = SIZE) -> None:
        self.width = width
        self.height = height
        self.flags = bytearray(width * height)

    def box(self, x0: int, y0: int, x1: int, y1: int) -> "Canvas":
        for y in range(max(0, y0), min(self.height, y1)):
            base = y * self.width
            for x in range(max(0, x0), min(self.width, x1)):
                self.flags[base + x] = 1
        return self

    def analyse(self) -> dict:
        return reference.analyse(self.width, self.height, self.flags)

    def write(self, path: Path) -> Path:
        """The same mask as a real RGBA PNG on disk: opaque grey on transparent."""
        pixels = bytearray()
        for value in self.flags:
            pixels += b"\x80\x80\x80\xff" if value else b"\x00\x00\x00\x00"
        png.write_png(path, self.width, self.height, bytes(pixels))
        return path


def figure(*, heads: float = 6.0, shadow: bool = False, floor: bool = False, hole: bool = False) -> Canvas:
    """A T-posed stick figure: head band, arm bar, torso, two legs.

    Drawn so `span_over_height` is 1.0 and the arm line sits at `1 / heads`
    of the subject's height, which is what :func:`reference.heads_from`
    measures.
    """
    canvas = Canvas()
    top, bottom = 100, 900
    height = bottom - top
    centre = SIZE // 2
    half_span = height // 2
    arm_line = top + int(round(height / heads))
    canvas.box(centre - 60, top, centre + 60, arm_line)                     # head and neck
    canvas.box(centre - half_span, arm_line, centre + half_span, arm_line + 70)  # the arms
    canvas.box(centre - 90, arm_line, centre + 90, top + int(height * 0.62))     # torso
    canvas.box(centre - 80, top + int(height * 0.62), centre - 10, bottom)       # left leg
    canvas.box(centre + 10, top + int(height * 0.62), centre + 80, bottom)       # right leg
    if hole:
        for y in range(arm_line + 120, arm_line + 260):
            base = y * SIZE
            for x in range(centre - 60, centre + 60):
                canvas.flags[base + x] = 0
    if shadow:
        canvas.box(centre - 220, bottom + 14, centre + 220, bottom + 34)
    if floor:
        canvas.box(40, SIZE - 14, SIZE - 40, SIZE - 2)
    return canvas


def refusal(function, *args, **kwargs) -> str:
    with pytest.raises(InputRejected) as caught:
        function(*args, **kwargs)
    return caught.value.message


# ------------------------------------------------------------- the analysis --


def test_a_clean_figure_measures_what_the_record_claims():
    measured = figure(heads=6.0).analyse()
    assert measured["width"] == SIZE and measured["height"] == SIZE
    assert measured["islands"] == 1
    assert measured["floor_band"] == 0.0
    assert measured["interior_hole_fraction"] == 0.0
    assert measured["span_over_height"] == pytest.approx(1.0, abs=0.02)
    assert measured["heads"] == pytest.approx(6.0, abs=0.2)
    assert 0.1 < measured["alpha_fraction"] < 0.85


# --------------------------------------------------- the keyer pre-checks --


def test_a_floor_band_is_refused_by_name():
    message = refusal(reference.check_keyer, figure(floor=True).analyse(), image="ref.png")
    assert "ground plane" in message
    assert "bottom" in message


def test_a_contact_shadow_is_refused_by_name():
    measured = figure(shadow=True).analyse()
    assert measured["islands"] == 2, "the shadow has to survive the dust threshold to be worth refusing"
    message = refusal(reference.check_keyer, measured, image="ref.png")
    assert "contact shadow" in message
    assert "foot bone" in message


def test_a_flood_through_hole_is_refused_by_name():
    measured = figure(hole=True).analyse()
    assert measured["interior_hole_fraction"] > reference.HOLE_MAX_FRACTION
    message = refusal(reference.check_keyer, measured, image="ref.png")
    assert "punched through" in message
    assert "interior holes" in message


def test_the_shadow_refusal_comes_before_the_island_count():
    """Both fire on the same picture; the one that names the cause has to win."""
    measured = figure(shadow=True).analyse()
    message = refusal(reference.check_keyer, measured, image="ref.png")
    assert "contact shadow" in message
    # And the island count is what would have caught it otherwise.
    assert "islands" in refusal(reference.check_geometry, measured, "character", image="ref.png")


def test_a_subject_that_kept_the_backdrop_is_refused_on_retained_alpha():
    # Not the whole frame: the bottom rows stay clear so the floor band, which
    # is checked first, has nothing to say and the alpha band is what refuses.
    measured = Canvas().box(0, 0, SIZE, 970).analyse()
    message = refusal(reference.check_keyer, measured, image="ref.png")
    assert "the key kept" in message and "outside" in message


# ------------------------------------------------ the geometry pre-checks --


def test_a_four_head_silhouette_is_noted_and_not_refused():
    measured = figure(heads=4.0).analyse()
    reference.check_keyer(measured, image="witch.png")
    notes = reference.check_geometry(measured, "character", image="witch.png")
    assert any("heads" in note for note in notes)
    assert any("strip" in note for note in notes)


def test_a_two_and_a_half_head_silhouette_is_refused():
    measured = figure(heads=2.5).analyse()
    message = refusal(reference.check_geometry, measured, "character", image="chibi.png")
    assert "heads" in message
    assert "no torso" in message


def test_a_seven_head_silhouette_is_neither_refused_nor_noted():
    measured = figure(heads=7.5).analyse()
    assert reference.check_geometry(measured, "character", image="knight.png") == []


def test_a_wide_subject_is_refused_on_span():
    canvas = Canvas()
    canvas.box(462, 200, 562, 300)          # head
    canvas.box(20, 300, SIZE - 20, 380)     # a span twice the height
    canvas.box(440, 300, 584, 700)          # torso and legs
    message = refusal(reference.check_geometry, canvas.analyse(), "character", image="wide.png")
    assert "as wide as it is tall" in message


def test_a_prop_touching_the_frame_is_refused_on_margin():
    canvas = Canvas().box(0, 200, 700, 800)
    message = refusal(reference.check_geometry, canvas.analyse(), "prop", image="crate.png")
    assert "touches the frame" in message
    assert "left" in message


def test_a_prop_is_not_held_to_a_character_s_proportions():
    canvas = Canvas().box(120, 300, 900, 500)  # 3.9 wide, no head at all
    notes = reference.check_geometry(canvas.analyse(), "prop", image="beam.png")
    assert all("heads" not in note for note in notes)
    assert all("wide as it is tall" not in note for note in notes)


def test_a_small_subject_is_noted_and_not_refused():
    canvas = Canvas().box(430, 430, 600, 600)
    notes = reference.check_geometry(canvas.analyse(), "prop", image="pebble.png")
    assert any("fills" in note for note in notes)


# ---------------------------------------------------------------- the door --


class _Args:
    """What argparse hands `run`/`run_fake`, minus everything neither reads."""

    def __init__(self, **fields) -> None:
        self.image = None
        self.name = None
        self.kind = None
        self.source = None
        self.sources = None
        self.overwrite = False
        self.print_format = None
        self.created_by = "agent:test"
        self.__dict__.update(fields)


@pytest.fixture
def project(tmp_path: Path) -> Path:
    (tmp_path / "assets-src").mkdir()
    (tmp_path / "assets-src" / "SOURCES.md").write_text(
        "# Sources\n\n## Reference images (`refs/`)\n\n"
        "| File | Origin | For | Date |\n|---|---|---|---|\n"
        "| `characters/vex_runner.png` | xAI grok | a body | 2026-08-18 |\n\n"
        "**Licence posture.** Everything after the table.\n",
        encoding="utf-8",
    )
    records.set_project(tmp_path)
    return tmp_path


def test_the_fake_door_stores_the_original_bytes_and_writes_all_three_files(project: Path):
    drawn = project / "drawn.png"
    figure(heads=6.0).write(drawn)
    original = drawn.read_bytes()
    result = reference.run_fake(
        _Args(image=str(drawn), name="ember_knight", kind="character", source="xAI Grok, image_edit")
    )

    stored = project / "assets-src" / "refs" / "characters" / "ember_knight.png"
    assert stored.read_bytes() == original, "the stored PNG is the file the user drew, byte for byte"

    record = json.loads((project / "assets-src" / "refs" / "characters" / "ember_knight.ref.json").read_text())
    assert record["kind"] == "ref" and record["tool"] == "imported" and record["fake"] is True
    assert record["backend"]["executor"] is None
    assert record["params"]["stated_source"] == "xAI Grok, image_edit"
    assert record["outputs"][0]["sha256"] == records.sha256_file(stored)
    assert record["inputs"][0]["source"] == "xAI Grok, image_edit"
    assert all(record["measured"][key] is None for key in ("heads", "islands", "alpha_fraction"))

    ledger = (project / "assets-src" / "SOURCES.md").read_text()
    assert "| `characters/ember_knight.png` |" in ledger
    assert ledger.index("| `characters/ember_knight.png` |") < ledger.index("**Licence posture.**")
    assert result["ledger_row"] == "added"


def test_a_taken_name_is_refused_and_overwrite_echoes_what_it_replaced(project: Path):
    drawn = project / "drawn.png"
    figure().write(drawn)
    arguments = dict(image=str(drawn), name="knight", kind="character", source="a friend's brief")
    reference.run_fake(_Args(**arguments))
    assert "already exists" in refusal(reference.run_fake, _Args(**arguments))
    again = reference.run_fake(_Args(**arguments, overwrite=True))
    assert again["replaced"]["sha256"] == records.sha256_file(drawn)
    assert again["ledger_row"] == "replaced"

    ledger = (project / "assets-src" / "SOURCES.md").read_text()
    assert ledger.count("| `characters/knight.png` |") == 1


def test_a_re_import_restates_the_origin_and_leaves_the_for_cell_standing(project: Path):
    """The door owns Origin; what somebody wrote about what was made from the
    picture is theirs, and a re-import must not quietly delete it."""
    drawn = project / "drawn.png"
    figure().write(drawn)
    reference.run_fake(_Args(image=str(drawn), name="vex_runner", kind="character", source="first words"))
    ledger = project / "assets-src" / "SOURCES.md"
    row = next(line for line in ledger.read_text().splitlines() if line.startswith("| `characters/vex_runner.png` |"))
    assert "first words" in row and "a body" in row, "the fixture's row kept its For cell"

    reference.run_fake(_Args(image=str(drawn), name="vex_runner", kind="character",
                             source="restated: xAI grok, image_edit", overwrite=True))
    row = next(line for line in ledger.read_text().splitlines() if line.startswith("| `characters/vex_runner.png` |"))
    assert "restated: xAI grok, image_edit" in row
    assert "a body" in row, "the For cell is not the door's to invent"


def test_importing_a_picture_that_is_already_at_its_destination_changes_no_byte(project: Path):
    """How a reference that predates the door gets its record."""
    stored = project / "assets-src" / "refs" / "props" / "crate.png"
    stored.parent.mkdir(parents=True)
    Canvas().box(200, 200, 800, 800).write(stored)
    original = stored.read_bytes()
    reference.run_fake(_Args(image=str(stored), name="crate", kind="prop",
                             source="drawn by hand in 2026", overwrite=True))
    assert stored.read_bytes() == original
    record = json.loads((stored.with_suffix(".ref.json")).read_text())
    assert record["outputs"][0]["sha256"] == records.sha256_file(stored)


def test_a_prop_record_says_null_for_heads_rather_than_a_number_nothing_reads(project: Path):
    drawn = project / "drawn.png"
    Canvas().box(200, 300, 800, 700).write(drawn)
    reference.run_fake(_Args(image=str(drawn), name="crate", kind="prop", source="drawn"))
    record = json.loads((project / "assets-src" / "refs" / "props" / "crate.ref.json").read_text())
    assert record["measured"]["heads"] is None
    assert record["measured"]["arm_line_fraction"] is None


def test_the_door_refuses_a_file_that_is_not_a_png(project: Path):
    fake = project / "drawn.png"
    fake.write_bytes(b"GIF89a not a png at all")
    assert "not a PNG" in refusal(reference.run_fake, _Args(image=str(fake), name="x", kind="prop", source="s"))


def test_the_door_needs_a_source(project: Path):
    drawn = project / "drawn.png"
    figure().write(drawn)
    with pytest.raises(UsageError) as caught:
        reference.run_fake(_Args(image=str(drawn), name="x", kind="character"))
    assert "--source is required" in caught.value.message


# --------------------------------------------------------------- the format --


def test_the_format_text_has_one_home_and_the_copies_are_generated():
    text = reference.format_text()
    assert "1024 px or more on its long side" in text
    assert "seven heads or more" not in reference.FORMAT, "the amendment removed the proportion rule"
    assert "seven heads or more" in reference.FORMAT_AMENDMENT, "and the amendment says what it removed"
    assert "refuses below three heads" in text
    assert "rust" not in reference.RENDERERS, (
        "the Rust copy is generated at build time by crates/forge_mcp/build.rs, "
        "which reads FORMAT and FORMAT_AMENDMENT from this file; a second generator "
        "of the same copy is the second source this constant exists to prevent"
    )
    markdown = reference.format_markdown()
    assert markdown.startswith("<!-- GENERATED by `just ref-format markdown`")
    for line in text.splitlines():
        if line.strip():
            assert f"> {line}" in markdown


def test_print_format_writes_the_text_and_touches_nothing(project: Path, capsys):
    result = reference.run(_Args(print_format="text"))
    assert result == {}, "nothing is claimed; the text is the output"
    printed = capsys.readouterr().out
    assert "A reference is one PNG" in printed
    assert "refuses below three heads" in printed, "the amendment travels with the text"
    assert not (project / "assets-src" / "refs").exists()


# ------------------------------------------- the keyer, where it can be run --


def test_the_keyer_is_mesh_s_own_on_a_real_flat_backdrop(tmp_path: Path):
    pytest.importorskip("numpy")
    pytest.importorskip("cv2")
    pytest.importorskip("PIL")
    drawn = tmp_path / "flat.png"
    pixels = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            inside = 260 < x < 760 and 120 < y < 900
            pixels += b"\x30\x40\x50\xff" if inside else b"\xd6\xd6\xd8\xff"
    png.write_png(drawn, SIZE, SIZE, bytes(pixels))
    measured = reference.keyed_alpha(drawn)
    assert measured["backdrop_rgb"] == [214, 214, 216]
    assert measured["keyer_tolerance"] == 28
    assert measured["islands"] == 1
    assert measured["alpha_fraction"] == pytest.approx(0.37, abs=0.02)


def test_a_picture_under_the_long_side_is_refused_before_the_keyer(tmp_path: Path):
    pytest.importorskip("numpy")
    pytest.importorskip("PIL")
    small = tmp_path / "small.png"
    png.write_png(small, 512, 512, bytes(512 * 512 * 4))
    assert "long side" in refusal(reference.keyed_alpha, small)
