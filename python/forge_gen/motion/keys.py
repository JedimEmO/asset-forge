"""``forge-gen motion keys``: generate motion with ARDY under authored keyframe constraints.

The step past plain prompting: a keys file authors sparse constraints — per
keyed frame, which joints are pinned, and which way a bone should *aim*.
Supported constraint groups are Hips, LeftHand, RightHand, LeftFoot and
RightFoot. Other bones, including Head, are refused before model loading.
Positions come from a base take (the body performance to keep); aims are
authored, in character space, and become wrist/bone rotations via a minimal
rotation of the bone's child axis. ARDY generates the full-body motion that
satisfies them.

This exists because text prompting cannot author choreography: generated
takes barely move the wrist (~9 deg across a whole take), so a sword's blade
line — the thing a combat animation IS — never comes out of a prompt. With
the hand's aim keyed, the model supplies what it is actually good at: hips,
weight, balance, the counter-arm.

Keys file (JSON)::

    {
      "joints": ["RightHand", "LeftHand", "Hips"],   // the constraint set
      "keys": [
        {"frame": 8,
         "pos": {"RightHand": [-0.30, 0.85, 0.20]},
         "aim": {"RightHand": [-0.55, -0.10, -0.83]}},
        {"frame": 31, "aim": {"RightHand": [-0.3, 0.0, 0.95]}},
        {"frame": 42}                                 // pin pose as-is
      ]
    }

``aim`` vectors are [right, up, forward] in the CHARACTER's frame at that
frame of the base take (facing derived from the shoulder line); ``pos`` is
[right, world-y, forward] about the hips' ground point — both survive
whatever world heading the take happens to have. A key with neither pins
the listed joints exactly as the base take has them.

One optional key the old schema did not have: ``"pose_frame": N`` takes the
full-body pose (and the character frame the ``pos``/``aim`` values are
read in) from frame ``N`` of the base take instead of ``frame``. It is how
the recoil preset holds one steady pose across a window; a file without it
behaves exactly as before.

    forge-gen motion keys --base assets-src/ardy/gen_sword_heavy.npz \\
        --keys assets-src/ardy/gen_sword_heavy.keys.json \\
        --prompt "A samurai draws their sword in one fast horizontal slash." \\
        --out-dir out/keyed/iai --samples 8

``--preset recoil`` is the one hard-coded edit this grew out of, folded in.
Text prompting alone cannot produce short, sharp gestures: ARDY runs at
20 fps and will not generate an impulse only two or three frames wide. A
pistol recoil is the canonical case — every plain-prompt take holds a
correct aim pose and then refuses to kick (measured: 2 of 48 takes showed
any jolt at all, peaking at 1.8 m/s where a real recoil snaps far harder).
The preset takes the steadiest frame of the base take (or ``--base-frame``),
holds it across a ``--window`` of seconds centred in the clip, and lifts
both hands by a ``--kick`` profile — sharp up, eased down — writing the
synthesized keys file beside the takes so the record names what ran:

    forge-gen motion keys --base assets-src/ardy/gen_pistol_idle.npz \\
        --prompt "A person fires a pistol, the recoil kicking their hands up." \\
        --preset recoil --kick 0.14 --window 0.6 --out-dir out/keyed/recoil --samples 8

Outputs: ``<name>_s<seed>_<k>.npz`` (``name`` = ``--name``, else the keys
file's stem, else the preset) with a ``.take.json`` beside each, and the
keys file (copied or synthesized) as ``<name>.keys.json`` in the out-dir.

GPU note: same ~16 GB as the rest of the ARDY tools.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import sys
from pathlib import Path

from forge_gen import records
from forge_gen.exit_codes import InputRejected, UsageError
from forge_gen.motion import session
from forge_gen.motion.sweep import DEFAULT_FPS, KNOWN_MODELS

PRESETS = ("recoil",)

# ARDY SkeletonBase.expand_joint_names accepts these semantic groups, not
# arbitrary skeleton bones. Validate before loading the model.
CONSTRAINT_JOINTS = ("Hips", "LeftHand", "RightHand", "LeftFoot", "RightFoot")

#: The joints the recoil preset constrains (the old script's HANDS + Hips).
RECOIL_JOINTS = ("LeftHand", "RightHand", "Hips")
RECOIL_HANDS = ("RightHand", "LeftHand")

#: Preset defaults, also the CLI defaults.
RECOIL_KICK_M = 0.14
RECOIL_WINDOW_S = 0.6
RECOIL_DURATION_S = 2.0


# --------------------------------------------------------------- geometry --


def recoil_profile(n: int, kick: float, rise_frames: int = 2) -> list[float]:
    """Vertical hand offset over n frames: snap up, then ease back down.

    Sharp on the way up (that is what reads as recoil) and slower on the way
    back, which is how a real muzzle rise settles.
    """
    out = [0.0] * n
    for i in range(n):
        if i <= rise_frames:
            out[i] = kick * (i / max(rise_frames, 1))
        else:
            decay = (i - rise_frames) / max(n - 1 - rise_frames, 1)
            out[i] = kick * (1.0 - decay) ** 2
    return out


def bone_axis_rest(skeleton, joint: int):
    """The joint's aim axis in its own rest frame: toward its ``...End`` child
    if it has one, else its first child. Rest global rotations are identity,
    so the neutral offset direction IS the local axis."""
    import numpy as np

    neutral = skeleton.neutral_joints.detach().cpu().numpy()
    parents = [int(p) for p in skeleton.joint_parents]
    names = {v: k for k, v in skeleton.bone_index.items()}
    children = [c for c, p in enumerate(parents) if p == joint]
    if not children:
        raise InputRejected(f"joint {names[joint]} has no child to aim")
    ends = [c for c in children if names[c].endswith("End")]
    child = ends[0] if ends else children[0]
    axis = neutral[child] - neutral[joint]
    return axis / np.linalg.norm(axis)


def character_frame(glob_p, skeleton, frame: int):
    """Columns [right, up, forward] of the character's frame at ``frame``,
    derived from the shoulder line so it survives any world heading."""
    import numpy as np

    li = skeleton.bone_index["LeftArm"]
    ri = skeleton.bone_index["RightArm"]
    right = glob_p[frame, ri] - glob_p[frame, li]
    right[1] = 0.0
    right /= np.linalg.norm(right)
    up = np.array([0.0, 1.0, 0.0])
    forward = np.cross(up, right)
    forward /= np.linalg.norm(forward)
    return np.stack([right, up, forward], axis=1)


def minimal_rotation(src, dst):
    """Rotation matrix taking unit vector src onto unit vector dst."""
    import numpy as np

    v = np.cross(src, dst)
    c = float(np.dot(src, dst))
    if np.linalg.norm(v) < 1e-8:
        return np.eye(3) if c > 0 else -np.eye(3) + 2 * np.outer(src, src)
    vx = np.array([[0, -v[2], v[1]], [v[2], 0, -v[0]], [-v[1], v[0], 0]])
    return np.eye(3) + vx + vx @ vx / (1.0 + c)


def steadiest_frame(glob_p) -> int:
    """The frame of least movement: the most representative 'at aim' pose of a held-pose take."""
    import numpy as np

    motion = np.linalg.norm(np.diff(glob_p, axis=0), axis=-1).mean(axis=1)
    return int(np.argmin(motion)) + 1


def to_character_pos(world, glob_p, skeleton, frame: int):
    """A world position as the keys schema states one: [right, world-y, forward] about the hips' ground point at ``frame``."""
    import numpy as np

    basis = character_frame(glob_p, skeleton, frame)
    right, _, forward = basis.T
    hips = skeleton.bone_index["Hips"]
    origin = glob_p[frame, hips] * np.array([1.0, 0.0, 1.0])
    delta = np.asarray(world, dtype=np.float64) - origin
    return [float(delta @ right), float(delta[1]), float(delta @ forward)]


def synthesize_recoil_keys(glob_p, skeleton, *, base_frame: int, frames, profile: list[float]) -> dict:
    """The recoil as a keys spec: every frame of the window holds ``base_frame``'s pose, both hands lifted by the profile.

    Positions are authored in the character frame of ``base_frame`` (the
    ``pose_frame`` every key names), so the keys path rebuilds exactly the
    arrays the old constrained generator held: the base pose repeated, hands
    raised by the profile, nothing else touched.
    """
    keys = []
    for frame, lift in zip(frames, profile):
        pos = {}
        for name in RECOIL_HANDS:
            world = glob_p[base_frame, skeleton.bone_index[name]].copy()
            world[1] += lift
            pos[name] = [round(v, 6) for v in to_character_pos(world, glob_p, skeleton, base_frame)]
        keys.append({"frame": int(frame), "pose_frame": int(base_frame), "pos": pos})
    return {"joints": list(RECOIL_JOINTS), "keys": keys}


# --------------------------------------------------------------- argparse --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--base", required=True, metavar="NPZ", help="npz supplying the body performance (and the pose to hold)")
    parser.add_argument("--prompt", required=True, metavar="TEXT", help="what the model is told")
    parser.add_argument("--out-dir", required=True, metavar="DIR", help="where the takes, records and the keys file go")
    parser.add_argument("--keys", metavar="JSON", help="authored keyframes (see the docstring)")
    parser.add_argument("--preset", choices=PRESETS, help="a built-in edit instead of --keys: recoil")
    parser.add_argument("--kick", type=float, default=RECOIL_KICK_M, metavar="METRES", help="recoil: how far the hands snap up at the peak (default 0.14)")
    parser.add_argument("--window", type=float, default=RECOIL_WINDOW_S, metavar="SECONDS", help="recoil: length of the constrained window (default 0.6)")
    parser.add_argument("--base-frame", type=int, default=None, metavar="N", help="recoil: frame of the base take to hold (default: its steadiest)")
    parser.add_argument("--duration", type=float, default=None, metavar="S", help="seconds to generate (default: the base take's length; recoil: 2.0)")
    parser.add_argument("--samples", type=int, default=8, metavar="N", help="takes to draw (default 8)")
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--model", default="core", help="ARDY model nickname (default core)")
    parser.add_argument("--cfg", type=float, default=2.0, metavar="W", help="text CFG weight (default 2.0)")
    parser.add_argument("--cfg-constraint", type=float, default=2.0, metavar="W", help="CFG weight on the constraint; raise it when the model smooths an authored key away (default 2.0)")
    parser.add_argument("--no-postprocess", action="store_true", help="skip foot-skate correction, which also smooths motion")
    parser.add_argument("--name", metavar="STEM", help="output stem (default: the keys file's stem, or the preset)")


def add_parser(subparsers) -> None:
    """Register ``motion keys``."""
    parser = subparsers.add_parser(
        "keys",
        help="Keyframe-constrained generation (ARDY; --preset recoil)",
        description=__doc__,
    )
    _add_arguments(parser)


def _inner_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="forge_gen.motion.keys --inner", description="inner half of motion keys")
    _add_arguments(parser)
    parser.add_argument("--project", default=None)
    parser.add_argument("--created-by", default=None)
    return parser


def _forward_argv(args, *, base: Path, out_dir: Path, keys: Path | None) -> list[str]:
    argv = ["--base", str(base), "--prompt", args.prompt, "--out-dir", str(out_dir), "--model", args.model]
    if keys is not None:
        argv += ["--keys", str(keys)]
    if args.preset:
        argv += ["--preset", args.preset, "--kick", f"{args.kick:g}", "--window", f"{args.window:g}"]
        if args.base_frame is not None:
            argv += ["--base-frame", str(args.base_frame)]
    if args.duration is not None:
        argv += ["--duration", f"{args.duration:g}"]
    argv += ["--samples", str(args.samples), "--seed", str(args.seed)]
    argv += ["--cfg", f"{args.cfg:g}", "--cfg-constraint", f"{args.cfg_constraint:g}"]
    if args.no_postprocess:
        argv.append("--no-postprocess")
    if args.name:
        argv += ["--name", args.name]
    project = getattr(args, "project", None)
    if project:
        argv += ["--project", str(Path(project).resolve())]
    created_by = getattr(args, "created_by", None)
    if created_by:
        argv += ["--created-by", created_by]
    return argv


def _validate(args) -> tuple[Path, Path, Path | None, str]:
    """``(base, out_dir, keys, name)`` — the refusals the outer makes before any backend work."""
    if bool(args.keys) == bool(args.preset):
        raise UsageError("give exactly one of --keys FILE or --preset NAME")
    if not args.prompt.strip():
        raise InputRejected("an empty prompt: the model needs to be told what the keys are for")
    if args.samples < 1:
        raise UsageError("--samples must be at least 1")
    if args.preset == "recoil":
        if args.kick <= 0:
            raise UsageError("--kick must be positive metres")
        if args.window <= 0:
            raise UsageError("--window must be positive seconds")
    if args.duration is not None and args.duration <= 0:
        raise UsageError("--duration must be positive")
    base = Path(args.base).expanduser().resolve()
    if not base.is_file():
        raise InputRejected(f"--base {base} is not a file")
    keys = None
    if args.keys:
        keys = Path(args.keys).expanduser().resolve()
        if not keys.is_file():
            raise InputRejected(f"--keys {keys} is not a file")
        spec = load_keys(keys)
        check_keys(spec)
    out_dir = Path(args.out_dir).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    name = args.name or (keys.stem if keys is not None else args.preset)
    name = name.removesuffix(".keys")
    return base, out_dir, keys, name


def load_keys(path: Path) -> dict:
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except json.JSONDecodeError as err:
        raise InputRejected(f"{path} is not JSON: {err}") from err


def check_keys(spec: dict) -> None:
    """The shape the docstring promises, refused early with the defect named."""
    if not isinstance(spec, dict) or not isinstance(spec.get("joints"), list) or not spec["joints"]:
        raise InputRejected("keys file: 'joints' must be a non-empty list of joint names")
    for joint in spec["joints"]:
        if not isinstance(joint, str) or joint not in CONSTRAINT_JOINTS:
            raise InputRejected(f"keys file: unsupported constraint joint {joint!r}; supported: {', '.join(CONSTRAINT_JOINTS)}")
    if len(set(spec["joints"])) != len(spec["joints"]):
        raise InputRejected("keys file: constraint joints must not repeat")
    if not isinstance(spec.get("keys"), list) or not spec["keys"]:
        raise InputRejected("keys file: 'keys' must be a non-empty list")
    for i, key in enumerate(spec["keys"]):
        if not isinstance(key, dict) or type(key.get("frame")) is not int or key["frame"] < 0:
            raise InputRejected(f"keys file: key #{i} needs an integer 'frame' >= 0")
        if "pose_frame" in key and (type(key["pose_frame"]) is not int or key["pose_frame"] < 0):
            raise InputRejected(f"keys file: key #{i} 'pose_frame' must be an integer >= 0")
        for field in ("pos", "aim"):
            table = key.get(field, {})
            if not isinstance(table, dict):
                raise InputRejected(f"keys file: key #{i} '{field}' must be an object of joint -> [x, y, z]")
            for joint, vec in table.items():
                if not (isinstance(vec, list) and len(vec) == 3 and all(type(v) in (int, float) and math.isfinite(v) for v in vec)):
                    raise InputRejected(f"keys file: key #{i} {field}.{joint} must be three finite numbers [x, y, z]")
                if field == "aim" and not any(vec):
                    raise InputRejected(f"keys file: key #{i} aim.{joint} must be a nonzero direction")


def params_for(args, *, keys_path: Path, duration_s: float, history_frames, diffusion_steps, sample: int) -> dict:
    return {
        "model": args.model,
        "model_repo": None,
        "repo": None,
        "prompt": args.prompt,
        "label": None,
        "seed": args.seed,
        "cfg": args.cfg,
        "cfg_constraint": args.cfg_constraint,
        "duration_s": duration_s,
        "sample": sample,
        "diffusion_steps": diffusion_steps,
        "history_frames": history_frames,
        "postprocess": not args.no_postprocess,
        "keys_file": keys_path.name,
        "keys_sha256": records.sha256_file(keys_path),
        "preset": args.preset,
        # All samples of a keyed run share one forward pass (the batch IS
        # --samples), and the grid is one cell wide everywhere else.
        "batch_size": args.samples,
        "grid": {"prompts": 1, "seeds": 1, "cfg": 1, "durations": 1, "samples": args.samples},
    }


# ------------------------------------------------------------------ outer --


def run(args) -> dict:
    from forge_gen import backends as backends_mod
    from forge_gen import launcher

    base, out_dir, keys, _ = _validate(args)
    backend = backends_mod.load_backend("ardy")
    result = launcher.run_inner_checked(backend, "motion.keys", _forward_argv(args, base=base, out_dir=out_dir, keys=keys))
    return _summary(result, out_dir)


def _summary(result: dict, out_dir: Path) -> dict:
    takes = result.get("takes", [])
    return {
        "record": takes[0]["record"] if takes else None,
        "outputs": [take["path"] for take in takes],
        "records": [take["record"] for take in takes],
        "keys": result.get("keys"),
        "takes": takes,
        "out_dir": str(out_dir),
        "fake": bool(result.get("fake", False)),
        "_text": f"wrote {len(takes)} takes to {out_dir} (keys: {result.get('keys')})\n"
        + "\n".join(f"  {take['name']}" for take in takes),
    }


# ------------------------------------------------------------------- fake --


def _base_duration(base: Path) -> float | None:
    """The base take's length in seconds, read with the stdlib npz reader; ``None`` when it cannot be read."""
    from forge_gen import npz

    try:
        arrays = npz.read_npz(base)
        frames = arrays["root_positions"].shape[0]
        fps = arrays["fps"].int_scalar() if "fps" in arrays else DEFAULT_FPS
    except (KeyError, ValueError, OSError, IndexError):
        return None
    return frames / fps if fps else None


def run_fake(args) -> dict:
    """Still figures, the keys file copied (or a recoil spec synthesized on the rest pose), records with ``fake: true``."""
    from forge_gen import npz, placeholders

    base, out_dir, keys, name = _validate(args)
    keys_out = out_dir / f"{name}.keys.json"
    targets = [out_dir / f"{name}_s{args.seed}_{k}.npz" for k in range(args.samples)]
    placeholders.refuse_real(*targets, *(session.record_path_for(t) for t in targets))
    if keys is None:
        # The synthesized spec is guarded too; a copy of the caller's own
        # --keys file is not — its content is exactly what they named.
        placeholders.refuse_real(keys_out)
    if keys is not None:
        if keys != keys_out:
            shutil.copyfile(keys, keys_out)
    else:
        duration = args.duration if args.duration is not None else RECOIL_DURATION_S
        num_frames = int(duration * DEFAULT_FPS)
        n = int(args.window * DEFAULT_FPS)
        start = max(1, (num_frames - n) // 2)
        profile = recoil_profile(n, args.kick)
        # No skeleton here: the hands' rest position in the profile's frame is
        # not known without one, so the fake spec keys the lift alone about
        # the origin. It is a placeholder like the takes beside it.
        spec = {
            "fake": True,  # the mark refuse_real recognises; nothing reads a fake spec
            "joints": list(RECOIL_JOINTS),
            "keys": [
                {"frame": start + i, "pose_frame": int(args.base_frame or 0), "pos": {h: [0.0, round(lift, 6), 0.0] for h in RECOIL_HANDS}}
                for i, lift in enumerate(profile)
            ],
        }
        with open(keys_out, "w", encoding="utf-8") as handle:
            json.dump(spec, handle, indent=2)
            handle.write("\n")
    if args.duration is not None:
        duration_s = args.duration
    elif args.preset:
        duration_s = RECOIL_DURATION_S
    else:
        duration_s = _base_duration(base)  # what the real path would default to: the base take's length
    frames = max(1, int((duration_s if duration_s is not None else 1.0) * DEFAULT_FPS))
    folder = KNOWN_MODELS.get(args.model)
    model_repo = f"{session.HF_ORG}/{folder}" if folder else None
    backend = records.backend_block(name=session.BACKEND, commit=placeholders.FAKE_COMMIT, model=model_repo)
    upstream = session.backend_upstream()
    takes = []
    for k in range(args.samples):
        take_name = f"{name}_s{args.seed}_{k}"
        path = out_dir / f"{take_name}.npz"
        npz.write_take(path, frames=frames, fps=DEFAULT_FPS, prompt=args.prompt)
        params = params_for(args, keys_path=keys_out, duration_s=duration_s, history_frames=None, diffusion_steps=None, sample=k)
        params["model_repo"] = model_repo
        params["repo"] = upstream
        rec = session.take_record(
            npz_path=path,
            prompt=args.prompt,
            model_name=args.model,
            params=params,
            frames=frames,
            fps=DEFAULT_FPS,
            created_by=getattr(args, "created_by", None),
            backend=backend,
            keys_path=keys_out,
            base_take=base,
            note="placeholder take from a --fake run: a still figure in the rest pose; the keys were not applied",
        )
        rec["fake"] = True
        record = records.write(rec, session.record_path_for(path))
        takes.append({"name": take_name, "path": str(path), "record": str(record), "sample": k, "seed": args.seed, "frames": frames, "fps": float(DEFAULT_FPS)})
    return _summary({"takes": takes, "keys": str(keys_out), "fake": True}, out_dir)


# ------------------------------------------------------------------ inner --


def main_inner(argv: list[str]) -> dict:
    import numpy as np
    import torch
    from ardy.constraints import EndEffectorConstraintSet
    from ardy.motion_rep.tools import length_to_mask
    from ardy.postprocess import post_process_motion
    from ardy.tools import seed_everything, to_numpy

    args = _inner_parser().parse_args(argv)
    if args.project:
        records.set_project(args.project)
    base, out_dir, keys, name = _validate(args)

    model = session.load_model(args.model)
    device = session.device()
    fps = model.motion_rep.fps
    skeleton = model.skeleton

    data = np.load(base, allow_pickle=True)
    if "local_rot_mats" not in data.files or "root_positions" not in data.files:
        raise InputRejected(f"{base} carries no local_rot_mats/root_positions — not an ARDY take")
    local = np.asarray(data["local_rot_mats"], dtype=np.float64)
    root = np.asarray(data["root_positions"], dtype=np.float64)
    if local.ndim != 4 or local.shape[1] != skeleton.nbjoints:
        raise InputRejected(f"{base} has {local.shape[1] if local.ndim == 4 else '?'} joints; the {args.model} skeleton has {skeleton.nbjoints}")
    glob_r, glob_p = session.fk(local, root, skeleton)
    base_frames = local.shape[0]

    keys_out = out_dir / f"{name}.keys.json"
    if args.preset == "recoil":
        duration = args.duration if args.duration is not None else RECOIL_DURATION_S
        num_frames = int(duration * fps)
        # Pick the steadiest frame of the base take as the pose to hold: least
        # movement means the most representative "at aim" pose.
        base_frame = args.base_frame if args.base_frame is not None else steadiest_frame(glob_p)
        if not 0 <= base_frame < base_frames:
            raise InputRejected(f"--base-frame {base_frame} is outside the base take ({base_frames} frames)")
        print(f"holding base frame {base_frame} ({base_frame / fps:.2f}s of {base.name})", flush=True)
        n = int(args.window * fps)
        if n < 1:
            raise InputRejected(f"--window {args.window}s is under one frame at {fps} fps")
        start = max(1, (num_frames - n) // 2)
        frames_window = list(range(start, start + n))
        profile = recoil_profile(n, args.kick)
        spec = synthesize_recoil_keys(glob_p, skeleton, base_frame=base_frame, frames=frames_window, profile=profile)
        with open(keys_out, "w", encoding="utf-8") as handle:
            json.dump(spec, handle, indent=2)
            handle.write("\n")
        print(f"constraining hands+hips on frames {frames_window[0]}-{frames_window[-1]} (kick {args.kick} m over {n} frames) -> {keys_out.name}", flush=True)
    else:
        duration = args.duration if args.duration is not None else base_frames / fps
        num_frames = int(duration * fps)
        spec = load_keys(keys)
        if keys != keys_out:
            shutil.copyfile(keys, keys_out)
    check_keys(spec)

    joints = spec["joints"]
    unknown = [j for j in joints if j not in skeleton.bone_index]
    if unknown:
        raise InputRejected(f"keys file names joints the skeleton lacks: {', '.join(unknown)}")
    key_list = sorted(spec["keys"], key=lambda k: k["frame"])
    frames = np.array([k["frame"] for k in key_list], dtype=int)
    if frames.max() >= min(base_frames, num_frames):
        raise InputRejected(f"key frame {int(frames.max())} beyond take ({base_frames}) or window ({num_frames})")
    pose_frames = np.array([k.get("pose_frame", k["frame"]) for k in key_list], dtype=int)
    if pose_frames.max() >= base_frames:
        raise InputRejected(f"pose_frame {int(pose_frames.max())} beyond the base take ({base_frames})")

    # Full-body pose arrays at the keyed frames, with authored positions and
    # aims baked in.
    pos = glob_p[pose_frames].copy()
    rots = glob_r[pose_frames].copy()
    hips = skeleton.bone_index["Hips"]
    for i, key in enumerate(key_list):
        frame = int(pose_frames[i])
        basis = character_frame(glob_p, skeleton, frame)
        right, _, forward = basis.T
        # Authored positions are [right, world-y, forward] about the hips'
        # ground point at that frame, so they survive any world heading.
        for jname, p in key.get("pos", {}).items():
            if jname not in skeleton.bone_index:
                raise InputRejected(f"key frame {key['frame']}: pos names unknown joint {jname!r}")
            j = skeleton.bone_index[jname]
            origin = glob_p[frame, hips] * np.array([1.0, 0.0, 1.0])
            pos[i, j] = origin + right * p[0] + np.array([0.0, p[1], 0.0]) + forward * p[2]
        for jname, aim in key.get("aim", {}).items():
            if jname not in skeleton.bone_index:
                raise InputRejected(f"key frame {key['frame']}: aim names unknown joint {jname!r}")
            j = skeleton.bone_index[jname]
            target = basis @ (np.asarray(aim, dtype=np.float64) / np.linalg.norm(aim))
            axis = bone_axis_rest(skeleton, j)
            current = glob_r[frame, j] @ axis
            rots[i, j] = minimal_rotation(current, target) @ glob_r[frame, j]
        have = ", ".join(sorted(set(key.get("aim", {})) | set(key.get("pos", {})))) or "pose as-is"
        if args.preset is None:
            print(f"  key frame {key['frame']:3d} ({key['frame'] / fps:.2f}s): {have}", flush=True)

    constraint = EndEffectorConstraintSet(
        skeleton,
        torch.tensor(frames, dtype=torch.long),
        torch.tensor(pos, dtype=torch.float32),
        torch.tensor(rots, dtype=torch.float32),
        None,
        joint_names=joints,
    )

    diffusion_steps = int(model.diffusion.num_base_steps)
    seed_everything(args.seed)
    texts = [args.prompt] * args.samples
    lengths = torch.tensor([num_frames] * args.samples, device=device)
    observed_motion, motion_mask = model.motion_rep.create_conditions_from_constraints_batched(
        [constraint], lengths, to_normalize=True, device=device
    )
    with torch.no_grad():
        motion = model(
            texts,
            num_frames,
            num_denoising_steps=diffusion_steps,
            pad_mask=length_to_mask(lengths),
            first_heading_angle=torch.zeros(args.samples, device=device),
            motion_mask=motion_mask,
            observed_motion=observed_motion,
            cfg_weight=(args.cfg, args.cfg_constraint),
            crop_history_length=None,
        )
        output = model.motion_rep.inverse(motion, is_normalized=True)
    if not args.no_postprocess:
        output.update(
            post_process_motion(
                output["local_rot_mats"], output["root_positions"], output["foot_contacts"], skeleton, constraint_lst=[constraint]
            )
        )
    output = to_numpy(output)

    repo = session.model_repo(args.model)
    upstream = session.backend_upstream()
    backend = session.backend_block(args.model)
    count = int(output["posed_joints"].shape[0])
    takes = []
    for k, sample in enumerate(session.split_batch(output, count)):
        take_name = f"{name}_s{args.seed}_{k}"
        path = out_dir / f"{take_name}.npz"
        session.write_take(path, sample, fps=fps, prompt=args.prompt)
        frames_written = session.take_frames(sample)
        params = params_for(args, keys_path=keys_out, duration_s=float(duration), history_frames=None, diffusion_steps=diffusion_steps, sample=k)
        params["model_repo"] = repo
        params["repo"] = upstream
        record = session.write_take_record(
            path,
            prompt=args.prompt,
            model_name=args.model,
            params=params,
            frames=frames_written,
            fps=fps,
            created_by=args.created_by,
            backend=backend,
            keys_path=keys_out,
            base_take=base,
        )
        takes.append({"name": take_name, "path": str(path), "record": str(record), "sample": k, "seed": args.seed, "frames": frames_written, "fps": float(fps)})
    print(f"wrote {count} takes to {out_dir}", flush=True)
    return {"takes": takes, "keys": str(keys_out), "fake": False}


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--inner":
        sys.exit(session.inner_entry(main_inner, sys.argv[2:]))
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen motion keys ...\n")
    sys.exit(2)
