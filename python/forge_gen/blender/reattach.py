"""Put SkinTokens' weights back on the profile's own armature, by joint order.

    python3 python/forge_gen/blender/reattach.py \
            out/skin/<name>.skinned.glb --handed-in out/prepare/<name>.glb \
            [--armature out/skin/<name>/skeleton.blend]
            [--out assets-src/blender/<name>.blend] [--profile DIR] [--json]

**A library, called by ``forge gen skin``** as its last step, not a
subcommand. What comes back from ``demo.py --use_skeleton`` is weights for
our skeleton with the *names* gone (``bone_0 … bone_54``) and every joint
moved — 7.6 mm on average against a contract tolerance of 0.1 mm, because
the checkpoint tokenises joint positions on a 256-level grid. A body with
those bones binds nothing: ``forge rig check`` reports 0 driven, 27 orphaned,
and the character holds its rest pose through every clip in the library.

The two glbs agree on joint *order* — same count, identical parent array —
which is what ``skin.py`` calls ``by_order`` alignment. So: take the
per-vertex joint indices exactly as they came back, read the name for index
*i* off the skin that went in, and bind the mesh to an **untouched**
armature — the fitted skeleton ``fit_rig.py`` built for this body when
``--armature`` names one, and the profile's own ``rig.blend`` otherwise. The
returned skeleton is discarded whole; nothing about it is repaired, averaged
or nudged. What ships out is a rest pose whose *rotations* are the contract's,
which is what every clip in the library was baked against.

It goes out as a ``.blend`` on purpose, so the result is judged by the real
gate — ``forge gen export`` measures every bone's rest translation against
the contract's *direction* and holds its length inside the fit's own band,
and ``forge rig check`` then plays the walk on it. A door that wrote its own
glb would be a second exporter with its own opinions.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

TAG = "reattach"
OBJECT_NAME = "Body"
DEFAULT_OUT_DIR = "out/skin"


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("skinned", help="the glb SkinTokens returned")
    parser.add_argument("--handed-in", required=True, metavar="GLB", help="the prepared glb that went in — its skin holds the names")
    parser.add_argument("--out", metavar="BLEND", help=f"where the rebound .blend goes (default: {DEFAULT_OUT_DIR}/<stem>_skintokens.blend)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument(
        "--armature",
        metavar="BLEND",
        help="the .blend holding the armature to bind to (default: the profile's rig.blend; the fit loop passes its fitted skeleton)",
    )
    parser.add_argument("--map", metavar="JSON", help="(inner) the joint-order map, already computed")


# ------------------------------------------------------------------ shared --


def _skin_joint_names(path: Path) -> list[str]:
    """The names of ``skins[0].joints``, in order — the only ordering both files agree on."""
    document = _document(path)
    skins = document.get("skins") or []
    if not skins:
        raise InputRejected(f"{path.name} has no skin — there is no joint order to read")
    nodes = document.get("nodes", [])
    return [nodes[index].get("name", f"<node {index}>") for index in skins[0]["joints"]]


def _document(path: Path) -> dict:
    data = path.read_bytes()
    if data[:4] != b"glTF":
        raise InputRejected(f"{path.name} is not a binary glTF")
    offset, document = 12, None
    while offset < len(data):
        length, kind = struct.unpack_from("<II", data, offset)
        offset += 8
        if kind == 0x4E4F534A:
            document = json.loads(data[offset : offset + length])
        offset += length
    if document is None:
        raise InputRejected(f"{path.name} has no JSON chunk")
    return document


def _armature_blend(args, profile) -> Path:
    """The .blend the weights are bound onto — ``--armature``, else the profile's own.

    Two callers on purpose: the outer half resolves it so a missing file is
    refused before Blender starts, and the inner half resolves it again from
    the same flag rather than being handed a second opinion.
    """
    named = getattr(args, "armature", None)
    if not named:
        return profile.rig_blend.resolve()
    path = Path(named).expanduser().resolve()
    if not path.is_file():
        raise UsageError(f"--armature {path} is not a file — `fit_rig` writes the fitted skeleton, or leave it out for the profile's")
    return path


def _out_path(args, source: Path) -> Path:
    if args.out:
        return Path(args.out).expanduser().resolve()
    root = records.project() or Path.cwd()
    stem = source.stem.split(".")[0]
    return (Path(root) / DEFAULT_OUT_DIR / f"{stem}_skintokens.blend").resolve()


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    skinned = _common.existing_file(args.skinned, what="skinned mesh")
    handed_in = _common.existing_file(args.handed_in, what="prepared mesh")
    returned = _skin_joint_names(skinned)
    original = _skin_joint_names(handed_in)
    if len(returned) != len(original):
        raise InputRejected(
            f"{skinned.name} came back with {len(returned)} joint(s) and {handed_in.name} handed in {len(original)} — "
            "the by-order alignment skin.py found does not hold, and there is no honest map by index"
        )
    mapping = dict(zip(returned, original))
    if len(mapping) != len(returned):
        raise InputRejected(f"{skinned.name} repeats a joint name — the map by index would collide")

    out = _out_path(args, skinned)
    out.parent.mkdir(parents=True, exist_ok=True)
    map_path = out.with_suffix(".map.json")
    map_path.write_text(json.dumps({"returned": returned, "handed_in": original}, indent=2) + "\n", encoding="utf-8")

    from forge_gen import profile as profile_mod

    profile = profile_mod.load_profile(args.profile)
    armature_blend = _armature_blend(args, profile)
    argv = [
        os.fspath(skinned),
        "--handed-in", os.fspath(handed_in),
        "--out", os.fspath(out),
        "--profile", os.fspath(profile.dir),
        "--armature", os.fspath(armature_blend),
        "--map", os.fspath(map_path),
    ]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen reattach (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)

    from forge_gen import profile as profile_mod

    profile = profile_mod.load_profile(args.profile)
    armature_node = str(profile.section("export").get("armature_node", "Armature"))
    pairs = json.loads(Path(args.map).read_text(encoding="utf-8"))
    mapping = dict(zip(pairs["returned"], pairs["handed_in"]))

    armature_blend = _armature_blend(args, profile)
    bpy.ops.wm.open_mainfile(filepath=os.fspath(armature_blend))
    armature = bpy.data.objects.get(armature_node)
    if armature is None or armature.type != "ARMATURE":
        raise BackendFailed(f"{armature_blend} holds no {armature_node!r} armature — the file the weights are being bound to is broken")

    before = set(bpy.data.objects)
    _common.import_gltf(args.skinned)
    imported = [obj for obj in bpy.data.objects if obj not in before]
    in_scene = set(bpy.context.scene.objects)
    meshes = [obj for obj in imported if obj.type == "MESH" and obj in in_scene]
    if len(meshes) != 1:
        raise InputRejected(f"{Path(args.skinned).name} imported {len(meshes)} mesh object(s) — the re-attach expects exactly one")
    body = meshes[0]

    groups = [group.name for group in body.vertex_groups]
    if not groups:
        raise BackendFailed(f"{Path(args.skinned).name} imported with no vertex groups — there is no skin to re-attach")
    stray = [name for name in groups if name not in mapping]
    if stray:
        raise BackendFailed(
            f"{len(stray)} vertex group(s) name a joint the input never had ({', '.join(stray[:6])}) — "
            "the by-order map is not the map this file wants"
        )
    # Two passes through a scratch prefix: renaming Hips -> Hips while a
    # bone_0 -> Hips is still pending would collide and Blender would silently
    # append .001, which is the kind of defect that survives to a screenshot.
    # The prefix has to be a name Blender will actually store — a NUL is
    # treated as a terminator and the group comes back called "Group".
    scratch = "reattach__"
    for group in body.vertex_groups:
        group.name = scratch + mapping[group.name]
    for group in body.vertex_groups:
        if not group.name.startswith(scratch):
            raise BackendFailed(f"Blender did not keep the scratch name for {group.name!r} — the two-pass rename cannot be trusted")
        group.name = group.name[len(scratch):]
    renamed = [group.name for group in body.vertex_groups]
    collided = [name for name in renamed if name not in set(mapping.values())]
    if collided:
        raise BackendFailed(f"renaming collided: {', '.join(collided[:6])}")
    _common.log(TAG, f"renamed {len(renamed)} vertex group(s) by joint order — bone_0 -> {mapping[groups[0]] if groups[0] in mapping else '?'}")

    contract = {bone["name"] for bone in profile.bones}
    outside = sorted(set(renamed) - contract)
    if outside:
        raise BackendFailed(f"{len(outside)} group(s) outside the contract after the map: {', '.join(outside[:6])}")

    # The returned armature is discarded whole: it is the thing the skin
    # step found unusable, and keeping any of it would put a quantised rest
    # pose into a file the export gate measures against the contract.
    world = body.matrix_world.copy()
    body.parent = None
    body.matrix_world = world
    for modifier in [m for m in body.modifiers if m.type == "ARMATURE"]:
        body.modifiers.remove(modifier)
    for obj in imported:
        if obj is not body:
            bpy.data.objects.remove(obj, do_unlink=True)
    for block in list(bpy.data.armatures):
        if block.users == 0:
            bpy.data.armatures.remove(block)

    if not _is_identity(body.matrix_world):
        _common.log(TAG, f"applying the importer's axis conversion: {tuple(round(v, 4) for v in body.matrix_world.to_euler())}")
        _common.apply_transforms(body)
    body.name = OBJECT_NAME
    body.data.name = OBJECT_NAME
    for material in body.data.materials:
        if material is not None and material.name.rpartition(".")[2].isdigit():
            material.name = material.name.rpartition(".")[0] or "Body"

    body.parent = armature
    body.matrix_parent_inverse = armature.matrix_world.inverted()
    modifier = body.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.fspath(out))
    _common.log(TAG, f"wrote {out}")

    measured = {
        "vertices": len(body.data.vertices),
        "triangles": _common.triangle_count(body),
        "vertex_groups": len(renamed),
        "contract_bones_without_a_group": sorted(contract - set(renamed)),
        "map": "by_order",
        "armature": armature_node,
        "rest_pose_from": str(armature_blend),
        "blender_version": _common.blender_identity()["version"],
    }
    _common.log(
        TAG,
        f"{measured['vertices']} verts, {measured['vertex_groups']} of {len(contract)} contract bone(s) carry weight, "
        f"bound to {armature_node} at the profile's own rest pose — ready for `forge gen export`",
    )
    return _common.success(None, [out], measured=measured)


def _is_identity(matrix, tolerance: float = 1e-6) -> bool:
    for row in range(4):
        for column in range(4):
            want = 1.0 if row == column else 0.0
            if abs(matrix[row][column] - want) > tolerance:
                return False
    return True


# ---------------------------------------------------------------- both ends --


def _main_outer(argv: list[str]) -> int:
    from forge_gen import cli
    from forge_gen.exit_codes import ForgeGenError

    parser = argparse.ArgumentParser(prog="reattach", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
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
