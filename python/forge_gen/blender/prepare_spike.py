"""Prepare a mesh for SkinTokens: normalised geometry plus the profile's armature, no weights.

    python3 python/forge_gen/blender/prepare_spike.py <lift.glb|rigged.blend>
            [--out out/prepare/<stem>.glb] [--profile DIR] [--stature 1.80]
            [--yaw-deg 0] [--budget N] [--project DIR]

**A Phase 0 spike, not a door.** This is the prototype of the plan's ``forge
gen prepare`` (designs/forge2.md, Phase 2) and is deliberately *not*
registered in ``cli.py``: nothing promotes through it, nothing in ``just ci``
runs it, and it writes no record — the record for a skinned body is written
by the step that skins, ``forge_gen.spike_skin``, which hashes this file's
output as its ``mesh`` input. When the real ``forge gen prepare`` lands it
takes this file's shape, gains a record and a ``--fake`` half, and this one
goes.

What it makes is the input SkinTokens' ``--use_skeleton`` wants: one glb
holding

* the mesh, normalised exactly as ``forge gen rig`` normalises it — yawed to
  the rig's front, scaled to stature, feet at Z=0, centred on the skeleton,
  through the same T-pose fit gate, the same dust filter and the same
  triangle budget (this file calls ``rig.py``'s own ``_normalize``,
  ``_skeleton_fit_or_die`` and ``_cleanup_and_budget``, so the two cannot
  drift), wearing its texture at the profile's matte register;
* the profile's armature, opened from ``rigs/<name>/rig.blend`` so it is the
  contract's own source and rests in the contract pose to 0.1 mm — as a
  **sibling** object of the mesh, not its parent;
* **no vertex groups and no Armature modifier**, because the weights are
  what SkinTokens is being asked for. A file that arrives carrying weights
  would let a spike "succeed" on weights nobody generated.

The second door is for the spike itself: handed a rigged ``.blend``
(``assets-src/blender/vex_runner.blend``) it strips the skin — vertex
groups, Armature modifier, the parenting — and exports the same shape, so
the spike can run on a body that already exists when ``out/lifts/`` is
empty. That path re-runs no gate: the mesh in a rigged .blend has already
been through all of them, and running the fit gate on it again would only
measure the same numbers. The .blend is opened and never saved: it is a
committed source.

The export is ``export.py``'s settings read from the profile's ``[export]``
(``materials``, ``y_up``) through the shared ``_common.export_glb`` — the
same glTF the body pipeline writes — with the armature present and no skin.
Blender exports a bare armature as its bone node hierarchy (the profile's
own ``rig.glb`` is exactly that), and this file checks the exported glb
holds every contract bone as a node before it says it is done: if the
exporter ever drops an armature nothing is skinned to, the whole spike is
skinning to nothing and would otherwise look fine.
"""

from __future__ import annotations

import argparse
import os
import sys
from argparse import Namespace
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod  # noqa: E402
from forge_gen import records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.blender import rig as rig_mod  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

#: The log prefix; the spike notes quote these lines.
TAG = "prepare"

#: What the mesh object is called, as everything downstream expects.
OBJECT_NAME = rig_mod.OBJECT_NAME

#: Where the output lands when ``--out`` is not given, under the project.
DEFAULT_OUT_DIR = Path("out") / "prepare"


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("source", help="the raw lift .glb, or a rigged .blend to strip")
    parser.add_argument("--out", metavar="GLB", help=f"where to write the glb (default: {DEFAULT_OUT_DIR}/<stem>.glb)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument("--stature", type=float, metavar="M", help="height to scale the body to (default: the profile's reference stature)")
    parser.add_argument("--yaw-deg", type=float, default=0.0, metavar="DEG", help="turn about +Z so the body faces the rig's front")
    parser.add_argument("--budget", type=int, metavar="N", help="triangle ceiling before the decimate kicks in (default: the profile's [rig] tri_budget)")


def _out_path(args, source: Path) -> Path:
    if args.out:
        return Path(args.out).expanduser().resolve()
    root = records.project() or Path.cwd()
    return (Path(root) / DEFAULT_OUT_DIR / f"{source.stem}.glb").resolve()


def _spec(args, source: Path, out: Path) -> dict:
    """Every number the run is held to, from ``rig.py``'s own reader.

    Built through ``rig._spec`` on purpose: the fit gate, the budget, the
    dust diagonal and the matte must be the numbers ``forge gen rig`` uses,
    and a second reader of the same profile is the shape of every silent
    mismatch this repository's ledger records. ``--name`` is the output's
    stem because the record this step does not write would want one; a stem
    that is not a library name is refused here rather than three steps
    later.
    """
    name = source.stem
    if not rig_mod.NAME_PATTERN.match(name):
        raise UsageError(f"{source.name} has a stem {name!r} that is not [a-z0-9_]+ — rename the input, or pass --out with a library name")
    spec = rig_mod._spec(
        Namespace(
            glb=os.fspath(source),
            out=os.fspath(out),
            record=os.fspath(out.with_suffix(".unwritten.json")),
            name=name,
            prompt=None,
            profile=args.profile,
            stature=args.stature,
            yaw_deg=args.yaw_deg,
            budget=args.budget,
        )
    )
    export = spec["profile"].section("export")
    spec["materials"] = str(export.get("materials", "EXPORT"))
    spec["y_up"] = bool(export.get("y_up", True))
    return spec


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    """Validate under the system python, then run this file under Blender."""
    source = _common.existing_file(args.source, what="mesh")
    if source.suffix.lower() not in (".glb", ".gltf", ".blend"):
        raise UsageError(f"{source.name} is neither a .glb lift nor a rigged .blend")
    out = _out_path(args, source)
    spec = _spec(args, source, out)
    argv = [
        os.fspath(source),
        "--out", os.fspath(out),
        "--profile", os.fspath(spec["profile"].dir),
        "--stature", str(spec["stature"]),
        f"--yaw-deg={spec['yaw_deg']}",
        "--budget", str(spec["budget"]),
    ]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    """The Blender half: build the scene one of two ways, then export and check."""
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen prepare (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    source = Path(args.source).resolve()
    out = _out_path(args, source)
    spec = _spec(args, source, out)
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
        f"0 vertex groups — ready for `forge_gen.spike_skin {out.name}`",
    )
    return _common.success(None, [out], measured=measured, fit=measured.get("fit"))


def _from_lift(source: Path, spec: dict) -> tuple:
    """The raw-lift door: open the profile's rig, import, normalise, gate, clean."""
    import bpy

    profile = spec["profile"]
    bpy.ops.wm.open_mainfile(filepath=os.fspath(profile.rig_blend.resolve()))
    armature = bpy.data.objects.get(spec["armature_node"])
    if armature is None or armature.type != "ARMATURE":
        raise BackendFailed(f"{profile.rig_blend} holds no {spec['armature_node']!r} armature object — the profile is broken")
    for bone_name in (spec["root"], spec["wrist"]):
        if bone_name not in armature.data.bones:
            raise BackendFailed(f"{profile.rig_blend} has no bone {bone_name!r} — the profile is broken")

    body = _common.import_single_mesh(source, name=OBJECT_NAME, tag=TAG)
    rig_mod._normalize(body, armature, spec)
    fit = rig_mod._skeleton_fit_or_die(body, armature, spec)
    tris, dust = rig_mod._cleanup_and_budget(body, spec)
    _common.matte(body, metallic=spec["metallic"], roughness=spec["roughness"], double_sided=spec["double_sided"])
    measured = {
        "vertices": len(body.data.vertices),
        "triangles": tris,
        "dust_islands_dropped": dust,
        "fit": fit,
        "source": "lift",
    }
    return body, armature, measured


def _from_rigged_blend(source: Path, spec: dict) -> tuple:
    """The already-rigged door: open it, strip the skin, keep the armature as a sibling.

    No gate is re-run and nothing is re-normalised: this mesh went through
    all of them when it was rigged, and measuring it twice would only say
    the same thing in a second place. What changes is the skin, which is
    exactly what the spike is asking SkinTokens to produce.
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
        "vertex_groups_stripped": stripped,
        "fit": None,
        "source": "rigged_blend",
    }
    return body, armature, measured


def _refuse_any_skin(body, armature) -> None:
    """The one invariant of this step: the mesh arrives at SkinTokens bare."""
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
    skeleton, and the spike would report a fine-looking rig on the wrong
    bones.
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


# ---------------------------------------------------------------- both ends --


def _main_outer(argv: list[str]) -> int:
    """Run under the system python: validate, hand the file to Blender, summarise.

    The same contract every ``forge gen`` command keeps — the exit-code
    table, one JSON object under ``--json`` — spelled out here because this
    step is not registered in ``cli.py`` and so does not get it for free.
    """
    from forge_gen import cli
    from forge_gen.exit_codes import ForgeGenError

    parser = argparse.ArgumentParser(prog="prepare_spike", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    _add_arguments(parser)
    parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
    parser.add_argument("--project", default=None, metavar="DIR", help="the project root paths are written relative to")
    parser.add_argument("--created-by", default=None, metavar="WHO", help="human | agent:<name> | unknown")
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
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        sys.exit(_main_outer(sys.argv[1:]))
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
