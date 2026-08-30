"""Draw the profile's rest pose as an OpenPose stick figure — the pose image a reference is conditioned on.

    python3 python/forge_gen/spike_pose.py [--out out/spike/tpose_pose.png]
            [--profile DIR] [--size 1024] [--json]

Phase 3 conditions ``generate_reference`` on "the profile's T-pose skeleton
rendered as a pose image" (designs/forge2.md). This is that image, and it is
drawn here rather than rendered because **nothing in the toolkit can draw a
bare skeleton**, which is worth writing down once so nobody looks again:

* ``forge views`` and ``forge turntable`` frame a subject from its *mesh*
  bounds and refuse a scene with none — "the model spawned nothing with
  bounds" (``forge_studio::render``) — and ``rigs/humanoid/rig.glb`` holds
  55 joints and zero meshes;
* ``forge bones`` is a text report, on purpose, and has no picture in it;
* ``forge sheet`` poses a *clip* on a *body*, so it needs both, and answers
  a different question;
* the sweep's stick sheet (``forge gen motion review``) draws a take's
  joints, from an ``.npz``, through numpy and matplotlib inside the ARDY
  environment, as a multi-panel figure with axes and titles. It is a
  reviewer's contact sheet, not a conditioning image.

So: stdlib only, through ``forge_gen.png`` — the same encoder the ``--fake``
placeholders use — reading ``contract.json``'s rest translations through
``profile.rest_world()``. No Pillow, no matplotlib, no wgpu adapter, no
display.

**What is drawn, and what is invented.** Fourteen of the eighteen COCO
keypoints are contract joints, read straight from the rest pose: the
shoulders, elbows and wrists (``LeftArm``/``LeftForeArm``/``LeftHand`` and
their mirrors), hips, knees and ankles (``LeftUpLeg``/``LeftLeg``/
``LeftFoot``), and the neck, which OpenPose puts at the midpoint of the
shoulders rather than where the ``Neck`` bone sits. The remaining four —
nose, both eyes, both ears — have no bones in this contract, and are placed
around the ``Head`` joint from the head-scale constants below. They are a
drawing convention, not a measurement, and the ``--json`` payload says
which keypoints were read and which were invented.

The view is the front, because ``[bones] front`` is ``+Z`` and the fit gate
measures a front-facing T-pose. The camera stands in front of the figure
looking back at it, so the character's own left hand is drawn on the right
of the image — the way a person facing you has their left hand on your
right, which is what a pose model is trained on. The figure is framed from
the floor to the reference stature so it fills nine tenths of the height:
the same framing the reference format asks a subject for, so the pose and
the drawing the model makes from it agree about where the body is.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from forge_gen import png, profile as profile_mod  # noqa: E402
from forge_gen.exit_codes import UsageError  # noqa: E402

#: The log prefix.
TAG = "pose"

#: Where the image lands when ``--out`` is not given.
DEFAULT_OUT = Path("out") / "spike" / "tpose_pose.png"

#: The eighteen COCO keypoints, in the order every OpenPose consumer reads them.
KEYPOINTS = (
    "nose", "neck",
    "right_shoulder", "right_elbow", "right_wrist",
    "left_shoulder", "left_elbow", "left_wrist",
    "right_hip", "right_knee", "right_ankle",
    "left_hip", "left_knee", "left_ankle",
    "right_eye", "left_eye", "right_ear", "left_ear",
)

#: Which contract bone each keypoint is, where one exists. ``neck`` is the
#: midpoint of the two shoulders (OpenPose's neck, not the Neck bone), and
#: the five head keypoints are placed by :data:`HEAD_OFFSETS`.
FROM_CONTRACT = {
    "right_shoulder": "RightArm",
    "right_elbow": "RightForeArm",
    "right_wrist": "RightHand",
    "left_shoulder": "LeftArm",
    "left_elbow": "LeftForeArm",
    "left_wrist": "LeftHand",
    "right_hip": "RightUpLeg",
    "right_knee": "RightLeg",
    "right_ankle": "RightFoot",
    "left_hip": "LeftUpLeg",
    "left_knee": "LeftLeg",
    "left_ankle": "LeftFoot",
}

#: The face, in metres from the ``Head`` joint, as ``(across, up)``. Across
#: is toward the figure's own left (+X in the rest pose). Invented, not
#: measured: this contract has no facial bones, and a pose image with no
#: face reads to a pose model as a figure whose head is turned away.
HEAD_OFFSETS = {
    "nose": (0.0, 0.020),
    "left_eye": (0.032, 0.055),
    "right_eye": (-0.032, 0.055),
    "left_ear": (0.075, 0.050),
    "right_ear": (-0.075, 0.050),
}

#: The limbs, by keypoint index, in OpenPose's own order.
LIMBS = (
    (1, 2), (1, 5), (2, 3), (3, 4), (5, 6), (6, 7),
    (1, 8), (8, 9), (9, 10), (1, 11), (11, 12), (12, 13),
    (1, 0), (0, 14), (14, 16), (0, 15), (15, 17),
)

#: OpenPose's eighteen keypoint colours, reused for the limbs in order.
COLOURS = (
    (255, 0, 0), (255, 85, 0), (255, 170, 0), (255, 255, 0), (170, 255, 0), (85, 255, 0),
    (0, 255, 0), (0, 255, 85), (0, 255, 170), (0, 255, 255), (0, 170, 255), (0, 85, 255),
    (0, 0, 255), (85, 0, 255), (170, 0, 255), (255, 0, 255), (255, 0, 170), (255, 0, 85),
)

#: How much of the image height the figure spans, floor to stature.
FILL = 0.9

#: Limb half-width and joint radius as fractions of the image's long side —
#: the proportions the annotator draws at 512 px, so a 1024 px image looks
#: like a 1024 px OpenPose image and not like a 512 px one scaled up.
LIMB_RADIUS = 4.0 / 512.0
JOINT_RADIUS = 4.0 / 512.0

#: The annotator composites limbs at this weight and joints at full.
LIMB_ALPHA = 0.6


def keypoints(prof: profile_mod.Profile) -> tuple[list[tuple[float, float] | None], list[str]]:
    """The eighteen keypoints in the rest pose's own metres, as ``(across, up)``.

    ``across`` is +X of the rest pose — the figure's own left — and ``up``
    is +Y. Depth is dropped: this is the front view, and the T-pose has
    nothing to say in Z that a front view could show.
    """
    rest = prof.rest_world()
    missing = [bone for bone in FROM_CONTRACT.values() if bone not in rest]
    if missing:
        raise UsageError(f"{prof.dir}: the contract has no bone(s) {', '.join(missing)} — this drawing is written for a humanoid profile")
    points: list[tuple[float, float] | None] = [None] * len(KEYPOINTS)
    for name, bone in FROM_CONTRACT.items():
        x, y, _ = rest[bone][0]
        points[KEYPOINTS.index(name)] = (x, y)
    left = points[KEYPOINTS.index("left_shoulder")]
    right = points[KEYPOINTS.index("right_shoulder")]
    points[KEYPOINTS.index("neck")] = ((left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0)
    head_x, head_y, _ = rest["Head"][0]
    for name, (across, up) in HEAD_OFFSETS.items():
        points[KEYPOINTS.index(name)] = (head_x + across, head_y + up)
    invented = sorted(HEAD_OFFSETS) + ["neck"]
    return points, invented


class Canvas:
    """An RGB canvas with the two marks a pose image is made of."""

    def __init__(self, size: int) -> None:
        self.size = size
        self.pixels = bytearray(size * size * 3)

    def _blend(self, x: int, y: int, colour: tuple, coverage: float) -> None:
        if coverage <= 0.0 or not (0 <= x < self.size and 0 <= y < self.size):
            return
        base = (y * self.size + x) * 3
        for channel in range(3):
            was = self.pixels[base + channel]
            self.pixels[base + channel] = int(round(was + (colour[channel] - was) * min(coverage, 1.0)))

    def capsule(self, a: tuple, b: tuple, radius: float, colour: tuple, alpha: float) -> None:
        """A segment with round ends — one limb."""
        (x0, y0), (x1, y1) = a, b
        dx, dy = x1 - x0, y1 - y0
        length_sq = dx * dx + dy * dy
        lo_x = max(0, int(min(x0, x1) - radius - 1))
        hi_x = min(self.size - 1, int(max(x0, x1) + radius + 1))
        lo_y = max(0, int(min(y0, y1) - radius - 1))
        hi_y = min(self.size - 1, int(max(y0, y1) + radius + 1))
        for y in range(lo_y, hi_y + 1):
            for x in range(lo_x, hi_x + 1):
                px, py = x + 0.5 - x0, y + 0.5 - y0
                t = 0.0 if length_sq < 1e-9 else max(0.0, min(1.0, (px * dx + py * dy) / length_sq))
                distance = ((px - t * dx) ** 2 + (py - t * dy) ** 2) ** 0.5
                self._blend(x, y, colour, alpha * (radius + 0.5 - distance))

    def disc(self, centre: tuple, radius: float, colour: tuple) -> None:
        """A filled circle — one joint."""
        cx, cy = centre
        for y in range(max(0, int(cy - radius - 1)), min(self.size - 1, int(cy + radius + 1)) + 1):
            for x in range(max(0, int(cx - radius - 1)), min(self.size - 1, int(cx + radius + 1)) + 1):
                distance = ((x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2) ** 0.5
                self._blend(x, y, colour, radius + 0.5 - distance)

    def write(self, path: Path) -> Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        png.write_png(path, self.size, self.size, bytes(self.pixels), channels=3)
        return path


def draw(prof: profile_mod.Profile, size: int) -> tuple[Canvas, list[tuple[float, float]], list[str]]:
    """The whole image: project, limbs under joints, the way the annotator stacks them."""
    metres, invented = keypoints(prof)
    stature = float(prof.section("bones")["reference_stature_m"])
    scale = FILL * size / stature
    top = (1.0 - FILL) * size / 2.0
    floor = top + FILL * size

    def project(point: tuple) -> tuple[float, float]:
        # +X is the figure's own left, and the camera stands in front of it:
        # its left hand lands on the right of the image, as a person facing
        # you has theirs on your right.
        return (size / 2.0 + point[0] * scale, floor - point[1] * scale)

    placed = [project(point) for point in metres]
    canvas = Canvas(size)
    limb_radius = LIMB_RADIUS * size
    joint_radius = JOINT_RADIUS * size
    for index, (a, b) in enumerate(LIMBS):
        canvas.capsule(placed[a], placed[b], limb_radius, COLOURS[index % len(COLOURS)], LIMB_ALPHA)
    for index, point in enumerate(placed):
        canvas.disc(point, joint_radius, COLOURS[index % len(COLOURS)])
    return canvas, placed, invented


def run(args) -> dict:
    try:
        prof = profile_mod.load_profile(args.profile)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err
    size = int(args.size)
    if size < 64:
        raise UsageError(f"--size {size} is too small to be a pose image")
    out = Path(args.out).expanduser().resolve() if args.out else (Path.cwd() / DEFAULT_OUT).resolve()

    canvas, placed, invented = draw(prof, size)
    written = canvas.write(out)
    sys.stdout.write(f"{TAG}: {prof.name} rest pose, front view, {size}x{size}\n")
    for index, name in enumerate(KEYPOINTS):
        mark = "  (drawn, not measured)" if name in invented else ""
        source = FROM_CONTRACT.get(name, "—")
        sys.stdout.write(f"{TAG}:   {index:>2} {name:<15} {source:<14} at ({placed[index][0]:7.1f}, {placed[index][1]:7.1f}) px{mark}\n")
    sys.stdout.write(f"{TAG}: wrote {written}\n")
    return {
        "ok": True,
        "outputs": [os.fspath(written)],
        "profile": prof.name,
        "size": size,
        "keypoints": {name: [round(v, 2) for v in placed[index]] for index, name in enumerate(KEYPOINTS)},
        "invented_keypoints": invented,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="spike_pose", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", metavar="PNG", help=f"where the image goes (default: {DEFAULT_OUT})")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument("--size", type=int, default=1024, metavar="PX", help="square side in pixels")
    parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
    args = parser.parse_args(argv)
    try:
        result = run(args)
    except UsageError as err:
        sys.stderr.write(f"{TAG}: {err.message}\n")
        return err.code
    if args.json:
        sys.stdout.write(json.dumps(result, ensure_ascii=False) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
