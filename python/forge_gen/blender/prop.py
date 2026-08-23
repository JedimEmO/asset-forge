"""Normalize a lifted .glb into a shipped prop, via headless Blender.

    forge-gen prop <lift.glb> --out <prop.glb> --record <prop.json>
                   (--height <m> | --length <m>) [--yaw-deg 0]
                   [--hang | --held | --grip <m> --long-axis ±x|±y|±z [--roll-deg 0]]
                   [--socket NAME] [--budget N] [--profile DIR] [--created-by WHO]

A TRELLIS mesh arrives as a shape in a unit cube: right-looking, but unitless,
floating, possibly several shells, and dressed in the full PBR wardrobe. A
prop in this library is a narrower promise — real metres, feet on the origin
(or grip at it), one matte painted look, one self-contained file — and this
tool is the distance between the two.

What it does, in order: joins every imported mesh into one object, merges
coincident vertices and drops loose geometry, turns ``--yaw-deg``, scales
uniformly until the chosen dimension reads ``--height`` (Blender +Z) or
``--length`` (the longest extent), then places the result. A ground prop
sits with its lowest vertex at Z=0, centred in X and Y, because that is
where a level drops it; ``--hang`` puts the *highest* vertex there instead,
for a decor kind the level hangs from a ceiling (a cobweb, roots);
``--held`` centres the bounding box at the origin for a socketed prop.

A weapon wants more than centring: the socket table (the profile's
``sockets.json``, its ``authoring_frame``) expects the prop's long axis
along glTF +Y (hilt to tip), its front — a blade's edge, a pistol's sights —
toward glTF −Z, and the grip at the origin. That frame is converted to
Blender's axes once, here, and ``--grip <m>`` does the rest at import:
``--long-axis`` names which axis of the lift runs hilt→tip (sign included,
so ``-x`` means the tip is at low X), the tool turns it onto the authoring
long axis, ``--roll-deg`` then spins about it until the front faces the
authoring front (look at the views and turn), and the point ``<m>`` along
the axis from the hilt end lands at the origin, centred across. The report
says where the extents landed so a grip that missed is a number, not a
surprise. The socket's offset at the attach site stays the last centimetre
of fit; ``--socket NAME`` names which one, checked against ``sockets.json``
and written into the record.

The material step is the style register applied with a hatchet: keep the
baked base-color texture, force the profile's ``[material]`` metallic and
roughness (matte, painted, no gloss), disconnect everything else TRELLIS
baked. Its normal and metallic-roughness maps model real-world response;
this library paints its lighting into the diffuse and wants nothing arguing
with that.

What gates a prop is the tri budget (``[prop] tri_budget`` in the profile —
a 6000-vertex lift is the prop register; the 2000-tri table it replaced
sheared a crate's rivets into smears), and the same self-contained .glb
proof the character exporter runs. The output goes where ``--out`` says —
never into a library; ``forge promote model`` is the door, and it runs the
measurement a consumer will trust.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from forge_gen import glb as glb_mod  # noqa: E402
from forge_gen import placeholders, profile as profile_mod, records  # noqa: E402
from forge_gen.blender import _common  # noqa: E402
from forge_gen.exit_codes import InputRejected, UsageError  # noqa: E402

#: The log prefix; the skills quote these lines.
TAG = "prop"

#: The mesh object's name in the exported file — the convention ``forge_rig::measure`` reports.
OBJECT_NAME = "Prop"

#: What ``params.placement`` may say.
PLACEMENTS = ("floor", "hang", "held", "grip")


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("glb", help="the lifted .glb to normalize")
    parser.add_argument("--out", required=True, metavar="GLB", help="where to write the prop (out/props/<name>.glb)")
    parser.add_argument("--record", required=True, metavar="JSON", help="where to write the generator record")
    size = parser.add_mutually_exclusive_group()
    size.add_argument("--height", type=float, metavar="M", help="scale until the Blender +Z extent is this many metres")
    size.add_argument("--length", type=float, metavar="M", help="scale until the longest extent is this many metres")
    parser.add_argument("--yaw-deg", type=float, default=0.0, metavar="DEG", help="turn about +Z before anything else")
    place = parser.add_mutually_exclusive_group()
    place.add_argument("--hang", action="store_true", help="origin at the top: a hanging prop")
    place.add_argument("--held", action="store_true", help="origin at the bounding-box centre: a socketed prop")
    place.add_argument("--grip", type=float, metavar="M", help="a weapon: put the point this many metres up the long axis at the origin")
    parser.add_argument("--long-axis", default="z", metavar="AXIS", help="with --grip: the lift's hilt-to-tip axis, e.g. z, -x (default z)")
    parser.add_argument("--roll-deg", type=float, default=0.0, metavar="DEG", help="with --grip: spin about the long axis until the front faces the authoring front")
    parser.add_argument("--socket", metavar="NAME", help="the profile socket this prop is authored for (checked against sockets.json)")
    parser.add_argument("--budget", type=int, metavar="N", help="triangle ceiling (default: the profile's [prop] tri_budget)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or rigs/humanoid)")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "prop",
        help="Lifted glb -> normalized prop in headless Blender",
        description=__doc__,
    )
    _add_arguments(parser)


def _load_profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


def _spec(args) -> dict:
    """Validate the arguments and resolve everything the inner needs, as one dict.

    Runs under both interpreters: the outer refuses a bad command line in
    milliseconds, and the inner re-derives the same numbers from the same
    profile so nothing is carried across the process boundary as a guess.
    """
    if (args.height is None) == (args.length is None):
        raise UsageError("give exactly one of --height/--length")
    if args.grip is not None:
        try:
            _common.parse_blender_axis(args.long_axis)
        except ValueError as err:
            raise UsageError(f"--long-axis: {err}") from err
    profile = _load_profile(args.profile)
    prop_section = profile.section("prop")
    material = profile.section("material")
    budget = int(args.budget) if args.budget is not None else int(prop_section["tri_budget"])
    if budget <= 0:
        raise UsageError(f"--budget {budget} is not a triangle count")
    socket = None
    if args.socket:
        try:
            socket = profile.socket(args.socket)["name"]
        except profile_mod.ProfileError:
            names = ", ".join(s["name"] for s in profile.sockets.get("sockets", []))
            raise UsageError(f"--socket {args.socket!r} is not in {profile.dir}/sockets.json (sockets: {names})") from None

    frame = profile.sockets.get("authoring_frame") or {}
    long_axis_gltf = str(frame.get("long_axis", prop_section.get("long_axis", "+Y")))
    front_gltf = str(frame.get("front", prop_section.get("front", "-Z")))
    if frame and prop_section.get("long_axis") and frame.get("long_axis") != prop_section.get("long_axis"):
        sys.stderr.write(
            f"{TAG}: sockets.json authoring_frame long_axis {frame.get('long_axis')} disagrees with "
            f"profile.toml [prop] long_axis {prop_section.get('long_axis')}; sockets.json wins\n"
        )

    if args.grip is not None:
        placement = "grip"
    elif args.held:
        placement = "held"
    elif args.hang:
        placement = "hang"
    else:
        placement = "floor"

    long_axis = None
    if args.grip is not None:
        text = args.long_axis.strip().lower()
        long_axis = ("-" if text.startswith("-") else "+") + text.lstrip("+-")

    return {
        "glb": args.glb,
        "out": args.out,
        "record": args.record,
        "profile": profile,
        "budget": budget,
        "placement": placement,
        "height": args.height,
        "length": args.length,
        "yaw_deg": float(args.yaw_deg),
        "grip": args.grip,
        "long_axis": long_axis,
        "roll_deg": float(args.roll_deg) if args.grip is not None else None,
        "socket": socket,
        "long_axis_gltf": long_axis_gltf,
        "front_gltf": front_gltf,
        "metallic": float(material["metallic"]),
        "roughness": float(material["roughness"]),
        "double_sided": not bool(material.get("backface_culling", False)),
    }


def _params(spec: dict) -> dict:
    """The record's ``params``: every knob, ``None`` where it did not apply."""
    return {
        "height_m": spec["height"],
        "length_m": spec["length"],
        "yaw_deg": spec["yaw_deg"],
        "placement": spec["placement"],
        "grip_m": spec["grip"],
        "long_axis": spec["long_axis"],
        "roll_deg": spec["roll_deg"],
        "socket": spec["socket"],
        "tri_budget": spec["budget"],
        "matte": {"metallic": spec["metallic"], "roughness": spec["roughness"]},
    }


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
        f"--yaw-deg={spec['yaw_deg']}",
        "--budget", str(spec["budget"]),
        "--profile", os.fspath(spec["profile"].dir),
    ]
    if spec["height"] is not None:
        argv += ["--height", str(spec["height"])]
    else:
        argv += ["--length", str(spec["length"])]
    if spec["placement"] == "hang":
        argv.append("--hang")
    elif spec["placement"] == "held":
        argv.append("--held")
    elif spec["placement"] == "grip":
        # ``=``-joined, so a leading minus (``-x``) is a value and not a flag.
        argv += ["--grip", str(spec["grip"]), f"--long-axis={spec['long_axis']}", f"--roll-deg={spec['roll_deg']}"]
    if spec["socket"]:
        argv += ["--socket", spec["socket"]]
    argv += _common.passthrough_argv(args)
    inner = _common.run_blender_module(__file__, argv)
    return _common.outer_result(inner)


def run_fake(args) -> dict:
    """A placeholder prop that passes ``verify_glb``, and a record that says it is one."""
    spec = _spec(args)
    source = _common.existing_file(args.glb, what="lift")
    out = Path(args.out).expanduser().resolve()
    record_path = Path(args.record).expanduser().resolve()
    placeholders.placeholder_glb(out, name=OBJECT_NAME)
    info = glb_mod.verify_glb(out)
    rec = placeholders.fake_record("prop", _common.TOOL, backend=_common.BACKEND_NAME, created_by=getattr(args, "created_by", None))
    records.add_input(rec, "mesh", source)
    rec["params"] = _params(spec)
    rec["measured"] = _measure_document(info)
    records.add_output(rec, out)
    records.write(rec, record_path)
    return {"record": os.fspath(record_path), "outputs": [os.fspath(out)], "measured": rec["measured"]}


def _measure_document(info: dict) -> dict:
    """What a glTF document says about itself — the fake path's only honest measurement."""
    document = info["document"]
    vertices = 0
    triangles = 0
    lo = [None, None, None]
    hi = [None, None, None]
    accessors = document.get("accessors", [])
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            position = primitive.get("attributes", {}).get("POSITION")
            if position is None:
                continue
            accessor = accessors[position]
            vertices += int(accessor.get("count", 0))
            for axis in range(3):
                if "min" in accessor:
                    lo[axis] = accessor["min"][axis] if lo[axis] is None else min(lo[axis], accessor["min"][axis])
                if "max" in accessor:
                    hi[axis] = accessor["max"][axis] if hi[axis] is None else max(hi[axis], accessor["max"][axis])
            indices = primitive.get("indices")
            count = int(accessors[indices]["count"]) if indices is not None else int(accessor.get("count", 0))
            triangles += count // 3
    bounds = None if None in lo or None in hi else {"min": lo, "max": hi}
    return {
        "vertices": vertices,
        "triangles": triangles,
        "bounds_m": bounds,
        "materials": len(document.get("materials", [])),
        "images": len(document.get("images", [])),
    }


# ------------------------------------------------------------------- inner --


def run_in_blender(argv: list[str]) -> dict:
    """The Blender half: import, clean, place, matte, budget, export, verify, record."""
    import bpy

    parser = argparse.ArgumentParser(prog="forge-gen prop (blender)")
    _add_arguments(parser)
    _common.add_inner_flags(parser)
    args = parser.parse_args(argv)
    _common.apply_inner_flags(args)
    spec = _spec(args)
    source = Path(args.glb).resolve()
    out = Path(args.out).resolve()
    record_path = Path(args.record).resolve()

    bpy.ops.wm.read_factory_settings(use_empty=True)
    prop = _common.import_single_mesh(source, name=OBJECT_NAME, tag=TAG)
    _common.merge_and_drop_loose(prop, tag=TAG)
    _place(prop, spec)
    _common.matte(prop, metallic=spec["metallic"], roughness=spec["roughness"], double_sided=spec["double_sided"])

    tris = _common.triangle_count(prop)
    if tris > spec["budget"]:
        raise InputRejected(
            f"{tris} tris exceeds the {spec['budget']} budget — regenerate with fewer verts "
            "(a lower decimation target at the lift); do not raise the budget by reflex",
            triangles=tris,
            tri_budget=spec["budget"],
        )

    packed = _common.pack_images()
    if packed:
        _common.log(TAG, f"packed {packed} image(s)")
    bpy.ops.object.select_all(action="SELECT")
    # The character exporter's settings, minus nothing: props ride the same
    # loader and must make the same promises about axes, units and materials.
    export = spec["profile"].toml.get("export", {})
    _common.export_glb(out, materials="EXPORT", y_up=bool(export.get("y_up", True)))

    try:
        info = glb_mod.verify_glb(out)
    except glb_mod.GlbError as err:
        raise _common.BackendFailed(f"the exported file failed its own check: {err}") from err
    _common.log(TAG, glb_mod.report(out, info))

    measured = {
        "vertices": len(prop.data.vertices),
        "triangles": tris,
        "bounds_m": _common.bounds_gltf(prop),
        "materials": len([slot for slot in prop.material_slots if slot.material is not None]),
        "images": info["images"],
        "blender_version": _common.blender_identity()["version"],
    }
    _common.log(TAG, f"{measured['vertices']} verts, {tris} tris, {measured['materials']} material(s), {info['images']} embedded image(s)")
    _common.log(TAG, f"bounds {_common.describe_bounds(prop)}")

    rec = _common.new_record("prop", created_by=args.created_by)
    records.add_input(rec, "mesh", source)
    rec["params"] = _params(spec)
    rec["measured"] = measured
    _common.finish_record(rec, outputs=[out], record_path=record_path)
    return _common.success(record_path, [out], measured=measured)


def _place(prop, spec: dict) -> None:
    """Yaw, align a weapon's long axis, scale to size, then put the origin where the placement says."""
    import math

    from mathutils import Matrix, Vector

    _common.yaw(prop, spec["yaw_deg"])

    long_axis_blender = Vector(_common.gltf_axis_to_blender(spec["long_axis_gltf"]))
    front_blender = Vector(_common.gltf_axis_to_blender(spec["front_gltf"]))
    if spec["placement"] == "grip":
        # Turn the named axis onto the authoring long axis (hilt low, tip
        # high), then roll about it so the front faces the authoring front.
        # Rotations are baked into the mesh here so the bounds below are
        # measured in the frame that ships.
        axis = Vector(_common.parse_blender_axis(spec["long_axis"]))
        align = axis.rotation_difference(long_axis_blender).to_matrix().to_4x4()
        roll = Matrix.Rotation(math.radians(spec["roll_deg"]), 4, long_axis_blender)
        prop.data.transform(roll @ align)
        prop.data.update()
        _common.log(
            TAG,
            f"grip: lift {spec['long_axis']} -> {_common.describe_axis(long_axis_blender)} (glTF {spec['long_axis_gltf']}), "
            f"rolled {spec['roll_deg']:.1f} deg; the front should face {_common.describe_axis(front_blender)} (glTF {spec['front_gltf']})",
        )

    lo, hi = _common.bounds(prop)
    size = hi - lo
    if spec["height"] is not None:
        scale = spec["height"] / size.z if size.z > 1e-6 else 1.0
    else:
        longest = max(size)
        scale = spec["length"] / longest if longest > 1e-6 else 1.0
    prop.scale = (scale, scale, scale)

    lo = lo * scale
    hi = hi * scale
    centre = (lo + hi) / 2.0
    if spec["placement"] == "grip":
        # The grip point is ``--grip`` metres up the long axis from the hilt
        # end; it goes to the origin, with the cross-section centred. The
        # long axis is +Z here (the authoring frame converted), and the
        # general form below keeps it honest if a profile ever says otherwise.
        along = long_axis_blender
        hilt = Vector(tuple(lo[i] if along[i] >= 0 else hi[i] for i in range(3)))
        location = -centre
        for i in range(3):
            if abs(along[i]) > 0.5:
                location[i] = -(hilt[i] + along[i] * spec["grip"])
        prop.location = location
    elif spec["placement"] == "held":
        prop.location = -centre
    elif spec["placement"] == "hang":
        prop.location = Vector((-centre.x, -centre.y, -hi.z))
    else:
        prop.location = Vector((-centre.x, -centre.y, -lo.z))

    _common.apply_transforms(prop)


if __name__ == "__main__":
    try:
        import bpy  # noqa: F401  (running inside Blender)
    except ImportError:
        print(_common.not_in_blender_message("prop"))
        sys.exit(2)
    else:
        sys.exit(_common.dispatch(run_in_blender, _common.argv_after_dashes(), tag=TAG))
