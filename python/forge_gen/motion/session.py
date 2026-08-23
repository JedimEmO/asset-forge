"""The inner half's shared state for ARDY: one model load, forward kinematics, take and record writers.

Runs under ``backends/ardy/.env`` only. Generating a single clip pays the
model + text-encoder load (~1 min, ~16 GB), so :func:`load_model` loads once
per process and every cell of a sweep or every sample of a keyed run reuses
it. :func:`fk` is the forward kinematics the old ``ardy_keyframe_gen.py`` and
``ardy_constrained_gen.py`` each carried a copy of; it lives here once.
:func:`write_take` writes the ``.npz`` exactly as upstream's ``np.savez``
did (``fps`` and ``text`` as 0-d arrays beside the motion arrays), and
:func:`take_record` is the one place that knows what a ``take`` record says.

Nothing here is imported at module level that the system python lacks:
torch, numpy and ardy come in inside the functions, and the record side is
:mod:`forge_gen.records` (stdlib).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

from forge_gen import records

#: The tool name a take record carries, as the Rust sidecar's generator block spells it.
TOOL = "ardy"

#: The backend directory name.
BACKEND = "ardy"

#: The hub org the released models live under (``ardy.model.registry.HF_ORG``).
HF_ORG = "nvidia"

#: One model per process.
_MODEL = None
_MODEL_NAME: str | None = None
_MODEL_RESOLVED: str | None = None


# ------------------------------------------------------------------- model --


def device() -> str:
    """``cuda:0`` when torch sees a card, else ``cpu`` (slow, but it runs)."""
    import torch

    return "cuda:0" if torch.cuda.is_available() else "cpu"


def resolve(name: str) -> str:
    """The released folder name behind a nickname (``core`` → ``ARDY-Core-RP-20FPS-Horizon40``)."""
    from ardy.model.registry import resolve_model_name

    return resolve_model_name(name, checkpoints_dir=None)


def load_model(name: str = "core"):
    """Load ARDY ``name`` once; later calls with the same name return the loaded model.

    The text encoder (LLM2Vec over Llama-3-8B, from ``TEXT_ENCODERS_DIR``)
    loads with it, which is most of the time and the VRAM.
    """
    global _MODEL, _MODEL_NAME, _MODEL_RESOLVED
    if _MODEL is not None and _MODEL_NAME == name:
        return _MODEL
    from ardy.model import load_model as ardy_load_model

    resolved = resolve(name)
    started = time.time()
    model = ardy_load_model(resolved, device=device(), checkpoints_dir=None)
    fps = model.motion_rep.fps
    print(f"Loaded {resolved} @{fps}fps in {time.time() - started:.0f}s", flush=True)
    _MODEL, _MODEL_NAME, _MODEL_RESOLVED = model, name, resolved
    return model


def model_repo(name: str) -> str:
    """``nvidia/<folder>`` for a nickname."""
    return f"{HF_ORG}/{resolve(name)}"


def model_revision(name: str) -> str | None:
    """The hub commit the cached snapshot is at, when the cache has it."""
    try:
        from huggingface_hub import snapshot_download

        path = Path(snapshot_download(repo_id=model_repo(name), local_files_only=True))
    except Exception:  # noqa: BLE001 - unknown is unknown
        return None
    return path.name if path.parent.name == "snapshots" else None


def default_history_frames(model) -> int:
    """ARDY's own default for ``crop_history_length``: the largest patch-aligned window under 10 s minus the horizon."""
    fps = model.motion_rep.fps
    patch = model.num_frames_per_token
    max_window = (int(10 * fps) // patch) * patch
    return ((max_window - model.gen_horizon_len) // patch) * patch


# ---------------------------------------------------------------------- fk --


def fk(local_rot_mats, root_positions, skeleton):
    """Global joint rotations and positions from local rotations and the root path.

    The constraint set wants both, and the npz only stores local rotations.
    Same maths as ``ardy_keyframe_gen.py`` / ``ardy_constrained_gen.py``
    carried: offsets from the skeleton's neutral pose, chained parent-first.
    Returns ``(glob_r [T,J,3,3], pos [T,J,3])`` as float64 numpy arrays.
    """
    import numpy as np

    parents = [int(p) for p in skeleton.joint_parents]
    neutral = skeleton.neutral_joints.detach().cpu().numpy().astype(np.float64)
    offsets = np.array([neutral[i] - (neutral[p] if p >= 0 else 0.0) for i, p in enumerate(parents)])
    local = np.asarray(local_rot_mats, dtype=np.float64)
    root = np.asarray(root_positions, dtype=np.float64)
    T, J = local.shape[0], local.shape[1]
    glob_r = np.zeros((T, J, 3, 3))
    pos = np.zeros((T, J, 3))
    for j, p in enumerate(parents):
        if p < 0:
            glob_r[:, j] = local[:, j]
            pos[:, j] = root
        else:
            glob_r[:, j] = glob_r[:, p] @ local[:, j]
            pos[:, j] = pos[:, p] + np.einsum("tij,j->ti", glob_r[:, p], offsets[j])
    return glob_r, pos


def skeleton_for(joint_count: int):
    """The ARDY skeleton with this many joints (27 for ``core``), without loading a model."""
    from ardy.skeleton.registry import build_skeleton

    return build_skeleton(joint_count)


def posed_joints(local_rot_mats, root_positions):
    """Just the positions, for a take that carries no ``posed_joints`` member."""
    import numpy as np

    local = np.asarray(local_rot_mats)
    return fk(local, root_positions, skeleton_for(local.shape[1]))[1]


# ------------------------------------------------------------------- takes --


def split_batch(output: dict, count: int) -> list[dict]:
    """One dict per sample from a batched model output (arrays whose first axis is the batch)."""
    return [
        {k: (v[b] if hasattr(v, "shape") and len(v.shape) > 0 and v.shape[0] == count else v) for k, v in output.items()}
        for b in range(count)
    ]


def write_take(path: str | os.PathLike, sample: dict, *, fps, prompt: str) -> Path:
    """Write one take as ``np.savez`` of its arrays plus ``fps`` and ``text`` 0-d members — what upstream wrote."""
    import numpy as np

    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    np.savez(target, **{k: np.asarray(v) for k, v in sample.items()}, fps=np.asarray(fps), text=np.asarray(prompt))
    return target


def take_frames(sample: dict) -> int:
    """How many frames a sample carries."""
    return int(sample["posed_joints"].shape[0])


# ----------------------------------------------------------------- records --


def _git_head(directory: Path) -> str | None:
    try:
        done = subprocess.run(
            ["git", "-C", str(directory), "rev-parse", "HEAD"], capture_output=True, text=True, timeout=20, check=False
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() or None if done.returncode == 0 else None


def backend_commit() -> str | None:
    """The commit the ARDY checkout is at: the clone's HEAD, else the install receipt's, else unknown."""
    from forge_gen import backends as backends_mod

    try:
        backend = backends_mod.load_backend(os.environ.get("FORGE_BACKEND", BACKEND))
    except Exception:  # noqa: BLE001
        return None
    if backend.checkout.exists():
        head = _git_head(backend.checkout.resolve())
        if head:
            return head
    receipt = backend.installed()
    if receipt and receipt.get("commit"):
        return str(receipt["commit"])
    return None


def backend_upstream() -> str | None:
    """The upstream URL ``backend.toml`` names."""
    from forge_gen import backends as backends_mod

    try:
        return backends_mod.load_backend(os.environ.get("FORGE_BACKEND", BACKEND)).upstream
    except Exception:  # noqa: BLE001
        return None


def backend_block(model_name: str) -> dict:
    """The record's ``backend`` block for this process: python, torch, commit, model and its hub revision."""
    try:
        import torch

        torch_version = torch.__version__
    except Exception:  # noqa: BLE001
        torch_version = None
    return records.backend_block(
        name=BACKEND,
        commit=backend_commit(),
        python=".".join(map(str, sys.version_info[:3])),
        torch=torch_version,
        model=model_repo(model_name),
        model_revision=model_revision(model_name),
    )


#: Every knob a take record states, in the order the contract lists them;
#: ``records.write`` sorts the keys anyway. ``cfg`` is the text-CFG weight
#: (the Rust reader projects ``params.cfg`` into ``ArdyParams::cfg``), and
#: ``repo`` the upstream URL (``ArdyParams::repo``). ``batch_size`` and
#: ``grid`` (prompts/seeds/cfg/durations/samples counts) are the two knobs
#: that decide which cells share a forward pass — without them, sample 3 of
#: a batch of 8 cannot be re-derived from its own record.
PARAM_KEYS = (
    "model",
    "model_repo",
    "repo",
    "prompt",
    "label",
    "seed",
    "cfg",
    "cfg_constraint",
    "duration_s",
    "sample",
    "diffusion_steps",
    "history_frames",
    "postprocess",
    "keys_file",
    "keys_sha256",
    "preset",
    "batch_size",
    "grid",
)


def take_record(
    *,
    npz_path: str | os.PathLike,
    prompt: str,
    model_name: str,
    params: dict,
    frames: int,
    fps,
    created_by: str | None = None,
    backend: dict | None = None,
    keys_path: str | os.PathLike | None = None,
    base_take: str | os.PathLike | None = None,
    note: str | None = None,
) -> dict:
    """The ``take`` record for one written ``.npz``: every knob stated, the prompt as an input, the file hashed.

    ``params`` supplies what it knows under :data:`PARAM_KEYS`; anything it
    does not name is written ``null`` (unknown), never defaulted.
    """
    rec = records.new_record("take", TOOL, created_by=created_by)
    rec["backend"] = backend if backend is not None else records.backend_block(name=BACKEND)
    records.add_input(rec, "prompt", prompt=prompt)
    if base_take is not None:
        records.add_input(rec, "base_take", base_take, source="the take whose pose the keys are authored against")
    if keys_path is not None:
        records.add_input(rec, "keys", keys_path, source="authored keyframe constraints")
    full = {key: params.get(key) for key in PARAM_KEYS}
    full.setdefault("model", model_name)
    full["model"] = full["model"] or model_name
    rec["params"] = full
    records.add_output(rec, npz_path)
    rec["measured"] = {"frames": int(frames), "fps": float(fps)}
    rec["note"] = note
    return rec


def record_path_for(npz_path: str | os.PathLike) -> Path:
    """``<take>.take.json`` beside the take."""
    path = Path(npz_path)
    return path.with_name(path.stem + ".take.json")


def write_take_record(npz_path: str | os.PathLike, **kwargs) -> Path:
    """Build and write the record beside its take; returns the record path."""
    rec = take_record(npz_path=npz_path, **kwargs)
    return records.write(rec, record_path_for(npz_path))


# ------------------------------------------------------------------ output --


def emit_json(payload: dict) -> None:
    """The inner's last stdout line: one JSON object the outer holds back and returns."""
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def inner_entry(main, argv: list[str]) -> int:
    """Run an inner ``main(argv) -> dict`` and speak the exit-code table on the way out.

    The dict is printed as the last stdout line; a :class:`ForgeGenError`
    becomes its payload and code (the outer relays it unchanged); any other
    exception is a backend failure with the traceback on stderr.
    """
    import traceback

    from forge_gen import exit_codes
    from forge_gen.exit_codes import BackendFailed, ForgeGenError

    try:
        result = main(argv)
        result.setdefault("ok", True)
        emit_json(result)
        return exit_codes.OK
    except ForgeGenError as err:
        sys.stderr.write(f"forge-gen[inner]: {err.error}: {err.message}\n")
        emit_json(err.payload())
        return err.code
    except KeyboardInterrupt:
        return 130
    except Exception as err:  # noqa: BLE001 - the last line must still be JSON
        traceback.print_exc()
        failure = BackendFailed(f"{err.__class__.__name__}: {err}")
        emit_json(failure.payload())
        return failure.code
