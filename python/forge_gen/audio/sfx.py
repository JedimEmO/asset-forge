"""``forge-gen sfx``: one sound effect from a prompt, through MOSS-SoundEffect v2 (1.3B DiT).

    forge-gen sfx --prompt "heavy sci-fi blast door sliding shut with a metallic clunk" \\
        --seconds 3 --out out/audio/door_blast_close.wav

Output: the WAV where ``--out`` says plus a ``forge_record`` beside it
(``<stem>.json``, or ``--record``). Describe the *sound*, not the game
event: material, action, environment, tail. Loading the model is most of a
call; ``--batch-file`` (one ``name|seconds|prompt`` per line, into
``--out-dir``) renders a session on one load.

Every render is seeded, and the seed is recorded. The seven sfx that shipped
before this could not be regenerated at all: nothing seeded the sampler and
nothing wrote a record, so the files are all that is left of them. The seed
goes to the pipeline's own ``seed=`` — its noise initialiser — and to
torch's global RNG; the predecessor set only the latter, and the pipeline's
default of 0 quietly governed every render it made.

The outer half (``run``) validates, resolves the backend and execs the inner
half under the backend's venv; the inner half (``main_inner``) imports
torch. ``build_record`` is pure — the record for a rendered effect from its
numbers — so the schema can be checked without the DiT resident.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import random
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import launcher, placeholders, records
from forge_gen.exit_codes import InputRejected, UsageError

#: The backend directory this command runs through.
BACKEND = "moss_sfx"

#: The record's ``tool``, as the sidecar's generator block names it
#: (``forge_library::schema::Generator::tool``) — not the backend's name.
TOOL = "moss_sound_effect"

#: The weights, overridable for a fine-tune: ``MOSS_SFX_MODEL=<dir or hub id>``.
MODEL_ENV = "MOSS_SFX_MODEL"
DEFAULT_MODEL = "OpenMOSS-Team/MOSS-SoundEffect-v2.0"

#: The pipeline denoises a fixed latent of this many seconds and crops; more is a ValueError deep inside.
MAX_SECONDS = 30.0

DEFAULT_SECONDS = 3.0
DEFAULT_STEPS = 100
DEFAULT_CFG = 4.0


def model_id() -> str:
    """The weights this run will load: ``$MOSS_SFX_MODEL`` or the published id."""
    return os.environ.get(MODEL_ENV) or DEFAULT_MODEL


# --------------------------------------------------------------- the record --


def build_record(
    *,
    prompt: str,
    seconds: float,
    seed: int,
    steps: int,
    cfg: float,
    out_path: str | os.PathLike,
    model: str,
    created_by: str | None = None,
    commit: str | None = None,
    python: str | None = None,
    torch: str | None = None,
    model_revision: str | None = None,
    fake: bool = False,
) -> dict:
    """The record for one rendered effect.

    Split out of the inner half so the schema can be checked without the
    1.3B DiT resident — the fixture capture and the tests call it directly.
    Every field here is what the sampler was actually given. That is a
    stronger claim than reproducibility: MOSS is not bit-reproducible, so the
    hash is what proves the file is the one that was auditioned, and the seed
    is what gets a near-identical render back.

    ``measured`` is read from the file on disk, never from the request: a
    pipeline that cropped or padded would otherwise be recorded as having
    done what it was asked.
    """
    if fake:
        rec = placeholders.fake_record("sfx", TOOL, backend=BACKEND, created_by=created_by, model=model)
    else:
        rec = records.new_record("sfx", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=BACKEND, commit=commit, python=python, torch=torch, model=model, model_revision=model_revision
        )
    records.add_input(rec, "prompt", prompt=prompt)
    rec["params"] = {
        "model": model,
        "seed": int(seed),
        "duration_s": float(seconds),
        "steps": int(steps),
        "cfg": float(cfg),
    }
    records.add_output(rec, out_path)
    rec["measured"] = measure_wav(out_path)
    return rec


def measure_wav(path: str | os.PathLike) -> dict:
    """``duration_s``, ``sample_rate``, ``channels`` of a PCM WAV, read with the stdlib.

    ``None`` for all three when the file is not a WAV the stdlib reads — a
    float-PCM file, say — because a guess written there would be a
    measurement nobody made.
    """
    try:
        with wave.open(os.fspath(path), "rb") as handle:
            rate = handle.getframerate()
            frames = handle.getnframes()
            return {
                "duration_s": round(frames / rate, 4) if rate else None,
                "sample_rate": rate,
                "channels": handle.getnchannels(),
            }
    except (wave.Error, EOFError, OSError):
        return {"duration_s": None, "sample_rate": None, "channels": None}


# ------------------------------------------------------------------- parsing --


def add_parser(subparsers) -> None:
    """Register ``sfx``."""
    parser = subparsers.add_parser(
        "sfx",
        help="One sound effect from a prompt (MOSS-SoundEffect)",
        description=__doc__,
    )
    parser.add_argument("--prompt", metavar="TEXT", help="the sound: material, action, environment, tail")
    parser.add_argument("--out", metavar="WAV", help="where the WAV goes (single job)")
    parser.add_argument("--record", metavar="JSON", help="where the record goes (default: <out stem>.json beside it)")
    parser.add_argument("--seconds", type=float, default=None, metavar="S", help=f"length (default {DEFAULT_SECONDS:g}; max {MAX_SECONDS:g})")
    parser.add_argument("--seed", type=int, default=None, metavar="N", help="sampler seed (default: a fresh random one, recorded)")
    parser.add_argument("--steps", type=int, default=DEFAULT_STEPS, metavar="N", help=f"diffusion steps (default {DEFAULT_STEPS})")
    parser.add_argument("--cfg", type=float, default=DEFAULT_CFG, metavar="X", help=f"classifier-free guidance (default {DEFAULT_CFG:g})")
    parser.add_argument("--batch-file", metavar="FILE", help='one "name|seconds|prompt" per line; # comments; into --out-dir')
    parser.add_argument("--out-dir", metavar="DIR", help="where a batch's <name>.wav and <name>.json go")
    parser.add_argument("--model", default=None, metavar="ID", help=f"weights (default ${MODEL_ENV} or {DEFAULT_MODEL})")


def parse_batch_file(path: str | os.PathLike) -> list[tuple[str, float, str]]:
    """``[(name, seconds, prompt)]`` from a batch file; a malformed line is a refusal naming its number."""
    jobs: list[tuple[str, float, str]] = []
    try:
        with open(path, encoding="utf-8") as handle:
            lines = handle.read().splitlines()
    except OSError as err:
        raise InputRejected(f"cannot read --batch-file {path}: {err}") from err
    for number, raw in enumerate(lines, start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("|", 2)
        if len(parts) != 3:
            raise InputRejected(f"{path}:{number}: expected name|seconds|prompt, got {line!r}")
        name, seconds_text, prompt = (part.strip() for part in parts)
        if not name or any(ch in name for ch in "/\\") or name in (".", ".."):
            raise InputRejected(f"{path}:{number}: {name!r} is not a file stem")
        try:
            seconds = float(seconds_text)
        except ValueError:
            raise InputRejected(f"{path}:{number}: seconds {seconds_text!r} is not a number") from None
        jobs.append((name, seconds, prompt))
    if not jobs:
        raise InputRejected(f"{path} has no jobs (every line blank or a comment)")
    return jobs


def _check_job(prompt: str, seconds: float, *, where: str) -> None:
    if not prompt or not prompt.strip():
        raise InputRejected(f"{where}: the prompt is empty — describe the sound: material, action, environment, tail")
    if not seconds > 0:
        raise InputRejected(f"{where}: --seconds must be > 0, got {seconds:g}")
    if seconds > MAX_SECONDS:
        raise InputRejected(f"{where}: --seconds {seconds:g} exceeds the pipeline's {MAX_SECONDS:g} s latent; render shorter pieces")


def plan(args) -> dict:
    """Turn the arguments into the spec the inner half renders: validated, absolute.

    ``{"model", "seed", "steps", "cfg", "created_by", "project", "jobs":
    [{"prompt", "seconds", "out", "record"}]}``. The seed is drawn here when
    none was given — a seed nobody chose is still a seed worth having, and
    writing it down is the difference between "close enough to re-render"
    and the seven shipped effects that can never be made again. One seed per
    run, re-applied per job, so re-rendering one line of a batch gives back
    the same sound it gave inside the batch.
    """
    if args.batch_file and (args.prompt or args.out):
        raise UsageError("--batch-file replaces --prompt/--out; pass one or the other")
    if args.batch_file:
        if not args.out_dir:
            raise UsageError("--batch-file needs --out-dir")
        if args.record:
            raise UsageError("--record names one file; a batch writes <name>.json beside each WAV")
        out_dir = Path(args.out_dir).resolve()
        jobs = []
        for name, seconds, prompt in parse_batch_file(args.batch_file):
            seconds = float(seconds if args.seconds is None else args.seconds)
            _check_job(prompt, seconds, where=f"{args.batch_file}: {name}")
            jobs.append(
                {
                    "prompt": prompt,
                    "seconds": seconds,
                    "out": str(out_dir / f"{name}.wav"),
                    "record": str(out_dir / f"{name}.json"),
                }
            )
    else:
        if args.prompt is None or not args.out:
            raise UsageError("either --prompt TEXT --out WAV, or --batch-file FILE --out-dir DIR")
        out = Path(args.out).resolve()
        if out.suffix.lower() != ".wav":
            raise InputRejected(f"--out {args.out}: the generator writes PCM WAV; name it .wav (transcode afterwards if needed)")
        seconds = float(DEFAULT_SECONDS if args.seconds is None else args.seconds)
        _check_job(args.prompt, seconds, where="--prompt")
        record = Path(args.record).resolve() if args.record else out.with_suffix(".json")
        jobs = [{"prompt": args.prompt, "seconds": seconds, "out": str(out), "record": str(record)}]
    if args.steps < 1:
        raise InputRejected(f"--steps must be >= 1, got {args.steps}")
    if args.cfg < 0:
        raise InputRejected(f"--cfg must be >= 0, got {args.cfg:g}")
    seed = args.seed if args.seed is not None else random.randrange(2**31)
    if seed < 0:
        raise InputRejected(f"--seed must be >= 0, got {seed}")
    project = records.project()
    return {
        "model": args.model or model_id(),
        "seed": int(seed),
        "steps": int(args.steps),
        "cfg": float(args.cfg),
        "created_by": getattr(args, "created_by", None),
        "project": str(project) if project else None,
        "jobs": jobs,
    }


def _success(spec: dict, rendered: list[dict]) -> dict:
    first = rendered[0]
    return {
        "ok": True,
        "record": first["record"],
        "outputs": [job["out"] for job in rendered],
        "records": [job["record"] for job in rendered],
        "seed": spec["seed"],
        "model": spec["model"],
        "_text": "\n".join(f"[sfx] OK {job['out']} (seed {spec['seed']}) -> {job['record']}" for job in rendered),
    }


# --------------------------------------------------------------------- outer --


def checkout_commit(backend: backends_mod.Backend) -> str | None:
    """HEAD of the upstream checkout the inner half will run from, or ``None`` when git will not say."""
    checkout = backend.checkout
    if not checkout.exists():
        return None
    try:
        done = subprocess.run(
            ["git", "-C", str(checkout), "rev-parse", "--verify", "HEAD"],
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return done.stdout.strip() or None


def run(args) -> dict:
    """Validate, resolve the backend (exit 3 before any GPU work), render under its venv."""
    spec = plan(args)
    backend = backends_mod.load_backend(BACKEND)
    launcher.resolve_interpreter(backend)
    spec["commit"] = checkout_commit(backend)
    with tempfile.TemporaryDirectory(prefix="forge-sfx-") as scratch:
        spec_path = Path(scratch) / "spec.json"
        spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
        result = launcher.run_inner_checked(backend, "audio.sfx", ["--spec", str(spec_path)])
    rendered = result.get("jobs") or []
    if not rendered:
        from forge_gen.exit_codes import BackendFailed

        raise BackendFailed("the inner half returned no rendered jobs")
    return _success(spec, rendered)


def run_fake(args) -> dict:
    """Silence where the sound would be, and a record that says so."""
    spec = plan(args)
    rendered = []
    for job in spec["jobs"]:
        placeholders.silence_wav(job["out"])
        rec = build_record(
            prompt=job["prompt"],
            seconds=job["seconds"],
            seed=spec["seed"],
            steps=spec["steps"],
            cfg=spec["cfg"],
            out_path=job["out"],
            model=spec["model"],
            created_by=spec["created_by"],
            fake=True,
        )
        records.write(rec, job["record"])
        rendered.append({"out": job["out"], "record": job["record"]})
    return _success(spec, rendered)


# --------------------------------------------------------------------- inner --


def _snapshot_revision(model: str) -> str | None:
    """The hub revision the cached snapshot is, for ``backend.model_revision``; ``None`` for a local dir or offline."""
    if os.path.isdir(model):
        return None
    try:
        from huggingface_hub import snapshot_download

        path = snapshot_download(model, local_files_only=True)
    except Exception:  # noqa: BLE001 - a revision is a nicety; the model id is the fact
        return None
    revision = os.path.basename(os.path.normpath(path))
    return revision if len(revision) == 40 else None


def main_inner(argv: list[str]) -> int:
    """Render every job in the spec under the backend's venv; torch is imported here and nowhere above.

    Prints progress lines and, last, one JSON object ``{"ok": true, "jobs":
    [{"out", "record"}]}``. A missing model, a CUDA fault, a pipeline
    ``ValueError`` — anything the pipeline raises — is a backend failure (5)
    with the traceback on stderr; the launcher carries the tail out.
    """
    parser = argparse.ArgumentParser(prog="forge_gen.audio.sfx --inner", add_help=True)
    parser.add_argument("--spec", required=True)
    args = parser.parse_args(argv)
    spec = json.loads(Path(args.spec).read_text(encoding="utf-8"))
    if spec.get("project"):
        records.set_project(spec["project"])
    # Opt back in by exporting TORCHDYNAMO_DISABLE=0; backend.toml sets 1 as a default.
    os.environ.setdefault("TORCHDYNAMO_DISABLE", "1")

    import soundfile
    import torch
    from moss_soundeffect_v2 import MossSoundEffectPipeline

    model = spec["model"]
    seed = int(spec["seed"])
    print(f"[sfx] loading {model}", flush=True)
    pipe = MossSoundEffectPipeline.from_pretrained(model, torch_dtype=torch.bfloat16, device="cuda")
    revision = _snapshot_revision(model)

    rendered = []
    for job in spec["jobs"]:
        # Re-seeded per job rather than once per run, so that re-rendering one
        # line of a batch gives back the same sound it gave inside the batch.
        torch.manual_seed(seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(seed)
        print(f"[sfx] {job['seconds']:g} s, {spec['steps']} steps, cfg {spec['cfg']:g}, seed {seed}: {job['prompt']}", flush=True)
        audio = pipe(
            prompt=job["prompt"],
            seconds=job["seconds"],
            num_inference_steps=spec["steps"],
            cfg_scale=spec["cfg"],
            seed=seed,
        )
        out_path = Path(job["out"])
        out_path.parent.mkdir(parents=True, exist_ok=True)
        # pipe.save_audio -> torchaudio.save -> torchcodec, whose ffmpeg libs
        # collide with system glib here; soundfile writes the wav fine.
        # PCM_16 on purpose: the stdlib wave module measures it, and a game
        # engine decodes it without a float-WAV path.
        wav = audio[0].detach().float().cpu().numpy().T
        soundfile.write(os.fspath(out_path), wav, pipe.sample_rate, subtype="PCM_16")
        rec = build_record(
            prompt=job["prompt"],
            seconds=job["seconds"],
            seed=seed,
            steps=spec["steps"],
            cfg=spec["cfg"],
            out_path=out_path,
            model=model,
            created_by=spec.get("created_by"),
            commit=spec.get("commit"),
            python=platform.python_version(),
            torch=torch.__version__,
            model_revision=revision,
        )
        records.write(rec, job["record"])
        print(f"[sfx] OK {out_path} (seed {seed})", flush=True)
        rendered.append({"out": str(out_path), "record": job["record"]})
    sys.stdout.write(json.dumps({"ok": True, "jobs": rendered}) + "\n")
    sys.stdout.flush()
    return 0


def _inner_entry(argv: list[str]) -> int:
    """``main_inner`` with a refusal turned into its exit code and JSON line, so the outer relays it unchanged."""
    import traceback

    from forge_gen import exit_codes
    from forge_gen.exit_codes import ForgeGenError

    try:
        return main_inner(argv)
    except ForgeGenError as err:
        sys.stderr.write(f"forge-gen: {err.error}: {err.message}\n")
        sys.stdout.write(json.dumps(err.payload(), ensure_ascii=False) + "\n")
        sys.stdout.flush()
        return err.code
    except KeyboardInterrupt:
        return 130
    except Exception as err:  # noqa: BLE001 - anything the model raises is a backend failure, with its traceback
        traceback.print_exc()
        payload = {"ok": False, "error": "backend_failed", "message": f"{err.__class__.__name__}: {err}"}
        sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
        sys.stdout.flush()
        return exit_codes.BACKEND_FAILED


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--inner":
        sys.exit(_inner_entry(sys.argv[2:]))
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen sfx ...\n")
    sys.exit(2)
