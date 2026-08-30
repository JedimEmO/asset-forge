"""Export a rigged .blend to a .glb the rig contract will accept, via headless Blender.

    forge-gen export <name.blend> --out <name.glb> --record <name.export.json>
                     [--profile DIR] [--created-by WHO]

This is the gate between a Blender file and the library. ``forge promote
body`` judges the exported .glb again, but by then the artist has left the
room and the only thing left to look at is a binary. Everything checked here
is checked *inside the file that can still be fixed*, and named in
Blender's own vocabulary: object names, modifiers, image datablocks.

Every check below is fatal except the ones marked WARN, and they are all
gathered before anything is reported — an artist fixing five things wants
five lines, not five round trips. Nothing is written until every check has
passed: a refusal leaves no half-right .glb behind to be promoted by
mistake.

What is checked, and why each one is worth a run of Blender:

* **One armature, named as the profile's ``armature_node``.** An engine
  binds an animation curve to a bone by hashing the bone's full name path
  from the animation root, and the root's name is the first segment of
  every one of those hashes. A rig named ``rig`` spawns fine and holds its
  rest pose forever.
* **Every contract bone, with its contract parent.** Same reason, one level
  down. Names and parents come from ``contract.json`` — the file
  ``forge rig check`` and the game hold the body to — so a bone inserted
  above the root, or any contract bone reparented, is refused here the way
  it would be refused there.
* **The rest pose points where the contract points, and is as long as this
  body is.** Bone segments are compared against the profile's ``rig.blend``
  itself, joint for joint — the **direction** of each local rest translation
  within ``rest_direction_tolerance_deg``, its **length** inside
  ``[length_ratio_min, length_ratio_max]`` of the contract's, and a segment
  under ``rest_zero_length_m`` by position at ``rest_tolerance_m``, since a
  zero-length segment has no direction to check. This is the one rule the
  fitted skeleton trades: bone *lengths* belong to the body, and a rig
  nudged in *direction* binds perfectly and then plays every clip off a pose
  it was never baked against, because clips are baked against rest
  **rotations** and a direction is a restatement of one.
* **Extra bones only where the profile allows them.** ``[export]
  extra_bones`` is ``leaf`` (a childless bone below a contract bone — a
  holster, a marker), ``run`` (an unbranched chain — hair, a tail, a cape
  strand), or ``none``. A branch inside a run is refused because a runtime
  skips the whole run; a contract bone *under* an extra is the inserted-bone
  failure from the profile's one rule, and no naming changes it. Each extra
  is listed in the record.
* **Meshes are skinned, untransformed, textured and self-contained.** An
  object transform that is not identity bakes a hidden offset into the
  export; a missing material or an unpacked image exports as flat pink,
  which is the kind of defect that survives all the way to a screenshot.

``export_materials="EXPORT"``, deliberately: the rig carries no look, a
lifted mesh *is* its look, and the .glb is where the textures ship. The
post-export self-check (``forge_gen.glb.verify_glb``) then proves the file
is self-contained — one JSON chunk, one BIN chunk, and no ``uri`` anywhere
under ``buffers`` or ``images`` — because a .glb that references a texture
on the artist's disk is a .glb that renders pink on everybody else's.
"""

from __future__ import annotations

import argparse
import math
import os
import re
import struct
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod  # noqa: E402
from forge_gen import placeholders, png, profile as profile_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

#: The log prefix.
TAG = "export"

#: What ``[export] extra_bones`` may say.
EXTRA_BONE_POLICIES = ("none", "leaf", "run")

#: The policy when the profile does not say: a lone leaf is the shape the
#: profile README blesses for a holster or a marker.
DEFAULT_EXTRA_BONES = "leaf"

#: Blender's collision suffix: ``Cape`` imported twice becomes ``Cape.001``,
#: and whichever one the artist meant is now a coin flip.
DUPLICATE_SUFFIX = re.compile(r"\.\d{3}$")


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("blend", help="the rigged .blend to export")
    parser.add_argument("--out", required=True, metavar="GLB", help="where to write the .glb")
    parser.add_argument("--record", required=True, metavar="JSON", help="where to write the generator record")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or rigs/humanoid)")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "export",
        help="Rigged .blend -> self-contained body .glb",
        description=__doc__,
    )
    _add_arguments(parser)


def _load_profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


#: The three numbers the direction rule reads, and where each one lives.
#: ``contract.json`` is the projection ``forge rig export-contract`` writes
#: and the file the Rust half reads, so it is asked first; ``profile.toml``
#: is the source those numbers are written in once, and answers when a
#: contract has not been regenerated yet. A pytest holds the two equal, so
#: this order can never be the difference between two verdicts.
BONE_TOLERANCES = ("rest_direction_tolerance_deg", "length_ratio_min", "length_ratio_max", "rest_zero_length_m")


def _bone_tolerances(profile: profile_mod.Profile) -> dict:
    """``{name: value}`` for :data:`BONE_TOLERANCES`, from the contract, else the profile."""
    export = profile.section("export")
    out = {}
    for key in BONE_TOLERANCES:
        if key in profile.contract:
            out[key] = float(profile.contract[key])
        elif key in export:
            out[key] = float(export[key])
        else:
            raise UsageError(
                f"{profile.dir}: neither contract.json nor profile.toml's [export] names {key} — "
                "the rest-direction rule has no number to hold a bone to. Add it to profile.toml and "
                "run `forge rig export-contract`"
            )
    return out


def _spec(args) -> dict:
    profile = _load_profile(args.profile)
    bones = profile.section("bones")
    export = profile.section("export")
    policy = str(export.get("extra_bones", DEFAULT_EXTRA_BONES))
    if policy not in EXTRA_BONE_POLICIES:
        raise UsageError(f"{profile.dir}/profile.toml: [export] extra_bones {policy!r} is not one of {', '.join(EXTRA_BONE_POLICIES)}")
    if not profile.rig_blend.is_file():
        raise UsageError(f"{profile.dir} has no {profile.rig_blend.name} — the rest pose is measured against it")
    return {
        "blend": args.blend,
        "out": args.out,
        "record": args.record,
        "profile": profile,
        "armature_node": str(bones["armature_node"]),
        "armature_data": bones.get("armature_data"),
        "rest_tolerance": float(export["rest_tolerance_m"]),
        **_bone_tolerances(profile),
        "transform_tolerance": float(export["transform_tolerance"]),
        "max_influences": int(export.get("max_influences", 4)),
        "y_up": bool(export.get("y_up", True)),
        "materials": str(export.get("materials", "EXPORT")),
        "extra_bones": policy,
    }


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    """Validate, then run this file under Blender and relay its record."""
    spec = _spec(args)
    blend = _common.existing_file(args.blend, what=".blend")
    out = Path(args.out).expanduser().resolve()
    record = Path(args.record).expanduser().resolve()
    argv = [
        os.fspath(blend),
        "--out", os.fspath(out),
        "--record", os.fspath(record),
        "--profile", os.fspath(spec["profile"].dir),
    ]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


def run_fake(args) -> dict:
    """A placeholder body that ``verify_glb`` and ``forge promote body`` both accept, and a record that says so.

    The skeleton is real — every contract bone as a node with its rest
    transform, one skin over all of them — because a body placeholder that
    binds nothing would not exercise the promote gate. The mesh is a board
    of the reference stature standing on the ground, weighted wholly to the
    root.
    """
    spec = _spec(args)
    blend = _common.existing_file(args.blend, what=".blend")
    out = Path(args.out).expanduser().resolve()
    record_path = Path(args.record).expanduser().resolve()
    placeholders.refuse_real(out, record_path)
    fake_body_glb(out, spec["profile"])
    info = glb_mod.verify_glb(out)
    rec = placeholders.fake_record("export", _common.TOOL, backend=_common.BACKEND_NAME, created_by=getattr(args, "created_by", None))
    records.add_input(rec, "blend", blend)
    records.add_input(rec, "reference", spec["profile"].rig_blend, source=f"profile:{spec['profile'].name}")
    rec["params"] = {"profile": spec["profile"].name, "extra_bones": []}
    rec["measured"] = _measured(info, blender_version=None)
    records.add_output(rec, out)
    records.write(rec, record_path)
    return {"record": os.fspath(record_path), "outputs": [os.fspath(out)], "measured": rec["measured"]}


def _measured(info: dict, *, blender_version: str | None) -> dict:
    return {
        "nodes": info["nodes"],
        "meshes": info["meshes"],
        "images": info["images"],
        "skins": info["skins"],
        "generator_string": info["generator"],
        "blender_version": blender_version,
    }


#: One copy of the inverse-bind arithmetic, in ``placeholders`` beside the
#: other thing that writes a skinned placeholder.
_inverse_rigid = placeholders.inverse_rigid


def fake_body_glb(path: str | os.PathLike, profile: profile_mod.Profile, *, name: str = "Body") -> Path:
    """Write the smallest skinned .glb on the profile's contract that the promote gate accepts."""
    bones = profile.bones
    armature_node = str(profile.section("bones")["armature_node"])
    stature = float(profile.section("bones")["reference_stature_m"])
    half = 0.3
    rest = profile.rest_world()

    # Nodes: bones in contract order (indices match the contract), then the
    # armature node, then the mesh node — the layout the fixture mannequin uses.
    nodes = []
    for index, bone in enumerate(bones):
        children = [i for i, other in enumerate(bones) if other.get("parent") == index]
        node = {
            "name": bone["name"],
            "translation": [float(v) for v in bone["rest_translation"]],
            "rotation": [float(v) for v in bone["rest_rotation"]],
        }
        if children:
            node["children"] = children
        nodes.append(node)
    root_index = profile.bone_index(profile.root)
    armature_index = len(nodes)
    mesh_index = armature_index + 1
    nodes.append({"name": armature_node, "children": [root_index, mesh_index]})
    nodes.append({"name": name, "mesh": 0, "skin": 0})

    positions = struct.pack("<12f", -half, 0.0, 0.0, half, 0.0, 0.0, half, stature, 0.0, -half, stature, 0.0)
    texcoords = struct.pack("<8f", 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0)
    joints = struct.pack("<16H", *([root_index, 0, 0, 0] * 4))
    weights = struct.pack("<16f", *([1.0, 0.0, 0.0, 0.0] * 4))
    indices = struct.pack("<6H", 0, 1, 2, 0, 2, 3)
    ibm = b"".join(struct.pack("<16f", *_inverse_rigid(*rest[bone["name"]])) for bone in bones)
    image = png.solid_png(1, 1, (128, 128, 128, 255))
    parts = [positions, texcoords, joints, weights, indices, ibm, image]
    views = []
    offset = 0
    for blob in parts:
        padded = len(blob) + (-len(blob) % 4)
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob)})
        offset += padded
    for i in range(4):
        views[i]["target"] = 34962
    views[4]["target"] = 34963
    binary = b"".join(blob + b"\x00" * (-len(blob) % 4) for blob in parts)
    document = {
        "asset": {"version": "2.0", "generator": "forge-gen --fake"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [armature_index]}],
        "nodes": nodes,
        "meshes": [
            {
                "name": name,
                "primitives": [
                    {
                        "attributes": {"POSITION": 0, "TEXCOORD_0": 1, "JOINTS_0": 2, "WEIGHTS_0": 3},
                        "indices": 4,
                        "material": 0,
                        "mode": 4,
                    }
                ],
            }
        ],
        "skins": [{"name": armature_node, "inverseBindMatrices": 5, "skeleton": root_index, "joints": list(range(len(bones)))}],
        "materials": [
            {
                "name": name,
                "doubleSided": True,
                "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.9},
            }
        ],
        "textures": [{"source": 0, "sampler": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 10497, "wrapT": 10497}],
        "images": [{"name": name, "mimeType": "image/png", "bufferView": 6}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-half, 0.0, 0.0], "max": [half, stature, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"},
            {"bufferView": 2, "componentType": 5123, "count": 4, "type": "VEC4"},
            {"bufferView": 3, "componentType": 5126, "count": 4, "type": "VEC4"},
            {"bufferView": 4, "componentType": 5123, "count": 6, "type": "SCALAR"},
            {"bufferView": 5, "componentType": 5126, "count": len(bones), "type": "MAT4"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(glb_mod.build_glb(document, binary))
    return target


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    """The Blender half: open, check everything, then export, verify and record."""
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen export (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    spec = _spec(args)
    blend = Path(args.blend).resolve()
    out = Path(args.out).resolve()
    record_path = Path(args.record).resolve()
    profile = spec["profile"]

    # The reference is read first, into the empty factory scene, because
    # ``libraries.load`` refuses to read from the file that is open — and
    # the file being exported may be rig.blend itself (which then fails the
    # no-mesh check like any other bare skeleton, rather than crashing).
    bpy.ops.wm.read_factory_settings(use_empty=True)
    reference = _reference_bones(profile, spec)
    bpy.ops.wm.open_mainfile(filepath=os.fspath(blend))

    problems: list[str] = []
    warnings: list[str] = []

    arm_obj = _check_armature(problems, spec)
    extras: list[str] = []
    if arm_obj is not None:
        extras = _check_bones(arm_obj, reference, problems, spec)
    _check_meshes(arm_obj, problems, warnings, spec)

    for warning in warnings:
        _common.log(TAG, f"WARN {warning}")
    if problems:
        listed = "\n  - ".join(problems)
        raise InputRejected(
            f"{blend.name} does not meet the rig contract ({len(problems)} problem(s)):\n  - {listed}",
            problems=problems,
        )
    _common.log(TAG, f"{blend.name} passed every pre-export check")
    for extra in extras:
        _common.log(TAG, f"note: extra bone {extra}")

    bpy.ops.object.select_all(action="DESELECT")
    _common.export_glb(out, materials=spec["materials"], y_up=spec["y_up"])
    _common.log(TAG, f"wrote {out}")
    try:
        info = glb_mod.verify_glb(out)
    except glb_mod.GlbError as err:
        raise BackendFailed(f"the exported file failed its own check: {err}") from err
    _common.log(TAG, glb_mod.report(out, info))

    measured = _measured(info, blender_version=_common.blender_identity()["version"])
    rec = _common.new_record("export", created_by=args.created_by)
    records.add_input(rec, "blend", blend)
    records.add_input(rec, "reference", profile.rig_blend, source=f"profile:{profile.name}")
    rec["params"] = {"profile": profile.name, "extra_bones": extras}
    rec["measured"] = measured
    _common.finish_record(rec, outputs=[out], record_path=record_path)
    return _common.success(record_path, [out], measured=measured)


def _reference_bones(profile: profile_mod.Profile, spec: dict) -> dict:
    """Names → (contract parent, rest head, rest tail): parents from contract.json, positions from rig.blend.

    The armature *data* is loaded rather than appended as an object: data
    with no object never enters the scene, so it cannot be exported by
    accident, and ``head_local``/``tail_local`` are already armature-space —
    which is world space, since the contract armature's own transform is the
    identity. The armature is picked by its bone count against the contract
    (and by ``[bones] armature_data`` when the profile names it), and its
    bone names are cross-checked against the contract, so the wrong file
    cannot quietly become the reference.
    """
    import bpy

    path = os.fspath(profile.rig_blend.resolve())
    contract_names = [bone["name"] for bone in profile.bones]
    with bpy.data.libraries.load(path) as (source, target):
        target.armatures = list(source.armatures)
    loaded = [a for a in target.armatures if a is not None]
    matching = [a for a in loaded if len(a.bones) == len(contract_names)]
    wanted = spec.get("armature_data")
    if wanted:
        matching = [a for a in matching if a.name == wanted] or matching
    try:
        if not matching:
            counts = ", ".join(f"{a.name}={len(a.bones)}" for a in loaded) or "none"
            raise BackendFailed(
                f"{path} holds no {len(contract_names)}-bone armature (found: {counts}) — that file is the "
                "contract's source, so the wrong one would validate every mesh against nothing"
            )
        armature = matching[0]
        names = {bone.name for bone in armature.bones}
        missing = sorted(set(contract_names) - names)
        if missing:
            raise BackendFailed(
                f"{path} armature {armature.name!r} lacks contract bone(s) {', '.join(missing[:6])} — "
                "rig.blend and contract.json disagree; `forge-gen rig-build` then `forge rig export-contract`"
            )
        by_index = {index: bone["name"] for index, bone in enumerate(profile.bones)}
        reference = {}
        for bone in profile.bones:
            blend_bone = armature.bones[bone["name"]]
            parent = bone.get("parent")
            reference[bone["name"]] = (
                by_index[parent] if parent is not None else None,
                tuple(blend_bone.head_local),
                tuple(blend_bone.tail_local),
            )
    finally:
        for armature in loaded:
            bpy.data.armatures.remove(armature)
    return reference


def _check_armature(problems: list, spec: dict):
    """Exactly one armature, named as the profile says, sitting at the origin."""
    import bpy

    wanted = spec["armature_node"]
    armatures = [o for o in bpy.context.scene.objects if o.type == "ARMATURE"]
    if len(armatures) != 1:
        found = ", ".join(o.name for o in armatures) or "none"
        problems.append(
            f"the scene holds {len(armatures)} armature objects ({found}), want exactly one — "
            "the exporter picks a skin's armature per object, so a second rig silently splits the character"
        )
        return None

    arm_obj = armatures[0]
    if arm_obj.name != wanted:
        problems.append(
            f"the armature object is named '{arm_obj.name}', want '{wanted}' — the root name is the first "
            "segment of every animation target path, so every clip in the library would bind to nothing"
        )
    if not _is_identity(arm_obj.matrix_world, spec["transform_tolerance"]):
        problems.append(
            f"the armature object transform is not the identity ({_describe(arm_obj.matrix_world)}) — it would "
            "bake a hidden offset or scale into every pose. Apply it in Blender (Object > Apply > All "
            "Transforms); this tool will not do it for you"
        )
    return arm_obj


def _check_bones(arm_obj, reference: dict, problems: list, spec: dict) -> list[str]:
    """Every contract bone present, parented and posed as the contract has it,
    and every extra bone where the profile's policy allows one. Returns the extras."""
    bones = arm_obj.data.bones
    missing = sorted(name for name in reference if name not in bones)
    if missing:
        shown = ", ".join(missing[:8]) + (" ..." if len(missing) > 8 else "")
        problems.append(
            f"{len(missing)} contract bone(s) missing ({shown}) — skin onto the profile's rig.blend "
            "rather than rebuilding the skeleton"
        )

    matrix = arm_obj.matrix_world
    for name, (want_parent, want_head, want_tail) in sorted(reference.items()):
        bone = bones.get(name)
        if bone is None:
            continue
        got_parent = bone.parent.name if bone.parent else None
        if got_parent != want_parent:
            problems.append(
                f"{name} is parented to {got_parent or '<none>'}, the contract says {want_parent or '<none>'} — "
                "reparenting rewrites its animation target path and every curve aimed at it finds nothing"
            )
        # The segment each bone hangs on, which is exactly what the glTF node
        # carries as its rest translation: head to head, from the contract
        # parent — or from the armature's own origin for the one rootless
        # bone. Tails are not compared at all: a tail follows its child's
        # head, it is no part of the contract, and on a fitted skeleton every
        # one of them has legitimately moved.
        origin = (0.0, 0.0, 0.0)
        want_parent_head = reference[want_parent][1] if want_parent is not None and want_parent in reference else origin
        got_parent_head = (
            tuple(matrix @ bones[want_parent].head_local) if want_parent is not None and bones.get(want_parent) is not None else origin
        )
        _check_one_segment(
            name,
            want=_subtract(want_head, want_parent_head),
            got=_subtract(tuple(matrix @ bone.head_local), got_parent_head),
            spec=spec,
            problems=problems,
        )

    policy = spec["extra_bones"]
    extras: list[str] = []
    for bone in bones:
        if bone.name in reference:
            continue
        extras.append(bone.name)
        if bone.parent is None:
            problems.append(
                f"extra bone {bone.name} is a second root — the contract has one root, and an extra one "
                "exports as a sibling node nothing in the library knows how to reach"
            )
            continue
        if policy == "none":
            problems.append(
                f"extra bone {bone.name} — this profile allows no bones beyond the contract ([export] extra_bones = \"none\")"
            )
            continue
        children = list(bone.children)
        contract_children = [child.name for child in children if child.name in reference]
        if contract_children:
            problems.append(
                f"extra bone {bone.name} has contract bone(s) {', '.join(contract_children)} beneath it — an "
                "inserted bone rewrites every descendant's animation path, and no naming changes that"
            )
            continue
        if policy == "leaf" and children:
            named = ", ".join(child.name for child in children)
            problems.append(
                f"extra bone {bone.name} has children ({named}) — this profile allows extra bones only as lone "
                "leaves ([export] extra_bones = \"leaf\")"
            )
        elif policy == "run" and len(children) > 1:
            named = ", ".join(child.name for child in children)
            problems.append(
                f"extra bone {bone.name} has {len(children)} children ({named}) — a run of extra bones must be "
                "unbranched or the runtime warns and skips the whole chain; a cape is several parallel runs, "
                "not one branching run"
            )
    return extras


def _check_meshes(arm_obj, problems: list, warnings: list, spec: dict) -> None:
    """Every mesh that will be exported: skinned, untransformed, and carrying its own look."""
    import bpy

    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        problems.append("the scene holds no mesh object — the export would be a bare skeleton")
        return

    # Materials are shared between objects, so their images are gathered here
    # and checked once at the end. A palette used by eight submeshes losing
    # one texture is one defect, and reporting it eight times buries the rest
    # of the list.
    used_materials: dict = {}
    for obj in sorted(meshes, key=lambda o: o.name):
        if arm_obj is not None and obj.parent is not arm_obj:
            parent = obj.parent.name if obj.parent else "<nothing>"
            problems.append(
                f"mesh {obj.name} is parented to {parent}, want the armature — an unparented skin exports "
                "as a separate node and never follows the rig"
            )
        skinned = any(m.type == "ARMATURE" and (arm_obj is None or m.object is arm_obj) for m in obj.modifiers)
        if not skinned:
            problems.append(
                f"mesh {obj.name} has no Armature modifier pointing at the rig — without one the exporter "
                "writes no skin and the mesh stands still through every clip"
            )
        if not _is_identity(obj.matrix_world, spec["transform_tolerance"]):
            problems.append(
                f"mesh {obj.name} has a non-identity object transform ({_describe(obj.matrix_world)}) — apply "
                "it in Blender (Object > Apply > All Transforms). This tool refuses rather than applying it, "
                "because applying a transform also moves the mesh off the pose it was skinned in"
            )

        materials = [slot for slot in obj.data.materials if slot is not None]
        if not materials:
            problems.append(
                f"mesh {obj.name} has no material — it exports untextured and renders as flat white or pink "
                "depending on the viewer"
            )
        for material in materials:
            used_materials.setdefault(material.name, (material, obj.name))

        for label, name in (("object", obj.name), ("mesh data", obj.data.name)):
            if DUPLICATE_SUFFIX.search(name):
                warnings.append(
                    f"{label} name '{name}' carries a Blender duplicate suffix — two datablocks collided and "
                    "which one survived is not written down anywhere; rename it before this ships"
                )

    for name in sorted(used_materials):
        material, first_user = used_materials[name]
        _check_material_images(first_user, material, problems)

    _common.log(TAG, f"{len(meshes)} mesh object(s), {len(used_materials)} material(s)")


def _check_material_images(first_user: str, material, problems: list) -> None:
    """Every image a material reaches must live inside the .blend.

    An image node with nothing in it, or an image still pointing at a file on
    the artist's disk, is the same defect from the library's side: the .glb
    ships without those pixels and the mesh renders pink.
    """
    # ``node_tree is None`` rather than ``not use_nodes``: the flag is deprecated
    # in Blender 5.x and gone in 6, and a material with no tree is exactly the
    # case with no images to find either way.
    if material.node_tree is None:
        return
    for node in _image_nodes(material.node_tree, set()):
        if node.image is None:
            problems.append(
                f"material {material.name} (on {first_user}) has an empty Image Texture node — whatever it "
                "was meant to sample is gone"
            )
            continue
        if node.image.packed_file is None:
            source = node.image.filepath or "<no file>"
            problems.append(
                f"image {node.image.name} ({source}) used by material {material.name} (on {first_user}) is "
                "not packed — pack it (File > External Data > Pack Resources) so the .glb carries its own textures"
            )


def _image_nodes(node_tree, seen: set):
    """Every Image Texture node reachable from a node tree, groups included."""
    for node in node_tree.nodes:
        if node.type == "TEX_IMAGE":
            yield node
        elif node.type == "GROUP" and node.node_tree is not None:
            if node.node_tree in seen:
                continue
            seen.add(node.node_tree)
            yield from _image_nodes(node.node_tree, seen)


def _is_identity(matrix, tolerance: float) -> bool:
    translation, rotation, scale = matrix.decompose()
    return (
        translation.length < tolerance
        and abs(rotation.angle) < tolerance
        and max(abs(component - 1.0) for component in scale) < tolerance
    )


def _describe(matrix) -> str:
    translation, rotation, scale = matrix.decompose()
    return (
        f"location {tuple(round(v, 4) for v in translation)}, "
        f"rotation {round(rotation.angle, 4)} rad, "
        f"scale {tuple(round(v, 4) for v in scale)}"
    )


def _distance(vector, other) -> float:
    return max(abs(a - b) for a, b in zip(vector, other))


def _subtract(vector, other) -> tuple:
    return tuple(a - b for a, b in zip(vector, other))


def _length(vector) -> float:
    return sum(component * component for component in vector) ** 0.5


def _angle_deg(want, got) -> float:
    """The angle between two vectors, in degrees, clamped against float noise."""
    dot = sum(a * b for a, b in zip(want, got)) / (_length(want) * _length(got))
    return math.degrees(math.acos(max(-1.0, min(1.0, dot))))


def _check_one_segment(name: str, *, want, got, spec: dict, problems: list) -> None:
    """One bone's rest translation against the contract's: direction, then length.

    The trade the fitted skeleton is built on, stated in one place. A bone's
    **length** is a fact about the body — a four-head witch's upper arm is
    not the reference's — so it is only held inside a wide band, wide enough
    that 0.02 of the reference reads as a collapsed skeleton rather than a
    short body. A bone's **direction** is a restatement of its rest rotation,
    which every clip in the library was baked against, so it is held to a
    degree; the whole fitted skeleton the spike built measured 0.0000° of
    drift, and a degree is a generous ceiling on a quantity that moved by
    nothing.

    A segment shorter than ``rest_zero_length_m`` at either end has no
    direction to check and is compared by position at ``rest_tolerance_m``,
    which is what the rule was before the fit and still is for a bone that
    sits exactly on its parent.
    """
    zero = spec["rest_zero_length_m"]
    want_length, got_length = _length(want), _length(got)
    if want_length < zero or got_length < zero:
        drift = _distance(want, got)
        if drift > spec["rest_tolerance"]:
            problems.append(
                f"{name} sits on its parent in the contract ({want_length * 1000.0:.3f} mm) but "
                f"{got_length * 1000.0:.3f} mm from it here — a zero-length segment has no direction to check, "
                f"so it is held to position at {spec['rest_tolerance'] * 1000.0:.2f} mm and it is "
                f"{drift * 1000.0:.2f} mm off"
            )
        return
    angle = _angle_deg(want, got)
    if angle > spec["rest_direction_tolerance_deg"]:
        problems.append(
            f"{name}'s rest translation points {angle:.1f} deg off the contract's "
            f"({got[0]:+.3f}, {got[1]:+.3f}, {got[2]:+.3f} against {want[0]:+.3f}, {want[1]:+.3f}, {want[2]:+.3f}). "
            "Clips are baked against rest ROTATIONS, and a rotated bone binds perfectly and animates wrongly. "
            "Lengths are yours; directions are not."
        )
    ratio = got_length / want_length
    if not spec["length_ratio_min"] <= ratio <= spec["length_ratio_max"]:
        problems.append(
            f"{name} is {ratio:.2f}x the contract's length ({got_length * 100.0:.1f} cm against "
            f"{want_length * 100.0:.1f} cm), outside [{spec['length_ratio_min']:.2f}, {spec['length_ratio_max']:.2f}] — "
            "a bone at a fiftieth of its reference is a collapsed skeleton, not a short body"
        )


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        print(_common.not_in_blender_message("export"))
        sys.exit(2)
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
