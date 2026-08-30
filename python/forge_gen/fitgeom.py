"""Where a body's shoulder, crotch, floor and limbs are, read from its vertices alone.

One module, two readers. ``blender/prepare.py`` calls it inside Blender on
the arrays ``foreach_get`` hands it; ``fit.py`` calls it on the arrays it
already pulls out of the glb. Both ask the same questions of the same
geometry, so the T-pose gate and the fit cannot disagree about where a
shoulder is — which is exactly what happened while the gate measured the
mesh against *the skeleton's* wrists and the fit then moved those wrists to
the mesh.

Everything here is in the **glTF frame**: ``+X`` right, ``+Y`` up, ``+Z``
front, metres, after the body has been normalised (feet at ``y = 0``,
centred on the mirror plane). :func:`from_blender` turns Blender's ``+Z``-up
arrays into it, which is the one conversion prepare needs.

Stdlib plus numpy. It opens nothing and spends no card.

# The shoulder line

``shoulder_y`` is the median height of every vertex further out than
:data:`SHOULDER_FRACTION` of the body's own half-span. On a T-pose that is
the arm tube and nothing else, and its median height is the height the arms
leave the body at — the witch reads 1.3143 m against the 1.311 m the fit
spike cross-checked her at. It is a statement about *this body's pose*
and says nothing about stature, which is the whole reason it replaced the
reach gate.

# The limb sections

:func:`limb_sections` answers "how thick is this limb, in units of the bone
it hangs on". Along the run's own frozen axis it takes three stations, and
at each one the vertices within :data:`BAND_M` along the axis — and then
**only the cluster reachable by :data:`LINK_M` links from the point nearest
the axis.** The clustering is not optional: the slab at mid-thigh also
holds the far leg, the crotch and the skirt, and without it the number
measures the body rather than the limb.

The station's radius is the median distance of that cluster to the axis;
the run's radius is the median of its three stations; the ratio is that over
the run's reference length. Measured 2026-08-30 on the prepared glbs still
on disk:

===============  =============  =============  =========================
body             upper arm L/R  forearm L/R    judged
===============  =============  =============  =========================
courier_qwen     0.200 / 0.206  0.214 / 0.212  walked as a sliver
courier_flux     0.244 / 0.253  0.164 / 0.166  never judged on a strip
courier_v2       0.505 / 0.545  0.669 / 0.674  walked and shot correctly
vex_runner       0.239 / 0.311  0.608 / 0.610  ships
drow_warlock     0.549 / 0.545  0.682 / 0.568  walks a swept clip
moss_witch_v4    0.218 / 0.160  0.872 / 0.844  **walks, aims and rolls**
===============  =============  =============  =========================

The last row is why ``[fit] limb_radius_min_fraction`` ships as a **printed
note and not a refusal**. On the first four bodies 0.22 separates the sliver
from everything that walks with 8.7 % of headroom; the witch then measures
below every arm of the sliver and walks anyway, because she is a thin arm
inside a wide sleeve and her 1.04 m half-span puts the contract's upper-arm
run across the sleeve rather than through the arm. A number that ranks a
shipped body below a broken one is not a threshold, and a gate nobody can
calibrate ships as a number rather than as a refusal.

The **off-axis** distance travels beside the radius, because a limb can be
thin for two different reasons and only one of them is thinness: the
sliver's right arm sits a median 1.47 m off the contract's axis, which is a
direction failure. A number the record prints rather than guesses at.
"""

from __future__ import annotations

import numpy as np

#: A vertex further out than this fraction of the half-span is arm-tube
#: geometry. 0.55 is wide enough to take the whole tube on a body whose
#: hands are mitts and narrow enough to leave the shoulders out of it.
SHOULDER_FRACTION = 0.55

#: And beyond this fraction it is the arm's far end. The profile carries the
#: same number as ``[fit] arm_tip_fraction`` and ``prepare`` passes it in;
#: this default is here so the module answers on its own.
ARM_TIP_FRACTION = 0.9

#: A band whose nearest vertex to the mirror plane sits further out than this
#: fraction of the half-span has a gap in it — two legs rather than one torso.
CROTCH_GAP_FRACTION = 0.02

#: How tall a crotch scan band is (metres).
CROTCH_BAND_M = 0.01

#: How far below a band the gap must persist before the scan believes it — a
#: single band with a hole in it is a hole, not a crotch.
CROTCH_CONFIRM_M = 0.05

#: The crotch is looked for below this fraction of the body's height. Above
#: it there is nothing a leg gap could be.
CROTCH_CEILING = 0.75

#: A band with fewer vertices than this says nothing either way.
MIN_BAND_VERTICES = 4

#: Where along a run the sections are taken.
STATIONS = (0.25, 0.5, 0.75)

#: A section takes the vertices within this far along the axis (metres).
BAND_M = 0.02

#: And keeps only the cluster reachable by links this long (metres).
LINK_M = 0.035

#: Fewer vertices in the band than this is not a section anybody can measure.
MIN_SECTION_VERTICES = 8

#: And fewer than this in the cluster is a speck, not a cross-section. Lower
#: than the band's floor on purpose: ``vex_runner``'s thighs carry five
#: vertices per band at this resolution, and a leg that reports ``null``
#: teaches nothing where a leg that reports 0.15 says the number does not
#: separate a sliver from a body that ships. Legs are never refused on it.
MIN_CLUSTER_VERTICES = 3


def from_blender(points: np.ndarray) -> np.ndarray:
    """Blender's ``+Z`` up to glTF's ``+Y`` up: ``(x, y, z) -> (x, z, -y)``."""
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    return np.column_stack((points[:, 0], points[:, 2], -points[:, 1]))


def measure(points: np.ndarray, *, arm_tip_fraction: float = ARM_TIP_FRACTION) -> dict:
    """The heights and the half-span this body states about itself.

    ``shoulder_y`` is the arm tube's own median height and ``arm_tip_y`` the
    median of its outermost slice, beyond ``arm_tip_fraction`` of the
    half-span. On a T-pose the two agree to a centimetre; the gap between
    them is how far the outer half of the arm has fallen, which is what
    ``prepare``'s T-pose gate reads. **It under-reads a droop**, because the
    tube whose median sets the shoulder line is the same tube that is
    falling: over a metre of fall the gap comes out at a stable 0.23 of it,
    so 0.15 m of tolerance refuses an arm that has fallen about 0.65 m. That
    is why ``[fit] arm_height_tolerance_m`` is a budget and not a
    measurement of where bodies break.

    ``heads`` is the body's total height over the head-and-neck standing
    above its own shoulder line — a note the record prints, never a gate
    here: the head count that refuses anything is measured on the reference
    PNG at the import door, where a redraw is cheap.
    """
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    if len(points) == 0:
        raise ValueError("no vertices to measure")
    xs, ys = points[:, 0], points[:, 1]
    half_span = float(max(xs.max(), -xs.min()))
    lowest = float(ys.min())
    highest = float(ys.max())
    outboard = points[np.abs(xs) > SHOULDER_FRACTION * half_span] if half_span > 1e-9 else points[:0]
    tips = points[np.abs(xs) > arm_tip_fraction * half_span] if half_span > 1e-9 else points[:0]
    shoulder_y = float(np.median(outboard[:, 1])) if len(outboard) else None
    arm_tip_y = float(np.median(tips[:, 1])) if len(tips) else shoulder_y
    head = (highest - shoulder_y) if shoulder_y is not None else None
    return {
        "half_span": round(half_span, 5),
        "lowest_y": round(lowest, 5),
        "highest_y": round(highest, 5),
        "shoulder_y": None if shoulder_y is None else round(shoulder_y, 5),
        "arm_tip_y": None if arm_tip_y is None else round(arm_tip_y, 5),
        "crotch_y": _crotch(points, half_span),
        "heads": None if not head or head <= 1e-6 else round((highest - lowest) / head, 3),
        "vertices": int(len(points)),
        "arm_tube_vertices": int(len(outboard)),
        "arm_tip_vertices": int(len(tips)),
    }


def _crotch(points: np.ndarray, half_span: float) -> float | None:
    """The height at which the gap between the legs opens, scanning down.

    A band "has a gap" when its nearest vertex to the mirror plane sits
    further out than :data:`CROTCH_GAP_FRACTION` of the half-span. Walking
    down from :data:`CROTCH_CEILING`, the crotch is the first band that has a
    gap and whose next :data:`CROTCH_CONFIRM_M` of bands all have one too —
    the confirmation is what keeps a hole in the mesh from reading as a
    crotch.

    ``None`` when the gap never opens: a robe, a skirt or a cloak closes it
    entirely, and on a body like that the crotch is a fact about the garment.
    Unknown is returned as unknown; it is a cross-check either way, and
    ``fit.py`` never places a joint from it.
    """
    if half_span <= 1e-9:
        return None
    xs, ys = points[:, 0], points[:, 1]
    lowest, highest = float(ys.min()), float(ys.max())
    bands = int((highest - lowest) / CROTCH_BAND_M)
    if bands < 4:
        return None
    gap_floor = CROTCH_GAP_FRACTION * half_span
    gapped: list[bool | None] = []
    for index in range(bands):
        low, high = lowest + index * CROTCH_BAND_M, lowest + (index + 1) * CROTCH_BAND_M
        band = (ys >= low) & (ys < high)
        gapped.append(None if int(band.sum()) < MIN_BAND_VERTICES else bool(np.abs(xs[band]).min() > gap_floor))
    confirm = max(1, int(CROTCH_CONFIRM_M / CROTCH_BAND_M))
    for index in range(int(CROTCH_CEILING * bands), confirm, -1):
        window = [state for state in gapped[index - confirm : index + 1] if state is not None]
        if window and all(window):
            return round(lowest + (index + 1) * CROTCH_BAND_M, 5)
    return None


def limb_sections(
    points: np.ndarray,
    origin,
    tip,
    *,
    stations: tuple[float, ...] = STATIONS,
    band_m: float = BAND_M,
    link_m: float = LINK_M,
) -> dict:
    """How thick the limb along ``origin -> tip`` is, in units of that run's length.

    ``{reference_length_m, radius_m, ratio, off_axis_m, stations: [...]}``,
    with ``ratio: None`` when no station held enough geometry to measure —
    unknown, never a default.
    """
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    origin = np.asarray(origin, dtype=np.float64)
    vector = np.asarray(tip, dtype=np.float64) - origin
    length = float(np.linalg.norm(vector))
    if length <= 1e-9:
        raise ValueError("the run has no length, so it has no axis")
    axis = vector / length
    along = (points - origin) @ axis
    rows = [_section(points, origin, axis, along, fraction * length, band_m, link_m) for fraction in stations]
    radii = [row["radius_m"] for row in rows if row["radius_m"] is not None]
    offs = [row["off_axis_m"] for row in rows if row["off_axis_m"] is not None]
    radius = float(np.median(radii)) if radii else None
    return {
        "reference_length_m": round(length, 5),
        "radius_m": None if radius is None else round(radius, 5),
        "ratio": None if radius is None else round(radius / length, 4),
        "off_axis_m": round(float(np.median(offs)), 4) if offs else None,
        "stations": [
            {
                "at": round(fraction, 3),
                "radius_m": None if row["radius_m"] is None else round(row["radius_m"], 5),
                "ratio": None if row["radius_m"] is None else round(row["radius_m"] / length, 4),
                "off_axis_m": None if row["off_axis_m"] is None else round(row["off_axis_m"], 4),
                "vertices": row["vertices"],
            }
            for fraction, row in zip(stations, rows)
        ],
    }


def _section(points, origin, axis, along, at, band_m, link_m) -> dict:
    """One station: the band, the cluster inside it, its median radius and its offset."""
    mask = np.abs(along - at) <= band_m
    count = int(mask.sum())
    if count < MIN_SECTION_VERTICES:
        return {"radius_m": None, "off_axis_m": None, "vertices": count}
    banded = points[mask]
    radial = (banded - origin) - np.outer(along[mask], axis)
    distance = np.linalg.norm(radial, axis=1)
    keep = _cluster(banded, int(np.argmin(distance)), link_m)
    if len(keep) < MIN_CLUSTER_VERTICES:
        return {"radius_m": None, "off_axis_m": None, "vertices": len(keep)}
    centre = radial[keep].mean(axis=0)
    return {
        "radius_m": float(np.median(distance[keep])),
        "off_axis_m": float(np.linalg.norm(centre)),
        "vertices": int(len(keep)),
    }


def _cluster(banded: np.ndarray, seed: int, link_m: float) -> list[int]:
    """Everything reachable from ``seed`` by hops of at most ``link_m``.

    A plain flood over the pairwise distances, affordable because a band of a
    limb is hundreds of vertices and never thousands; the alternative — no
    clustering — measures the far leg, the crotch and the skirt.
    """
    count = len(banded)
    reached = np.zeros(count, dtype=bool)
    reached[seed] = True
    frontier = [seed]
    while frontier:
        gaps = np.linalg.norm(banded[frontier][None, :, :] - banded[:, None, :], axis=2).min(axis=1)
        found = (~reached) & (gaps <= link_m)
        if not found.any():
            break
        frontier = np.flatnonzero(found).tolist()
        reached[found] = True
    return np.flatnonzero(reached).tolist()
