"""Skin a prepared mesh, fit the skeleton to it, and skin it again on the skeleton that fits.

    forge-gen skin out/prepare/<name>.glb
            [--source out/lifts/<name>.glb] [--out out/skin/<name>.skinned.glb]
            [--blend assets-src/blender/<name>.blend] [--record <name>.rig.json]
            [--work out/skin/<name>] [--name vex_runner] [--profile DIR]
            [--top-k 5 --top-p 0.95 --temperature 1.0 --repetition-penalty 2.0
             --num-beams 10] [--model-ckpt PATH] [--hf-path PATH]
            [--min-free-gb 14 | --allow-busy] [--timeout 1800] [--json]

The second of the two doors that make a body, and the whole of the fitted
skeleton: **five steps, one door, and no options about the number of
passes.**

1. **skin** the prepared glb through SkinTokens' ``demo.py --use_skeleton``;
2. **fit** — ``forge_gen.fit`` reads those weights and says how long every
   bone is *on this body*, once;
3. **build the armature** — ``blender/fit_rig.py`` scales the profile's own
   ``rig.blend`` by the fit's per-run ratios, keeping every rest **rotation**
   and aborting if one moves;
4. **prepare again** against that skeleton and **skin again**, because
   re-attaching pass-1 weights to joints that moved up to 14.7 cm would bind
   the body to the skeleton the fit just corrected;
5. **re-attach** — ``blender/reattach.py`` puts the returned weights onto the
   fitted armature by joint *order* and writes the working ``.blend``.

There is **no ``--passes``, no ``--diagnose`` and no ``--skip-refit``**: the
spike measured the second fit walking the torso downhill 73.5 mm a time, so a
knob whose only correct value is off is surface rather than a diagnostic.
``fit.convergence()`` survives as a library function with no caller here and
is exercised by ``test_fit.py`` against the two frozen reports.

Two SkinTokens runs at ~27 s, two prepares at ~1.7 s, one armature build at
~5 s and one re-attach at ~5 s: about a minute, one card, one job at a time.

# What it reads back, and what it refuses

After each skin it reads the output as an outside consumer and says four
things:

* **which bones came back**, against the profile's 55 contract names: the
  question is whether our names survive a model whose templates are Mixamo
  and VRoid, and a rig missing one contract bone is a rig every clip in the
  library binds to partially;
* **weighted vertices per bone**, so the plated shells the ladder used to
  rescue (a lamp on a shoulder pad, an exo-brace on a shin) can be seen
  landing somewhere sane rather than orbiting an arm;
* **the unweighted fraction**, against the profile's own
  ``[rig] unweighted_abort_fraction`` — the shell-abort gate, and the one
  refusal in this file that is about the weights themselves;
* **influences per vertex**, against ``[export] max_influences``.

Beyond that abort it reports and does not gate. Go or no-go on a body is a
human call made on the *strip rendered on the real body* (`just sheet walk`),
not on this table — numbers say a rig is wired, a picture says what it is.
What this door refuses otherwise is only what would make the numbers
meaningless: a busy card, a stale bpy_server, a mesh that arrives already
skinned, a fit whose own gate says the measurement is not trustworthy.

The record it writes is a real one, ``kind: "rig"``, ``tool: "skintokens"``,
through ``records.py`` like every other generator — hashing the prepared glb
as its ``mesh`` input and the skinned glb as its output, and stating every
sampling knob it passed, plus the whole ``fit`` block: the ratios, the runs
they were measured on, the raw left/right disagreement, the grounding
factors and the ``motion_scale`` a consumer multiplies a root track by. It
claims **integrity and never reproduction**: ``demo.py`` samples with
``do_sample=True`` and takes no seed, so ``seed`` is ``null`` because it is
unknown, not because it is zero. The record names its ``skinner`` and the
encoder's licence note the way a lift record names its ``texture_baker``: a
licence fact that lives only in an installer is a fact nobody reading a
record can see.

Two traps upstream's ``demo.py`` carries, both learned by reading it:

* it starts ``bpy_server.py`` itself, in its **own process group**
  (``preexec_fn=os.setsid``), and cleans it up from an ``atexit`` hook — so
  a killed or timed-out run leaves that server alive holding port 59876,
  and the *next* run pings it, finds it healthy, and quietly talks to a
  server from the previous environment. This script therefore refuses to
  start while the port is taken, and says so again if one survives its own
  run. Kill it by PID (`ss -lptn 'sport = :59876'`), never by pattern:
  ``pkill -f`` matches the shell running it;
* its ``--model_ckpt`` default and the checkpoints ``download.py`` fetches
  are paths **relative to the checkout**, and it launches ``bpy_server.py``
  by bare name, so the run only works with the checkout as its working
  directory. That is what ``backend.toml``'s ``cwd = "checkout"`` says, and
  this file does the same when the backend is not described yet.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import subprocess
import sys
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from forge_gen import doctor, glb as glb_mod, launcher, profile as profile_mod, records, vram_cap  # noqa: E402
from forge_gen.exit_codes import BackendFailed, InputRejected, MissingBackend, UsageError  # noqa: E402

#: The log prefix; the spike notes quote these lines.
TAG = "skin"

#: The backend directory and record name.
BACKEND_NAME = "skintokens"

#: What the record's ``params.skinner`` says — the string a reader of a body
#: sees, the way a lift record's ``texture_baker`` names nvdiffrast.
SKINNER = "skintokens (VAST-AI/SkinTokens, MIT)"

#: The licence fact that travels with every rig this tool makes.
SKINNER_NOTE = (
    "code and weights MIT; the vendored Michelangelo point-cloud encoder files carry "
    "an open licence question upstream (SkinTokens issue #9)"
)

#: ``demo.py``'s own default checkpoint, restated so the record states it.
#: Never inherited: a recipe names every knob, and a knob read out of
#: somebody else's default is a knob nobody wrote down.
DEFAULT_MODEL_CKPT = "experiments/articulation_xl_quantization_256_token_4/grpo_1400.ckpt"

#: ``demo.py``'s sampling defaults, restated for the same reason.
DEFAULT_SAMPLING = {
    "top_k": 5,
    "top_p": 0.95,
    "temperature": 1.0,
    "repetition_penalty": 2.0,
    "num_beams": 10,
}

#: The port ``bpy_server.py`` binds (``src/server/spec.py`` ``BPY_PORT``).
BPY_PORT = 59876

#: What the plan budgets for a SkinTokens run; upstream says "at least 14 GB".
DEFAULT_MIN_FREE_GB = 14.0

#: Where the outputs land when ``--out``/``--work`` are not given, under the project.
DEFAULT_OUT_DIR = Path("out") / "skin"

#: Where the working ``.blend`` and its rig record land: the committed,
#: non-derivable source tree ``forge promote body`` reads.
DEFAULT_BLEND_DIR = Path("assets-src") / "blender"

#: A weight at or below this is not an influence.
WEIGHT_EPSILON = 1e-6

#: The fit's own band on a run's ratio, and the thinnest band it will average
#: over. Shipped defaults, restated here because the door states every knob
#: it runs the fit with rather than inheriting one from a parser.
RATIO_MIN = 0.4
RATIO_MAX = 2.5
DEFAULT_MIN_SUPPORT = 8.0

#: What a ``--fake`` skeleton's bones are scaled by. Not 1.0 on purpose: a
#: placeholder at exactly the contract's lengths would exercise none of the
#: schema the fitted skeleton exists to fill.
FAKE_FIT_RATIO = 0.95


# --------------------------------------------------------------- arguments --


def _add_arguments(parser: argparse.ArgumentParser, *, common: bool = True) -> None:
    """Every flag. ``common=False`` when ``cli.py`` is folding its own in."""
    parser.add_argument("glb", help="the prepared glb: normalised mesh + the profile's armature, no vertex groups")
    parser.add_argument(
        "--source",
        metavar="GLB",
        help="the lift the prepared glb was made from, re-prepared against the fitted skeleton "
        "(default: the mesh input of the prepare record beside it)",
    )
    parser.add_argument("--out", metavar="GLB", help=f"where the skinned glb goes (default: {DEFAULT_OUT_DIR}/<name>.skinned.glb)")
    parser.add_argument("--blend", metavar="BLEND", help=f"where the rigged .blend goes (default: {DEFAULT_BLEND_DIR}/<name>.blend)")
    parser.add_argument("--record", metavar="JSON", help="where the generator record goes (default: <name>.rig.json beside the .blend)")
    parser.add_argument("--work", metavar="DIR", help=f"where the loop's intermediates go (default: {DEFAULT_OUT_DIR}/<name>)")
    parser.add_argument("--name", metavar="NAME", help="[a-z0-9_]+ library name for the record (default: the input's stem)")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or the project's)")
    parser.add_argument("--top-k", type=int, default=DEFAULT_SAMPLING["top_k"], metavar="N")
    parser.add_argument("--top-p", type=float, default=DEFAULT_SAMPLING["top_p"], metavar="P")
    parser.add_argument("--temperature", type=float, default=DEFAULT_SAMPLING["temperature"], metavar="T")
    parser.add_argument("--repetition-penalty", type=float, default=DEFAULT_SAMPLING["repetition_penalty"], metavar="R")
    parser.add_argument("--num-beams", type=int, default=DEFAULT_SAMPLING["num_beams"], metavar="N")
    parser.add_argument("--model-ckpt", default=DEFAULT_MODEL_CKPT, metavar="PATH", help="checkpoint, relative to the checkout")
    parser.add_argument("--hf-path", default=None, metavar="PATH", help="a local transformer to load over the checkpoint's (upstream's --hf_path)")
    parser.add_argument("--min-free-gb", type=float, default=DEFAULT_MIN_FREE_GB, metavar="GB", help="refuse to start with less free VRAM than this")
    parser.add_argument("--allow-busy", action="store_true", help="start anyway on a busy card (you have read the doctor line and mean it)")
    parser.add_argument("--timeout", type=float, default=1800.0, metavar="S", help="kill the run after this long")
    parser.add_argument(
        "--analyse-only",
        action="store_true",
        help="re-read the skinned glb and the fit report this run already wrote and re-write the record, "
        "spending no card — the same numbers, no second sample",
    )
    if common:
        parser.add_argument("--json", action="store_true", help="last stdout line is one JSON object")
        parser.add_argument("--fake", action="store_true", help="write placeholder outputs through the same doors")
        parser.add_argument("--project", default=None, metavar="DIR", help="the project root paths are written relative to")
        parser.add_argument("--created-by", default=None, metavar="WHO", help="human | agent:<name> | unknown")


def add_parser(subparsers) -> None:
    parser = subparsers.add_parser(
        "skin",
        help="Prepared glb -> SkinTokens weights on a skeleton fitted to this body",
        description=__doc__,
    )
    _add_arguments(parser, common=False)


def log(message: str) -> None:
    """One progress line, the shape every skill quotes."""
    sys.stdout.write(f"{TAG}: {message}\n")
    sys.stdout.flush()


# --------------------------------------------------------- where it all is --


def _profile(directory: str | None) -> profile_mod.Profile:
    try:
        return profile_mod.load_profile(directory)
    except profile_mod.ProfileError as err:
        raise UsageError(str(err)) from err


def _backend() -> tuple[Path, Path, dict]:
    """``(interpreter, checkout, env)`` for SkinTokens, or :class:`MissingBackend`.

    Through ``backends.load_backend`` + ``launcher`` when the backend is
    described — one resolution order for every backend, override variable
    included. While ``backends/skintokens/backend.toml`` does not exist yet
    (the spike may run before the backend lands) the same order is walked by
    hand, so a spike run and a Phase 2 run pick the same interpreter.
    """
    from forge_gen import backends

    try:
        backend = backends.load_backend(BACKEND_NAME)
    except MissingBackend:
        return _backend_by_hand()
    interpreter = launcher.resolve_interpreter(backend)
    checkout = launcher.inner_cwd(backend) or backend.checkout.resolve()
    return interpreter, checkout, launcher.inner_env(backend, interpreter)


def _backend_by_hand() -> tuple[Path, Path, dict]:
    """The documented order — the override variable, then ``.env`` — with no ``backend.toml``."""
    directory = Path(os.environ.get("FORGE_BACKENDS", profile_mod.repo_root() / "backends")) / BACKEND_NAME
    hint = f"bash {directory}/install.sh  (or --adopt-env <prefix> --adopt-checkout <clone>)"
    override = os.environ.get(launcher.override_var(BACKEND_NAME))
    interpreter = None
    if override:
        candidate = Path(override).expanduser()
        interpreter = candidate if candidate.is_file() else launcher._python_under(candidate)
    if interpreter is None:
        interpreter = launcher._python_under(directory / ".env")
    if interpreter is None:
        raise MissingBackend(
            f"{BACKEND_NAME} is not installed — no {directory}/.env and no {launcher.override_var(BACKEND_NAME)}",
            backend=BACKEND_NAME,
            hint=hint,
        )
    checkout = directory / ".checkout"
    if not checkout.exists():
        raise MissingBackend(f"{BACKEND_NAME}'s upstream checkout is not at {checkout}", backend=BACKEND_NAME, hint=hint)
    env = dict(os.environ)
    env.setdefault("PYTHONNOUSERSITE", "1")
    env.setdefault("FORGE_BACKEND", BACKEND_NAME)
    return interpreter, checkout.resolve(), env


def _interpreter_says(interpreter: Path, expression: str) -> str | None:
    """One value out of the backend's own interpreter, or ``None`` when it will not say.

    ``None`` reaches the record as ``null``: a version this cannot read is
    unknown, and a default written in its place would be a measurement
    nobody made.
    """
    try:
        done = subprocess.run(
            [os.fspath(interpreter), "-c", expression],
            capture_output=True,
            text=True,
            timeout=180,
            check=False,
            env={**os.environ, "PYTHONNOUSERSITE": "1"},
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    value = done.stdout.strip()
    return value or None


def _commit(checkout: Path) -> str | None:
    """The checkout's HEAD, or ``None``."""
    try:
        done = subprocess.run(
            ["git", "-C", os.fspath(checkout), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() or None


# ------------------------------------------------------------- the run gate --


def _port_taken(port: int = BPY_PORT) -> bool:
    """Whether something already answers on ``localhost:<port>``."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.settimeout(0.5)
        return probe.connect_ex(("127.0.0.1", port)) == 0


def _refuse_a_stale_server() -> None:
    if not _port_taken():
        return
    raise BackendFailed(
        f"something already answers on 127.0.0.1:{BPY_PORT} — demo.py starts its own bpy_server there and only "
        "pings it, so this run would talk to whatever is already listening (usually a server orphaned by a "
        f"killed run). Find it by PID and kill that: ss -lptn 'sport = :{BPY_PORT}' — never pkill -f, which "
        "matches the shell running it",
        port=BPY_PORT,
    )


def _refuse_a_busy_card(min_free_gb: float, allow_busy: bool) -> dict:
    """One reader of the card, doctor's. The card is shared and two generates never co-reside."""
    report = doctor.gpu_report()
    if not report.get("ok"):
        log(f"WARN the card cannot be read ({report.get('error')}) — running blind")
        return report
    free_gb = (report["total_mb"] - report["used_mb"]) / 1024.0
    holders = ", ".join(f"pid {app['pid']} {app['name']} {app['used_mb'] / 1024.0:.1f} GB" for app in report.get("apps", [])) or "nothing nvidia-smi can see"
    log(f"{report['name']}: {free_gb:.1f} GB free of {report['total_mb'] / 1024.0:.1f} GB; holders: {holders}")
    if free_gb < min_free_gb and not allow_busy:
        raise BackendFailed(
            f"only {free_gb:.1f} GB of VRAM is free and this needs about {min_free_gb:.0f} — {holders}. "
            "Free the card (a studio window, a resident server) and run again, or pass --allow-busy if you "
            "have read that line and mean it",
            free_gb=round(free_gb, 2),
            apps=report.get("apps", []),
        )
    return report


# ----------------------------------------------------------------- the run --


def _demo_argv(args, source: Path, out: Path) -> list[str]:
    """``demo.py``'s command line, every knob stated."""
    argv = [
        "demo.py",
        "--input", os.fspath(source),
        "--output", os.fspath(out),
        "--use_skeleton",
        "--use_transfer",
        "--use_postprocess",
        "--top_k", str(args.top_k),
        "--top_p", str(args.top_p),
        "--temperature", str(args.temperature),
        "--repetition_penalty", str(args.repetition_penalty),
        "--num_beams", str(args.num_beams),
        "--model_ckpt", args.model_ckpt,
    ]
    if args.hf_path:
        argv += ["--hf_path", args.hf_path]
    return argv


def _run_demo(interpreter: Path, checkout: Path, env: dict, argv: list[str], timeout: float) -> None:
    """Run it from the checkout, relaying every line; refuse anything but a clean exit.

    ``demo.py`` is upstream's and not ours to edit, so the opt-in VRAM
    ceiling (``$FORGE_VRAM_CAP_GB``, off in every ordinary run) goes in
    through ``forge_gen.vram_cap``'s wrapper: it sets the ceiling and then
    runs ``demo.py`` under its own ``__main__``, with ``sys.argv`` exactly as
    it would have been. ``forge_gen`` is on the inner ``PYTHONPATH`` already.
    """
    command = [os.fspath(interpreter), *argv]
    if env.get(vram_cap.CAP_VAR, "").strip():
        command = [os.fspath(interpreter), "-m", "forge_gen.vram_cap", *argv]
        log(f"{vram_cap.CAP_VAR}={env[vram_cap.CAP_VAR]} — running demo.py through forge_gen.vram_cap")
    log(f"$ (cd {checkout} && {' '.join(command)})")
    outcome = launcher.stream(command, env=env, cwd=checkout, timeout=timeout)
    if outcome.code != 0:
        raise BackendFailed(f"demo.py exited {outcome.code}", log_tail=outcome.tail)


# ------------------------------------------------------------ reading it back --


def _read_glb(path: Path) -> tuple[dict, bytes]:
    """``(document, binary)`` of a .glb, after ``verify_glb`` has said it is self-contained."""
    try:
        info = glb_mod.verify_glb(path)
    except glb_mod.GlbError as err:
        raise BackendFailed(f"the skinned file is not a glb an engine could load: {err}") from err
    data = path.read_bytes()
    offset, binary = 12, None
    while offset + 8 <= len(data):
        length, kind = struct.unpack_from("<II", data, offset)
        offset += 8
        if kind == glb_mod.CHUNK_BIN:
            binary = data[offset : offset + length]
        offset += length + (-length % 4)
    if binary is None:
        raise BackendFailed(f"{path.name} has no BIN chunk — nothing to read the weights out of")
    return info, binary


#: glTF component type → (struct code, size).
_COMPONENTS = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}

#: glTF accessor type → component count.
_COUNTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def _accessor(document: dict, binary: bytes, index: int) -> list[tuple]:
    """One accessor read out of the BIN chunk, honouring ``byteStride``.

    Plain ``struct`` on purpose, the way ``glb.verify_glb`` reads a file: the
    question is what an outside consumer finds in the weights, and asking
    the library that wrote them would only prove it agrees with itself.
    """
    accessor = document["accessors"][index]
    if "sparse" in accessor:
        raise BackendFailed(f"accessor {index} is sparse — this reader does not handle that, and no exporter here writes one")
    code, size = _COMPONENTS[accessor["componentType"]]
    per = _COUNTS[accessor["type"]]
    count = accessor["count"]
    view = document["bufferViews"][accessor["bufferView"]]
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    stride = view.get("byteStride") or size * per
    layout = "<" + code * per
    return [struct.unpack_from(layout, binary, start + i * stride) for i in range(count)]


def _qrotate(q: tuple, v: tuple) -> tuple:
    x, y, z, w = q
    vx, vy, vz = v
    tx = 2.0 * (y * vz - z * vy)
    ty = 2.0 * (z * vx - x * vz)
    tz = 2.0 * (x * vy - y * vx)
    return (vx + w * tx + (y * tz - z * ty), vy + w * ty + (z * tx - x * tz), vz + w * tz + (x * ty - y * tx))


def _qmul(a: tuple, b: tuple) -> tuple:
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def _node_world(document: dict) -> dict[int, tuple]:
    """Every node's world position, walked from the scene roots.

    Rotations chain, scales are ignored: nothing in this pipeline exports a
    scaled bone, and a scale that did appear would show up as displacement
    in the rest-pose comparison below rather than hiding in it.
    """
    nodes = document.get("nodes", [])
    out: dict[int, tuple] = {}

    def walk(index: int, position: tuple, rotation: tuple) -> None:
        node = nodes[index]
        moved = _qrotate(rotation, tuple(float(v) for v in node.get("translation", (0.0, 0.0, 0.0))))
        here = (position[0] + moved[0], position[1] + moved[1], position[2] + moved[2])
        turned = _qmul(rotation, tuple(float(v) for v in node.get("rotation", (0.0, 0.0, 0.0, 1.0))))
        out[index] = here
        for child in node.get("children", []):
            walk(child, here, turned)

    scene = document.get("scenes", [{}])[document.get("scene", 0)]
    for root in scene.get("nodes", []):
        walk(root, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 1.0))
    return out


def _skeleton(document: dict) -> dict:
    """The first skin as a table: joint names, world rest positions, parent indices *within the skin*."""
    nodes = document.get("nodes", [])
    joints = document["skins"][0]["joints"]
    place = {node: index for index, node in enumerate(joints)}
    parents: list[int | None] = [None] * len(joints)
    for node_index, node in enumerate(nodes):
        for child in node.get("children", []):
            if child in place and node_index in place:
                parents[place[child]] = place[node_index]
    world = _node_world(document)
    return {
        "names": [nodes[node].get("name", f"<node {node}>") for node in joints],
        "world": [world.get(node, (0.0, 0.0, 0.0)) for node in joints],
        "parents": parents,
    }


def _distance(a: tuple, b: tuple) -> float:
    return sum((x - y) ** 2 for x, y in zip(a, b)) ** 0.5


def _alignment(source: dict, result: dict) -> dict:
    """How the returned skeleton lines up with the one we handed in.

    Three answers, and the middle one is the one the spike was written to
    find: ``named`` — the joints came back under our names, in our order;
    ``by_order`` — same count, same parent array, different names, so the
    model kept the skeleton it was given and only lost its labels (upstream
    exports joints as ``bone_<i>``, since a skeleton token carries geometry
    and not a name); ``none`` — it predicted its own skeleton and the
    ``--use_skeleton`` claim does not hold on this mesh.

    ``displacement`` is what matters even when the order is perfect: the
    contract rest pose is frozen and every clip in the library is baked
    against it, so a joint that came back a centimetre from where it went in
    is a rig no clip can be played on. It is measured against the *input*
    skeleton, which is the profile's own ``rig.blend`` and therefore the
    contract itself.
    """
    same_names = source["names"] == result["names"]
    same_shape = len(source["names"]) == len(result["names"]) and source["parents"] == result["parents"]
    if same_names:
        mode = "named"
    elif same_shape:
        mode = "by_order"
    else:
        mode = "none"
    gaps = (
        [_distance(a, b) for a, b in zip(source["world"], result["world"])]
        if len(source["world"]) == len(result["world"])
        else []
    )
    worst = max(range(len(gaps)), key=lambda i: gaps[i]) if gaps else None
    return {
        "mode": mode,
        "same_parent_array": same_shape,
        "joints_in": len(source["names"]),
        "joints_out": len(result["names"]),
        "displacement_max_m": round(max(gaps), 5) if gaps else None,
        "displacement_mean_m": round(sum(gaps) / len(gaps), 5) if gaps else None,
        "displacement_worst_joint": source["names"][worst] if worst is not None else None,
    }


def analyse(source_path: Path, path: Path, prof: profile_mod.Profile) -> tuple[dict, list[str]]:
    """Read the skinned glb back beside what went in, and say what the skinner did.

    Both files, not just the output, because "did our 55 bones come back" is
    only answerable against the skeleton that was handed over: a model that
    returns the right joints under generic names would otherwise read as a
    total failure, and a model that returns its own skeleton under *our*
    count would read as a success.
    """
    info, binary = _read_glb(path)
    document = info["document"]
    if not document.get("skins"):
        raise BackendFailed(
            f"{path.name} carries no skin — SkinTokens returned a mesh with no weights at all, which is the "
            "spike's answer and not a bug in this reader"
        )
    source_info, _ = _read_glb(source_path)
    source_document = source_info["document"]
    if not source_document.get("skins"):
        raise BackendFailed(f"{source_path.name} carries no skin — it is not a prepared glb from prepare_spike")

    contract = [bone["name"] for bone in prof.bones]
    handed_in = _skeleton(source_document)
    came_back = _skeleton(document)
    alignment = _alignment(handed_in, came_back)
    # What each returned joint *is*: its own name when the names survived,
    # else the name of the joint that went in at the same index — and only
    # when the hierarchy proves it is the same skeleton. Never guessed.
    if alignment["mode"] == "none":
        joints = came_back["names"]
    else:
        joints = list(handed_in["names"])
    present = [name for name in contract if name in joints]
    missing = [name for name in contract if name not in joints]
    extra = [name for name in joints if name not in contract]

    weighted: dict[str, int] = {name: 0 for name in joints}
    histogram: dict[str, int] = {}
    vertices = unweighted = 0
    primitives = 0
    max_influences = 0
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            attributes = primitive.get("attributes", {})
            sets = sorted(key for key in attributes if key.startswith("JOINTS_"))
            if not sets:
                continue
            primitives += 1
            joint_rows = [_accessor(document, binary, attributes[key]) for key in sets]
            weight_rows = [_accessor(document, binary, attributes[key.replace("JOINTS_", "WEIGHTS_")]) for key in sets]
            for vertex in range(len(joint_rows[0])):
                vertices += 1
                influences = 0
                for joint_set, weight_set in zip(joint_rows, weight_rows):
                    for joint, weight in zip(joint_set[vertex], weight_set[vertex]):
                        if weight > WEIGHT_EPSILON:
                            influences += 1
                            name = joints[joint] if joint < len(joints) else f"<joint {joint}>"
                            weighted[name] = weighted.get(name, 0) + 1
                if influences == 0:
                    unweighted += 1
                max_influences = max(max_influences, influences)
                histogram[str(influences)] = histogram.get(str(influences), 0) + 1

    if vertices == 0:
        raise BackendFailed(f"{path.name} has a skin but no primitive carrying JOINTS_0 — nothing is bound to it")

    silent = [name for name in contract if weighted.get(name, 0) == 0]
    fraction = unweighted / vertices
    abort_fraction = float(prof.section("rig")["unweighted_abort_fraction"])
    allowed_influences = int(prof.section("export").get("max_influences", 4))
    rest_tolerance = float(prof.section("export")["rest_tolerance_m"])
    source_vertices = _vertex_count(source_document)

    measured = {
        "skeleton_alignment": alignment["mode"],
        "skeleton_names_returned": came_back["names"] == handed_in["names"],
        "skeleton_same_parent_array": alignment["same_parent_array"],
        "joints_handed_in": alignment["joints_in"],
        "joints_returned": alignment["joints_out"],
        "rest_displacement_max_m": alignment["displacement_max_m"],
        "rest_displacement_mean_m": alignment["displacement_mean_m"],
        "rest_displacement_worst_joint": alignment["displacement_worst_joint"],
        "rest_tolerance_m": rest_tolerance,
        "vertices_handed_in": source_vertices,
        "vertices": vertices,
        "skinned_primitives": primitives,
        "contract_bones_present": len(present),
        "contract_bones_missing": missing,
        "joints_outside_the_contract": extra,
        "bones_with_no_weighted_vertex": silent,
        "weighted_vertices_by_bone": {name: weighted.get(name, 0) for name in contract},
        "unweighted_vertices": unweighted,
        "unweighted_fraction": round(fraction, 5),
        "unweighted_abort_fraction": abort_fraction,
        "influences_max": max_influences,
        "influences_allowed": allowed_influences,
        "influences_per_vertex": histogram,
        "glb_nodes": info["nodes"],
        "glb_images": info["images"],
        "glb_bytes": info["bytes"],
    }

    lines = [glb_mod.report(path, info)]
    if alignment["mode"] == "named":
        lines.append(f"skeleton — the {alignment['joints_out']} joints came back under our own names, in our order")
    elif alignment["mode"] == "by_order":
        lines.append(
            f"skeleton — the names are gone ({', '.join(came_back['names'][:3])}, …) but the "
            f"{alignment['joints_out']} joints came back in the order they went in, with an identical parent "
            "array: this is our skeleton, relabelled. Weights below are read through that mapping"
        )
    else:
        lines.append(
            f"skeleton — NOT ours: {alignment['joints_out']} joints came back against the "
            f"{alignment['joints_in']} handed in, and the hierarchy does not match. --use_skeleton did not hold "
            "on this mesh, and the weights below are for a skeleton nothing in the library knows"
        )
    if alignment["displacement_max_m"] is not None:
        verdict = "inside" if alignment["displacement_max_m"] <= rest_tolerance else "OUTSIDE"
        lines.append(
            f"rest pose — joints moved {alignment['displacement_mean_m'] * 1000.0:.1f} mm on average, "
            f"{alignment['displacement_max_m'] * 1000.0:.1f} mm at worst ({alignment['displacement_worst_joint']}); "
            f"that is {verdict} the contract's {rest_tolerance * 1000.0:.1f} mm. Every clip is baked against the "
            "frozen rest pose, so a skeleton that moved is one no clip plays on — take the weights, leave the rig"
        )
    lines.append(
        f"bones — {len(present)} of {len(contract)} contract bones carry weights' worth of identity, "
        f"{len(extra)} joint(s) outside the contract"
    )
    if missing:
        lines.append(f"bones MISSING — {', '.join(missing)}")
    if extra:
        lines.append(f"bones EXTRA — {', '.join(extra)}")
    lines.append(
        f"mesh — {source_vertices} vertices went in, {vertices} came back"
        + ("" if source_vertices == vertices else " (--use_transfer re-exported the mesh; the counts should match)")
    )
    lines.append(
        f"weights — {unweighted} of {vertices} vertices weightless ({fraction:.2%}; the profile aborts a "
        f"bone-heat bind above {abort_fraction:.0%})"
    )
    lines.append(
        "influences — "
        + ", ".join(f"{k}: {histogram[k]}" for k in sorted(histogram, key=int))
        + f" (max {max_influences}, the contract allows {allowed_influences})"
    )
    if silent:
        lines.append(f"bones with no weighted vertex ({len(silent)}) — {', '.join(silent)}")
    lines.append("weighted vertices per bone, contract order:")
    for name in contract:
        lines.append(f"    {name:<22} {weighted.get(name, 0)}")
    lines.append(
        "verdict is not this table's to give: render the walk on this body (just sheet walk --body <path>) "
        "and look. Numbers say a rig is wired; the picture says what it is."
    )
    return measured, lines


def _vertex_count(document: dict) -> int:
    """POSITION accessors summed over every primitive — what went in, in the same unit the output is counted in."""
    total = 0
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            index = primitive.get("attributes", {}).get("POSITION")
            if index is not None:
                total += document["accessors"][index]["count"]
    return total


# -------------------------------------------------------------- the record --


def fit_block(report: dict, prof: profile_mod.Profile) -> dict:
    """The ``fit`` object a rig record carries, from one fit report.

    Everything a reader needs to judge the skeleton this body got without
    opening the report: what each run measured and what it was given after
    mirroring, the **raw** left/right disagreement (so symmetrising hides
    nothing), the grounding factors, the two tolerances it was held to, and
    the ``motion_scale`` a consumer multiplies a root track by. ``passes`` is
    1 and says so, because that is a decision and not a default.
    """
    from forge_gen import fit as fit_mod

    ends = {run["end"]: run for run in report["runs"]}
    raw: dict[str, float] = {}
    worst = {"run": None, "gap": None}
    for end, run in sorted(ends.items()):
        if not end.startswith("Left"):
            continue
        mirror = ends.get("Right" + end[len("Left") :])
        if mirror is None:
            continue
        mean = 0.5 * (run["ratio_measured"] + mirror["ratio_measured"])
        if abs(mean) <= 1e-6:
            continue
        gap = round(abs(run["ratio_measured"] - mirror["ratio_measured"]) / abs(mean), 4)
        raw[run["run"].replace("Left", "")] = gap
        if worst["gap"] is None or gap > worst["gap"]:
            worst = {"run": run["run"], "gap": gap}
    bands = report.get("asymmetry") or fit_mod.symmetry_bands(prof)
    return {
        "passes": 1,
        "motion_scale": report["motion_scale"],
        "asymmetry_arms": bands["asymmetry_arms"],
        "asymmetry_other": bands["asymmetry_other"],
        # `None` on a report written before the door named its sources:
        # unknown, which is what a record says when it does not know.
        "sources": report.get("sources"),
        "ratios": {row["bone"]: row["ratio"] for row in report["bones"]},
        "runs": [
            {
                "run": run["run"],
                "reference_length_m": run["reference_length_m"],
                "ratio": run["ratio"],
                "ratio_measured": run["ratio_measured"],
                "support": run["support"],
                "off_axis_m": run["off_axis_m"],
                "mirrored": bool(report.get("symmetrised")) and ("Right" + run["end"][4:] in ends or "Left" + run["end"][5:] in ends),
            }
            for run in report["runs"]
        ],
        "raw_asymmetry": raw,
        "grounding": {side: values["factor"] for side, values in (report.get("grounding") or {}).items()},
        "symmetry_worst": worst,
    }


def _record(args, spec: dict, measured: dict, source: Path, outputs: list[Path], record_path: Path) -> Path:
    rec = records.new_record("rig", BACKEND_NAME, created_by=getattr(args, "created_by", None))
    rec["backend"] = records.backend_block(
        name=BACKEND_NAME,
        commit=spec["commit"],
        python=spec["python"],
        torch=spec["torch"],
        model=args.model_ckpt,
        model_revision=args.hf_path,
    )
    records.add_input(rec, "mesh", source)
    records.add_input(rec, "reference", spec["profile"].rig_blend, source=f"profile:{spec['profile'].name}")
    rec["params"] = {
        "name": spec["name"],
        "profile": spec["profile"].name,
        "profile_sha256": spec["profile_sha256"],
        "skinner": {"tool": SKINNER, "commit": spec["commit"], "note": SKINNER_NOTE},
        "fit": spec["fit"],
        "use_skeleton": True,
        "use_transfer": True,
        "use_postprocess": True,
        "top_k": args.top_k,
        "top_p": args.top_p,
        "temperature": args.temperature,
        "repetition_penalty": args.repetition_penalty,
        "num_beams": args.num_beams,
        # demo.py samples (do_sample=True) and takes no seed: this rig can be
        # hashed and never re-run. `null` because it is unknown, not zero.
        "seed": None,
        "model_ckpt": args.model_ckpt,
        "hf_path": args.hf_path,
    }
    rec["measured"] = measured
    for path in outputs:
        records.add_output(rec, path)
    return records.write(rec, record_path)


# ------------------------------------------------------------------- entry --


def _places(args, prof: profile_mod.Profile) -> dict:
    """Every path this run reads and writes, resolved once and refused early."""
    prepared = Path(args.glb).expanduser()
    if not prepared.is_file():
        raise UsageError(f"prepared mesh {prepared} does not exist — make one with `forge gen prepare <lift.glb>`")
    prepared = prepared.resolve()
    name = args.name or prepared.stem.split(".")[0]
    root = Path(records.project() or Path.cwd())
    out = Path(args.out).expanduser().resolve() if args.out else (root / DEFAULT_OUT_DIR / f"{name}.skinned.glb").resolve()
    if not out.suffix:
        raise UsageError(f"--out {out} has no suffix, and demo.py reads a suffixless --output as a directory")
    work = Path(args.work).expanduser().resolve() if args.work else (root / DEFAULT_OUT_DIR / name).resolve()
    blend = Path(args.blend).expanduser().resolve() if args.blend else (root / DEFAULT_BLEND_DIR / f"{name}.blend").resolve()
    record = Path(args.record).expanduser().resolve() if args.record else blend.with_name(f"{name}.rig.json")
    return {
        "name": name,
        "prepared": prepared,
        "lift": _lift_of(args, prepared),
        "profile": prof,
        "pass1": work / f"{name}.p1.skinned.glb",
        "prepared2": work / f"{name}.p2.glb",
        "prepare2_record": work / f"{name}.p2.prepare.json",
        "skinned": out,
        "work": work,
        "blend": blend,
        "record": record,
        "fit_report": blend.with_name(f"{name}.fit.json"),
    }


def _lift_of(args, prepared: Path) -> Path:
    """The mesh the prepared glb was made from: ``--source``, else its own record.

    The second prepare needs the *lift*, not the prepared file, because it
    re-runs the same normalisation against a different skeleton. That is
    what the prepare record is for — it hashes the lift as its ``mesh``
    input — and a prepared glb with no record beside it is refused by name
    rather than guessed at.
    """
    if getattr(args, "source", None):
        path = Path(args.source).expanduser().resolve()
        if not path.is_file():
            raise UsageError(f"--source {path} is not a file")
        return path
    record_path = prepared.with_suffix(".prepare.json")
    if not record_path.is_file():
        raise UsageError(
            f"there is no {record_path.name} beside {prepared.name}, so the lift it was prepared from is unknown — "
            "the loop prepares that lift a second time against the fitted skeleton. Re-run `forge gen prepare`, "
            "or name the lift with --source"
        )
    document = json.loads(record_path.read_text(encoding="utf-8"))
    for entry in document.get("inputs") or []:
        if entry.get("role") == "mesh":
            root = records.project() or record_path.parent
            path = Path(entry["path"])
            path = path if path.is_absolute() else (Path(root) / path)
            if not path.is_file():
                raise UsageError(f"{record_path.name} names {entry['path']} as its mesh input and it is not there — pass --source")
            return path.resolve()
    raise UsageError(f"{record_path.name} has no mesh input — pass --source with the lift this body came from")


def _skin_once(args, places: dict, backend: tuple, source: Path, out: Path, *, label: str) -> None:
    """One SkinTokens run, with the card and the port checked first."""
    interpreter, checkout, env = backend
    _refuse_a_stale_server()
    _refuse_a_busy_card(args.min_free_gb, args.allow_busy)
    out.parent.mkdir(parents=True, exist_ok=True)
    log(f"{label}: skinning {source.name} -> {out.name}")
    _run_demo(interpreter, checkout, env, _demo_argv(args, source, out), args.timeout)
    if not out.is_file():
        raise BackendFailed(f"demo.py exited 0 but wrote no {out} — read the log above")
    if _port_taken():
        log(
            f"WARN a bpy_server is still listening on {BPY_PORT} after the run — demo.py's atexit hook did not "
            f"fire. Kill it by PID (ss -lptn 'sport = :{BPY_PORT}'), or the next run will talk to it"
        )


def _abort_on_a_failed_skin(measured: dict, *, label: str) -> None:
    """The one gate on the weights themselves: too much of the body came back bare."""
    fraction = measured["unweighted_fraction"]
    ceiling = measured["unweighted_abort_fraction"]
    if fraction > ceiling:
        raise BackendFailed(
            f"{label}: {fraction:.1%} of the vertices came back with no weight at all, past the profile's "
            f"[rig] unweighted_abort_fraction {ceiling:.0%} — the skin failed, and a body bound like this holds "
            "its rest pose through every clip in the library. Re-lift the mesh or re-roll the skin; do not ship a statue"
        )


def _fit_once(places: dict, skinned: Path, report_path: Path) -> dict:
    """Step 2: read the weights, measure every run, refuse an untrustworthy fit."""
    from forge_gen import fit as fit_mod

    prof = places["profile"]
    document, binary = fit_mod._chunks(skinned)
    order = fit_mod._joint_order(document, argparse.Namespace(names=os.fspath(places["prepared"]), map=None), prof)
    names = [bone["name"] for bone in prof.bones]
    points, dense = fit_mod._dense_weights(document, binary, len(order))
    report = fit_mod.fit(
        points,
        fit_mod._reorder(dense, order, names),
        prof,
        min_support=DEFAULT_MIN_SUPPORT,
        symmetry=True,
        ground=True,
    )
    report["source"] = str(skinned)
    report["profile_dir"] = str(prof.dir)
    bands = fit_mod.symmetry_bands(prof)
    problems, warnings = fit_mod.gate(report, ratio_min=RATIO_MIN, ratio_max=RATIO_MAX, min_support=DEFAULT_MIN_SUPPORT, **bands)
    report["asymmetry"] = bands
    report["problems"] = problems
    report["warnings"] = warnings
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    for line in fit_mod.run_table(report).splitlines():
        log(line)
    log(f"motion_scale {report['motion_scale']:.4f}, report {report_path}")
    for warning in warnings:
        log(f"WARN {warning}")
    if problems:
        listed = "\n  - ".join(problems)
        raise InputRejected(
            f"the fit is not trustworthy ({len(problems)} problem(s)):\n  - {listed}",
            report=os.fspath(report_path),
        )
    return report


def run(args) -> dict:
    """The five steps, in order, with one card held at a time."""
    from forge_gen.blender import fit_rig, prepare, reattach

    prof = _profile(args.profile)
    places = _places(args, prof)
    # What the three Blender halves need from this door's own flags: the
    # project, so their records state paths the way every other record does,
    # and who asked, so a body's chain says so at every step.
    inner = argparse.Namespace(
        json=False,
        project=getattr(args, "project", None),
        created_by=getattr(args, "created_by", None),
    )

    interpreter = checkout = None
    if args.analyse_only:
        if not places["skinned"].is_file() or not places["fit_report"].is_file():
            raise UsageError(
                f"--analyse-only, and there is no {places['skinned']} and {places['fit_report']} to read"
            )
        try:
            interpreter, checkout, _ = _backend()
        except MissingBackend as err:
            log(f"WARN {err.message} — the record's backend block will say null where it cannot know")
        log(f"reading {places['skinned']} and {places['fit_report']} without running anything")
        report = json.loads(places["fit_report"].read_text(encoding="utf-8"))
        handed_in = places["prepared2"] if places["prepared2"].is_file() else places["prepared"]
    else:
        backend = _backend()
        interpreter, checkout, _env = backend
        log(f"interpreter {interpreter}")
        log(f"checkout {checkout}")

        # 1 — skin, on the profile's own skeleton.
        _skin_once(args, places, backend, places["prepared"], places["pass1"], label="pass 1")
        first, lines = analyse(places["prepared"], places["pass1"], prof)
        for line in lines:
            log(line)
        _abort_on_a_failed_skin(first, label="pass 1")

        # 2 — fit, once. See the module doc for why there is no second.
        report = _fit_once(places, places["pass1"], places["fit_report"])

        # 3 — the per-body armature, rest rotations copied and checked.
        fit_rig.run(
            argparse.Namespace(report=os.fspath(places["fit_report"]), source=os.fspath(prof.dir), out=os.fspath(places["work"]), **vars(inner))
        )
        skeleton_glb = places["work"] / fit_rig.SKELETON_GLB
        skeleton_blend = places["work"] / fit_rig.SKELETON_BLEND

        # 4 — prepare and skin AGAIN, on the skeleton the fit just corrected.
        prepare.run(
            argparse.Namespace(
                source=os.fspath(places["lift"]),
                out=os.fspath(places["prepared2"]),
                record=os.fspath(places["prepare2_record"]),
                profile=os.fspath(prof.dir),
                skeleton=os.fspath(skeleton_glb),
                stature=None,
                yaw_deg=0.0,
                budget=None,
                **vars(inner),
            )
        )
        _skin_once(args, places, backend, places["prepared2"], places["skinned"], label="pass 2")
        handed_in = places["prepared2"]

        # 5 — re-attach by joint order onto the fitted armature.
        reattach.run(
            argparse.Namespace(
                skinned=os.fspath(places["skinned"]),
                handed_in=os.fspath(handed_in),
                out=os.fspath(places["blend"]),
                profile=os.fspath(prof.dir),
                armature=os.fspath(skeleton_blend),
                map=None,
                **vars(inner),
            )
        )

    measured, lines = analyse(handed_in, places["skinned"], prof)
    for line in lines:
        log(line)
    _abort_on_a_failed_skin(measured, label="pass 2")
    measured["fit_report"] = records.record_path(places["fit_report"])
    measured["passes"] = 1

    contract_path = prof.dir / str(prof.toml["profile"].get("contract", "contract.json"))
    spec = {
        "name": places["name"],
        "profile": prof,
        "profile_sha256": records.sha256_file(contract_path),
        "commit": _commit(checkout) if checkout else None,
        "python": _interpreter_says(interpreter, "import sys; print('.'.join(str(v) for v in sys.version_info[:3]))") if interpreter else None,
        "torch": _interpreter_says(interpreter, "import torch; print(torch.__version__)") if interpreter else None,
        "fit": fit_block(report, prof),
    }
    outputs = [places["skinned"]] + ([places["blend"]] if places["blend"].is_file() else [])
    written = _record(args, spec, measured, places["prepared"], outputs, places["record"])
    log(f"wrote {written}")
    log(f"next — forge gen export {places['blend']} --out <glb> --record <json>")
    return {
        "ok": True,
        "record": os.fspath(written),
        "outputs": [os.fspath(path) for path in outputs],
        "fit_report": os.fspath(places["fit_report"]),
        "motion_scale": spec["fit"]["motion_scale"],
        "measured": measured,
    }


def run_fake(args) -> dict:
    """The whole loop as placeholders: a fitted-looking skeleton and a record that says it is one.

    The skeleton comes out with every bone's rest translation scaled by
    :data:`FAKE_FIT_RATIO` and every rest rotation untouched — the move a
    real fit makes — so a sidecar's ``bones[]`` and its ``motion_scale`` are
    exercised end to end with no card. Every measurement in the record is
    ``null``, because nothing was measured.
    """
    from forge_gen import placeholders

    prof = _profile(args.profile)
    places = _places(args, prof)
    placeholders.refuse_real(places["skinned"], places["blend"], places["record"], places["fit_report"])
    placeholders.placeholder_body_glb(places["skinned"], prof, scale=FAKE_FIT_RATIO)
    placeholders.placeholder_blend(places["blend"])
    places["fit_report"].parent.mkdir(parents=True, exist_ok=True)
    report = {
        "fake": True,
        "profile": prof.name,
        "root": prof.root,
        "passes": 1,
        "motion_scale": FAKE_FIT_RATIO,
        "note": "placeholder from a --fake run; nothing about it is a measurement",
    }
    places["fit_report"].write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    rec = placeholders.fake_record("rig", BACKEND_NAME, backend=BACKEND_NAME, created_by=getattr(args, "created_by", None))
    records.add_input(rec, "mesh", places["prepared"])
    records.add_input(rec, "reference", prof.rig_blend, source=f"profile:{prof.name}")
    rec["params"] = {
        "name": places["name"],
        "profile": prof.name,
        "skinner": {"tool": SKINNER, "commit": None, "note": SKINNER_NOTE},
        "fit": {
            "passes": 1,
            "motion_scale": FAKE_FIT_RATIO,
            "sources": {"limbs": "weights", "root": "geometry", "shoulder_line": "geometry", "ground": "geometry"},
            "ratios": None,
            "runs": None,
            "raw_asymmetry": None,
            "grounding": None,
            "symmetry_worst": None,
        },
        "seed": None,
        "model_ckpt": args.model_ckpt,
        "hf_path": args.hf_path,
    }
    rec["measured"] = {
        "vertices": None,
        "unweighted_fraction": None,
        "contract_bones_present": None,
        "skeleton_alignment": None,
        "fit_report": records.record_path(places["fit_report"]),
        "passes": 1,
    }
    for path in (places["skinned"], places["blend"]):
        records.add_output(rec, path)
    records.write(rec, places["record"])
    return {
        "record": os.fspath(places["record"]),
        "outputs": [os.fspath(places["skinned"]), os.fspath(places["blend"])],
        "fit_report": os.fspath(places["fit_report"]),
        "motion_scale": FAKE_FIT_RATIO,
        "measured": rec["measured"],
    }


def main(argv: list[str] | None = None) -> int:
    from forge_gen import cli, placeholders
    from forge_gen.exit_codes import ForgeGenError

    parser = argparse.ArgumentParser(prog="forge-gen skin", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    _add_arguments(parser)
    args = parser.parse_args(argv)
    if args.project:
        if not os.path.isdir(args.project):
            sys.stderr.write(f"{TAG}: --project {args.project} is not a directory\n")
            return 2
        records.set_project(args.project)
    try:
        result = run_fake(args) if placeholders.requested(args) else run(args)
    except ForgeGenError as err:
        cli.emit(err.payload(), as_json=args.json)
        sys.stderr.write(f"{TAG}: {err.error}: {err.message}\n")
        return err.code
    cli.emit(result, as_json=args.json)
    return 0


if __name__ == "__main__":
    sys.exit(main())
