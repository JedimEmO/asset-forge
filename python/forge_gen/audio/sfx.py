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
nothing wrote a record, so the files are all that is left of them.

**It runs on the ComfyUI host**, through the TTS-Audio-Suite pack, so there
is no venv here and no inner half that imports torch: the whole of it is
load ``backends/moss_sfx/workflows/sfx.api.json``, patch the knobs this run
states into it, ``POST /prompt``, poll, fetch. The graph's own
``enable_audio_cache`` is off in the template — it is the pack's in-memory
cache, and it would hand back an earlier render as a re-roll, which is the
one thing ``cached`` exists to observe rather than infer.

``SaveAudio`` writes FLAC (ComfyUI v0.34.2 has no WAV save node), so the
effect is transcoded to 16-bit PCM here before anything measures it; ffmpeg
is a dependency this verb did not have when it wrote the WAV itself with
soundfile, and a missing one is exit 6 **before** the card is leased.

``build_record`` is pure — the record for a rendered effect from its
numbers — so the schema can be checked with nothing running.
"""

from __future__ import annotations

import os
import random
import sys
import tempfile
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import comfy, placeholders, records
from forge_gen.audio import ffmpeg_bin, transcode_wav
from forge_gen.exit_codes import InputRejected, UsageError

#: The backend directory this command runs through.
BACKEND = "moss_sfx"

#: The tracked graph this command runs, under ``backends/moss_sfx/workflows/``.
WORKFLOW = "sfx.api.json"

#: Seconds between ``/history`` polls.
POLL_S = 2.0

#: How long one effect may take end to end. The DiT ``torch.compile``s
#: itself on first use and can spend several minutes doing it — the pack
#: says so in its own tooltip, and the venv's ``TORCHDYNAMO_DISABLE`` died
#: with the venv (designs/hosting.md, ComfyUI, 2026-08-30).
GENERATE_TIMEOUT_S = 1800.0

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
    executor: str = "comfy",
    comfyui_commit: str | None = None,
    workflow_sha256: str | None = None,
    packs: dict | None = None,
    workflow: str | None = None,
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
        rec["backend"]["executor"] = executor
    else:
        rec = records.new_record("sfx", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=BACKEND, commit=commit, python=python, torch=torch, model=model,
            model_revision=model_revision, executor=executor, comfyui_commit=comfyui_commit,
            workflow_sha256=workflow_sha256, packs=packs,
        )
    records.add_input(rec, "prompt", prompt=prompt)
    rec["params"] = {
        "model": model,
        "seed": int(seed),
        "duration_s": float(seconds),
        "steps": int(steps),
        "cfg": float(cfg),
    }
    if workflow:
        rec["params"]["workflow"] = workflow
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
    parser.add_argument("--timeout", type=float, default=GENERATE_TIMEOUT_S, metavar="S", help=f"seconds one effect may take (default {GENERATE_TIMEOUT_S:g}; the first of a session compiles the DiT)")


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


# ---------------------------------------------------------------- the graph --


def _say(text: str) -> None:
    """One progress line on stderr, so the JSON last line stays the last line."""
    sys.stderr.write(f"[sfx] {text}\n")
    sys.stderr.flush()


def _progress(seconds: float, entry) -> None:
    """A line every half minute: the first effect of a session compiles the DiT."""
    if seconds >= 30 and int(seconds) % 30 == 0:
        _say(f"{seconds:.0f}s on the host (the first render of a session compiles the DiT)")


def template_inputs(spec: dict, job: dict, *, prefix: str) -> dict:
    """Every knob ``sfx.api.json`` marks, filled from a checked spec.

    All of them, always: the template refuses a marker nobody fills, which
    is "a recipe states every knob" said in a place a graph can enforce.
    """
    return {
        "prompt": job["prompt"],
        "seconds": float(job["seconds"]),
        "seed": int(spec["seed"]),
        "steps": int(spec["steps"]),
        "cfg": float(spec["cfg"]),
        "filename_prefix": prefix,
    }


def run(args) -> dict:
    """Validate, load the tracked graph, render every job on the host, record each.

    A batch is a graph per job at one seed, the same as it was a call per
    job at one seed: re-rendering one line of a batch on its own gives back
    the sound it gave inside the batch.
    """
    spec = plan(args)
    backend = backends_mod.load_backend(BACKEND)
    if spec["model"] != DEFAULT_MODEL:
        raise InputRejected(
            f"the host's sound-effect node offers only {DEFAULT_MODEL}, not {spec['model']!r}",
            hint=f"unset ${MODEL_ENV}, or add a template for the other weights",
        )
    # Everything that can be refused before the card is leased: a missing
    # ffmpeg (6), a template that cannot be built or patched (3).
    ffmpeg = ffmpeg_bin()
    host = comfy.host_backend(backend)
    base = comfy.base_url(backend, records.project())
    graph, template_sha = comfy.load_template(backend, WORKFLOW)
    where = str(backend.workflow(WORKFLOW))
    points = comfy.patch_points(graph, where)
    facts = {
        "executor": "comfy",
        "comfyui_commit": comfy.host_commit(host),
        "workflow_sha256": template_sha,
        "packs": comfy.packs_block(backend),
    }

    rendered = []
    comfy_blocks = []
    for job in spec["jobs"]:
        out = Path(job["out"])
        inputs = template_inputs(spec, job, prefix=comfy.output_prefix(out.stem))
        patched = comfy.patch(graph, inputs, where)
        _say(f"{job['seconds']:g} s, {spec['steps']} steps, cfg {spec['cfg']:g}, seed {spec['seed']}: {job['prompt']}")
        prompt_id = comfy.submit(base, patched, comfy.client_id())
        entry = comfy.wait_for(base, prompt_id, timeout=float(getattr(args, "timeout", GENERATE_TIMEOUT_S)), poll=POLL_S, on_progress=_progress)
        out.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="forge-sfx-") as scratch:
            saved = comfy.fetch(base, entry, Path(scratch))
            transcode_wav(ffmpeg, saved[0], out)
        rec = build_record(
            prompt=job["prompt"],
            seconds=job["seconds"],
            seed=spec["seed"],
            steps=spec["steps"],
            cfg=spec["cfg"],
            out_path=out,
            model=spec["model"],
            created_by=spec["created_by"],
            workflow=WORKFLOW,
            **facts,
        )
        records.write(rec, job["record"])
        _say(f"OK {out} (seed {spec['seed']})")
        rendered.append({"out": str(out), "record": job["record"]})
        comfy_blocks.append(
            {
                "template": f"backends/{BACKEND}/workflows/{WORKFLOW}",
                "template_sha256": template_sha,
                "inputs": inputs,
                "comfyui_commit": facts["comfyui_commit"],
                "packs": facts["packs"],
                "prompt_id": prompt_id,
                "cached": comfy.was_cached(entry, points["filename_prefix"][0]),
            }
        )
    summary = _success(spec, rendered)
    # One job, one block — the shape the daemon copies into the row. A batch
    # is several graphs, and the row is about the job it ran, so the first
    # is the one it names and the rest travel beside it.
    summary["comfy"] = comfy_blocks[0]
    if len(comfy_blocks) > 1:
        summary["comfy_batch"] = comfy_blocks
    return summary


def run_fake(args) -> dict:
    """A placeholder tone where the sound would be, and a record that says so.

    A tone and not silence, so the placeholder passes the same ``forge
    audio inspect`` a real render must; and never over a file an earlier
    ``--fake`` run did not write.
    """
    spec = plan(args)
    placeholders.refuse_real(*(path for job in spec["jobs"] for path in (job["out"], job["record"])))
    rendered = []
    for job in spec["jobs"]:
        placeholders.placeholder_wav(job["out"], seconds=job["seconds"])
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
