"""Fit a skeleton's bone LENGTHS to one body, from the skin weights it already carries.

    python3 python/forge_gen/fit.py out/skin/<name>.skinned.glb
            [--names GLB | --map JSON] [--profile DIR]
            [--out out/skin/<stem>.fit.json] [--compare EARLIER.fit.json]
            [--min-support N] [--ratio-min 0.4] [--ratio-max 2.5]
            [--asymmetry-arms F] [--asymmetry-other F]
            [--no-symmetry] [--no-ground] [--json]

Stdlib plus numpy, nothing else: it runs under this repository's system
``python3`` (numpy 2.4 here), and under the ``skintokens`` or ``ardy``
backend interpreters just as well. It opens no Blender and spends no card.

**A library, not a subcommand.** ``forge gen skin`` imports it and runs it
once, between the two skinning passes; the ``__main__`` half below is how a
report is re-read or re-made by hand, and there is no ``forge gen fit``. The
decision it implements is the one ``decisions.md`` records on 2026-08-30:
freeze what every clip in the library binds to — bone **names**,
**hierarchy** and **rest rotations** — and let bone **lengths** belong to the
body. It reads a mesh SkinTokens has already skinned and answers one question
per bone: *how long is this bone on this body?* It writes numbers and refuses
when the numbers are not trustworthy; turning them into an armature is
``python/forge_gen/blender/fit_rig.py``.

# What is frozen, and what one number per run may move

For a bone ``b`` whose contract parent is ``p``, the contract fixes the
**world direction** of the segment between their rest joints. That direction
is a restatement of the rest rotations, so keeping it exactly is keeping the
rest pose the clips were baked against: a fitted local translation is the
contract's local translation times a positive scalar, and a positive scalar
does not rotate a vector. Only scalars are measured here.

# Landmarks, and why not every bone gets its own number

A joint is measurable from weights only where the body actually articulates.
Between the shoulder and the collarbone, or between ``Spine`` and ``Spine1``,
SkinTokens draws no boundary worth reading — measured per bone, the shipped
body ``vex_runner`` (which passes today's fit gate) came back with a
collarbone at 0.85 and a shoulder at 0.58 of their reference lengths, two
numbers whose product is right and whose split is invented. So this file
measures **landmarks** and interpolates between them:

* a **landmark** is a joint the weights can see: the hip, knee, ankle and toe
  of each leg; the shoulder, elbow and wrist of each arm; the neck;
* a **run** is the chain of bones from an already-placed bone to the next
  landmark below it — ``Hips → Spine → Spine1 → Spine2 → Spine3 → Neck`` is
  one run, ``Spine3 → LeftShoulder → LeftArm`` is another;
* every bone in a run takes the **same ratio**, the one that lands the run's
  end joint on the landmark: ``r = (C − J[start]) · V / (V · V)`` for
  ``V = J_ref[end] − J_ref[start]``, a least-squares projection onto the run's
  own frozen direction.

Leaves outside every run — the fingers, ``HandEnd`` — inherit the ratio of
the nearest run above them, and say so. ``Head`` is deliberately **not** a
landmark: the contract puts its joint inside the skull while the weights put
their boundary at the base of it, so the boundary is not an estimate of that
joint, and the measured ratio ranged over 0.27–2.26 on three bodies while the
neck run it now inherits stayed inside 0.7–1.1.

**The landmark set is body-plan knowledge and lives in ``profile.toml``**,
as ``[fit] landmarks`` beside the T-pose gate: which joints a skinner can
see is a fact about the body plan, not about this file, and a profile for
something that is not a humanoid would name its own.

# The estimator: the weight-product centroid of the transition band

SkinTokens returns smooth weights — on the witch a quarter of all vertices
have no weight above 0.9 — and a smooth skin draws a **transition band**
around each joint: the ring of surface where the parent hands the vertex over
to the child. With ``a(v)`` the parent's own weight and ``b(v)`` the summed
weight of the child and all its descendants, the estimate is::

    C = sum(a * b * v) / sum(a * b)

``a·b`` peaks where the hand-over is even and falls to zero at both ends, so
the ring is what is averaged and neither cloud's bulk can drag the answer
down its own length. Three notes on why it is this and not the obvious
alternatives:

* **A hard band threshold is too thin to trust.** Counting only vertices with
  both weights over 0.15 leaves 7 vertices at ``vex_runner``'s left knee; the
  product uses every vertex the two bones share, and its left/right
  disagreement across three bodies fell from 24 % to 1–6 % on the legs.
* **A 1-D weight crossing along the bone's own axis is wrong wherever a limb
  leaves its parent sideways.** The shoulder axis is three-quarters vertical
  and the whole arm lies to one side of it, so the arm's lateral extent adds
  to its projection and the crossing puts the shoulder *higher* than it is —
  the exact error this estimator exists to remove. The crossing is still computed
  as ``split_length_m`` and reported as a cross-check, never used to place a
  joint.
* **The support is an effective sample size**, ``(Σab)² / Σ(ab)²``, not a
  vertex count: it is the number of vertices the answer actually rests on,
  and it is what ``--min-support`` gates.

# Where the chain starts, and where it ends

**The root comes from geometry, not from the weights.** ``Hips`` has no
parent and so no run, and the weight-product estimator that places a
landmark well places a *centre* badly: on ``vex_runner``, a body that
already ships, it put the hip line 5.8 cm high and asked for a
``motion_scale`` of 1.0605. The weights know where a limb ends; they do not
know where a body's middle is (``decisions.md``, 2026-08-30). So the root's
height is read off the body itself — its own floor and its own height,
which ``prepare`` has already established — as

    root_y = lowest_y + reference_root_y * (highest_y - lowest_y) / reference_stature

and its **x and z stay the contract's**, because ``prepare``'s
``_normalize`` has already centred the mesh on the skeleton's mirror plane
and on the root's depth and there is nothing left there to measure.

Two cross-checks travel beside it and neither places anything: the **weight
band's** own hip line (``hip_line_weights_m``), and the **crotch**
(``crotch_y_m``, from ``fitgeom``). The crotch is reported rather than used
because it was measured on five bodies and does not survive a garment: it
reads 0.90 m on ``vex_runner`` against a contract hip line of 0.927, 0.73 m
on ``courier_v2``, 0.47 m on the witch and 0.49 m on the warlock, whose
robes close the gap between the legs entirely, and nothing on
``courier_qwen``, which has no gap to find. A number that ranges over 43 cm
across four bodies of the same stature is a picture of their clothes.

**The shoulder line comes from the weights, and the geometry is a
cross-check.** The design this file implements said the root *and the
shoulder line* would be anchored to geometry. Only the root is, and the
record says ``"shoulder_line": "weights"`` because that is what happened.
The reason is in ``fitgeom`` itself: ``shoulder_y`` is the median height of
every vertex further out than 0.55 of the body's own half-span — the arm
tube, chosen deliberately "narrow enough to leave the shoulders out of it".
It is a statement about a **pose** (are the arms level?) and not a
measurement of the joint where an arm leaves the torso. On ``ember_knight``
that band starts at 50.5 cm from the mirror plane, past the elbow the fit
puts at 42.2 cm, so the number is the outer arm's height and it reads
1.3871 m against a shoulder joint the weights place at 1.5327 m.

And there is no way to spend it that keeps the one invariant this file
exists to keep. A run's whole freedom is **one positive scalar** along the
contract's frozen direction, because a scalar cannot rotate a vector and
the rest rotations are what every baked clip binds to. ``Spine3 → Arm``
runs diagonally (0.191 out, 0.173 up on the humanoid), so anchoring its
*height* to the arm tube means either rotating the segment — forbidden — or
solving the scalar from the height alone, which on ``ember_knight`` would
put the shoulder joint 8.7 cm from the mirror plane on a body whose
fingertips are 91.7 cm out: an arm leaving the torso from inside the chest,
to match a number that was never a measurement of that joint. The root can
be anchored because the root has no parent segment and therefore no
direction to break; the shoulder cannot, and saying so is cheaper than a
record that claims it was.

So ``shoulder_line`` in the report carries both numbers — where the weights
put the joint, what the arm tube measures, and the gap — and places
nothing. Measured: 14.6 cm apart on ``ember_knight`` (plate armour, a
pauldron the skinner reads as shoulder), 6.5 cm on ``vex_runner``
re-skinned, 1.7 cm on the witch of the original spike. Three bodies is not
a threshold, so the gap is printed and never refused (2026-08-31).

**Feet on the ground.** The measured leg joints are pinned to the mesh, and
on a body in a long robe the bands ride up: the witch's ankle measures 15 cm
above her own soles. A toe joint off the floor makes every contact frame in
every clip sink or hover, so after the runs are read, the three segments
below each hip are scaled by the one factor ``k`` per side that puts
``ToeBase`` at the contract's own toe height. The weights set the leg's
**proportions**; the ground sets its **length**.

**Symmetry.** A mirrored body gets a mirrored skeleton: each left/right pair
of runs takes the mean of the two independently measured ratios (turn it off
with ``--no-symmetry``). Averaging halves the estimator's noise and keeps a
walk from limping. The **raw** disagreement is what the gate reads, so
symmetrising hides nothing.

# The gate

Exit 4 when

* a measured run's ratio is outside ``[--ratio-min, --ratio-max]`` (0.4–2.5);
* a left/right pair of runs disagrees by more than the profile's own
  tolerance — bodies are mirrored, the estimator is not, so this measures the
  estimator. **Two bands, because the arms are the hard case**: an arm
  boundary inside a sleeve is a guess the two sides make differently, and the
  worst measured on a body that walks is 28.7 % (the warlock's forearm)
  against 16.2 % anywhere else (the hip, on two bodies). So
  ``[fit] asymmetry_arms = 0.35`` on ``Arm->ForeArm`` and ``ForeArm->Hand``
  and ``[fit] asymmetry_other = 0.20`` elsewhere. A single 10 % rule refuses
  ``vex_runner``, which ships;
* a measured run's support is below ``--min-support`` (8 effective vertices).

Warnings, printed and never fatal, cover a grounding factor outside
[0.7, 1.4] and a landmark centroid sitting far off its run's frozen axis —
which is the design's real cost showing itself: a body whose shoulder does
not lie in the direction the contract points cannot be fitted by length
alone, and the warning is where that shows up as a number.

# Reading a second pass

``--compare`` takes an earlier fit report and prints how far every joint
moved between the two, through :func:`convergence`. That was the design's
convergence claim — skin, fit, re-skin, fit again, and the second fit should
move nothing — and the spike measured it false: the second pass walks the
torso downhill 73.5 mm a time, because the weights are always made against
the skeleton that was handed in. So **the door fits once**, there is no
``--passes``, and :func:`convergence` survives as a library function with no
caller, exercised by ``test_fit.py`` against the two frozen reports. That is
how the evidence is kept without adding a knob whose only correct value is
off.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import numpy as np

from forge_gen import profile as profile_mod  # noqa: E402
from forge_gen.exit_codes import ForgeGenError, InputRejected, UsageError  # noqa: E402

TAG = "fit"

#: The chain below each hip that the grounding pass scales, per side.
GROUNDED_CHAIN = ("Leg", "Foot", "ToeBase")

#: The runs the wider symmetry band applies to, by the landmark they end on.
#: An arm boundary inside a sleeve is the one the two sides disagree about;
#: see the module doc for the two numbers and the bodies they came from.
ARM_LANDMARKS = ("LeftForeArm", "RightForeArm", "LeftHand", "RightHand")

#: A vertex enters the 1-D cross-check when either side owns this much of it.
SPLIT_FLOOR = 0.3


# ------------------------------------------------------------------- glTF --


def _chunks(path: Path) -> tuple[dict, bytes]:
    """The JSON document and the BIN chunk of a binary glTF."""
    data = path.read_bytes()
    if data[:4] != b"glTF":
        raise InputRejected(f"{path.name} is not a binary glTF")
    offset, document, binary = 12, None, b""
    while offset + 8 <= len(data):
        length, kind = struct.unpack_from("<II", data, offset)
        offset += 8
        if kind == 0x4E4F534A:
            document = json.loads(data[offset : offset + length])
        elif kind == 0x004E4942:
            binary = data[offset : offset + length]
        offset += length
    if document is None:
        raise InputRejected(f"{path.name} has no JSON chunk")
    return document, binary


_COMPONENT = {5120: "i1", 5121: "u1", 5122: "i2", 5123: "u2", 5125: "u4", 5126: "f4"}
_COUNT = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
#: glTF's normalised integer weight encodings, and what to divide them by.
_NORMALISE = {5121: 255.0, 5123: 65535.0}


def _accessor(document: dict, binary: bytes, index: int) -> np.ndarray:
    """One accessor as a (count, components) array; byteStride honoured."""
    accessor = document["accessors"][index]
    if "bufferView" not in accessor:
        raise InputRejected("a sparse or zero-filled accessor is not something this reader handles, and no exporter here writes one")
    view = document["bufferViews"][accessor["bufferView"]]
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    columns = _COUNT[accessor["type"]]
    dtype = np.dtype("<" + _COMPONENT[accessor["componentType"]])
    stride = view.get("byteStride") or columns * dtype.itemsize
    raw = np.frombuffer(binary, dtype=np.uint8, count=accessor["count"] * stride, offset=start)
    out = raw.reshape(accessor["count"], stride)[:, : columns * dtype.itemsize].copy().view(dtype).reshape(accessor["count"], columns)
    if accessor.get("normalized") and accessor["componentType"] in _NORMALISE:
        return out.astype(np.float64) / _NORMALISE[accessor["componentType"]]
    return out


def _skin_joint_names(document: dict) -> list[str]:
    skins = document.get("skins") or []
    if not skins:
        raise InputRejected("the file has no skin — there is no joint order to read")
    nodes = document.get("nodes", [])
    return [nodes[index].get("name", f"<node {index}>") for index in skins[0]["joints"]]


# ------------------------------------------------------------ the contract --


def _rest_world(prof: profile_mod.Profile) -> tuple[list[str], list[int | None], np.ndarray]:
    """Bone names, contract parents, and each bone's world rest joint (glTF axes)."""
    bones = prof.bones
    names = [bone["name"] for bone in bones]
    parents = [bone["parent"] for bone in bones]
    world = prof.rest_world()
    joints = np.array([world[name][0] for name in names], dtype=np.float64)
    return names, parents, joints


def _subtrees(parents: list[int | None]) -> list[list[int]]:
    """For every bone, itself and every descendant."""
    children: list[list[int]] = [[] for _ in parents]
    for index, parent in enumerate(parents):
        if parent is not None:
            children[parent].append(index)
    out: list[list[int] | None] = [None] * len(parents)

    def walk(index: int) -> list[int]:
        cached = out[index]
        if cached is not None:
            return cached
        gathered = [index]
        for child in children[index]:
            gathered += walk(child)
        out[index] = gathered
        return gathered

    for index in range(len(parents)):
        walk(index)
    return [gathered for gathered in out if gathered is not None]


# ------------------------------------------------------------- estimators --


def _centroid(points: np.ndarray, own: np.ndarray, child: np.ndarray) -> tuple[np.ndarray | None, float, float]:
    """The weight-product centroid of a joint's transition band, its support and its squareness.

    Support is the effective sample size ``(Σw)² / Σw²`` of the product
    weights — the number of vertices the centroid actually rests on, which a
    raw count over a threshold is not. Squareness is the product-weighted
    mean of ``2·min(a, b)/(a + b)``: 1.0 is a band every vertex of which is
    split evenly between the two bones, and near 0 is a scatter that belongs
    to neither.
    """
    weight = own * child
    total = float(weight.sum())
    if total <= 1e-9:
        return None, 0.0, 0.0
    support = total * total / float((weight * weight).sum())
    centroid = (points * weight[:, None]).sum(axis=0) / total
    both = own + child
    both = np.where(both > 0.0, both, 1.0)
    squareness = float((weight * (2.0 * np.minimum(own, child) / both)).sum() / total)
    return centroid, support, squareness


def _split_length(points: np.ndarray, origin: np.ndarray, axis: np.ndarray, own: np.ndarray, child: np.ndarray) -> float | None:
    """The 1-D cross-check: the cut along the axis that misclassifies the least weight.

    For a threshold ``t`` the cost is the parent weight sitting beyond it plus
    the child weight sitting before it; the minimum is the best separation the
    axis admits. Reported beside the fit, never used to place a joint — see
    the module doc for why a projection along the axis is not the estimator.
    """
    mask = (own >= SPLIT_FLOOR) | (child >= SPLIT_FLOOR)
    if int(mask.sum()) < 8:
        return None
    projected = (points[mask] - origin) @ axis
    order = np.argsort(projected)
    projected = projected[order]
    parent_w = own[mask][order]
    child_w = child[mask][order]
    cost = (parent_w.sum() - np.concatenate(([0.0], np.cumsum(parent_w)))) + np.concatenate(([0.0], np.cumsum(child_w)))
    best = int(np.argmin(cost))
    if best <= 0:
        return float(projected[0])
    if best >= len(projected):
        return float(projected[-1])
    return float(0.5 * (projected[best - 1] + projected[best]))


# ------------------------------------------------------------------- fit --


def landmarks_of(prof: profile_mod.Profile) -> tuple[str, ...]:
    """``[fit] landmarks`` — the joints this body plan's weights can see."""
    try:
        named = prof.section("fit")["landmarks"]
    except (KeyError, profile_mod.ProfileError) as err:
        raise UsageError(
            f"{prof.dir}/profile.toml has no [fit] landmarks — the fit measures runs between named joints "
            "and cannot guess which of a body plan's joints a skinner can see"
        ) from err
    return tuple(str(name) for name in named)


def fit(points: np.ndarray, dense: np.ndarray, prof: profile_mod.Profile, *, min_support: float, symmetry: bool, ground: bool) -> dict:
    """Every bone's fitted length, in contract order. The gate is separate."""
    from forge_gen import fitgeom

    names, parents, reference = _rest_world(prof)
    subtree = _subtrees(parents)
    index_of = {name: i for i, name in enumerate(names)}
    root = index_of[prof.root]
    fitted = np.array(reference, dtype=np.float64)
    landmark_names = landmarks_of(prof)
    stature = float(prof.section("bones")["reference_stature_m"])

    # --- the root: height from the body's own floor and height, x and z from
    # the contract. The weight band is measured too, and reported, and never
    # used to place anything: on vex_runner it sits 5.8 cm high. See the
    # module doc.
    geometry = fitgeom.measure(points)
    legs = [i for i, parent in enumerate(parents) if parent == root and reference[i][1] < reference[root][1]]
    leg_mass = np.zeros(len(points))
    for leg in legs:
        leg_mass += dense[:, subtree[leg]].sum(axis=1)
    centroid, support, squareness = _centroid(points, dense[:, root], leg_mass)
    hip_line = float(np.mean([reference[leg][1] for leg in legs])) if legs else float(reference[root][1])
    height = float(geometry["highest_y"] - geometry["lowest_y"])
    root_measured = height > 1e-6 and stature > 1e-6
    if root_measured:
        fitted[root] = np.array(
            [reference[root][0], geometry["lowest_y"] + reference[root][1] * height / stature, reference[root][2]]
        )
    root_row = {
        "bone": prof.root,
        "kind": "root",
        "source_of_height": "geometry" if root_measured else "contract",
        "support": round(support, 1),
        "squareness": round(squareness, 3),
        "measured": bool(root_measured),
        "measured_height_m": round(height, 4),
        "floor_y_m": geometry["lowest_y"],
        "crotch_y_m": geometry["crotch_y"],
        "hip_line_weights_m": round(float(centroid[1]), 4) if centroid is not None else None,
        "hip_line_contract_m": round(hip_line, 4),
        "fitted_y_m": round(float(fitted[root][1]), 4),
        "reference_y_m": round(float(reference[root][1]), 4),
        "note": "height from this body's own floor and stature; x and z are the contract's; the weight band and the crotch are cross-checks",
    }

    # --- landmarks ---
    landmarks: dict[int, dict] = {}
    for name in landmark_names:
        index = index_of.get(name)
        if index is None or parents[index] is None:
            continue
        own = dense[:, parents[index]]
        child = dense[:, subtree[index]].sum(axis=1)
        centroid, support, squareness = _centroid(points, own, child)
        landmarks[index] = {
            "centroid": centroid,
            "support": support,
            "squareness": squareness,
            "own": own,
            "child": child,
            "usable": centroid is not None and support >= min_support,
        }

    # --- runs: from an already-placed bone to each landmark ---
    placed = {root}
    runs: list[dict] = []
    pending = [index for index, data in landmarks.items() if data["usable"]]
    for _ in range(len(pending) + 2):
        progressed = False
        for end in list(pending):
            chain: list[int] = []
            walk: int | None = end
            while walk is not None and walk not in placed:
                chain.append(walk)
                walk = parents[walk]
            if walk is None:
                continue
            start = walk
            vector = reference[end] - reference[start]
            span = float(vector @ vector)
            if span <= 1e-12:
                pending.remove(end)
                continue
            data = landmarks[end]
            ratio = float((data["centroid"] - fitted[start]) @ vector / span)
            off_axis = float(np.linalg.norm((data["centroid"] - fitted[start]) - ratio * vector))
            axis = vector / math.sqrt(span)
            runs.append(
                {
                    "run": f"{names[start]}->{names[end]}",
                    "start": names[start],
                    "end": names[end],
                    "bones": [names[i] for i in reversed(chain)],
                    "reference_length_m": round(math.sqrt(span), 5),
                    "ratio_measured": round(ratio, 4),
                    "ratio": round(ratio, 4),
                    "support": round(float(data["support"]), 1),
                    "squareness": round(float(data["squareness"]), 3),
                    "off_axis_m": round(off_axis, 4),
                    "split_length_m": (lambda s: None if s is None else round(s, 5))(
                        _split_length(points, fitted[start], axis, data["own"], data["child"])
                    ),
                    "landmark_centroid": [round(float(v), 5) for v in data["centroid"]],
                    "_chain": list(reversed(chain)),
                    "_start": start,
                }
            )
            for index in reversed(chain):
                placed.add(index)
            _place(fitted, reference, parents, [i for i in reversed(chain)], ratio)
            pending.remove(end)
            progressed = True
        if not progressed:
            break

    if symmetry:
        _symmetrise(runs)
    for run in runs:
        _place(fitted, reference, parents, run["_chain"], run["ratio"])

    ratio_of: dict[int, float] = {}
    run_of: dict[int, str] = {}
    for run in runs:
        for index in run["_chain"]:
            ratio_of[index] = run["ratio"]
            run_of[index] = run["run"]

    grounding = _ground(runs, ratio_of, run_of, fitted, reference, parents, index_of) if ground else {}

    # --- everything not in a run: inherit the nearest run above ---
    rows: list[dict] = []
    for index in sorted(range(len(names)), key=lambda i: _depth(parents, i)):
        parent = parents[index]
        if parent is None:
            rows.append(dict(root_row, reference_length_m=0.0, fitted_length_m=0.0, ratio=1.0, source="root", run=None))
            continue
        segment = reference[index] - reference[parent]
        length_ref = float(np.linalg.norm(segment))
        if index in ratio_of:
            ratio = ratio_of[index]
            source = "run"
            run_name = run_of[index]
        else:
            ratio, run_name = _inherit(ratio_of, run_of, parents, index)
            source = "inherited"
            if length_ref > 1e-9:
                fitted[index] = fitted[parent] + ratio * segment
            else:
                fitted[index] = fitted[parent]
        rows.append(
            {
                "bone": names[index],
                "parent": names[parent],
                "kind": "bone",
                "reference_length_m": round(length_ref, 5),
                "fitted_length_m": round(length_ref * ratio, 5),
                "ratio": round(ratio, 4),
                "source": source,
                "run": run_name,
            }
        )

    order = {name: i for i, name in enumerate(names)}
    rows.sort(key=lambda row: order[row["bone"]])
    for run in runs:
        run.pop("_chain", None)
        run.pop("_start", None)

    motion_scale = float(fitted[root][1] / reference[root][1]) if reference[root][1] > 1e-9 else 1.0
    return {
        "profile": prof.name,
        "root": prof.root,
        "landmarks": list(landmark_names),
        # What actually placed each thing, not what a design proposed. The
        # shoulder line says `weights` because the weights are what moved it;
        # `shoulder_line` below is the geometry cross-check beside it, which
        # places nothing. See the module doc.
        "sources": {"limbs": "weights", "root": "geometry", "shoulder_line": "weights", "ground": "geometry"},
        "shoulder_line": _shoulder_line(runs, fitted, index_of, geometry),
        "geometry": geometry,
        "symmetrised": bool(symmetry),
        "grounded": bool(ground),
        "runs": runs,
        "bones": rows,
        "fitted_joints": {name: [round(float(v), 5) for v in fitted[i]] for i, name in enumerate(names)},
        "reference_joints": {name: [round(float(v), 5) for v in reference[i]] for i, name in enumerate(names)},
        "grounding": grounding,
        "motion_scale": round(motion_scale, 4),
        "min_support": min_support,
        "vertices": int(len(points)),
    }


def _shoulder_line(runs: list[dict], fitted: np.ndarray, index_of: dict[str, int], geometry: dict) -> dict:
    """Where the arms left the body, by the weights and by the geometry, side by side.

    A **cross-check that places nothing**, the way the root's crotch and
    weight-band lines are. The weights placed the shoulder joint; this says
    how far that sits from the arm tube's own median height, so a reader of
    the record can see the gap without opening a mesh. There is no threshold
    on it: two bodies is not a calibration, and a gate nobody can calibrate
    ships as a number and not a refusal.

    The shoulder joints are the **starts** of the arm runs — the runs ending
    on an arm landmark whose own start is not one — so a profile that names
    other arm landmarks gets its own answer and nothing here knows what a
    humanoid is beyond :data:`ARM_LANDMARKS`.
    """
    joints = sorted(
        {
            run["start"]
            for run in runs
            if run["end"] in ARM_LANDMARKS and run["start"] not in ARM_LANDMARKS and run["start"] in index_of
        }
    )
    weights_y = float(np.mean([fitted[index_of[name]][1] for name in joints])) if joints else None
    arm_tube = geometry.get("shoulder_y")
    return {
        "joints": joints,
        "weights_m": None if weights_y is None else round(weights_y, 4),
        "arm_tube_geometry_m": arm_tube,
        "gap_m": None if weights_y is None or arm_tube is None else round(weights_y - float(arm_tube), 4),
        "note": "the weights placed these joints; the arm tube's median height is a cross-check and places nothing",
    }


def _place(fitted: np.ndarray, reference: np.ndarray, parents: list[int | None], chain: list[int], ratio: float) -> None:
    for index in chain:
        parent = parents[index]
        assert parent is not None
        fitted[index] = fitted[parent] + ratio * (reference[index] - reference[parent])


def _depth(parents: list[int | None], index: int) -> int:
    depth = 0
    walk = parents[index]
    while walk is not None:
        walk = parents[walk]
        depth += 1
    return depth


def _symmetrise(runs: list[dict]) -> None:
    """Give each left/right pair of runs the mean of their two measured ratios."""
    by_end = {run["end"]: run for run in runs}
    for run in runs:
        end = run["end"]
        if not end.startswith("Left"):
            continue
        mirror = by_end.get("Right" + end[len("Left") :])
        if mirror is None:
            continue
        mean = 0.5 * (run["ratio_measured"] + mirror["ratio_measured"])
        run["ratio"] = round(mean, 4)
        mirror["ratio"] = round(mean, 4)


def _inherit(ratio_of: dict[int, float], run_of: dict[int, str], parents: list[int | None], index: int) -> tuple[float, str | None]:
    walk = parents[index]
    while walk is not None:
        if walk in ratio_of:
            return ratio_of[walk], run_of.get(walk)
        walk = parents[walk]
    return 1.0, None


def _ground(
    runs: list[dict],
    ratio_of: dict[int, float],
    run_of: dict[int, str],
    fitted: np.ndarray,
    reference: np.ndarray,
    parents: list[int | None],
    index_of: dict[str, int],
) -> dict:
    """Scale each leg's sub-hip runs so its toe joint lands on the contract's toe height.

    Every run below the hip takes the same factor, so the knee and the ankle
    keep the share of the leg the weights gave them; only the leg's total
    length changes. See the module doc.
    """
    out: dict = {}
    for side in ("Left", "Right"):
        hip_name, toe_name = f"{side}UpLeg", f"{side}ToeBase"
        if hip_name not in index_of or toe_name not in index_of:
            continue
        hip, toe = index_of[hip_name], index_of[toe_name]
        # Exactly the runs that END on a joint below the hip. Matching by name
        # suffix instead would sweep in Hips->LeftUpLeg, whose landmark is the
        # anchor the whole correction is measured from.
        below = {f"{side}{part}" for part in GROUNDED_CHAIN}
        chain = [run for run in runs if run["end"] in below]
        if len(chain) != len(below):
            continue
        want = float(reference[toe][1])
        drop = float(fitted[hip][1] - fitted[toe][1])
        if drop <= 1e-6:
            continue
        factor = (float(fitted[hip][1]) - want) / drop
        for run in chain:
            run["ratio"] = round(run["ratio"] * factor, 4)
            run["grounding_factor"] = round(factor, 4)
            for bone in run["bones"]:
                ratio_of[index_of[bone]] = run["ratio"]
        for name in [f"{side}{part}" for part in GROUNDED_CHAIN]:
            index = index_of.get(name)
            if index is not None:
                _place(fitted, reference, parents, [index], ratio_of[index])
        out[side.lower()] = {
            "factor": round(float(factor), 4),
            "toe_before_m": round(float(fitted[hip][1] - drop), 4),
            "toe_after_m": round(float(fitted[toe][1]), 4),
            "hip_joint_m": round(float(fitted[hip][1]), 4),
            "runs": [run["run"] for run in chain],
        }
    return out


# -------------------------------------------------------------- the gate --


def symmetry_bands(prof: profile_mod.Profile, *, arms: float | None = None, other: float | None = None) -> dict:
    """``{asymmetry_arms, asymmetry_other}`` from the profile, or from an override.

    One reader of the two numbers, so the gate, the record's ``fit`` block and
    every message that quotes them cannot drift apart.
    """
    section = prof.section("fit")
    return {
        "asymmetry_arms": float(section["asymmetry_arms"]) if arms is None else float(arms),
        "asymmetry_other": float(section["asymmetry_other"]) if other is None else float(other),
    }


def asymmetry_band(end: str, *, asymmetry_arms: float, asymmetry_other: float) -> float:
    """How far a left/right pair ending on ``end`` may disagree.

    Two bands, not one: see the module doc. The arm runs are the ones whose
    boundary lives inside a sleeve, and a single tolerance tight enough to
    mean anything elsewhere refuses the body that ships. Keyed the way
    :func:`symmetry_bands` returns them, so the two are used together.
    """
    return asymmetry_arms if end in ARM_LANDMARKS else asymmetry_other


def gate(
    report: dict,
    *,
    ratio_min: float,
    ratio_max: float,
    asymmetry_arms: float,
    asymmetry_other: float,
    min_support: float,
) -> tuple[list[str], list[str]]:
    """The refusals this design ships with, and the warnings it prints."""
    problems: list[str] = []
    warnings: list[str] = []
    runs = {run["end"]: run for run in report["runs"]}

    for run in report["runs"]:
        if not ratio_min <= run["ratio"] <= ratio_max:
            problems.append(
                f"{run['run']} fitted to {run['ratio']:.2f}x its reference length "
                f"({run['ratio'] * run['reference_length_m']:.3f} m against {run['reference_length_m']:.3f} m) — "
                f"outside [{ratio_min:.2f}, {ratio_max:.2f}], so the landmark is not where this joint is"
            )
        if run["support"] < min_support:
            problems.append(
                f"{run['run']} rests on {run['support']:.1f} effective vertices, fewer than {min_support:.0f} — too thin a band to average"
            )

    for end, run in sorted(runs.items()):
        if not end.startswith("Left"):
            continue
        mirror = runs.get("Right" + end[len("Left") :])
        if mirror is None:
            continue
        mean = 0.5 * (run["ratio_measured"] + mirror["ratio_measured"])
        if abs(mean) <= 1e-6:
            continue
        gap = abs(run["ratio_measured"] - mirror["ratio_measured"]) / abs(mean)
        band = asymmetry_band(end, asymmetry_arms=asymmetry_arms, asymmetry_other=asymmetry_other)
        if gap > band:
            where = "an arm" if end in ARM_LANDMARKS else "a mirrored body"
            problems.append(
                f"{run['run']} measured {run['ratio_measured']:.2f}x and {mirror['run']} {mirror['ratio_measured']:.2f}x — "
                f"{gap * 100:.1f}% apart, past the {band * 100:.0f}% {where} allows"
            )

    for side, values in report.get("grounding", {}).items():
        if not 0.7 <= values["factor"] <= 1.4:
            warnings.append(
                f"the {side} leg needed a grounding factor of {values['factor']:.2f} to put its toe on the floor — "
                "the weights and the soles disagree by more than a boot"
            )
    for run in report["runs"]:
        if run["off_axis_m"] > 0.5 * max(run["reference_length_m"], 1e-6):
            warnings.append(
                f"{run['run']}'s landmark sits {run['off_axis_m'] * 100:.1f} cm off the run's frozen axis "
                f"(the run is {run['reference_length_m'] * 100:.1f} cm) — this body does not point where the contract points, "
                "and no length can fix a direction"
            )
    return problems, warnings


# ------------------------------------------------------------------ table --


def shoulder_line_note(report: dict) -> str | None:
    """One line saying where the arms left the body and what the geometry says.

    Printed by both callers — this file's ``__main__`` and ``forge gen
    skin`` — so the number a person reads and the number the record files are
    the same number and come from one place. ``None`` when the body has no
    arm run to measure, which is a body plan's business and not a fault.
    """
    line = report.get("shoulder_line") or {}
    if line.get("weights_m") is None:
        return None
    tube = line.get("arm_tube_geometry_m")
    if tube is None:
        return f"shoulder line {line['weights_m']:.4f} m from the weights; the arm tube's median height is unknown"
    return (
        f"shoulder line {line['weights_m']:.4f} m from the weights, against an arm tube whose median height is "
        f"{float(tube):.4f} m ({abs(line['gap_m']) * 100:.1f} cm apart) — a cross-check, not a gate"
    )


def run_table(report: dict) -> str:
    lines = [f"{'run':<34}{'ref cm':>7}{'fit cm':>7}{'meas':>7}{'used':>7}{'supp':>7}{'sq':>6}{'off cm':>7}"]
    for run in report["runs"]:
        lines.append(
            f"{run['run']:<34}{run['reference_length_m'] * 100:7.1f}"
            f"{run['ratio'] * run['reference_length_m'] * 100:7.1f}"
            f"{run['ratio_measured']:7.2f}{run['ratio']:7.2f}{run['support']:7.0f}{run['squareness']:6.2f}{run['off_axis_m'] * 100:7.1f}"
        )
    return "\n".join(lines)


def bone_table(report: dict) -> str:
    lines = [f"{'bone':<20}{'parent':<18}{'ref cm':>7}{'fit cm':>7}{'ratio':>7}  source"]
    for row in report["bones"]:
        lines.append(
            f"{row['bone']:<20}{str(row.get('parent') or '-'):<18}"
            f"{row['reference_length_m'] * 100:7.1f}{row['fitted_length_m'] * 100:7.1f}{row['ratio']:7.2f}  {row['source']}"
        )
    return "\n".join(lines)


def convergence(report: dict, earlier: dict) -> dict:
    """How far every joint moved between two passes — the design's convergence claim."""
    moves = {}
    for name, position in report["fitted_joints"].items():
        before = earlier["fitted_joints"].get(name)
        if before is None:
            continue
        moves[name] = math.dist(position, before)
    if not moves:
        return {}
    worst = max(moves, key=lambda key: moves[key])
    return {
        "joints": len(moves),
        "max_move_mm": round(moves[worst] * 1000.0, 3),
        "max_move_bone": worst,
        "mean_move_mm": round(sum(moves.values()) / len(moves) * 1000.0, 3),
        "ratio_changes": {
            run["run"]: round(run["ratio"] - was["ratio"], 4)
            for run in report["runs"]
            for was in [next((r for r in earlier.get("runs", []) if r["run"] == run["run"]), None)]
            if was is not None
        },
        "per_bone_mm": {name: round(value * 1000.0, 3) for name, value in sorted(moves.items(), key=lambda kv: -kv[1])},
    }


# ------------------------------------------------------------------- main --


def _dense_weights(document: dict, binary: bytes, joints: int) -> tuple[np.ndarray, np.ndarray]:
    """Positions and a (vertices, joints) weight table, summed over the influence sets."""
    points: list[np.ndarray] = []
    dense: list[np.ndarray] = []
    for mesh in document.get("meshes") or []:
        for primitive in mesh["primitives"]:
            attributes = primitive["attributes"]
            if "JOINTS_0" not in attributes or "WEIGHTS_0" not in attributes:
                continue
            position = _accessor(document, binary, attributes["POSITION"]).astype(np.float64)
            block = np.zeros((len(position), joints), dtype=np.float64)
            rows = np.arange(len(position))
            for slot in range(8):
                key = f"JOINTS_{slot}"
                if key not in attributes:
                    break
                joint_index = _accessor(document, binary, attributes[key]).astype(int)
                weight = _accessor(document, binary, attributes[f"WEIGHTS_{slot}"]).astype(np.float64)
                for column in range(joint_index.shape[1]):
                    np.add.at(block, (rows, joint_index[:, column]), weight[:, column])
            points.append(position)
            dense.append(block)
    if not points:
        raise InputRejected("no skinned primitive in the file — JOINTS_0/WEIGHTS_0 is what this reads")
    return np.concatenate(points), np.concatenate(dense)


def _reorder(dense: np.ndarray, order: list[str], names: list[str]) -> np.ndarray:
    """Columns from skin-joint order into contract order."""
    index = {name: i for i, name in enumerate(order)}
    missing = [name for name in names if name not in index]
    if missing:
        raise InputRejected(
            f"{len(missing)} contract bone(s) have no column in the skin ({', '.join(missing[:6])}) — "
            "pass --names with the glb that went in, or --map with the joint-order map"
        )
    return dense[:, [index[name] for name in names]]


def _joint_order(document: dict, args, prof: profile_mod.Profile) -> list[str]:
    """The contract name of every skin joint, in the file's own joint order."""
    contract = {bone["name"] for bone in prof.bones}
    own = _skin_joint_names(document)
    if set(own) >= contract:
        return own
    if args.names:
        other, _ = _chunks(Path(args.names).expanduser().resolve())
        named = _skin_joint_names(other)
    elif args.map:
        named = list(json.loads(Path(args.map).read_text(encoding="utf-8"))["handed_in"])
    else:
        raise UsageError(
            f"the skin's joints are named {', '.join(own[:3])}…, not the contract's — "
            "pass --names <the prepared glb that went in> or --map <the joint-order map>"
        )
    if len(named) != len(own):
        raise InputRejected(
            f"the skin has {len(own)} joint(s) and the name source has {len(named)} — "
            "the by-order alignment does not hold, and there is no honest map by index"
        )
    return named


def run(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="forge-gen fit", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("skinned", help="a skinned glb: JOINTS_0/WEIGHTS_0 and POSITION in the bind pose")
    parser.add_argument("--names", metavar="GLB", help="a glb whose skin joint order carries the contract names (the prepared mesh that went in)")
    parser.add_argument("--map", metavar="JSON", help="the joint-order map reattach.py wrote")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument("--out", metavar="JSON", help="where the fit report goes (default: out/skin/<stem>.fit.json)")
    parser.add_argument("--compare", metavar="JSON", help="an earlier fit report; print how far every joint moved")
    parser.add_argument("--min-support", type=float, default=8.0, metavar="N", help="fewest effective vertices a measured run may rest on (default: 8)")
    parser.add_argument("--ratio-min", type=float, default=0.4, metavar="R")
    parser.add_argument("--ratio-max", type=float, default=2.5, metavar="R")
    parser.add_argument("--asymmetry-arms", type=float, default=None, metavar="F", help="how far an arm's left/right pair may differ (default: the profile's [fit] asymmetry_arms)")
    parser.add_argument("--asymmetry-other", type=float, default=None, metavar="F", help="and every other pair (default: the profile's [fit] asymmetry_other)")
    parser.add_argument("--no-symmetry", action="store_true", help="keep each side's own measurement instead of the pair's mean")
    parser.add_argument("--no-ground", action="store_true", help="skip the grounding pass and report the weights raw")
    parser.add_argument("--bones", action="store_true", help="print the per-bone table as well as the per-run one")
    parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
    args = parser.parse_args(argv)

    skinned = Path(args.skinned).expanduser().resolve()
    if not skinned.is_file():
        raise UsageError(f"{skinned} is not a file")
    prof = profile_mod.load_profile(args.profile)
    document, binary = _chunks(skinned)
    order = _joint_order(document, args, prof)
    names = [bone["name"] for bone in prof.bones]
    points, dense = _dense_weights(document, binary, len(order))
    dense = _reorder(dense, order, names)

    report = fit(points, dense, prof, min_support=args.min_support, symmetry=not args.no_symmetry, ground=not args.no_ground)
    report["source"] = str(skinned)
    report["profile_dir"] = str(prof.dir)
    bands = symmetry_bands(prof, arms=args.asymmetry_arms, other=args.asymmetry_other)
    problems, warnings = gate(
        report,
        ratio_min=args.ratio_min,
        ratio_max=args.ratio_max,
        min_support=args.min_support,
        **bands,
    )
    report["asymmetry"] = bands
    report["problems"] = problems
    report["warnings"] = warnings
    if args.compare:
        report["convergence"] = convergence(report, json.loads(Path(args.compare).read_text(encoding="utf-8")))

    out = Path(args.out).expanduser().resolve() if args.out else Path("out/skin") / (skinned.stem.split(".")[0] + ".fit.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if not args.json:
        print(run_table(report))
        if args.bones:
            print()
            print(bone_table(report))
        print()
        root_row = report["bones"][[row["bone"] for row in report["bones"]].index(report["root"])]
        print(f"{TAG}: root {report['root']} at y {root_row['fitted_y_m']:.4f} m against the contract's {root_row['reference_y_m']:.4f} m ({root_row['support']:.0f} effective vertices)")
        note = shoulder_line_note(report)
        if note:
            print(f"{TAG}: {note}")
        for side, values in report.get("grounding", {}).items():
            print(f"{TAG}: {side} leg grounded by {values['factor']:.3f} — toe {values['toe_before_m']:.3f} m -> {values['toe_after_m']:.3f} m, hip joint {values['hip_joint_m']:.3f} m")
        print(f"{TAG}: motion_scale {report['motion_scale']:.4f}")
        if report.get("convergence"):
            conv = report["convergence"]
            print(f"{TAG}: convergence — worst joint moved {conv['max_move_mm']:.2f} mm ({conv['max_move_bone']}), mean {conv['mean_move_mm']:.2f} mm over {conv['joints']} joints")
        for warning in warnings:
            print(f"{TAG}: WARN {warning}")
        print(f"{TAG}: wrote {out}")

    if problems:
        listed = "\n  - ".join(problems)
        raise InputRejected(f"the fit is not trustworthy ({len(problems)} problem(s)):\n  - {listed}", report=str(out))
    if args.json:
        print(json.dumps({"ok": True, "report": str(out), "motion_scale": report["motion_scale"], "warnings": warnings}))
    return 0


def main(argv: list[str]) -> int:
    try:
        return run(argv)
    except ForgeGenError as err:
        sys.stderr.write(f"{TAG}: {err.error}: {err.message}\n")
        return err.code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
