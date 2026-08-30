"""The Phase 0 spike harness: the judgement it makes without a GPU.

The spike script cannot be tested end to end here — it needs a model — but it
rests on a small pure decision that would mislead silently if it were wrong,
and that is what these cover: ``spike_skin._alignment``, which decides whether
the skeleton that came back is the one we handed in. Get it wrong in one
direction and a relabelled copy of our own rig reads as a total failure; wrong
in the other and a skeleton the model invented reads as a success.

``spike_pose``'s projection was the other one. It drew the T-pose image that
conditioned the reference generator, and it left with the image models
(``designs/decisions.md``, "The reference image stays brought", 2026-08-30):
a brought reference is drawn by a person, who needs no pose image.
"""

from __future__ import annotations

from forge_gen import spike_skin


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
