"""Build a profile's canonical rig — ``rig.blend`` and ``rig.glb`` — via headless Blender.

    forge-gen rig-build [--source <clip.glb>] [--out-dir <profile dir>] [--profile DIR]
    forge-gen rig-build --from-rig <rig.glb> --out-dir <dir> [--profile DIR]

The rig is ARDY's driven skeleton *exactly* — same bone names, same
hierarchy, same rest pose — plus finger chains. It is derived from a
generated clip (the profile's ``fixture_clip``) rather than modelled from
scratch, because "exactly" is the whole point: an engine binds animation
curves by hashing a bone's full name path from the animation root, so a rig
that merely resembles the driven layout binds to nothing and reports no
error.

Two deliberate design constraints, both consequences of that hashing:

* **No root bone.** Inserting a parent above the root would rewrite every
  bone's path from ``["Hips", ...]`` to ``["root", "Hips", ...]`` and change
  every hash, which breaks binding silently. Root motion comes from the root
  bone's translation curve.
* **Fingers are added as leaves.** Adding children never changes an
  ancestor's path, so the driven bones' hashes are untouched.

Finger names follow Mixamo (``LeftHandIndex1..3``), matching the convention
the rest of the skeleton already uses; the layout — which chains, how far
across the knuckle line, each segment's length — is the profile's
``[fingers]`` table, so another profile can hang different hands off the
same driven layout.

``--from-rig`` is the other door: import an existing ``rig.glb`` with the
importer asked to keep glTF's own bone directions (``TEMPERANCE``), add
nothing, and save the ``.blend``. It is how a ``rig.blend`` is re-made for a
new Blender when the rig itself has not changed.

**Validating a Blender bump.** A ``.blend`` is not byte-stable across
Blender versions and neither is a glTF export, so a rebuilt rig is proven
by its *contract*, not its bytes: run ``forge-gen rig-build``, then ``forge
rig export-contract`` on the new ``rig.glb``, then the Rust drift test
(``cargo test -p forge_rig``) — the contract must re-derive within the
profile's ``rebuild_tolerance_m`` / ``rebuild_rotation_tolerance``, and a
re-exported ``contract.json`` must reproduce the committed file byte for
byte. If either moves, something about the rig changed: bump the profile
``version`` and expect every clip in the library to need rebaking. This
module's own check is the cheap half of that — every bone head lands on
the contract's rest position to ``rebuild_tolerance_m`` — and it says so
when it does not.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import profile as profile_mod  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError  # noqa: E402

#: The log prefix.
TAG = "rig-build"

#: The armature datablock's name when the profile's ``[bones]`` does not
#: carry ``armature_data``. The *object* name is the contract's
#: ``armature_node``; the datablock name is only how the exporter finds the
#: reference armature in ``rig.blend``.
DEFAULT_ARMATURE_DATA = "StandardRig"

#: Bytes a ``--fake`` .blend starts with (see ``rig.py``).
FAKE_BLEND_HEADER = b"BLENDER-v000RENDH"


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--source", metavar="GLB", help="the driven-only clip to lift the armature from (default: the profile's fixture_clip)")
    parser.add_argument("--from-rig", metavar="GLB", help="instead: import an existing rig.glb as is, add nothing, save the .blend")
    parser.add_argument("--out-dir", metavar="DIR", help="where rig.blend (and rig.glb) go (default: the profile directory)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or rigs/humanoid)")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "rig-build",
        help="Rebuild the profile's rig.blend (+ rig.glb) from its fixture clip, or from a rig.glb",
        description=__doc__,
    )
    _add_arguments(parser)


def _load_profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


def _spec(args) -> dict:
    profile = _load_profile(args.profile)
    bones = profile.section("bones")
    fingers = profile.section("fingers")
    chains = fingers.get("chain") or []
    if args.from_rig and args.source:
        raise UsageError("give --source or --from-rig, not both")
    if args.from_rig:
        source = Path(args.from_rig).expanduser()
        mode = "from-rig"
    else:
        source = Path(args.source).expanduser() if args.source else profile.fixture_clip
        mode = "build"
        if source is None:
            raise UsageError(f"{profile.dir}/profile.toml names no fixture_clip and no --source was given")
        if not chains:
            raise UsageError(f"{profile.dir}/profile.toml has no [[fingers.chain]] entries")
    out_dir = Path(args.out_dir).expanduser().resolve() if args.out_dir else profile.dir
    return {
        "profile": profile,
        "mode": mode,
        "source": source,
        "out_dir": out_dir,
        "out_blend": out_dir / profile.rig_blend.name,
        "out_glb": out_dir / profile.rig_glb.name,
        "armature_node": str(bones["armature_node"]),
        "armature_data": str(bones.get("armature_data") or DEFAULT_ARMATURE_DATA),
        "reference_stature": float(bones["reference_stature_m"]),
        "knuckle_offset": float(fingers["knuckle_offset_m"]),
        "thumb_segments": [float(v) for v in fingers["thumb_segments_m"]],
        "chains": [
            {"name": str(chain["name"]), "across": float(chain["across_m"]), "segments": [float(v) for v in chain["segments_m"]]}
            for chain in chains
        ],
        "rebuild_tolerance": float(fingers["rebuild_tolerance_m"]),
        "rebuild_rotation_tolerance": float(fingers["rebuild_rotation_tolerance"]),
        "y_up": bool(profile.toml.get("export", {}).get("y_up", True)),
    }


# ------------------------------------------------------------------- outer --


def run(args) -> dict:
    spec = _spec(args)
    source = _common.existing_file(spec["source"], what="source clip" if spec["mode"] == "build" else "rig")
    argv = ["--out-dir", os.fspath(spec["out_dir"]), "--profile", os.fspath(spec["profile"].dir)]
    argv += ["--from-rig", os.fspath(source)] if spec["mode"] == "from-rig" else ["--source", os.fspath(source)]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


def _is_fake_file(path: Path) -> bool:
    try:
        with open(path, "rb") as handle:
            return handle.read(len(FAKE_BLEND_HEADER)) == FAKE_BLEND_HEADER
    except OSError:
        return False


def run_fake(args) -> dict:
    """Placeholder rig files — but never over a real one.

    ``rig-build`` writes into a profile directory by default, and a fake run
    that replaced a real ``rig.blend`` with a stub would break every rig
    and export after it. A target that exists and is not itself a fake is
    refused.
    """
    from forge_gen.blender import export as export_mod

    spec = _spec(args)
    _common.existing_file(spec["source"], what="source clip" if spec["mode"] == "build" else "rig")
    outputs: list[Path] = [spec["out_blend"]] if spec["mode"] == "from-rig" else [spec["out_blend"], spec["out_glb"]]
    for target in outputs:
        if target.exists() and not _is_fake_file(target) and not (target.suffix == ".glb" and _is_fake_glb(target)):
            raise UsageError(f"--fake refuses to overwrite {target}: it is a real rig file, not a placeholder")
    spec["out_dir"].mkdir(parents=True, exist_ok=True)
    spec["out_blend"].write_bytes(FAKE_BLEND_HEADER + b"\n# forge-gen --fake placeholder; not a Blender file\n")
    if spec["mode"] == "build":
        export_mod.fake_body_glb(spec["out_glb"], spec["profile"], name="Body")
    return {"record": None, "outputs": [os.fspath(p) for p in outputs], "fake": True}


def _is_fake_glb(path: Path) -> bool:
    from forge_gen import glb as glb_mod

    try:
        return str(glb_mod.verify_glb(path).get("generator") or "").startswith("forge-gen --fake")
    except (glb_mod.GlbError, OSError):
        return False


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen rig-build (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    spec = _spec(args)
    source = Path(spec["source"]).resolve()
    profile = spec["profile"]

    bpy.ops.wm.read_factory_settings(use_empty=True)
    imported = _common.import_gltf(source, keep_bone_directions=True)
    armatures = [o for o in imported if o.type == "ARMATURE"]
    if len(armatures) != 1:
        raise InputRejected(f"{source} holds {len(armatures)} armature(s); want exactly one")
    arm_obj = armatures[0]
    arm_obj.name = spec["armature_node"]
    arm_obj.data.name = spec["armature_data"]

    # The clip's dummy skin anchor and its animation are scaffolding from the
    # ARDY export; the rig is the only thing we keep.
    for obj in [o for o in imported if o is not arm_obj]:
        bpy.data.objects.remove(obj, do_unlink=True)
    if arm_obj.animation_data:
        arm_obj.animation_data_clear()
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    if not _is_identity(arm_obj.matrix_world):
        bpy.ops.object.select_all(action="DESELECT")
        arm_obj.select_set(True)
        bpy.context.view_layer.objects.active = arm_obj
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

    before = [b.name for b in arm_obj.data.bones]
    stature = _report_stature(arm_obj, spec["reference_stature"])

    added = 0
    if spec["mode"] == "build":
        bpy.context.view_layer.objects.active = arm_obj
        bpy.ops.object.mode_set(mode="EDIT")
        added = _add_fingers(arm_obj, spec)
        bpy.ops.object.mode_set(mode="OBJECT")

    _verify(arm_obj, before)
    drift = _check_against_contract(arm_obj, profile, spec["rebuild_tolerance"])

    out_blend = spec["out_blend"]
    out_blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.fspath(out_blend))
    _common.log(TAG, f"{len(before)} source bones + {added} finger bones = {len(arm_obj.data.bones)} total")
    _common.log(TAG, f"wrote {out_blend}")
    outputs = [out_blend]

    if spec["mode"] == "build":
        bpy.ops.object.select_all(action="DESELECT")
        _common.export_glb(spec["out_glb"], materials="NONE", y_up=spec["y_up"])
        _common.log(TAG, f"wrote {spec['out_glb']}")
        outputs.append(spec["out_glb"])
    _common.log(TAG, "next — forge rig export-contract, then cargo test -p forge_rig (the drift test) before committing")
    return _common.success(
        None,
        outputs,
        bones=len(arm_obj.data.bones),
        fingers_added=added,
        stature_m=round(stature, 4),
        contract_drift_m=drift,
    )


def _is_identity(matrix) -> bool:
    translation, rotation, scale = matrix.decompose()
    return translation.length < 1e-6 and abs(rotation.angle) < 1e-6 and max(abs(c - 1.0) for c in scale) < 1e-6


def _report_stature(arm_obj, expected: float) -> float:
    """Joint span and the stature it implies (the highest bone tail — the head bone's end)."""
    heads = [(arm_obj.matrix_world @ b.head_local).z for b in arm_obj.data.bones]
    tails = [(arm_obj.matrix_world @ b.tail_local).z for b in arm_obj.data.bones]
    lo, hi = min(heads), max(heads)
    stature = max(tails)
    _common.log(TAG, f"joint span {lo:+.3f} .. {hi:+.3f} m (stature approx {stature:.2f} m, reference {expected})")
    return stature


def _add_fingers(arm_obj, spec: dict) -> int:
    """Add Mixamo-named finger chains as leaves under each hand, per the profile's ``[fingers]``.

    Offsets are in the hand bone's own space: +Y along the fingers, +X
    across the knuckles (mirrored on the right hand), +Z the back of the
    hand. The thumb root already exists in the driven layout; it is only
    extended, so its existing hash stays valid.
    """
    edit_bones = arm_obj.data.edit_bones
    added = 0

    for side in ("Left", "Right"):
        hand_name = f"{side}Hand"
        hand = edit_bones.get(hand_name)
        if hand is None:
            _common.log(TAG, f"no {hand_name}, skipping fingers")
            continue

        # Hand-local frame: y along the bone, x across the knuckles, z out the back.
        y_axis = (hand.tail - hand.head).normalized()
        z_axis = hand.z_axis.normalized()
        x_axis = y_axis.cross(z_axis).normalized()
        knuckle = hand.head + y_axis * spec["knuckle_offset"]

        for chain in spec["chains"]:
            # Mirror the across-knuckle spread so both hands splay outward.
            sign = 1.0 if side == "Left" else -1.0
            head = knuckle + x_axis * (chain["across"] * sign)
            parent = hand
            for index, length in enumerate(chain["segments"], start=1):
                bone = edit_bones.new(f"{side}Hand{chain['name']}{index}")
                bone.head = head
                bone.tail = head + y_axis * length
                bone.parent = parent
                bone.use_connect = index > 1
                # Match the hand's roll so finger curl stays in one plane.
                bone.align_roll(z_axis)
                parent = bone
                head = bone.tail
                added += 1

        # The thumb root is part of the driven layout already — extend it
        # rather than replacing it, so its existing hash stays valid.
        thumb_root = edit_bones.get(f"{side}HandThumb1")
        if thumb_root is None:
            continue
        t_axis = (thumb_root.tail - thumb_root.head).normalized()
        head = thumb_root.tail
        parent = thumb_root
        for index, length in enumerate(spec["thumb_segments"], start=2):
            bone = edit_bones.new(f"{side}HandThumb{index}")
            bone.head = head
            bone.tail = head + t_axis * length
            bone.parent = parent
            bone.use_connect = True
            bone.align_roll(z_axis)
            parent = bone
            head = bone.tail
            added += 1

    return added


def _verify(arm_obj, before: list) -> None:
    """Fail loudly on the two things that would silently break binding."""
    names = [b.name for b in arm_obj.data.bones]

    missing = [n for n in before if n not in names]
    if missing:
        raise BackendFailed(f"lost source bones: {missing}")

    duplicates = {n for n in names if names.count(n) > 1}
    if duplicates:
        raise BackendFailed(
            f"duplicate bone names {sorted(duplicates)} — identical name paths hash to identical animation targets"
        )

    # Adding a parent above the source root would rewrite every descendant's
    # animation path. Assert the source roots are still roots.
    for bone in arm_obj.data.bones:
        if bone.name in before and bone.parent is not None and bone.parent.name not in before:
            raise BackendFailed(f"{bone.name} was reparented under {bone.parent.name}, which changes its animation path hash")
    _common.log(TAG, "verified — source bones intact, names unique, paths unchanged")


def _check_against_contract(arm_obj, profile: profile_mod.Profile, tolerance: float) -> float | None:
    """Every contract bone's head on its contract rest position, to ``tolerance`` (metres).

    The contract is chained from the root in glTF axes and converted to
    Blender's (``(x, y, z)_gltf = (x, z, -y)_blender``). Names and parents
    must match exactly; a position that drifts is reported with the worst
    offender, and the Rust drift test is the word on rotations. Returns
    the largest drift seen, or ``None`` when a name is missing (which is
    its own refusal).
    """
    bones = arm_obj.data.bones
    contract_names = [bone["name"] for bone in profile.bones]
    missing = [name for name in contract_names if name not in bones]
    extra = [bone.name for bone in bones if bone.name not in contract_names]
    if missing or extra:
        _common.log(
            TAG,
            f"WARN the rebuilt rig does not name the contract's bones: missing {missing[:6]}, extra {extra[:6]} — "
            "the profile's contract.json must be regenerated (`forge rig export-contract`) and its version bumped",
        )
        return None
    by_index = {index: bone["name"] for index, bone in enumerate(profile.bones)}
    reparented = []
    for index, bone in enumerate(profile.bones):
        want = by_index[bone["parent"]] if bone.get("parent") is not None else None
        got = bones[bone["name"]].parent.name if bones[bone["name"]].parent else None
        if want != got:
            reparented.append(f"{bone['name']}: {got} (contract {want})")
    if reparented:
        raise BackendFailed(f"the rebuilt rig reparents contract bones: {'; '.join(reparented[:5])}")

    rest = profile.rest_world()
    worst = 0.0
    worst_name = None
    for name, (position, _rotation) in rest.items():
        gx, gy, gz = position
        want = (gx, -gz, gy)
        head = arm_obj.matrix_world @ bones[name].head_local
        drift = max(abs(head[i] - want[i]) for i in range(3))
        if drift > worst:
            worst, worst_name = drift, name
    if worst > tolerance:
        _common.log(
            TAG,
            f"WARN rest positions drift from contract.json by up to {worst * 1000:.3f} mm ({worst_name}), "
            f"over the {tolerance * 1000:.3f} mm rebuild tolerance — the rig changed; regenerate the contract and bump the version",
        )
    else:
        _common.log(TAG, f"rest positions match contract.json within {worst * 1000:.3f} mm ({len(rest)} bones)")
    return round(worst, 7)


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        print(_common.not_in_blender_message("rig-build"))
        sys.exit(2)
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
