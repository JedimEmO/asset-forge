"""What the four Blender modules share: the dispatch, the import, the matte, the export, the record.

Each module under ``forge_gen.blender`` is two programs in one file. Run by
``forge-gen`` under the system python it is the *outer* half — it validates
the arguments, finds Blender through ``launcher.blender_bin`` and hands the
file itself to ``blender --background --factory-startup --python <file> --
<argv>``. Run by Blender it is the *inner* half: ``import bpy`` succeeds,
the work happens, a record is written, and the last stdout line is one
JSON object the outer relays. This module is what the inner halves share,
and it is written so that it imports under either interpreter: ``bpy``,
``bmesh`` and ``mathutils`` are only ever imported inside functions.

Three things here were learned the hard way and are kept on purpose:

* **Blender's own stdout is a C buffer.** When stdout is a pipe, the banner
  Blender prints at start-up sits in a ``FILE*`` buffer until exit, and
  lands *after* every line the script printed — including the JSON line
  the launcher holds back as the result. :func:`emit_json` flushes the C
  streams first, so the JSON line really is last.
* **``sys.exit(code)`` from a ``--python`` script is Blender's exit code**,
  and it skips the ``Blender quit`` line; an uncaught exception is mapped to
  ``--python-exit-code 5`` by the launcher. :func:`dispatch` turns every
  refusal into the code the exit-code table assigns it and prints the
  payload the outer expects, so exit 4 from inside Blender is seen as
  ``input_rejected`` with its reason, not as "blender exited 4".
* **Keep the generator's normals; ship double-sided.** No normal
  recalculation on import and ``use_backface_culling = False`` on every
  material: a lifted mesh is an open shell, recalculating normals on an
  open surface has no outside to agree on, and a flipped patch is culled
  by the renderer into a hole that was not in the geometry.
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from pathlib import Path

#: The ``python/`` directory this package lives under — what goes on
#: ``sys.path`` when Blender runs one of these files directly.
PYTHON_DIR = Path(__file__).resolve().parents[2]

if __package__ in (None, "") or "forge_gen" not in sys.modules:
    # Running as a bare file under Blender: make the package importable
    # before anything below asks for it.
    if str(PYTHON_DIR) not in sys.path:
        sys.path.insert(0, str(PYTHON_DIR))

from forge_gen import exit_codes, records  # noqa: E402
from forge_gen.exit_codes import BackendFailed, ForgeGenError, InputRejected  # noqa: E402

#: The backend name every record written from inside Blender carries.
BACKEND_NAME = "blender"

#: The ``tool`` every record written from inside Blender carries.
TOOL = "blender"

#: The glTF importer's ``bone_heuristic`` values in order of preference.
#: TEMPERANCE keeps glTF's own bone directions; the others reorient bones
#: to look tidy in the viewport, which silently rewrites a rest pose.
BONE_HEURISTICS = ("TEMPERANCE", "BLENDER", "FORTUNE")

#: Vertices closer than this are the same vertex (metres, post-import).
MERGE_DISTANCE_M = 1e-4


# ------------------------------------------------------------ the dispatch --


def argv_after_dashes(argv: list[str] | None = None) -> list[str]:
    """The arguments after ``--`` — what Blender leaves for the script."""
    argv = list(sys.argv if argv is None else argv)
    if "--" in argv:
        return argv[argv.index("--") + 1 :]
    return []


def not_in_blender_message(command: str) -> str:
    """The one line a module prints when run by a python that has no ``bpy``."""
    return f"run me through forge-gen: python3 python/forge_gen {command} ... (this file is the Blender half)"


def flush_all() -> None:
    """Flush Python's stdout/stderr and then every C stdio stream.

    The C flush is what puts Blender's start-up banner *before* the JSON
    line rather than after it; without it the banner is the last line of
    the pipe and the launcher's held-line trick sees prose instead of JSON.
    ``ctypes.CDLL(None)`` is the POSIX way to reach the running process's
    own C runtime (``dlopen(NULL, ...)``); Windows' loader has no such
    "the calling program itself" handle and raises on a ``None`` name, so
    that name is only used there — ``msvcrt``, the C runtime every Windows
    Python build links against, is asked for by name instead.
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.flush()
        except (OSError, ValueError):
            pass
    try:
        import ctypes

        libc = ctypes.CDLL("msvcrt") if os.name == "nt" else ctypes.CDLL(None)
        libc.fflush(None)
    except (OSError, AttributeError, ValueError, TypeError):
        pass


def log(tag: str, message: str) -> None:
    """One progress line on stdout, prefixed the way the skills quote them."""
    sys.stdout.write(f"{tag}: {message}\n")
    sys.stdout.flush()


def emit_json(payload: dict) -> None:
    """Print one JSON object as the last line of stdout."""
    flush_all()
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    flush_all()


def dispatch(main, argv: list[str], *, tag: str) -> int:
    """Run an inner ``main(argv) -> dict`` and turn its outcome into an exit code.

    A returned dict is printed as the success payload (``ok`` set). A
    :class:`ForgeGenError` is printed as its payload and returned as its
    code; anything else is a traceback on stderr and a ``backend_failed``
    payload with exit 5 — still a JSON last line, so the outer never has to
    parse prose.
    """
    try:
        result = main(argv)
        if not isinstance(result, dict):
            result = {"result": result}
        result.setdefault("ok", True)
        emit_json(result)
        return exit_codes.OK
    except ForgeGenError as err:
        sys.stderr.write(f"{tag}: {err.error}: {err.message}\n")
        emit_json(err.payload())
        return err.code
    except SystemExit as err:
        # argparse inside Blender: its usage message already went to stderr.
        code = err.code if isinstance(err.code, int) else exit_codes.USAGE
        if code != exit_codes.OK:
            emit_json({"ok": False, "error": "usage", "message": f"{tag}: bad arguments inside Blender (exit {code})"})
        return code
    except Exception as err:  # noqa: BLE001 - the last line must still be JSON
        traceback.print_exc()
        emit_json({"ok": False, "error": "backend_failed", "message": f"{tag}: {err.__class__.__name__}: {err}"})
        return exit_codes.BACKEND_FAILED


# --------------------------------------------------------------- the outer --


def passthrough_argv(args) -> list[str]:
    """The common flags the outer forwards to the inner: ``--project``, ``--created-by``."""
    argv: list[str] = []
    project = getattr(args, "project", None)
    if project:
        argv += ["--project", os.fspath(Path(project).resolve())]
    created_by = getattr(args, "created_by", None)
    if created_by:
        argv += ["--created-by", str(created_by)]
    return argv


def add_inner_flags(parser) -> None:
    """The flags every inner parser accepts from :func:`passthrough_argv`."""
    parser.add_argument("--project", default=None)
    parser.add_argument("--created-by", default=None)


def apply_inner_flags(args) -> None:
    """Inside Blender: remember the project so record paths come out relative to it."""
    if getattr(args, "project", None):
        records.set_project(args.project)


def run_blender_module(module_file: str | os.PathLike, argv: list[str], *, timeout: float | None = None) -> dict:
    """The outer half's one call: run this file under Blender and return its JSON line.

    Refusals come back as the exception the inner raised (exit 4 is
    ``InputRejected`` with its reason); a missing Blender is ``MissingTool``
    before anything runs.
    """
    from forge_gen import launcher

    return launcher.run_blender_checked(os.fspath(Path(module_file).resolve()), argv, timeout=timeout)


def existing_file(path: str | os.PathLike, *, what: str) -> Path:
    """An input that must exist, as an absolute path — or a usage error naming it."""
    from forge_gen.exit_codes import UsageError

    target = Path(path).expanduser()
    if not target.is_file():
        raise UsageError(f"{what} {target} does not exist")
    return target.resolve()


def outer_result(inner: dict) -> dict:
    """What the outer returns to the CLI from the inner's payload: record, outputs, and the rest."""
    out = {key: value for key, value in inner.items() if key not in ("ok",)}
    out.setdefault("record", None)
    out.setdefault("outputs", [])
    return out


# -------------------------------------------------------------- the record --


def blender_identity() -> dict:
    """``{"version", "build_hash", "python"}`` of the Blender this runs inside."""
    import bpy

    build_hash = bpy.app.build_hash
    if isinstance(build_hash, bytes):
        build_hash = build_hash.decode("ascii", "replace")
    return {
        "version": ".".join(str(v) for v in bpy.app.version),
        "build_hash": str(build_hash) if build_hash and build_hash != "Unknown" else None,
        "python": ".".join(str(v) for v in sys.version_info[:3]),
    }


def backend_block() -> dict:
    """The record's ``backend`` block for a Blender run.

    ``commit`` is Blender's build hash — the closest thing a binary release
    has to a pinned commit — and ``model`` is the version string, because a
    glTF export is not byte-stable across Blender versions and a reader
    wants to know which one wrote the file.
    """
    identity = blender_identity()
    return records.backend_block(
        name=BACKEND_NAME,
        commit=identity["build_hash"],
        python=identity["python"],
        torch=None,
        model=f"blender {identity['version']}",
        model_revision=None,
    )


def new_record(kind: str, *, created_by: str | None) -> dict:
    """A fresh record of ``kind`` with the Blender backend block filled in."""
    rec = records.new_record(kind, TOOL, created_by=created_by)
    rec["backend"] = backend_block()
    return rec


def finish_record(rec: dict, *, outputs: list[str | os.PathLike], record_path: str | os.PathLike) -> Path:
    """Hash the outputs into the record and write it atomically."""
    for path in outputs:
        records.add_output(rec, path)
    return records.write(rec, record_path)


def success(record_path: str | os.PathLike | None, outputs: list[str | os.PathLike], **extra) -> dict:
    """The inner success payload: absolute record and output paths, plus whatever the command adds."""
    payload = {
        "ok": True,
        "record": str(Path(record_path).resolve()) if record_path is not None else None,
        "outputs": [str(Path(p).resolve()) for p in outputs],
    }
    payload.update(extra)
    return payload


# -------------------------------------------------------------- the import --


def import_gltf(path: str | os.PathLike, *, keep_bone_directions: bool = False) -> list:
    """``bpy.ops.import_scene.gltf`` and the list of objects it created.

    With ``keep_bone_directions`` the importer is asked for ``TEMPERANCE``
    (or the nearest thing this build offers) so a rig's rest pose comes in
    exactly as the file states it; the enum has changed names across
    releases, so whatever is available is picked and said.
    """
    import bpy

    kwargs: dict = {"filepath": os.fspath(Path(path).resolve())}
    properties = bpy.ops.import_scene.gltf.get_rna_type().properties
    if "disable_bone_shape" in properties:
        # The 4.x importer hangs an icosphere custom shape on every bone of
        # a skinned file — an object outside the scene that a "join every
        # mesh" step would otherwise find.
        kwargs["disable_bone_shape"] = True
    if keep_bone_directions:
        kwargs["guess_original_bind_pose"] = False
        try:
            prop = bpy.ops.import_scene.gltf.get_rna_type().properties["bone_heuristic"]
            available = [item.identifier for item in prop.enum_items]
            for preferred in BONE_HEURISTICS:
                if preferred in available:
                    kwargs["bone_heuristic"] = preferred
                    break
            log("import", f"bone_heuristic={kwargs.get('bone_heuristic')} (available: {', '.join(available)})")
        except (KeyError, AttributeError):
            log("import", "bone_heuristic unavailable, using importer default")
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(**kwargs)
    return [obj for obj in bpy.data.objects if obj not in before]


def import_single_mesh(path: str | os.PathLike, *, name: str, tag: str) -> object:
    """Bring a lifted file in as exactly one free mesh object called ``name``.

    The superset of what the prop and rig imports used to do separately:
    every mesh the importer made is freed from whatever hierarchy the file
    had (keeping its world transform), stripped of any skin it arrived with
    — modifiers, vertex groups, parent — and joined into one object; every
    non-mesh object the import created (an armature, an empty) is removed.
    A lift has none of that scaffolding; a file that does is not trusted to
    have it right, because the rig step binds from scratch.
    """
    import bpy

    imported = import_gltf(path)
    if not imported:
        raise InputRejected(f"{path} imported nothing — not a glTF file Blender can read")
    in_scene = set(bpy.context.scene.objects)
    meshes = [obj for obj in imported if obj.type == "MESH" and obj in in_scene]
    if not meshes:
        raise InputRejected(f"{path} holds no meshes")

    for obj in meshes:
        world = obj.matrix_world.copy()
        obj.parent = None
        obj.matrix_world = world
        obj.modifiers.clear()
        obj.vertex_groups.clear()
    for obj in imported:
        if obj.type != "MESH" or obj not in in_scene:
            bpy.data.objects.remove(obj, do_unlink=True)
    # A skinned import leaves its armature datablock behind once the object
    # is gone; it would otherwise be saved into the .blend as an orphan.
    for armature in list(bpy.data.armatures):
        if armature.users == 0:
            bpy.data.armatures.remove(armature)

    bpy.ops.object.select_all(action="DESELECT")
    for obj in meshes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
        log(tag, f"joined {len(meshes)} mesh objects into one")
    single = bpy.context.view_layer.objects.active
    single.name = name
    single.data.name = name
    return single


def apply_transforms(obj) -> None:
    """Bake location, rotation and scale into the mesh data."""
    import bpy

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def yaw(obj, degrees: float) -> None:
    """Turn the object about world +Z and bake it in.

    Through the world matrix, not ``rotation_euler``: the glTF importer
    leaves its objects in quaternion rotation mode, where an Euler nudge is
    silently ignored — the first ``--yaw-deg`` did nothing and the fit gate
    then judged the unturned mesh.
    """
    import math

    from mathutils import Matrix

    if not degrees:
        return
    obj.matrix_world = Matrix.Rotation(math.radians(degrees), 4, "Z") @ obj.matrix_world
    apply_transforms(obj)


def merge_and_drop_loose(obj, *, tag: str) -> int:
    """Merge coincident vertices and drop loose ones; returns how many went.

    The generator's own normals are kept: recalc is unreliable on the open
    shells these meshes carry, and a flipped patch culls into a hole (the
    rig step learned this on the back of a skull).
    """
    import bmesh

    mesh = bmesh.new()
    mesh.from_mesh(obj.data)
    before = len(mesh.verts)
    bmesh.ops.remove_doubles(mesh, verts=mesh.verts[:], dist=MERGE_DISTANCE_M)
    loose = [v for v in mesh.verts if not v.link_faces]
    if loose:
        bmesh.ops.delete(mesh, geom=loose, context="VERTS")
    mesh.to_mesh(obj.data)
    mesh.free()
    obj.data.update()
    merged = before - len(obj.data.vertices)
    if merged:
        log(tag, f"merged/dropped {merged} verts in cleanup")
    return merged


# -------------------------------------------------------------- measuring --


def bounds(obj) -> tuple:
    """``(lo, hi)`` of the object's vertices in world space, as ``mathutils.Vector``."""
    from mathutils import Vector

    coords = [obj.matrix_world @ v.co for v in obj.data.vertices]
    if not coords:
        zero = Vector((0.0, 0.0, 0.0))
        return zero, zero
    lo = Vector((min(c[i] for c in coords) for i in range(3)))
    hi = Vector((max(c[i] for c in coords) for i in range(3)))
    return lo, hi


def bounds_gltf(obj) -> dict:
    """The world bounds in the exported (glTF, +Y up) frame: ``{"min": [...], "max": [...]}``.

    Blender ``(x, y, z)`` is glTF ``(x, z, -y)``, so the glTF Z extent is
    the negated Blender Y extent with min and max swapped.
    """
    lo, hi = bounds(obj)
    return {
        "min": [round(lo.x, 5), round(lo.z, 5), round(-hi.y, 5)],
        "max": [round(hi.x, 5), round(hi.z, 5), round(-lo.y, 5)],
    }


def triangle_count(obj) -> int:
    """Triangles the mesh will draw as, counting every n-gon as n-2."""
    return sum(max(len(p.vertices) - 2, 0) for p in obj.data.polygons)


def describe_bounds(obj) -> str:
    """``x -0.100..+0.100  y ...  z ... m`` in Blender axes, for the log."""
    lo, hi = bounds(obj)
    return f"x {lo.x:+.3f}..{hi.x:+.3f}  y {lo.y:+.3f}..{hi.y:+.3f}  z {lo.z:+.3f}..{hi.z:+.3f} m"


# ------------------------------------------------------------ the material --


def matte(obj, *, metallic: float, roughness: float, double_sided: bool) -> int:
    """Keep the baked base colour, silence every other PBR opinion. Returns materials touched.

    The style register applied with a hatchet: TRELLIS's normal and
    metallic-roughness maps model real-world response, and this library
    paints its lighting into the diffuse and wants nothing arguing with it.
    Every link into the Principled BSDF except Base Color is cut, metallic
    and roughness are forced to the profile's numbers, the image nodes that
    fed the cut links are deleted (so their textures are not packed and
    exported for nothing), and the material ships double-sided — an open
    shell's culled back face reads as a hole.
    """
    touched = 0
    for slot in obj.material_slots:
        material = slot.material
        # ``node_tree is None`` rather than ``not use_nodes``: the flag is
        # deprecated in Blender 5.x and gone in 6.
        if material is None or material.node_tree is None:
            continue
        principled = next((n for n in material.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if principled is None:
            continue
        for input_socket in principled.inputs:
            if input_socket.name == "Base Color":
                continue
            for link in list(input_socket.links):
                material.node_tree.links.remove(link)
        principled.inputs["Metallic"].default_value = float(metallic)
        principled.inputs["Roughness"].default_value = float(roughness)
        material.use_backface_culling = not double_sided
        keep = {link.from_node for link in principled.inputs["Base Color"].links}
        for node in list(material.node_tree.nodes):
            if node.type == "TEX_IMAGE" and node not in keep:
                material.node_tree.nodes.remove(node)
        touched += 1
    return touched


def pack_images() -> int:
    """Pack every image that still points at a file; returns how many were packed."""
    import bpy

    packed = 0
    for image in bpy.data.images:
        if image.packed_file is None and image.has_data:
            image.pack()
            packed += 1
    return packed


def count_images() -> int:
    """Image datablocks with pixels — what the export will embed."""
    import bpy

    return sum(1 for image in bpy.data.images if image.has_data or image.packed_file is not None)


# --------------------------------------------------------------- the export --


def export_glb(path: str | os.PathLike, *, materials: str = "EXPORT", y_up: bool = True, use_selection: bool = False) -> Path:
    """``bpy.ops.export_scene.gltf`` with the character exporter's settings.

    glTF Binary, no animations, skins on, materials as asked (``EXPORT`` for
    a body or prop, whose look is its textures; ``NONE`` for the bare rig),
    modifiers not applied (``export_apply=False`` — the Armature modifier is
    the skin, and applying it would bake the rest pose into the vertices).
    No WebP: ``bevy_gltf`` does not decode it, and the exporter's default
    image format (PNG for what was PNG) is what a consumer reads. The
    exporter's ``export_loglevel`` is left alone: passing it explicitly
    trips a ``KeyError: 'loglevel'`` inside the 5.2 add-on.
    """
    import bpy

    target = Path(path).resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    kwargs = {
        "filepath": os.fspath(target),
        "export_format": "GLB",
        "export_animations": False,
        "export_skins": True,
        "export_materials": materials,
        "export_apply": False,
        "export_yup": bool(y_up),
        "use_selection": bool(use_selection),
    }
    available = bpy.ops.export_scene.gltf.get_rna_type().properties.keys()
    if "export_image_add_webp" in available:
        kwargs["export_image_add_webp"] = False

    bpy.ops.export_scene.gltf(**kwargs)
    return target


# ---------------------------------------------------------------- the axes --

#: glTF axis letter → Blender unit vector. glTF is +Y up, -Z forward;
#: Blender is +Z up, -Y forward: ``(x, y, z)_gltf = (x, z, -y)_blender``.
_GLTF_TO_BLENDER = {
    "X": (1.0, 0.0, 0.0),
    "Y": (0.0, 0.0, 1.0),
    "Z": (0.0, -1.0, 0.0),
}


def gltf_axis_to_blender(axis: str) -> tuple[float, float, float]:
    """``"+Y"`` (glTF) → ``(0, 0, 1)`` (Blender); ``"-Z"`` → ``(0, 1, 0)``."""
    text = axis.strip().upper()
    sign = -1.0 if text.startswith("-") else 1.0
    letter = text.lstrip("+-")
    if letter not in _GLTF_TO_BLENDER:
        raise ValueError(f"{axis!r} is not an axis (want one of ±X, ±Y, ±Z)")
    return tuple(sign * v for v in _GLTF_TO_BLENDER[letter])  # type: ignore[return-value]


def parse_blender_axis(axis: str) -> tuple[float, float, float]:
    """``"-x"`` → ``(-1, 0, 0)`` in Blender's own axes, as ``--long-axis`` is given."""
    text = axis.strip().lower()
    sign = -1.0 if text.startswith("-") else 1.0
    letter = text.lstrip("+-")
    unit = {"x": (1.0, 0.0, 0.0), "y": (0.0, 1.0, 0.0), "z": (0.0, 0.0, 1.0)}.get(letter)
    if unit is None:
        raise ValueError(f"{axis!r} is not an axis (want x, y or z with an optional sign)")
    return tuple(sign * v for v in unit)  # type: ignore[return-value]


def describe_axis(vector) -> str:
    """``(0, 0, 1)`` → ``"+Z"``; anything off-axis is printed as numbers."""
    for letter, index in (("X", 0), ("Y", 1), ("Z", 2)):
        value = vector[index]
        if abs(abs(value) - 1.0) < 1e-6 and all(abs(vector[i]) < 1e-6 for i in range(3) if i != index):
            return ("+" if value > 0 else "-") + letter
    return f"({vector[0]:.2f}, {vector[1]:.2f}, {vector[2]:.2f})"


__all__ = [
    "BACKEND_NAME",
    "TOOL",
    "BackendFailed",
    "InputRejected",
    "add_inner_flags",
    "apply_inner_flags",
    "apply_transforms",
    "argv_after_dashes",
    "backend_block",
    "blender_identity",
    "bounds",
    "bounds_gltf",
    "count_images",
    "describe_axis",
    "describe_bounds",
    "dispatch",
    "emit_json",
    "existing_file",
    "export_glb",
    "finish_record",
    "flush_all",
    "gltf_axis_to_blender",
    "import_gltf",
    "import_single_mesh",
    "log",
    "matte",
    "merge_and_drop_loose",
    "new_record",
    "not_in_blender_message",
    "outer_result",
    "pack_images",
    "parse_blender_axis",
    "passthrough_argv",
    "run_blender_module",
    "success",
    "triangle_count",
    "yaw",
]
