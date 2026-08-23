"""npz.write_take is a take numpy reads back, shaped as ARDY writes one."""

from __future__ import annotations

import zipfile

import pytest

from forge_gen import npz, placeholders

numpy = pytest.importorskip("numpy")


def test_numpy_reads_what_we_write(tmp_path):
    path = npz.write_take(tmp_path / "still.npz", frames=12, fps=20, prompt="stand still")
    with numpy.load(path) as data:
        assert data["local_rot_mats"].shape == (12, 27, 3, 3) and data["local_rot_mats"].dtype == numpy.float32
        assert data["global_rot_mats"].shape == (12, 27, 3, 3)
        assert data["posed_joints"].shape == (12, 27, 3)
        assert data["root_positions"].shape == (12, 3) and data["smooth_root_pos"].shape == (12, 3)
        assert data["foot_contacts"].shape == (12, 4) and data["foot_contacts"].dtype == numpy.bool_
        assert data["global_root_heading"].shape == (12, 2)
        assert data["fps"].dtype == numpy.int64 and int(data["fps"]) == 20
        assert str(data["text"]) == "stand still"
        assert numpy.allclose(data["local_rot_mats"][0, 0], numpy.eye(3))
        assert 0.8 < data["root_positions"][0, 1] < 1.1
        assert data["foot_contacts"].all()
    # Stored, not compressed — what ARDY writes and what forge_motion's reader expects.
    with zipfile.ZipFile(path) as archive:
        assert {info.compress_type for info in archive.infolist()} == {zipfile.ZIP_STORED}
        assert sorted(archive.namelist()) == sorted(
            f"{name}.npy" for name in (
                "local_rot_mats", "global_rot_mats", "posed_joints", "root_positions", "smooth_root_pos",
                "foot_contacts", "global_root_heading", "fps", "text",
            )
        )


def test_our_reader_agrees_with_numpy_on_every_dtype(tmp_path):
    arrays = {
        "f": npz.f32([1.5, -2.0, 3.25, 4.0, 5.0, 6.0], (2, 3)),
        "b": npz.bools([True, False, True], (3,)),
        "i": npz.int_scalar(20),
        "s": npz.str_scalar("héllo"),
        "empty": npz.str_scalar(""),
    }
    path = npz.write_npz(tmp_path / "x.npz", arrays)
    back = npz.read_npz(path)
    assert back["f"].shape == (2, 3) and back["f"].floats() == [1.5, -2.0, 3.25, 4.0, 5.0, 6.0]
    assert back["b"].bools() == [True, False, True]
    assert back["i"].int_scalar() == 20
    assert back["s"].str_scalar() == "héllo" and back["empty"].str_scalar() == ""
    with numpy.load(path) as data:
        assert data["f"].tolist() == [[1.5, -2.0, 3.25], [4.0, 5.0, 6.0]]
        assert data["b"].tolist() == [True, False, True]
        assert str(data["s"]) == "héllo"
        assert data["s"].dtype == numpy.dtype("<U5")


def test_numpy_headers_are_byte_identical(tmp_path):
    """The header numpy writes for the same array is ours: same padding, same dict text."""
    import io

    array = npz.f32([0.0] * 27 * 9, (1, 27, 3, 3))
    ours = npz.npy_bytes(array)
    buffer = io.BytesIO()
    numpy.save(buffer, numpy.zeros((1, 27, 3, 3), dtype=numpy.float32))
    assert ours == buffer.getvalue()
    buffer = io.BytesIO()
    numpy.save(buffer, numpy.array(20, dtype=numpy.int64))
    assert npz.npy_bytes(npz.int_scalar(20)) == buffer.getvalue()


def test_contacts_can_be_omitted_and_frames_must_be_positive(tmp_path):
    path = npz.write_take(tmp_path / "old.npz", frames=2, contacts=False)
    assert "foot_contacts" not in npz.read_npz(path)
    with pytest.raises(ValueError):
        npz.write_take(tmp_path / "none.npz", frames=0)


def test_placeholder_take_helper(tmp_path):
    path = placeholders.placeholder_take(tmp_path / "p.npz", frames=5, prompt="x")
    assert npz.read_npz(path)["local_rot_mats"].shape == (5, 27, 3, 3)
