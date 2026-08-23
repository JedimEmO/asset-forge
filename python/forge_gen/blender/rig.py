"""Skin a lifted character mesh to the profile's rig, via headless Blender.

    forge-gen rig <lift.glb> --out <name.blend> --record <name.rig.json> --name <name>
                  [--profile DIR] [--stature 1.80] [--yaw-deg 0] [--budget N]
                  [--prompt TEXT] [--created-by WHO]

The output is a rigged ``.blend`` skinned to the profile's frozen bone
contract, which ``forge-gen export`` turns into the ``.glb`` that ``forge
promote body`` runs the gates on. Nothing downstream knows or cares that no
artist touched it.

The trick that makes auto-rigging honest: the file *starts as* the
profile's ``rig.blend``. The armature object is named as the profile says,
sits at identity, and rests in the contract pose to 0.1 mm because it IS
the contract source, not a reconstruction of it. The lifted mesh is
imported into that file, normalized (yaw, stature, feet at Z=0, centred on
the skeleton), and bound with Blender's automatic weights. The rest pose is
never touched — which is why the *reference image* must be a strict T-pose.
The skeleton-fit check below refuses a mesh whose arms don't reach wrist
height at wrist span, and its failure message says "fix the reference
image" because that is the actual fix; bending weights around a bad pose
ships a character that deforms wrong under every clip in the library.

Bone-heat weighting fails quietly on the geometry TRELLIS can emit (open
surfaces, pinched shells): Blender prints a warning and leaves vertices
weightless, and a weightless vertex is a promote-time failure. So the tool
scans for them itself. A straggler copies the weights of the nearest vertex
bone heat *did* weight: a detached shell — an exo-brace on a shin, a lamp on
a shoulder pad, a crown over a hood — then moves with the surface it sits
on, where weighting it to the nearest bone *segment* sent a shoulder lamp
off with the upper arm. Deterministic, and good at every register. If more
than the profile's share of the mesh came back weightless, the bind as a
whole failed and the tool aborts rather than shipping a statue.
``remesh=True`` at generation time is the real defense; the ladder here is
for the tail.

The finger leaves will collect little or no weight on a mitt-resolution
mesh. That is the register, not a defect: the contract wants the bones
present and every vertex weighted to *some* contract bone, not every bone
deforming.

One ``Body`` object, dressed in its texture: a lifted character wears its
clothes as paint, and nothing downstream asks it to be split.

Every number here — the fit gate, the budget, the decimate margin, the
abort fraction, the shell fraction, the dust diagonal, the matte — comes
from the profile's ``profile.toml`` (``[fit]``, ``[rig]``, ``[material]``)
and is written into the record, so a body can be judged against the rule
it was held to.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import placeholders, profile as profile_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

#: The log prefix; the skills quote these lines.
TAG = "rig"

#: The mesh object's name — the convention ``forge_rig::measure`` reports as a mesh node.
OBJECT_NAME = "Body"

#: A library name.
NAME_PATTERN = re.compile(r"^[a-z0-9_]+$")

#: Voxel size of the watertight proxy the ladder falls back to (metres).
#: Two centimetres closes the slivers and pinholes that make bone heat's
#: solve singular while keeping limbs apart. The profile does not carry it:
#: it is a property of the rescue, not of the contract.
PROXY_VOXEL_M = 0.02

#: Bytes a ``--fake`` .blend starts with: Blender's own magic, so a sniff
#: says "a .blend" and the rest of the file says "not really".
FAKE_BLEND_HEADER = b"BLENDER-v000RENDH"


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("glb", help="the lifted .glb to rig")
    parser.add_argument("--out", required=True, metavar="BLEND", help="where to write the rigged .blend")
    parser.add_argument("--record", required=True, metavar="JSON", help="where to write the generator record")
    parser.add_argument("--name", required=True, metavar="NAME", help="[a-z0-9_]+ library name, written into the record")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or rigs/humanoid)")
    parser.add_argument("--stature", type=float, metavar="M", help="height to scale the body to (default: the profile's reference stature)")
    parser.add_argument("--yaw-deg", type=float, default=0.0, metavar="DEG", help="turn about +Z so the body faces the rig's front")
    parser.add_argument("--budget", type=int, metavar="N", help="triangle ceiling before the decimate kicks in (default: the profile's [rig] tri_budget)")
    parser.add_argument("--prompt", metavar="TEXT", help="the prompt the reference was made from, recorded as an input")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "rig",
        help="Lifted glb -> rigged .blend on the profile's skeleton",
        description=__doc__,
    )
    _add_arguments(parser)


def _load_profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


def _wrist_bone(profile: profile_mod.Profile, fit: dict) -> str:
    """The bone the fit gate measures reach against: ``[fit] wrist_bone``, else a hand of the driven layout."""
    named = fit.get("wrist_bone")
    if named:
        return str(named)
    hands = profile.motion_skeleton.get("hands") or []
    joints = profile.joints
    if hands:
        return joints[int(hands[0])]
    raise UsageError(f"{profile.dir}: neither [fit] wrist_bone nor motion_skeleton.json hands names a wrist")


def _spec(args) -> dict:
    """Validate the arguments and gather every number from the profile, once, as one dict."""
    if not NAME_PATTERN.match(args.name or ""):
        raise UsageError(f"--name {args.name!r} is not [a-z0-9_]+")
    profile = _load_profile(args.profile)
    bones = profile.section("bones")
    fit = profile.section("fit")
    rig = profile.section("rig")
    material = profile.section("material")
    stature = float(args.stature) if args.stature is not None else float(bones["reference_stature_m"])
    if stature <= 0:
        raise UsageError(f"--stature {stature} is not a height")
    budget = int(args.budget) if args.budget is not None else int(rig["tri_budget"])
    if budget <= 0:
        raise UsageError(f"--budget {budget} is not a triangle count")
    if not profile.rig_blend.is_file():
        raise UsageError(f"{profile.dir} has no {profile.rig_blend.name} — `forge-gen rig-build` makes it")
    contract_path = profile.dir / str(profile.toml["profile"].get("contract", "contract.json"))
    return {
        "glb": args.glb,
        "out": args.out,
        "record": args.record,
        "name": args.name,
        "prompt": args.prompt,
        "profile": profile,
        "profile_sha256": records.sha256_file(contract_path),
        "armature_node": str(bones["armature_node"]),
        "root": str(bones["root"]),
        "wrist": _wrist_bone(profile, fit),
        "stature": stature,
        "yaw_deg": float(args.yaw_deg),
        "budget": budget,
        "reach_min": float(fit["reach_min"]),
        "reach_max": float(fit["reach_max"]),
        "arm_height_tolerance": float(fit["arm_height_tolerance_m"]),
        "arm_tip_fraction": float(fit["arm_tip_fraction"]),
        "decimate_margin": float(rig["decimate_margin"]),
        "abort_fraction": float(rig["unweighted_abort_fraction"]),
        "shell_fraction": float(rig["shell_fraction"]),
        "dust_diagonal": float(rig["dust_diagonal_m"]),
        "metallic": float(material["metallic"]),
        "roughness": float(material["roughness"]),
        "double_sided": not bool(material.get("backface_culling", False)),
    }


def _params(spec: dict, *, fit: dict | None, dust: int | None, shells: int | None, slivers: int | None, proxy: bool | None) -> dict:
    return {
        "name": spec["name"],
        "profile": spec["profile"].name,
        "profile_sha256": spec["profile_sha256"],
        "stature_m": spec["stature"],
        "yaw_deg": spec["yaw_deg"],
        "tri_budget": spec["budget"],
        "fit": fit,
        "dust_islands_dropped": dust,
        "shells_rigid": shells,
        "slivers_smoothed": slivers,
        "proxy_used": proxy,
    }


def _add_inputs(rec: dict, spec: dict, source: Path) -> None:
    records.add_input(rec, "mesh", source)
    records.add_input(rec, "reference", spec["profile"].rig_blend, source=f"profile:{spec['profile'].name}")
    if spec["prompt"]:
        records.add_input(rec, "prompt", prompt=spec["prompt"])


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    """Validate, then run this file under Blender and relay its record."""
    spec = _spec(args)
    source = _common.existing_file(args.glb, what="lift")
    out = Path(args.out).expanduser().resolve()
    record = Path(args.record).expanduser().resolve()
    argv = [
        os.fspath(source),
        "--out", os.fspath(out),
        "--record", os.fspath(record),
        "--name", spec["name"],
        "--profile", os.fspath(spec["profile"].dir),
        "--stature", str(spec["stature"]),
        f"--yaw-deg={spec['yaw_deg']}",
        "--budget", str(spec["budget"]),
    ]
    if spec["prompt"]:
        argv += ["--prompt", spec["prompt"]]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


def run_fake(args) -> dict:
    """A placeholder .blend and a record that says it is one.

    There is no validator for a ``.blend`` outside Blender, so the placeholder
    is a file that begins with Blender's magic and says what it is; the
    record carries every knob as given and ``null`` for every measurement,
    because nothing was measured.
    """
    spec = _spec(args)
    source = _common.existing_file(args.glb, what="lift")
    out = Path(args.out).expanduser().resolve()
    record_path = Path(args.record).expanduser().resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(FAKE_BLEND_HEADER + b"\n# forge-gen --fake placeholder; not a Blender file\n")
    rec = placeholders.fake_record("rig", _common.TOOL, backend=_common.BACKEND_NAME, created_by=getattr(args, "created_by", None))
    _add_inputs(rec, spec, source)
    rec["params"] = _params(spec, fit=None, dust=None, shells=None, slivers=None, proxy=None)
    rec["measured"] = {"vertices": None, "triangles": None, "unweighted_before_rescue": None}
    records.add_output(rec, out)
    records.write(rec, record_path)
    return {"record": os.fspath(record_path), "outputs": [os.fspath(out)], "measured": rec["measured"]}


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    """The Blender half: open the rig, import, normalize, fit-gate, clean, bind, matte, save, record."""
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen rig (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    spec = _spec(args)
    source = Path(args.glb).resolve()
    out = Path(args.out).resolve()
    record_path = Path(args.record).resolve()
    profile = spec["profile"]

    bpy.ops.wm.open_mainfile(filepath=os.fspath(profile.rig_blend.resolve()))
    armature = bpy.data.objects.get(spec["armature_node"])
    if armature is None or armature.type != "ARMATURE":
        raise BackendFailed(f"{profile.rig_blend} holds no {spec['armature_node']!r} armature object — the profile is broken")
    for bone_name in (spec["root"], spec["wrist"]):
        if bone_name not in armature.data.bones:
            raise BackendFailed(f"{profile.rig_blend} has no bone {bone_name!r} — the profile is broken")

    body = _common.import_single_mesh(source, name=OBJECT_NAME, tag=TAG)
    _normalize(body, armature, spec)
    fit = _skeleton_fit_or_die(body, armature, spec)
    tris, dust_dropped = _cleanup_and_budget(body, spec)
    weights = _auto_weights(body, armature, spec)
    _common.matte(body, metallic=spec["metallic"], roughness=spec["roughness"], double_sided=spec["double_sided"])
    packed = _common.pack_images()
    if packed:
        _common.log(TAG, f"packed {packed} image(s)")

    out.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.fspath(out))

    measured = {
        "vertices": len(body.data.vertices),
        "triangles": tris,
        "unweighted_before_rescue": weights["unweighted_before_rescue"],
        "blender_version": _common.blender_identity()["version"],
    }
    _common.log(
        TAG,
        f"{measured['vertices']} verts, {tris} tris, {weights['unweighted_before_rescue']} vert(s) rescued "
        f"from the nearest weighted vertex, saved {out}",
    )
    _common.log(TAG, f"next — forge-gen export {out} --out <glb> --record <json>")

    rec = _common.new_record("rig", created_by=args.created_by)
    _add_inputs(rec, spec, source)
    rec["params"] = _params(
        spec,
        fit=fit,
        dust=dust_dropped,
        shells=weights["shells_rigid"],
        slivers=weights["slivers_smoothed"],
        proxy=weights["proxy_used"],
    )
    rec["measured"] = measured
    _common.finish_record(rec, outputs=[out], record_path=record_path)
    return _common.success(record_path, [out], measured=measured, fit=fit)


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
    # X centred on the rig's own mirror plane; depth centred where the spine
    # actually stands (the root bone's head), not on the bounding box's idea of it.
    root_y = armature.data.bones[spec["root"]].head_local.y
    centre = (lo + hi) / 2.0
    body.location = Vector((-centre.x, root_y - centre.y, -lo.z))
    _common.apply_transforms(body)


def _skeleton_fit_or_die(body, armature, spec: dict) -> dict:
    """Refuse a mesh that is not in the rest pose. The message names the real
    fix: iterate the reference image, never the weights."""
    bones = armature.data.bones
    wrist = bones[spec["wrist"]]
    hand_x = abs(wrist.head_local.x)
    hand_z = wrist.head_local.z

    xs = [v.co.x for v in body.data.vertices]
    span = max(max(xs), -min(xs))
    reach = span / hand_x if hand_x > 1e-6 else 0.0

    arm_tips = [v.co for v in body.data.vertices if abs(v.co.x) > spec["arm_tip_fraction"] * span]
    tip_z = sum(v.z for v in arm_tips) / len(arm_tips) if arm_tips else 0.0

    _common.log(
        TAG,
        f"fit — reach {reach:.2f} of wrist span (wrist x {hand_x:.3f} m, mesh half-span {span:.3f} m), "
        f"arm tips at z {tip_z:.3f} vs wrist z {hand_z:.3f} m",
    )
    fit = {"reach": round(reach, 4), "tip_z": round(tip_z, 4), "wrist_z": round(hand_z, 4)}

    problems = []
    # The ceiling makes room for the register's oversized mitts: a fist a
    # quarter-metre long ends well past the wrist joint it hangs from, and
    # that is style, not droop. Genuinely squat bodies still bounce — the
    # first warden take measured 1.52 and deserved to.
    if not spec["reach_min"] <= reach <= spec["reach_max"]:
        problems.append(
            f"the mesh spans {reach:.2f}x the skeleton's wrist reach (the gate is "
            f"{spec['reach_min']:.2f}–{spec['reach_max']:.2f}) — arms are not straight out to the sides"
        )
    if abs(tip_z - hand_z) > spec["arm_height_tolerance"]:
        problems.append(
            f"the widest geometry sits at z {tip_z:.2f} m but the wrists rest at "
            f"{hand_z:.2f} m (more than {spec['arm_height_tolerance']:.2f} m apart) — arms are not horizontal"
        )
    if problems:
        listed = "; ".join(problems)
        raise InputRejected(
            f"not a T-pose: {listed}. The rest pose is frozen, so fix the reference image "
            "(arms straight out, horizontal) and regenerate the mesh — do not bend weights around it.",
            fit=fit,
        )
    return fit


def _cleanup_and_budget(body, spec: dict) -> tuple[int, int]:
    """Merge, dissolve degenerates, drop dust, decimate to budget. Returns ``(tris, dust islands dropped)``."""
    import bmesh
    import bpy

    mesh = bmesh.new()
    mesh.from_mesh(body.data)
    bmesh.ops.remove_doubles(mesh, verts=mesh.verts[:], dist=_common.MERGE_DISTANCE_M)
    # Degenerate faces make the bone-heat Laplacian singular, and TRELLIS
    # meshes carry a spatter of tiny disconnected shells ("voxel dust") that
    # can fail the whole solve. Dust is judged by PHYSICAL size, never face
    # count: this runs after _normalize, so extents are metres, and a
    # centimetre-scale speck is invisible at any camera this register plays
    # at — while a disconnected mohawk spike or back-of-skull patch is real
    # geometry no matter how few faces it holds. (A face-count filter here
    # once deleted the back of a head; the player noticed.) Clean separate
    # shells bind fine — the proxy-weighting ladder backstops the rest.
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


def _auto_weights(body, armature, spec: dict) -> dict:
    """Bind with bone heat, then find what bone heat silently gave up on.

    Bone heat regularly fails outright on TRELLIS topology — remeshed, but
    still carrying slivers and near-degenerate fans that make its solve
    singular. When it does, the ladder: weight a watertight voxel-remeshed
    PROXY of the body (bone heat is happy on a closed shell), transfer the
    weights back by nearest-face interpolation, and only then hand the few
    remaining stragglers to the nearest-weighted-vertex rescue."""
    import bpy

    _bind(body, armature)
    shells = _detached_shell_vertices(body, spec["shell_fraction"])
    _clear_weights(body, armature, shells)
    unweighted = _unweighted(body, armature)
    total = len(body.data.vertices)
    abort_fraction = spec["abort_fraction"]
    proxy_used = False

    if len(unweighted) > abort_fraction * total:
        _common.log(TAG, f"bone heat lost {len(unweighted)} of {total} verts — weighting a voxel-remeshed proxy")
        proxy_used = True
        _proxy_weights(body, armature)
        unweighted = _unweighted(body, armature)
        if len(unweighted) > abort_fraction * total:
            raise BackendFailed(
                f"even the voxel-remeshed proxy left {len(unweighted)} of {total} vertices weightless "
                f"(more than {abort_fraction:.0%}) — the surface is broken; regenerate the mesh "
                "(another seed, or a higher decimation target at the lift)",
                unweighted=len(unweighted),
                vertices=total,
            )

    slivers = rigid = 0
    if unweighted:
        _common.log(
            TAG,
            f"{len(shells)} vert(s) in detached shells re-weighted from the surface they sit on, overriding bone heat",
        )
        slivers, rigid = _nearest_vertex_rescue(body, armature, unweighted)

    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    export = spec["profile"].toml.get("export", {})
    bpy.ops.object.vertex_group_limit_total(group_select_mode="ALL", limit=int(export.get("max_influences", 4)))
    bpy.ops.object.vertex_group_normalize_all(group_select_mode="ALL", lock_active=False)
    return {
        "unweighted_before_rescue": len(unweighted),
        "shells_rigid": rigid,
        "slivers_smoothed": slivers,
        "proxy_used": proxy_used,
        "shell_vertices": len(shells),
    }


def _bind(body, armature) -> None:
    """ARMATURE_AUTO parents the mesh and adds the Armature modifier the
    export gate requires, plus bone-heat weights when the solve succeeds."""
    import bpy

    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    armature.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")


def _detached_shell_vertices(body, shell_fraction: float) -> set:
    """Vertices of every connected island holding fewer than ``shell_fraction``
    of the mesh — the pieces bone heat cannot be trusted on: heat solves per
    connected piece, and a piece no bone passes through gets weighted by
    whichever bone it happens to see, a shoulder lamp to a thumb."""
    count = len(body.data.vertices)
    parent = list(range(count))

    def find(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    for edge in body.data.edges:
        a, b = edge.vertices
        parent[find(a)] = find(b)
    islands: dict[int, list[int]] = {}
    for index in range(count):
        islands.setdefault(find(index), []).append(index)
    limit = shell_fraction * count
    return {index for members in islands.values() if len(members) < limit for index in members}


def _clear_weights(body, armature, indices: set) -> None:
    if not indices:
        return
    targets = sorted(indices)
    for group in body.vertex_groups:
        if group.name in armature.data.bones:
            group.remove(targets)


def _unweighted(body, armature) -> list:
    bone_groups = {group.index for group in body.vertex_groups if group.name in armature.data.bones}
    return [
        vertex.index
        for vertex in body.data.vertices
        if sum(g.weight for g in vertex.groups if g.group in bone_groups) < 1e-6
    ]


def _proxy_weights(body, armature) -> None:
    """Weight a watertight copy, pour the weights back onto the real mesh."""
    import bpy

    proxy = body.copy()
    proxy.data = body.data.copy()
    bpy.context.collection.objects.link(proxy)

    remesh = proxy.modifiers.new("watertight", "REMESH")
    remesh.mode = "VOXEL"
    remesh.voxel_size = PROXY_VOXEL_M
    bpy.ops.object.select_all(action="DESELECT")
    proxy.select_set(True)
    bpy.context.view_layer.objects.active = proxy
    bpy.ops.object.modifier_apply(modifier=remesh.name)
    for group in list(proxy.vertex_groups):
        proxy.vertex_groups.remove(group)

    _bind(proxy, armature)
    proxy_lost = _unweighted(proxy, armature)
    _common.log(TAG, f"proxy has {len(proxy.data.vertices)} verts, {len(proxy_lost)} weightless after bone heat")

    for group in list(body.vertex_groups):
        body.vertex_groups.remove(group)
    transfer = body.modifiers.new("weights", "DATA_TRANSFER")
    transfer.object = proxy
    transfer.use_vert_data = True
    transfer.data_types_verts = {"VGROUP_WEIGHTS"}
    transfer.vert_mapping = "POLYINTERP_NEAREST"
    transfer.layers_vgroup_select_src = "ALL"
    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.datalayout_transfer(modifier=transfer.name)
    bpy.ops.object.modifier_apply(modifier=transfer.name)

    bpy.data.objects.remove(proxy, do_unlink=True)


def _nearest_vertex_rescue(body, armature, unweighted) -> tuple[int, int]:
    """Give a straggler the weights of the closest vertex that bone heat did
    weight. A weightless vertex is almost always part of a detached shell
    sitting on a weighted surface, and the surface's own weights are what
    make the shell move with it; weighting by the nearest bone *segment*
    instead put a shoulder lamp on the upper arm and a crown on the neck.

    Strays are grouped into islands first (connectivity through weightless
    vertices only). An island with no edge to any weighted vertex is a
    detached shell and takes ONE donor's weights — the donor nearest to any
    of its vertices — so it stays rigid: copying per vertex split a lamp
    between the pad and the arm and stretched it into a spike. A stray that
    does border weighted surface is a sliver of that surface, and copies per
    vertex so the seam stays smooth. The fallback when nothing at all is
    weighted is the nearest bone, which the abort above makes unreachable
    in practice.

    Returns ``(slivers smoothed, shells kept rigid)``."""
    from mathutils.kdtree import KDTree

    bone_groups = {group.index: group.name for group in body.vertex_groups if group.name in armature.data.bones}
    stray = set(unweighted)
    weighted = [v for v in body.data.vertices if v.index not in stray]
    if not weighted:
        _nearest_bone_rescue(body, armature, unweighted)
        return 0, 0

    tree = KDTree(len(weighted))
    for vertex in weighted:
        tree.insert(vertex.co, vertex.index)
    tree.balance()

    neighbours: dict[int, list[int]] = {index: [] for index in stray}
    touches_weighted: set[int] = set()
    for edge in body.data.edges:
        a, b = edge.vertices
        if a in stray and b in stray:
            neighbours[a].append(b)
            neighbours[b].append(a)
        elif a in stray:
            touches_weighted.add(a)
        elif b in stray:
            touches_weighted.add(b)

    def copy_weights(target: int, donor_index: int) -> None:
        for entry in body.data.vertices[donor_index].groups:
            name = bone_groups.get(entry.group)
            if name is not None and entry.weight > 0.0:
                body.vertex_groups[name].add([target], entry.weight, "REPLACE")

    seen: set[int] = set()
    slivers: list[list[int]] = []
    shells: list[list[int]] = []
    for seed in unweighted:
        if _seen_add(seen, seed) is False:
            continue
        island = []
        stack = [seed]
        while stack:
            current = stack.pop()
            island.append(current)
            for other in neighbours[current]:
                if _seen_add(seen, other):
                    stack.append(other)
        if any(index in touches_weighted for index in island):
            slivers.append(island)
        else:
            shells.append(island)

    for island in slivers:
        for index in island:
            _, donor, _ = tree.find(body.data.vertices[index].co)
            copy_weights(index, donor)

    # Shells stack on shells — a lamp on a pad on a torso — so resolve the
    # one nearest the current donor pool first and then let it donate, or
    # the lamp borrows from the sleeve under the pad instead of the pad.
    pool = [vertex.index for vertex in weighted]
    pending = list(shells)
    while pending:
        tree = KDTree(len(pool))
        for index in pool:
            tree.insert(body.data.vertices[index].co, index)
        tree.balance()
        best = None
        for island in pending:
            for index in island:
                _, donor, distance = tree.find(body.data.vertices[index].co)
                if best is None or distance < best[0]:
                    best = (distance, island, donor)
        _, island, donor = best
        for index in island:
            copy_weights(index, donor)
        pool.extend(island)
        pending.remove(island)
    _common.log(
        TAG,
        f"rescued {len(unweighted)} weightless vert(s) from the nearest weighted vertex — "
        f"{len(slivers)} sliver(s) smoothed, {len(shells)} detached shell(s) kept rigid",
    )
    return len(slivers), len(shells)


def _seen_add(seen: set, index: int) -> bool:
    """Add ``index`` to ``seen``; True when it was new."""
    if index in seen:
        return False
    seen.add(index)
    return True


def _nearest_bone_rescue(body, armature, unweighted) -> None:
    """Weight a straggler fully to the closest bone segment — the last rung,
    for a mesh with no weighted vertex to borrow from."""
    segments = []
    for bone in armature.data.bones:
        segments.append((bone.name, bone.head_local, bone.tail_local))

    def nearest(point) -> str:
        best_name, best_distance = None, None
        for name, head, tail in segments:
            direction = tail - head
            length_sq = direction.length_squared
            if length_sq < 1e-12:
                closest = head
            else:
                t = max(0.0, min(1.0, (point - head).dot(direction) / length_sq))
                closest = head + direction * t
            distance = (point - closest).length
            if best_distance is None or distance < best_distance:
                best_name, best_distance = name, distance
        return best_name

    for index in unweighted:
        name = nearest(body.data.vertices[index].co)
        group = body.vertex_groups.get(name) or body.vertex_groups.new(name=name)
        group.add([index], 1.0, "REPLACE")
    _common.log(TAG, f"rescued {len(unweighted)} weightless vert(s) to nearest bone")


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        print(_common.not_in_blender_message("rig"))
        sys.exit(2)
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
