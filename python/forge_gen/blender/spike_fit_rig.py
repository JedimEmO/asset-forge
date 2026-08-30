"""Build a per-body skeleton from a fit report: same names, same hierarchy, same rest rotations, new lengths.

    python3 python/forge_gen/blender/spike_fit_rig.py out/spike_fit/pass1.fit.json
            [--source rigs/humanoid] [--out out/spike_fit/profile]
            [--reach-max 2.0] [--arm-height-tolerance 0.35] [--json]

**A Phase 2 spike, not a door.** ``spike_fit.py`` measures how long every
bone is on one body; this turns those numbers into an armature, as a
**scratch profile** under ``out/`` — a copy of the source profile whose
``rig.blend`` and ``rig.glb`` are the fitted skeleton. Nothing under
``rigs/`` or ``assets/`` is touched, and the scratch profile is thrown away
with the rest of ``out/``.

# What it moves and what it may not

Each bone's **head** is placed by walking the hierarchy from the root and
scaling the segment it hangs on: ``head_new = parent_head_new + ratio *
(head_old - parent_head_old)``. The report's own ``fitted_joints`` are used
only for the root and then as a cross-check, because they are rounded to
10 µm and a 10 µm error at both ends of a 2 cm finger bone is 2.5e-4 of
quaternion — a *rotation*, and the one thing this script may not introduce.
Scaling the old segment vector keeps the direction to float precision by
construction. Each bone's **tail** goes to its child's new head when it used
to sit on its child's old head (a connected chain stays connected and keeps
its look), and otherwise along its own unchanged direction, scaled by the
bone's ratio.

Every bone's **world orientation is unchanged**, and that is checked rather
than assumed: the fit only ever scales a segment by a positive scalar along
the contract's own direction, so head-to-head directions come through
untouched, and re-applying each bone's saved roll re-creates the rest matrix
exactly. Before saving, every bone's rest rotation is compared against the
one it had, and a gap over ``ROTATION_TOLERANCE`` aborts the build.
That check is the whole safety argument of the design: clips bind by name
path and are baked against rest *rotations*, so rotations frozen and lengths
free is the line, and a script that quietly rotated a bone would produce a
rig that binds perfectly and animates wrongly.

The bones are unlinked from their parents' tails (``use_connect = False``)
before anything moves, because a connected child's head follows its parent's
tail and the second edit would undo the first.

# The scratch profile's own gates

``profile.toml`` is copied with two numbers loosened — ``[fit] reach_max``
and ``[fit] arm_height_tolerance_m`` — because the T-pose gate measures the
mesh against *the skeleton's* wrists, and a body whose wrists the skeleton
now matches is exactly the body that gate refused. Nothing else is relaxed:
the export gate's 0.1 mm rest tolerance, the influence cap and the stature
band stay where the shipped profile has them, and they are met against this
profile's own regenerated ``contract.json``.

After this runs, the profile is not yet self-consistent — ``contract.json``
still describes the source skeleton. Regenerate it::

    cargo run -q -p forge_rig --example export_contract -- out/spike_fit/profile
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

TAG = "fit-rig"

#: A rest rotation may differ from the one it had by this much per quaternion
#: component and still be the same rotation. Blender keeps edit-bone heads and
#: tails in single precision, so a 2 cm finger bone re-created from a scaled
#: segment lands within ~2e-6 of its old orientation and no closer; this is
#: still a hundred times tighter than the profile's own
#: ``rest_rotation_tolerance`` of 1e-3, which is what the gates downstream ask
#: for. Measured on the witch: the longest bones come back at ~1e-7, the
#: finger tips at 1.2-1.7e-6.
ROTATION_TOLERANCE = 1e-5
#: And a head may sit this far from the joint the report asked for.
HEAD_TOLERANCE_M = 1e-6


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("report", help="the fit report spike_fit.py wrote")
    parser.add_argument("--source", metavar="DIR", help="the profile to copy (default: the project's)")
    parser.add_argument("--out", metavar="DIR", default="out/spike_fit/profile", help="where the scratch profile goes")
    parser.add_argument("--reach-max", type=float, default=2.0, metavar="R", help="the scratch profile's [fit] reach_max")
    parser.add_argument("--arm-height-tolerance", type=float, default=0.35, metavar="M", help="the scratch profile's [fit] arm_height_tolerance_m")


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    report_path = _common.existing_file(args.report, what="fit report")
    report = json.loads(report_path.read_text(encoding="utf-8"))

    from forge_gen import profile as profile_mod

    source = profile_mod.load_profile(args.source)
    out_dir = Path(args.out).expanduser().resolve()
    if out_dir.resolve() == source.dir.resolve():
        raise UsageError(f"--out is the source profile ({source.dir}) — a spike writes under out/, never into a shipped profile")
    if out_dir.exists():
        shutil.rmtree(out_dir)
    shutil.copytree(source.dir, out_dir)
    _loosen(out_dir / "profile.toml", reach_max=args.reach_max, arm_height=args.arm_height_tolerance)
    _common.log(TAG, f"copied {source.dir} -> {out_dir}, [fit] reach_max {args.reach_max}, arm_height_tolerance_m {args.arm_height_tolerance}")

    argv = [os.fspath(report_path), "--source", os.fspath(source.dir), "--out", os.fspath(out_dir)]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    result = _common.outer_result(inner)
    result.setdefault("measured", {})["profile"] = str(out_dir)
    _common.log(TAG, f"next — cargo run -q -p forge_rig --example export_contract -- {out_dir}")
    return result


def _loosen(path: Path, *, reach_max: float, arm_height: float) -> None:
    """Rewrite the two T-pose numbers, in place, with a line saying why."""
    text = path.read_text(encoding="utf-8")
    text = re.sub(
        r"^reach_max = [0-9.]+.*$",
        f"reach_max = {reach_max}                    # SPIKE: loosened — the mesh is measured against the FITTED wrists",
        text,
        count=1,
        flags=re.M,
    )
    text = re.sub(
        r"^arm_height_tolerance_m = [0-9.]+.*$",
        f"arm_height_tolerance_m = {arm_height}      # SPIKE: loosened — this body is the point of the fit",
        text,
        count=1,
        flags=re.M,
    )
    path.write_text(text, encoding="utf-8")


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    import bpy
    from mathutils import Vector

    parser = argparse.ArgumentParser(prog="forge-gen fit-rig (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)

    from forge_gen import profile as profile_mod

    report = json.loads(Path(args.report).read_text(encoding="utf-8"))
    source = profile_mod.load_profile(args.source)
    out_dir = Path(args.out).resolve()
    armature_node = str(source.section("export").get("armature_node", "Armature"))
    ratios = {row["bone"]: float(row["ratio"]) for row in report["bones"]}
    reported = {name: Vector(_to_blender(position)) for name, position in report["fitted_joints"].items()}

    bpy.ops.wm.open_mainfile(filepath=os.fspath((out_dir / source.rig_blend.name).resolve()))
    arm_obj = bpy.data.objects.get(armature_node)
    if arm_obj is None or arm_obj.type != "ARMATURE":
        raise BackendFailed(f"the copied profile holds no {armature_node!r} armature — the source profile is broken")
    missing = sorted(bone.name for bone in arm_obj.data.bones if bone.name not in reported or bone.name not in ratios)
    if missing:
        raise InputRejected(f"the fit report has no joint for {len(missing)} bone(s) ({', '.join(missing[:6])}) — it was written against another profile")
    wanted = _heads(arm_obj, reported, ratios, report["root"])

    before_rotation = {bone.name: bone.matrix_local.to_quaternion().copy() for bone in arm_obj.data.bones}
    before_head = {bone.name: bone.head_local.copy() for bone in arm_obj.data.bones}

    bpy.context.view_layer.objects.active = arm_obj
    bpy.ops.object.mode_set(mode="EDIT")
    edit = arm_obj.data.edit_bones
    saved = {bone.name: (bone.head.copy(), bone.tail.copy(), float(bone.roll), bool(bone.use_connect)) for bone in edit}
    children: dict[str, list[str]] = {bone.name: [] for bone in edit}
    for bone in edit:
        if bone.parent is not None:
            children[bone.parent.name].append(bone.name)
    # A connected child's head follows its parent's tail, so every link goes
    # first: otherwise moving a parent undoes the child that was already set.
    for bone in edit:
        bone.use_connect = False
    for bone in edit:
        head_old, tail_old, roll, _connected = saved[bone.name]
        direction = tail_old - head_old
        length = direction.length
        if length < 1e-9:
            raise BackendFailed(f"{bone.name} has no length in the source rig — there is no direction to keep")
        direction = direction / length
        bone.head = Vector(wanted[bone.name])
        successor = _tail_child(bone.name, children, saved)
        if successor is not None:
            bone.tail = Vector(wanted[successor])
        else:
            bone.tail = bone.head + direction * (length * ratios.get(bone.name, 1.0))
        if (bone.tail - bone.head).length < 1e-6:
            raise BackendFailed(f"{bone.name} would come out zero-length — the fit gave it a ratio of {ratios.get(bone.name)}")
        bone.roll = roll
    bpy.ops.object.mode_set(mode="OBJECT")

    problems: list[str] = []
    moved = 0.0
    against_report = 0.0
    for bone in arm_obj.data.bones:
        against_report = max(against_report, (bone.head_local - reported[bone.name]).length)
        gap = max(abs(a - b) for a, b in zip(bone.matrix_local.to_quaternion(), before_rotation[bone.name]))
        if gap > ROTATION_TOLERANCE:
            problems.append(
                f"{bone.name}'s rest rotation moved by {gap:.2e} per component — the fit may scale a bone and may not turn one; "
                "every clip in the library is baked against these rotations"
            )
        placed = (bone.head_local - wanted[bone.name]).length
        if placed > HEAD_TOLERANCE_M:
            problems.append(f"{bone.name} landed {placed * 1000:.3f} mm from the joint the fit asked for")
        moved = max(moved, (bone.head_local - before_head[bone.name]).length)
    if problems:
        listed = "\n  - ".join(problems)
        raise BackendFailed(f"the fitted rig is not the contract's skeleton ({len(problems)} problem(s)):\n  - {listed}")
    _common.log(
        TAG,
        f"{len(arm_obj.data.bones)} bones re-lengthened, every rest rotation within {ROTATION_TOLERANCE:.0e}; "
        f"the furthest joint moved {moved * 100:.1f} cm, and every head is within {against_report * 1000:.3f} mm of the report's own joint",
    )

    out_blend = out_dir / source.rig_blend.name
    bpy.ops.wm.save_as_mainfile(filepath=os.fspath(out_blend))
    _common.log(TAG, f"wrote {out_blend}")
    out_glb = out_dir / source.rig_glb.name
    bpy.ops.object.select_all(action="DESELECT")
    _common.export_glb(out_glb, materials="NONE", y_up=bool(source.section("export").get("y_up", True)))
    _common.log(TAG, f"wrote {out_glb}")
    try:
        info = glb_mod.verify_glb(out_glb)
    except glb_mod.GlbError as err:
        raise BackendFailed(f"the exported rig failed its own check: {err}") from err
    _common.log(TAG, glb_mod.report(out_glb, info))

    measured = {
        "bones": len(arm_obj.data.bones),
        "max_joint_move_m": round(moved, 5),
        "max_gap_to_report_m": round(against_report, 6),
        "rotation_tolerance": ROTATION_TOLERANCE,
        "source_profile": str(source.dir),
        "blender_version": _common.blender_identity()["version"],
    }
    return _common.success(None, [out_blend, out_glb], measured=measured)


def _heads(arm_obj, reported: dict, ratios: dict[str, float], root: str) -> dict:
    """Every bone's new head, from the root down, by scaling the segment it hangs on.

    Exact where the report is not: the report rounds a joint to 10 µm, and
    two rounded ends of a 2 cm finger bone are a quarter of a milliradian of
    rotation. ``ratio * (head_old - parent_head_old)`` is the same direction
    to the last bit.
    """
    bones = arm_obj.data.bones
    if root not in bones:
        raise InputRejected(f"the rig has no {root!r} bone — the fit report is for another profile")
    out: dict = {root: reported[root].copy()}
    remaining = [bone for bone in bones if bone.name != root]
    for _ in range(len(remaining) + 2):
        progressed = False
        for bone in list(remaining):
            parent = bone.parent
            if parent is None or parent.name not in out:
                continue
            out[bone.name] = out[parent.name] + ratios[bone.name] * (bone.head_local - parent.head_local)
            remaining.remove(bone)
            progressed = True
        if not progressed:
            break
    if remaining:
        raise BackendFailed(f"{len(remaining)} bone(s) do not hang off {root} ({', '.join(b.name for b in remaining[:6])})")
    return out


def _to_blender(position: list[float]) -> tuple[float, float, float]:
    """glTF (+Y up) to Blender (+Z up): ``(x, y, z)_gltf = (x, z, -y)_blender``."""
    x, y, z = position
    return (x, -z, y)


def _tail_child(name: str, children: dict[str, list[str]], saved: dict[str, tuple]) -> str | None:
    """The child whose old head the bone's old tail sat on, if there is exactly one."""
    _head, tail, _roll, _connected = saved[name]
    on_tail = [child for child in children.get(name, []) if (saved[child][0] - tail).length < 1e-5]
    return on_tail[0] if len(on_tail) == 1 else None


# ---------------------------------------------------------------- both ends --


def _main_outer(argv: list[str]) -> int:
    from forge_gen import cli
    from forge_gen.exit_codes import ForgeGenError

    parser = argparse.ArgumentParser(prog="spike_fit_rig", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    _add_arguments(parser)
    parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
    parser.add_argument("--project", default=None, metavar="DIR")
    parser.add_argument("--created-by", default=None, metavar="WHO")
    args = parser.parse_args(argv)
    if args.project:
        records.set_project(args.project)
    try:
        result = run(args)
    except ForgeGenError as err:
        cli.emit(err.payload(), as_json=args.json)
        sys.stderr.write(f"{TAG}: {err.error}: {err.message}\n")
        return err.code
    result.setdefault("ok", True)
    cli.emit(result, as_json=args.json)
    return 0


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401
    except ImportError:
        sys.exit(_main_outer(sys.argv[1:]))
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
