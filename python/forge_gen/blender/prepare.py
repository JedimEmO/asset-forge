"""Prepare a mesh for the skinner: normalised geometry plus a skeleton, no weights.

    forge-gen prepare <lift.glb|rigged.blend> [--out out/prepare/<stem>.glb]
            [--record out/prepare/<stem>.prepare.json] [--profile DIR]
            [--skeleton rigs/humanoid/rig.blend] [--stature 1.80]
            [--yaw-deg 0] [--budget N]

The first of the two doors that make a body. What it writes is the input
SkinTokens' ``--use_skeleton`` wants, and what ``forge gen skin`` then hashes
as its ``mesh`` input, so the chain from a picture to a rig is
``lift -> prepare -> rig`` by hash:

* the mesh, normalised — yawed to the rig's front, scaled to stature, feet at
  Z=0, centred on the skeleton, through the T-pose gate, the dust filter
  and the triangle budget, its limb cross-sections measured into the record,
  wearing its texture at the profile's matte register;
* the skeleton, opened from ``--skeleton`` — the profile's own ``rig.blend``
  by default, and on the fit loop's second pass the per-body armature the fit
  just built — as a **sibling** object of the mesh, not its parent;
* **no vertex groups and no Armature modifier**, because the weights are what
  the skinner is being asked for. A file arriving with weights would let a run
  "succeed" on weights nobody generated.

``--skeleton`` is the only thing that differs between the fit loop's two
prepares (`designs/skin.md` § the doors), and it is why this door takes a
skeleton at all rather than always reaching for the profile's.

The second door is for a body that already exists: handed a rigged ``.blend``
(``assets-src/blender/<name>.blend``) it strips the skin — vertex groups,
Armature modifier, the parenting — and exports the same shape, so a body can
be re-skinned when ``out/lifts/`` no longer holds its lift. That path re-runs
no gate: the mesh in a rigged .blend has already been through all of them,
and measuring it twice would only say the same numbers in a second place.
The .blend is opened and never saved: it is a committed source.

# The gates, and what each one can actually see

**The T-pose gate measures the body against itself.** ``fitgeom.measure``
puts the shoulder line at the median height of the vertices further out than
0.55 of the body's own half-span — the arm tube and nothing else on a T-pose
— and the arm tips must sit within ``[fit] arm_height_tolerance_m`` of it.
It refuses a *pose*, which is all it was ever able to see. The reach check
that used to sit beside it is gone: it measured a body's span against the
skeleton's wrists, and the fit now moves those wrists to the body, so the
gate was measuring its own input (``profile.py`` refuses a profile that still
names ``reach_min`` or ``reach_max``).

**The sliver check is a note and refuses nothing.** Each limb run's median
cross-section radius, over the run's reference length, is measured, written
into the record, and printed; an arm run under
``[fit] limb_radius_min_fraction`` is named in a ``NOTE`` line. Measured
2026-08-30 on five prepared bodies (see ``fitgeom``'s own table): the body
that walked as a sliver reads 0.200-0.214 across all four arms, and 0.22
separated it cleanly from ``vex_runner`` at 0.239 and ``courier_v2`` at
0.505 — until the fifth body was measured. ``moss_witch_v4`` walks, aims and
rolls on the shipped clips with her upper arms at **0.218 and 0.160**, below
every arm of the sliver: she is thin inside a wide sleeve, and her half-span
of 1.04 m puts the contract's upper-arm run across the sleeve rather than
through the arm. No threshold separates a body that ships from one that does
not, so the number ships as a printed number, exactly as designs/skin.md
said it would if this happened.

The leg runs are measured and printed for the same reason and were never
gated: ``vex_runner``'s thighs read 0.15 against the sliver's own 0.16-0.30.
A T-pose isolates an arm; it does not isolate a leg, and it does not
undress one.

Dust filter, triangle budget, matte register: the numbers come from the
profile's ``[rig]`` and ``[material]`` and are written into the record, so a
body can be judged against the rule it was held to.

The export is ``export.py``'s settings read from the profile's ``[export]``
(``materials``, ``y_up``) through the shared ``_common.export_glb`` — the
same glTF the body pipeline writes — with the armature present and no skin.
Blender exports a bare armature as its bone node hierarchy (the profile's own
``rig.glb`` is exactly that), and this file checks the exported glb holds
every contract bone as a node before it says it is done: if the exporter ever
drops an armature nothing is skinned to, the whole run is skinning to nothing
and would otherwise look fine.
"""

from __future__ import annotations

import argparse
import math
import os
import re
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod  # noqa: E402
from forge_gen import placeholders, profile as profile_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

#: The log prefix; the skills quote these lines.
TAG = "prepare"

#: What the mesh object is called, as everything downstream expects — the
#: convention ``forge_rig::measure`` reports as a mesh node.
OBJECT_NAME = "Body"

#: A library name.
NAME_PATTERN = re.compile(r"^[a-z0-9_]+$")

#: Where the output lands when ``--out`` is not given, under the project.
DEFAULT_OUT_DIR = Path("out") / "prepare"

#: The arm runs the cross-section note is printed for, as ``(start, end)``
#: contract bones. It is a note and not a refusal — see the module doc.
ARM_RUNS = (
    ("LeftArm", "LeftForeArm"),
    ("RightArm", "RightForeArm"),
    ("LeftForeArm", "LeftHand"),
    ("RightForeArm", "RightHand"),
)

#: And the leg runs, measured and printed beside them. Neither set gates:
#: the number does not separate a sliver from a body that ships.
LEG_RUNS = (
    ("LeftUpLeg", "LeftLeg"),
    ("RightUpLeg", "RightLeg"),
    ("LeftLeg", "LeftFoot"),
    ("RightLeg", "RightFoot"),
)


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("source", help="the raw lift .glb, or a rigged .blend to strip")
    parser.add_argument("--out", metavar="GLB", help=f"where to write the glb (default: {DEFAULT_OUT_DIR}/<stem>.glb)")
    parser.add_argument("--record", metavar="JSON", help="where the generator record goes (default: <out>.prepare.json)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument(
        "--skeleton",
        metavar="BLEND|GLB",
        help="the armature to put in the file (default: the profile's rig.blend; the fit loop's second pass passes its own)",
    )
    parser.add_argument("--stature", type=float, metavar="M", help="height to scale the body to (default: the profile's reference stature)")
    parser.add_argument("--yaw-deg", type=float, default=0.0, metavar="DEG", help="turn about +Z so the body faces the rig's front")
    parser.add_argument("--depth-offset", type=float, default=0.0, metavar="M", help="shift the normalized mesh along glTF +Z (forward), in metres; use negative values for a body pushed forward by a backpack")
    parser.add_argument("--budget", type=int, metavar="N", help="triangle ceiling before the decimate kicks in (default: the profile's [rig] tri_budget)")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "prepare",
        help="Lifted glb -> normalised mesh + a skeleton, no weights",
        description=__doc__,
    )
    _add_arguments(parser)


def _load_profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


def _out_path(args, source: Path) -> Path:
    if args.out:
        return Path(args.out).expanduser().resolve()
    root = records.project() or Path.cwd()
    return (Path(root) / DEFAULT_OUT_DIR / f"{source.stem}.glb").resolve()


def _record_path(args, out: Path) -> Path:
    if getattr(args, "record", None):
        return Path(args.record).expanduser().resolve()
    return out.with_suffix(".prepare.json")


def _spec(args, source: Path, out: Path) -> dict:
    """Every number the run is held to, gathered from the profile once.

    ``--name`` is the output's stem: a stem that is not a library name is
    refused here rather than three steps later, when the record is already
    written and the name is in it.
    """
    name = out.stem
    if not NAME_PATTERN.match(name):
        raise UsageError(f"{source.name} would write a stem {name!r} that is not [a-z0-9_]+ — rename the input, or pass --out with a library name")
    profile = _load_profile(args.profile)
    bones = profile.section("bones")
    fit = profile.section("fit")
    rig = profile.section("rig")
    material = profile.section("material")
    export = profile.section("export")
    stature = float(args.stature) if args.stature is not None else float(bones["reference_stature_m"])
    if not math.isfinite(stature) or stature <= 0:
        raise UsageError(f"--stature {stature} is not a height")
    depth_offset = float(getattr(args, "depth_offset", 0.0))
    if not math.isfinite(depth_offset):
        raise UsageError("--depth-offset must be finite")
    if source.suffix.lower() == ".blend" and depth_offset != 0.0:
        raise UsageError("--depth-offset applies to a raw lift, not an already-rigged blend")
    budget = int(args.budget) if args.budget is not None else int(rig["tri_budget"])
    if budget <= 0:
        raise UsageError(f"--budget {budget} is not a triangle count")
    skeleton = Path(args.skeleton).expanduser().resolve() if getattr(args, "skeleton", None) else profile.rig_blend.resolve()
    if not skeleton.is_file():
        raise UsageError(f"--skeleton {skeleton} is not a file — the profile's is {profile.rig_blend}")
    if skeleton.suffix.lower() not in (".blend", ".glb", ".gltf"):
        raise UsageError(f"--skeleton {skeleton.name} is neither a .blend nor a .glb")
    contract_path = profile.dir / str(profile.toml["profile"].get("contract", "contract.json"))
    return {
        "name": name,
        "profile": profile,
        "profile_sha256": records.sha256_file(contract_path),
        "skeleton": skeleton,
        "armature_node": str(bones["armature_node"]),
        "root": str(bones["root"]),
        "stature": stature,
        "yaw_deg": float(args.yaw_deg),
        "depth_offset": depth_offset,
        "budget": budget,
        "arm_height_tolerance": float(fit["arm_height_tolerance_m"]),
        "arm_tip_fraction": float(fit["arm_tip_fraction"]),
        "limb_radius_min_fraction": float(fit["limb_radius_min_fraction"]),
        "decimate_margin": float(rig["decimate_margin"]),
        "dust_diagonal": float(rig["dust_diagonal_m"]),
        "metallic": float(material["metallic"]),
        "roughness": float(material["roughness"]),
        "double_sided": not bool(material.get("backface_culling", False)),
        "materials": str(export.get("materials", "EXPORT")),
        "y_up": bool(export.get("y_up", True)),
    }


def _params(spec: dict) -> dict:
    return {
        "name": spec["name"],
        "profile": spec["profile"].name,
        "profile_sha256": spec["profile_sha256"],
        "skeleton": records.record_path(spec["skeleton"]),
        "stature_m": spec["stature"],
        "yaw_deg": spec["yaw_deg"],
        "depth_offset_m": spec["depth_offset"],
        "tri_budget": spec["budget"],
        "dust_diagonal_m": spec["dust_diagonal"],
        "arm_height_tolerance_m": spec["arm_height_tolerance"],
        "limb_radius_min_fraction": spec["limb_radius_min_fraction"],
    }


def _add_inputs(rec: dict, spec: dict, source: Path) -> None:
    records.add_input(rec, "mesh", source)
    records.add_input(rec, "reference", spec["skeleton"], source=f"profile:{spec['profile'].name}")


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    """Validate under the system python, then run this file under Blender."""
    source = _common.existing_file(args.source, what="mesh")
    if source.suffix.lower() not in (".glb", ".gltf", ".blend"):
        raise UsageError(f"{source.name} is neither a .glb lift nor a rigged .blend")
    out = _out_path(args, source)
    spec = _spec(args, source, out)
    record = _record_path(args, out)
    argv = [
        os.fspath(source),
        "--out", os.fspath(out),
        "--record", os.fspath(record),
        "--profile", os.fspath(spec["profile"].dir),
        "--skeleton", os.fspath(spec["skeleton"]),
        "--stature", str(spec["stature"]),
        f"--yaw-deg={spec['yaw_deg']}",
        f"--depth-offset={spec['depth_offset']}",
        "--budget", str(spec["budget"]),
    ]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


def run_fake(args) -> dict:
    """A placeholder glb with the profile's bone nodes and a boxy body, no weights.

    The same shape a real prepare writes, so ``forge gen skin --fake`` and the
    export gate downstream have something with 55 bone nodes to read, and the
    same record with ``fake: true`` and ``null`` for every measurement —
    nothing here was measured. ``refuse_real`` runs first: ``FORGE_FAKE=1``
    left in a shell must not eat a real prepared mesh.
    """
    source = _common.existing_file(args.source, what="mesh")
    out = _out_path(args, source)
    spec = _spec(args, source, out)
    record_path = _record_path(args, out)
    placeholders.refuse_real(out, record_path)
    placeholders.placeholder_prepared_glb(out, spec["profile"], name=OBJECT_NAME)
    rec = placeholders.fake_record("prepare", _common.TOOL, backend=_common.BACKEND_NAME, created_by=getattr(args, "created_by", None))
    _add_inputs(rec, spec, source)
    rec["params"] = _params(spec)
    rec["measured"] = {
        "vertices": None,
        "triangles": None,
        "dust_islands_dropped": None,
        "shoulder_line_y_m": None,
        "arm_tip_y_m": None,
        "limb_radius": None,
    }
    records.add_output(rec, out)
    records.write(rec, record_path)
    return {"record": os.fspath(record_path), "outputs": [os.fspath(out)], "measured": rec["measured"]}


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    """The Blender half: build the scene one of two ways, then export, check and record."""
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen prepare (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    source = Path(args.source).resolve()
    out = _out_path(args, source)
    spec = _spec(args, source, out)
    record_path = _record_path(args, out)
    profile = spec["profile"]

    if source.suffix.lower() == ".blend":
        body, armature, measured = _from_rigged_blend(source, spec)
    else:
        body, armature, measured = _from_lift(source, spec)

    _refuse_any_skin(body, armature)
    packed = _common.pack_images()
    if packed:
        _common.log(TAG, f"packed {packed} image(s)")

    bpy.ops.object.select_all(action="DESELECT")
    _common.export_glb(out, materials=spec["materials"], y_up=spec["y_up"])
    _common.log(TAG, f"wrote {out}")
    try:
        info = glb_mod.verify_glb(out)
    except glb_mod.GlbError as err:
        raise BackendFailed(f"the exported file failed its own check: {err}") from err
    _common.log(TAG, glb_mod.report(out, info))
    bones = _check_armature_survived(info, profile)

    measured.update(
        {
            "bones_in_glb": bones,
            "nodes": info["nodes"],
            "meshes": info["meshes"],
            "images": info["images"],
            "skins": info["skins"],
            "blender_version": _common.blender_identity()["version"],
        }
    )
    _common.log(
        TAG,
        f"{measured['vertices']} verts, {measured['triangles']} tris, {bones} contract bone(s) as nodes, "
        f"0 vertex groups — ready for `forge gen skin {out.name}`",
    )

    rec = _common.new_record("prepare", created_by=args.created_by)
    _add_inputs(rec, spec, source)
    rec["params"] = _params(spec)
    rec["measured"] = measured
    _common.finish_record(rec, outputs=[out], record_path=record_path)
    return _common.success(record_path, [out], measured=measured)


def _from_lift(source: Path, spec: dict) -> tuple:
    """The raw-lift door: open the skeleton, import, normalise, gate, clean."""
    profile = spec["profile"]
    armature = _open_skeleton(spec)
    if spec["root"] not in armature.data.bones:
        raise BackendFailed(f"{spec['skeleton']} has no bone {spec['root']!r} — that skeleton is not this profile's")

    body = _common.import_single_mesh(source, name=OBJECT_NAME, tag=TAG)
    _normalize(body, armature, spec)
    fit = _skeleton_fit_or_die(body, spec)
    limbs = _limb_radius(body, spec)
    tris, dust = _cleanup_and_budget(body, spec)
    _common.matte(body, metallic=spec["metallic"], roughness=spec["roughness"], double_sided=spec["double_sided"])
    measured = {
        "vertices": len(body.data.vertices),
        "triangles": tris,
        "dust_islands_dropped": dust,
        "shoulder_line_y_m": fit["shoulder_line_y_m"],
        "arm_tip_y_m": fit["arm_tip_y_m"],
        "half_span_m": fit["half_span_m"],
        "crotch_y_m": fit["crotch_y_m"],
        "heads": fit["heads"],
        "limb_radius": limbs,
        "profile_bones": len(profile.bones),
        "source": "lift",
    }
    return body, armature, measured


def _open_skeleton(spec: dict):
    """The armature ``--skeleton`` names, as the only armature in a fresh scene.

    A ``.blend`` is opened, which is how the profile's own ``rig.blend``
    arrives: it *is* the contract's source and rests in the contract pose to
    0.1 mm, not a reconstruction of it. A ``.glb`` is imported into an empty
    file with ``keep_bone_directions``, which is how the fit loop's second
    prepare gets the per-body armature the fit just built — the same bytes
    ``forge gen export`` will later measure against.
    """
    import bpy

    skeleton = spec["skeleton"]
    node = spec["armature_node"]
    if skeleton.suffix.lower() == ".blend":
        bpy.ops.wm.open_mainfile(filepath=os.fspath(skeleton))
        armature = bpy.data.objects.get(node)
        if armature is None or armature.type != "ARMATURE":
            raise BackendFailed(f"{skeleton} holds no {node!r} armature object — the profile is broken")
        return armature

    bpy.ops.wm.read_homefile(use_empty=True)
    imported = _common.import_gltf(skeleton, keep_bone_directions=True)
    armatures = [obj for obj in imported if obj.type == "ARMATURE"]
    if len(armatures) != 1:
        found = ", ".join(obj.name for obj in armatures) or "none"
        raise BackendFailed(f"{skeleton} imported {len(armatures)} armature object(s) ({found}) — a skeleton file holds exactly one")
    armature = armatures[0]
    for obj in imported:
        if obj is not armature:
            bpy.data.objects.remove(obj, do_unlink=True)
    armature.name = node
    armature.data.name = node
    _common.log(TAG, f"skeleton from {skeleton.name}: {len(armature.data.bones)} bones as {node!r}")
    return armature


def _from_rigged_blend(source: Path, spec: dict) -> tuple:
    """The already-rigged door: open it, strip the skin, keep the armature as a sibling.

    No gate is re-run and nothing is re-normalised: this mesh went through
    all of them when it was rigged, and measuring it twice would only say
    the same thing in a second place. What changes is the skin, which is
    exactly what the skinner is being asked to produce. ``--skeleton`` is
    not read here: the armature that comes with the body is the one the body
    is already shaped for.
    """
    import bpy

    bpy.ops.wm.open_mainfile(filepath=os.fspath(source))
    armature = bpy.data.objects.get(spec["armature_node"])
    if armature is None or armature.type != "ARMATURE":
        found = ", ".join(o.name for o in bpy.context.scene.objects if o.type == "ARMATURE") or "none"
        raise InputRejected(f"{source.name} holds no {spec['armature_node']!r} armature object (armatures: {found})")
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        raise InputRejected(f"{source.name} holds no mesh object")

    stripped = 0
    for obj in meshes:
        world = obj.matrix_world.copy()
        obj.parent = None
        obj.matrix_world = world
        for modifier in [m for m in obj.modifiers if m.type == "ARMATURE"]:
            obj.modifiers.remove(modifier)
        stripped += len(obj.vertex_groups)
        obj.vertex_groups.clear()

    bpy.ops.object.select_all(action="DESELECT")
    for obj in meshes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
        _common.log(TAG, f"joined {len(meshes)} mesh objects into one")
    body = bpy.context.view_layer.objects.active
    body.name = OBJECT_NAME
    body.data.name = OBJECT_NAME
    _common.log(TAG, f"stripped {stripped} vertex group(s) and the Armature modifier from {body.name}")
    measured = {
        "vertices": len(body.data.vertices),
        "triangles": _common.triangle_count(body),
        "dust_islands_dropped": None,
        "vertex_groups_stripped": stripped,
        "shoulder_line_y_m": None,
        "arm_tip_y_m": None,
        "half_span_m": None,
        "crotch_y_m": None,
        "heads": None,
        "limb_radius": None,
        "source": "rigged_blend",
    }
    return body, armature, measured


def _refuse_any_skin(body, armature) -> None:
    """The one invariant of this step: the mesh arrives at the skinner bare."""
    if body.vertex_groups:
        names = ", ".join(group.name for group in body.vertex_groups[:6])
        raise BackendFailed(f"{body.name} still carries {len(body.vertex_groups)} vertex group(s) ({names}) — the skin is what SkinTokens is being asked for")
    if any(modifier.type == "ARMATURE" for modifier in body.modifiers):
        raise BackendFailed(f"{body.name} still carries an Armature modifier — the mesh must reach the skinner unbound")
    if body.parent is armature:
        raise BackendFailed(f"{body.name} is still parented to {armature.name} — the armature goes in as a sibling")


def _check_armature_survived(info: dict, profile) -> int:
    """Every contract bone must be a node in the exported glb.

    Checked because the failure it catches is invisible: an exporter that
    drops an armature nothing is skinned to would leave a mesh-only glb,
    SkinTokens' ``--use_skeleton`` would find no armature and predict its own
    skeleton, and the run would report a fine-looking rig on the wrong bones.
    """
    names = {node.get("name") for node in info["document"].get("nodes", [])}
    wanted = [bone["name"] for bone in profile.bones]
    missing = [name for name in wanted if name not in names]
    if missing:
        shown = ", ".join(missing[:8]) + (" ..." if len(missing) > 8 else "")
        raise BackendFailed(
            f"the exported glb is missing {len(missing)} of {len(wanted)} contract bone(s) as nodes ({shown}) — "
            "Blender dropped the armature nothing is skinned to, and SkinTokens would have predicted its own skeleton"
        )
    return len(wanted)


# ------------------------------------------------------------ the geometry --


def _points(body):
    """The body's vertices as an ``(N, 3)`` array in the glTF frame.

    ``foreach_get`` because the alternative is a python loop over sixty
    thousand vertices per station, and ``fitgeom.from_blender`` because
    everything the gates state is stated the way the contract is: ``+Y`` up.
    """
    import numpy as np

    from forge_gen import fitgeom

    flat = np.empty(len(body.data.vertices) * 3, dtype=np.float64)
    body.data.vertices.foreach_get("co", flat)
    return fitgeom.from_blender(flat.reshape(-1, 3))


def _normalize(body, armature, spec: dict) -> None:
    """Yaw to face the rig's front, scale to stature, feet to Z=0, centred on the
    skeleton's ground position. All object transforms applied — the export
    gate refuses anything else."""
    from mathutils import Vector

    _common.yaw(body, spec["yaw_deg"])
    _common.apply_transforms(body)

    lo, hi = _common.bounds(body)
    height = hi.z - lo.z
    if height < 1e-6:
        raise InputRejected("the mesh has no height")
    scale = spec["stature"] / height
    body.scale = (scale, scale, scale)

    lo = lo * scale
    hi = hi * scale
    # Align the bounds centre with the rig's mirror plane and root depth.
    # Protruding gear may need a recorded correction; glTF +Z is Blender -Y.
    root_y = armature.data.bones[spec["root"]].head_local.y
    centre = (lo + hi) / 2.0
    body.location = Vector((-centre.x, root_y - centre.y - spec["depth_offset"], -lo.z))
    _common.apply_transforms(body)


def _skeleton_fit_or_die(body, spec: dict) -> dict:
    """Refuse a mesh that is not in the rest pose, measured against the body itself.

    The shoulder line is the median height of the body's own outboard
    geometry and the arm tips are the same vertices seen as an end rather
    than a start; the gap between them is how far the arms droop or lift out
    of horizontal. Nothing here reads the skeleton, which is the point: the
    fit is about to give the skeleton this body's lengths, so a gate that
    measured the body against the skeleton's wrists was measuring its own
    output. The message names the real fix, which is the reference image.
    """
    from forge_gen import fitgeom

    geometry = fitgeom.measure(_points(body), arm_tip_fraction=spec["arm_tip_fraction"])
    shoulder = geometry["shoulder_y"]
    if shoulder is None:
        raise InputRejected(
            "no vertex sits outboard of the body's own half-span, so it has no shoulder line and no arms to measure — "
            "this is not a character in a T-pose"
        )
    tip_y = geometry["arm_tip_y"]

    _common.log(
        TAG,
        f"fit — arm tips at y {tip_y:.3f} m against this body's own shoulder line at {shoulder:.3f} m "
        f"(half-span {geometry['half_span']:.3f} m, {geometry['arm_tube_vertices']} arm-tube vertices, {geometry['heads']} heads)",
    )
    fit = {
        "shoulder_line_y_m": round(float(shoulder), 4),
        "arm_tip_y_m": round(float(tip_y), 4),
        "half_span_m": geometry["half_span"],
        "crotch_y_m": geometry["crotch_y"],
        "heads": geometry["heads"],
    }
    gap = abs(tip_y - shoulder)
    if gap > spec["arm_height_tolerance"]:
        raise InputRejected(
            f"{spec['name']}'s arm tips sit {gap:.2f} m from its own shoulder line (tips at y={tip_y:.2f} m, "
            f"shoulders at y={shoulder:.2f} m, measured from the arm tube's own geometry), past "
            f"[fit] arm_height_tolerance_m {spec['arm_height_tolerance']:.2f}. The arms are not horizontal in the "
            "reference image. Redraw with the arms straight out, palms down; a weight cannot fix a pose.",
            fit=fit,
        )
    return fit


def _limb_radius(body, spec: dict) -> dict:
    """Every limb run's cross-section, measured, recorded, and printed as a note.

    **A note, not a refusal.** ``[fit] limb_radius_min_fraction`` was
    calibrated at 0.22 on four bodies and then measured on a fifth — see the
    module doc — and the fifth is ``moss_witch_v4``, who walks, aims and
    rolls on the shipped clips and whose upper arms read 0.218 and 0.160,
    below every arm of the body that walked as a sliver. So there is no
    threshold that separates them, and a gate nobody can calibrate ships as
    a printed number rather than as a refusal.

    The number is still worth having: it is in the record for every run,
    it names the sliver correctly on the bodies where nothing else does, and
    the note is what a reviewer reads before spending a card on a skin. The
    off-axis distance travels beside it, because a limb can be thin for two
    reasons and only one of them is thinness.
    """
    from forge_gen import fitgeom

    points = _points(body)
    world = spec["profile"].rest_world()
    out: dict[str, dict] = {}
    thin: list[str] = []
    floor = spec["limb_radius_min_fraction"]
    for kind, runs in (("arm", ARM_RUNS), ("leg", LEG_RUNS)):
        for start, end in runs:
            if start not in world or end not in world:
                continue
            section = fitgeom.limb_sections(points, world[start][0], world[end][0])
            out[f"{start}->{end}"] = {
                "kind": kind,
                "ratio": section["ratio"],
                "radius_m": section["radius_m"],
                "reference_length_m": section["reference_length_m"],
                "off_axis_m": section["off_axis_m"],
                "stations": [station["ratio"] for station in section["stations"]],
                "gated": False,
            }
            ratio = section["ratio"]
            if kind == "arm" and ratio is not None and ratio < floor:
                thin.append(
                    f"{start}->{end} measures {ratio:.2f} of its run across "
                    f"({section['radius_m'] * 100.0:.1f} cm through a {section['reference_length_m'] * 100.0:.1f} cm bone)"
                )
    printed = ", ".join(f"{run} {row['ratio'] if row['ratio'] is not None else 'null'}" for run, row in out.items())
    _common.log(TAG, f"limb cross-sections (a note; nothing here refuses) — {printed}")
    if thin:
        _common.log(
            TAG,
            f"NOTE {spec['name']} has arm runs under [fit] limb_radius_min_fraction {floor:.2f}: {'; '.join(thin)}. "
            "A limb thinner than the bone it hangs on can animate as a sliver — look at the seven views before "
            "spending a card on it. This is the reference and not the lift: a posterized picture with 25-pixel "
            "shins gives the lifter no shading to lift volume from. It is a note and not a refusal because "
            "moss_witch_v4, who walks, measures 0.22 and 0.16 here and ships.",
        )
    return out


def _cleanup_and_budget(body, spec: dict) -> tuple[int, int]:
    """Merge, dissolve degenerates, drop dust, decimate to budget. Returns ``(tris, dust islands dropped)``."""
    import bmesh
    import bpy

    mesh = bmesh.new()
    mesh.from_mesh(body.data)
    bmesh.ops.remove_doubles(mesh, verts=mesh.verts[:], dist=_common.MERGE_DISTANCE_M)
    # Degenerate faces make a weight solve singular, and TRELLIS meshes carry
    # a spatter of tiny disconnected shells ("voxel dust"). Dust is judged by
    # PHYSICAL size, never face count: this runs after _normalize, so extents
    # are metres, and a centimetre-scale speck is invisible at any camera this
    # register plays at — while a disconnected mohawk spike or back-of-skull
    # patch is real geometry no matter how few faces it holds. (A face-count
    # filter here once deleted the back of a head; the player noticed.)
    bmesh.ops.dissolve_degenerate(mesh, dist=1e-6, edges=mesh.edges[:])
    seen = set()
    islands = []
    for face in mesh.faces:
        if face.index in seen:
            continue
        stack, island = [face], []
        while stack:
            candidate = stack.pop()
            if candidate.index in seen:
                continue
            seen.add(candidate.index)
            island.append(candidate)
            for edge in candidate.edges:
                stack.extend(linked for linked in edge.link_faces if linked.index not in seen)
        islands.append(island)

    def diagonal(island) -> float:
        coords = [vert.co for face in island for vert in face.verts]
        spans = (max(c[axis] for c in coords) - min(c[axis] for c in coords) for axis in range(3))
        return sum(span * span for span in spans) ** 0.5

    dust_diagonal = spec["dust_diagonal"]
    doomed_islands = [island for island in islands if diagonal(island) < dust_diagonal]
    if doomed_islands:
        sizes = sorted((len(island) for island in doomed_islands), reverse=True)
        _common.log(
            TAG,
            f"dropped {len(sizes)} dust island(s) of {sizes} face(s), each under {dust_diagonal * 100:.1f} cm across",
        )
        doomed = [face for island in doomed_islands for face in island]
        bmesh.ops.delete(mesh, geom=doomed, context="FACES")
    loose = [v for v in mesh.verts if not v.link_faces]
    if loose:
        bmesh.ops.delete(mesh, geom=loose, context="VERTS")
    # No normal recalculation, deliberately. TRELLIS's normals come out of its
    # surface field and are consistently outward; recalc is only reliable on
    # closed shells, and on the OPEN patches this mesh is full of (hair
    # spikes, skull plates) it can flip a whole patch inward — which under
    # backface culling renders as a hole in the back of the head.
    mesh.to_mesh(body.data)
    mesh.free()
    body.data.update()

    if not body.data.polygons:
        raise InputRejected("nothing is left of the mesh after cleanup — every island was dust")

    tris = _common.triangle_count(body)
    budget = spec["budget"]
    if tris > budget:
        ratio = budget * spec["decimate_margin"] / tris
        modifier = body.modifiers.new("budget", "DECIMATE")
        modifier.ratio = ratio
        bpy.context.view_layer.objects.active = body
        bpy.ops.object.modifier_apply(modifier=modifier.name)
        after = _common.triangle_count(body)
        _common.log(TAG, f"decimated {tris} -> {after} tris for the {budget} budget")
        tris = after
    return tris, len(doomed_islands)


# ---------------------------------------------------------------- both ends --


def _main_outer(argv: list[str]) -> int:
    """Run under the system python when this file is executed directly.

    ``forge gen prepare`` goes through ``cli.py`` and gets the exit-code table
    and the ``--json`` contract for free; this half is what makes
    ``python3 python/forge_gen/blender/prepare.py <glb>`` behave the same way
    for anyone reading the file rather than the command tree.
    """
    from forge_gen import cli
    from forge_gen.exit_codes import ForgeGenError

    parser = argparse.ArgumentParser(prog="prepare", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    _add_arguments(parser)
    parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
    parser.add_argument("--fake", action="store_true", help="write a placeholder through the same doors")
    parser.add_argument("--project", default=None, metavar="DIR", help="the project root paths are written relative to")
    parser.add_argument("--created-by", default=None, metavar="WHO", help="human | agent:<name> | unknown")
    args = parser.parse_args(argv)
    if args.project:
        records.set_project(args.project)
    try:
        result = run_fake(args) if placeholders.requested(args) else run(args)
    except ForgeGenError as err:
        cli.emit(err.payload(), as_json=args.json)
        sys.stderr.write(f"{TAG}: {err.error}: {err.message}\n")
        return err.code
    result.setdefault("ok", True)
    cli.emit(result, as_json=args.json)
    return 0


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        sys.exit(_main_outer(sys.argv[1:]))
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
