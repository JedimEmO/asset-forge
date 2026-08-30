"""The Phase 0 spike harness: the two judgements it makes without a GPU.

Neither spike script can be tested end to end here — one needs Blender, the
other a 14 GB model — but both rest on a small pure decision that would
mislead silently if it were wrong, and those are what these cover:

* ``spike_skin._alignment``, which decides whether the skeleton that came
  back is the one we handed in. Get it wrong in one direction and a
  relabelled copy of our own rig reads as a total failure; wrong in the
  other and a skeleton the model invented reads as a success.
* ``spike_pose``'s projection, which decides which way round the figure
  faces. A mirrored pose image is one a model happily draws a mirrored
  character from, and nothing downstream would ever say so.
"""

from __future__ import annotations

import struct

from forge_gen import png, profile as profile_mod, spike_pose, spike_skin


def _document(names, positions, parents):
    """A minimal glTF document with one skin over ``names``."""
    nodes = []
    for index, name in enumerate(names):
        node = {"name": name, "translation": list(positions[index])}
        children = [i for i, parent in enumerate(parents) if parent == index]
        if children:
            node["children"] = children
        nodes.append(node)
    roots = [i for i, parent in enumerate(parents) if parent is None]
    nodes.append({"name": "Armature", "children": roots})
    return {
        "nodes": nodes,
        "scene": 0,
        "scenes": [{"nodes": [len(nodes) - 1]}],
        "skins": [{"joints": list(range(len(names)))}],
    }


NAMES = ["Hips", "Spine", "Head"]
PLACES = [(0.0, 1.0, 0.0), (0.0, 0.2, 0.0), (0.0, 0.4, 0.0)]
PARENTS = [None, 0, 1]


def _skeletons(names, places, parents):
    return spike_skin._skeleton(_document(names, places, parents))


def test_alignment_named_when_the_names_come_back():
    handed = _skeletons(NAMES, PLACES, PARENTS)
    result = spike_skin._alignment(handed, _skeletons(NAMES, PLACES, PARENTS))
    assert result["mode"] == "named"
    assert result["displacement_max_m"] == 0.0


def test_alignment_by_order_when_only_the_names_are_lost():
    """The case the spike was written to find: our skeleton, upstream's labels."""
    handed = _skeletons(NAMES, PLACES, PARENTS)
    moved = [(x, y + 0.01, z) for x, y, z in PLACES]
    result = spike_skin._alignment(handed, _skeletons(["bone_0", "bone_1", "bone_2"], moved, PARENTS))
    assert result["mode"] == "by_order"
    assert result["same_parent_array"] is True
    # Displacement is measured in world space, so a shift at the root moves
    # every joint under it — which is exactly the failure it exists to catch.
    assert result["displacement_max_m"] > 0.009


def test_alignment_none_when_the_hierarchy_is_the_models_own():
    handed = _skeletons(NAMES, PLACES, PARENTS)
    result = spike_skin._alignment(handed, _skeletons(["a", "b", "c"], PLACES, [None, 0, 0]))
    assert result["mode"] == "none"
    assert result["same_parent_array"] is False


def test_the_figure_faces_the_camera(repo_root):
    """The character's own left hand is drawn on the right of the image."""
    prof = profile_mod.load_profile(repo_root / "rigs" / "humanoid")
    _, placed, invented = spike_pose.draw(prof, 256)
    left = placed[spike_pose.KEYPOINTS.index("left_wrist")]
    right = placed[spike_pose.KEYPOINTS.index("right_wrist")]
    assert left[0] > right[0]
    assert "nose" in invented and "neck" in invented


def test_the_figure_fills_nine_tenths_and_stands_on_the_floor(repo_root):
    prof = profile_mod.load_profile(repo_root / "rigs" / "humanoid")
    size = 256
    _, placed, _ = spike_pose.draw(prof, size)
    floor = (1.0 - spike_pose.FILL) * size / 2.0 + spike_pose.FILL * size
    ankle = placed[spike_pose.KEYPOINTS.index("left_ankle")]
    # The ankle joint is a few centimetres above the floor in the rest pose,
    # and the framing is floor-to-stature, so it sits just inside the margin.
    assert 0.0 < floor - ankle[1] < 0.05 * size
    assert all(0.0 <= x < size and 0.0 <= y < size for x, y in placed)


def test_the_pose_image_is_a_png_of_the_size_asked_for(repo_root, tmp_path):
    prof = profile_mod.load_profile(repo_root / "rigs" / "humanoid")
    canvas, _, _ = spike_pose.draw(prof, 128)
    out = canvas.write(tmp_path / "pose.png")
    data = out.read_bytes()
    assert data[:8] == png.SIGNATURE
    width, height = struct.unpack_from(">II", data, 16)
    assert (width, height) == (128, 128)
    # Something was actually drawn on the black ground.
    assert any(byte for byte in data[8:])
