"""``forge-gen speech``: one spoken line from text, a voice cloned from a reference clip.

    forge-gen speech --text "Hold the line. They breach on my mark." \\
        --voice kessa --out out/audio/kessa_hold_the_line.wav

    forge-gen speech --text "..." --voice path/to/clip.wav --out out/audio/line.wav   # a brought clip
    forge-gen speech --text "Testing a fresh voice" --out out/audio/test.wav         # no cloning

A voice is a reference clip — 5–15 s of clean speech defines a character's
voice permanently. Same reference in, same voice out; the record carries
the clip's hash so "same" is checkable. A bare ``--voice <name>`` is a voice
designed by ``forge-gen voice``: it resolves to
``assets-src/voices/<name>/ref.wav``, and the ``voice.json`` beside it is
recorded too, so a line's provenance chains back through the audition clip
to the description and the seed that made the voice. ``--lines-file``
(one ``stem|text`` per line, into ``--out-dir``) renders a session of them.

**It runs on the ComfyUI host**, through the TTS-Audio-Suite pack, so there
is no venv here and no inner half that imports torch. The reference travels
as an *uploaded file* — ``POST /upload/image`` puts the clip in ComfyUI's
input directory, ``LoadAudio`` reads it and ``CharacterVoicesNode`` hands it
on — and never as a path, which is what the old inner half could not do
(``decisions.md``, 2026-08-23: torchaudio reaches for torchcodec, whose
ffmpeg libraries will not load beside the system glib here). The decode now
happens in the host's own venv, which carries PyAV.

**The model is not the one the venv ran.** TTS-Audio-Suite offers MOSS-TTS
as ``1.7B`` (OpenMOSS-Team/MOSS-TTS-Local-Transformer) or as the 8B Delay
checkpoints, and the 8B is the one this repository measured OOM-ing on the
24 GB card with the audio tokenizer loaded. So a line spoken after this move
is a different voice from one spoken before it, at the same reference; the
records of the shipped lines still say what made them, and re-auditioning a
character is the only honest way to put a new line beside an old one.

The command is backend-agnostic on its face — ``--backend`` names who
speaks — and ``moss_tts`` is the one backend v1 ships. OmniVoice is a
documented v1.1 add; naming it today exits 2 and says so.

Each WAV gets a ``forge_record`` beside it (``<stem>.json``, or ``--record``):
the spoken line is the prompt, because that is what a reader searches for,
and the reference is an input with its sha256, because that is what makes
the same character come back. ``seed`` is now always a number and always
recorded: the node takes one, so every line has one whether or not the
caller chose it, and a drawn seed written down is the difference between a
line that can be asked for again and one that cannot.

``build_record`` is pure — the record from its numbers — so the schema can
be checked without the model resident.
"""

from __future__ import annotations

import os
import random
import re
import sys
import tempfile
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import placeholders, records
from forge_gen.audio import ffmpeg_bin, transcode_wav
from forge_gen.exit_codes import InputRejected, UsageError

#: The backends that can speak. ``moss_tts`` is the one that exists.
BACKENDS = ("moss_tts",)
DEFAULT_BACKEND = "moss_tts"

#: Names a user may reach for that are not here yet, with the answer.
PLANNED = {"omnivoice": "OmniVoice is a v1.1 add; only moss_tts speaks in this build"}

#: The record's ``tool`` — the sidecar's generator name, which for this backend is also the backend's name.
TOOL = "moss_tts"

#: What the host's engine node loads for its ``1.7B`` variant, read from the
#: pack's own ``model_specs.py`` at its pin on 2026-08-30. It is **not** the
#: ``-v1.5`` checkpoint the venv ran: the pack does not offer that one, and
#: the 8B Delay checkpoints it does offer are what OOMs here.
MODEL_ENV = "MOSS_TTS_MODEL"
DEFAULT_MODEL = "OpenMOSS-Team/MOSS-TTS-Local-Transformer"

#: The variant the tracked graph states, and the only one this command runs.
MODEL_VARIANT = "1.7B"

#: The tracked graph, under ``backends/moss_tts/workflows/``.
WORKFLOW = "speech.api.json"

#: Seconds between ``/history`` polls, and how long one line may take.
POLL_S = 2.0
GENERATE_TIMEOUT_S = 1800.0

#: Reference clip containers the processor reads (it decodes through its own audio loader).
REFERENCE_SUFFIXES = (".wav", ".mp3", ".flac", ".m4a")

#: Where designed voices live (``forge-gen voice``), relative to the project root.
VOICES_DIR = "assets-src/voices"

#: A bare ``--voice`` argument that is a voice's name rather than a path.
VOICE_NAME_RE = re.compile(r"^[a-z0-9_]+$")

#: What a reference should be, and what it must be. Outside the first pair
#: is a warning — the voice may come back thin; outside the second is a
#: refusal — a two-second clip is not a voice, a minute is a podcast.
REFERENCE_GOOD_S = (5.0, 15.0)
REFERENCE_HARD_S = (3.0, 30.0)

#: The sampling knobs the model card recommends; recorded with every line.
SAMPLING = {"temperature": 1.7, "top_p": 0.8, "top_k": 25, "repetition_penalty": 1.0, "max_new_tokens": 4096}

#: MOSS-TTS wants the language by name ("English"); the 31 it speaks, by ISO 639-1/-3 code.
LANGUAGES = {
    "zh": "Chinese", "yue": "Cantonese", "en": "English", "ar": "Arabic", "cs": "Czech",
    "da": "Danish", "nl": "Dutch", "fi": "Finnish", "fr": "French", "de": "German",
    "el": "Greek", "he": "Hebrew", "hi": "Hindi", "hu": "Hungarian", "it": "Italian",
    "ja": "Japanese", "ko": "Korean", "mk": "Macedonian", "ms": "Malay", "fa": "Persian",
    "pl": "Polish", "pt": "Portuguese", "ro": "Romanian", "ru": "Russian", "es": "Spanish",
    "sw": "Swahili", "sv": "Swedish", "tl": "Filipino", "th": "Thai", "tr": "Turkish",
    "vi": "Vietnamese",
}  # fmt: skip

#: What ``MossTTSEngineNode`` accepts for ``language``, captured from the
#: running host's ``GET /object_info`` on 2026-08-30. A name outside this is
#: a refusal here rather than a ``POST /prompt`` failure with the card
#: leased — which is also why ``tl`` maps to the node's own word, Filipino.
NODE_LANGUAGES = (
    "Auto", "Chinese", "English", "German", "Spanish", "French", "Japanese", "Italian",
    "Hungarian", "Korean", "Russian", "Persian", "Arabic", "Polish", "Portuguese", "Czech",
    "Danish", "Swedish", "Greek", "Turkish", "Cantonese", "Dutch", "Finnish", "Hindi",
    "Macedonian", "Malay", "Romanian", "Swahili", "Filipino", "Thai", "Vietnamese", "Hebrew",
)

DEFAULT_LANGUAGE = "en"


def model_id() -> str:
    """The weights this run will load: ``$MOSS_TTS_MODEL`` or the published id."""
    return os.environ.get(MODEL_ENV) or DEFAULT_MODEL


def language_name(code_or_name: str | None) -> str | None:
    """``en`` → ``English``; a name the model knows passes through; ``auto``/empty → ``None`` (the model infers).

    Anything else is passed through as given — the model card's tag is free
    text and a language not in the table is still a language — after a note
    on stderr, so a typo is visible rather than silently inferred around.
    """
    if code_or_name is None:
        return None
    text = code_or_name.strip()
    if not text or text.lower() in ("auto", "none", "infer"):
        return None
    key = text.lower()
    if key in LANGUAGES:
        return LANGUAGES[key]
    by_name = {name.lower(): name for name in LANGUAGES.values()}
    if key in by_name:
        return by_name[key]
    sys.stderr.write(f"forge-gen: speech: --language {text!r} is not one of the 31 the model card lists; passing it through as a tag\n")
    return text


def resolve_voice(arg: str) -> Path:
    """``--voice`` as a path to a clip: a bare name is ``<project>/assets-src/voices/<name>/ref.wav``.

    A name that resolves to nothing is refused with the command that makes
    it: a project never has to bring a voice, it designs one. Anything with
    a separator or a suffix is a path to a brought clip and is taken as is.
    """
    text = arg.strip()
    if VOICE_NAME_RE.match(text):
        root = records.project() or Path.cwd()
        ref = root / VOICES_DIR / text / "ref.wav"
        if not ref.is_file():
            raise InputRejected(
                f"--voice {text}: no designed voice at {ref} — `forge gen voice {text} --describe \"...\"` designs one, "
                f"or pass a path to a 5–15 s reference clip",
                voice=text,
            )
        return ref
    return Path(text)


def voice_record_beside(reference: str | os.PathLike | None) -> Path | None:
    """The ``voice.json`` beside a designed voice's ``ref.*``, when there is one."""
    if reference is None:
        return None
    ref = Path(reference)
    if ref.stem.lower() != "ref":
        return None
    record = ref.with_name("voice.json")
    return record if record.is_file() else None


def voice_name(reference: str | os.PathLike | None) -> str | None:
    """The voice's name from its clip: the file's stem, or the folder's when the file is ``ref.*``.

    ``assets-src/voices/kessa/ref.wav`` is kessa; ``calm.wav`` is calm. A
    name is what a reader searches for; the hash is what identifies it.
    """
    if reference is None:
        return None
    path = Path(reference)
    if path.stem.lower() == "ref" and path.parent.name:
        return path.parent.name
    return path.stem


# --------------------------------------------------------------- the record --


def build_record(
    *,
    text: str,
    out_path: str | os.PathLike,
    model: str,
    reference: str | os.PathLike | None,
    language: str | None,
    voice_text: str | None = None,
    voice_record: str | os.PathLike | None = None,
    seed: int | None = None,
    sampling: dict | None = None,
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
    """The record for one spoken line.

    Split out of the inner half so the schema can be checked without the
    model resident. The line itself is the ``prompt`` input; the reference
    clip is the ``reference`` input, hashed, and ``params.reference`` repeats
    its path because the Rust projection (``speech_params``) reads it from
    either place. ``voice_record`` — the ``voice.json`` of a designed voice —
    is a second hashed input and ``params.voice_record``, so the line's
    provenance reaches the description and the seed. ``seed`` is ``None``
    unless one was given to torch's RNG. ``voice_text`` is carried as given
    and unused by this backend: MOSS-TTS takes no transcript of the
    reference; OmniVoice will.
    """
    if fake:
        rec = placeholders.fake_record("speech", TOOL, backend=TOOL, created_by=created_by, model=model)
        rec["backend"]["executor"] = executor
    else:
        rec = records.new_record("speech", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=TOOL, commit=commit, python=python, torch=torch, model=model,
            model_revision=model_revision, executor=executor, comfyui_commit=comfyui_commit,
            workflow_sha256=workflow_sha256, packs=packs,
        )
    records.add_input(rec, "prompt", prompt=text)
    reference_entry = records.add_input(rec, "reference", reference) if reference is not None else None
    record_entry = records.add_input(rec, "voice_record", voice_record) if voice_record is not None else None
    params = {
        "model": model,
        "seed": None if seed is None else int(seed),
        "voice": voice_name(reference),
        "reference": reference_entry["path"] if reference_entry else None,
        "voice_record": record_entry["path"] if record_entry else None,
        "language": language,
        "voice_text": voice_text,
    }
    params.update(sampling or {})
    if workflow:
        params["workflow"] = workflow
    rec["params"] = params
    records.add_output(rec, out_path)
    rec["measured"] = measure_wav(out_path)
    return rec


def measure_wav(path: str | os.PathLike) -> dict:
    """``duration_s``, ``sample_rate``, ``channels`` of a PCM WAV via the stdlib; nulls when it is not one."""
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
    """Register ``speech``."""
    parser = subparsers.add_parser(
        "speech",
        help="One spoken line (MOSS-TTS)",
        description=__doc__,
    )
    parser.add_argument("--text", metavar="TEXT", help="the line to speak; [pause 1.5s] is an explicit pause")
    parser.add_argument("--out", metavar="WAV", help="where the WAV goes (single line)")
    parser.add_argument("--record", metavar="JSON", help="where the record goes (default: <out stem>.json beside it)")
    parser.add_argument("--voice", metavar="NAME|REF", help=f"a voice designed by `voice` (a name under {VOICES_DIR}/), or a reference clip, 5–15 s of clean speech (.wav/.mp3/.flac/.m4a); omit for an uncloned voice")
    parser.add_argument("--voice-text", metavar="TEXT", help="transcript of the reference (recorded; moss_tts does not use it)")
    parser.add_argument("--language", default=DEFAULT_LANGUAGE, metavar="LANG", help=f'"en", "Norwegian", … (default {DEFAULT_LANGUAGE}); "auto" lets the model infer')
    parser.add_argument("--backend", default=DEFAULT_BACKEND, metavar="NAME", help=f"who speaks (default {DEFAULT_BACKEND}; OmniVoice is v1.1)")
    parser.add_argument("--seed", type=int, default=None, metavar="N", help="seed torch's RNG for the sampler (recorded only when given)")
    parser.add_argument("--lines-file", metavar="FILE", help='one "stem|text" per line; # comments; into --out-dir')
    parser.add_argument("--out-dir", metavar="DIR", help="where a batch's <stem>.wav and <stem>.json go")
    parser.add_argument("--model", default=None, metavar="ID", help=f"weights (default ${MODEL_ENV} or {DEFAULT_MODEL})")


def parse_lines_file(path: str | os.PathLike) -> list[tuple[str, str]]:
    """``[(stem, text)]`` from a lines file; a malformed line is a refusal naming its number."""
    jobs: list[tuple[str, str]] = []
    try:
        with open(path, encoding="utf-8") as handle:
            lines = handle.read().splitlines()
    except OSError as err:
        raise InputRejected(f"cannot read --lines-file {path}: {err}") from err
    for number, raw in enumerate(lines, start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("|", 1)
        if len(parts) != 2:
            raise InputRejected(f"{path}:{number}: expected stem|text, got {line!r}")
        stem, text = (part.strip() for part in parts)
        if not stem or any(ch in stem for ch in "/\\") or stem in (".", ".."):
            raise InputRejected(f"{path}:{number}: {stem!r} is not a file stem")
        jobs.append((stem, text))
    if not jobs:
        raise InputRejected(f"{path} has no lines (every line blank or a comment)")
    return jobs


def check_reference(path: str | os.PathLike) -> Path:
    """The reference clip, absolute, refused when it is not a clip this can use.

    A WAV is measured with the stdlib and held to :data:`REFERENCE_HARD_S`,
    with a warning outside :data:`REFERENCE_GOOD_S`; other containers are
    decoded by the model's own loader and only their extension is checked
    here. The fix for a refusal is the clip, never a flag.
    """
    ref = Path(path).expanduser()
    if not ref.is_file():
        raise InputRejected(f"--voice {path}: no such file — a voice is a 5–15 s reference clip")
    if ref.suffix.lower() not in REFERENCE_SUFFIXES:
        raise InputRejected(f"--voice {path}: {ref.suffix or 'no extension'} is not one of {', '.join(REFERENCE_SUFFIXES)}")
    ref = ref.resolve()
    if ref.suffix.lower() == ".wav":
        measured = measure_wav(ref)
        seconds = measured["duration_s"]
        if seconds is None:
            sys.stderr.write(f"forge-gen: speech: {ref} is not a PCM WAV the stdlib reads; its length was not checked\n")
        else:
            low, high = REFERENCE_HARD_S
            if seconds < low:
                raise InputRejected(f"--voice {path} is {seconds:.1f} s; a voice needs {REFERENCE_GOOD_S[0]:g}–{REFERENCE_GOOD_S[1]:g} s of clean speech")
            if seconds > high:
                raise InputRejected(f"--voice {path} is {seconds:.1f} s; trim it to {REFERENCE_GOOD_S[0]:g}–{REFERENCE_GOOD_S[1]:g} s of clean speech")
            good_low, good_high = REFERENCE_GOOD_S
            if not good_low <= seconds <= good_high:
                sys.stderr.write(f"forge-gen: speech: {ref} is {seconds:.1f} s; {good_low:g}–{good_high:g} s clones best\n")
    return ref


def plan(args) -> dict:
    """Turn the arguments into the spec the inner half speaks: validated, absolute.

    ``{"backend", "model", "reference", "voice_record", "language",
    "voice_text", "seed", "sampling", "created_by", "project", "jobs":
    [{"text", "out", "record"}]}``.
    """
    backend = (args.backend or DEFAULT_BACKEND).strip()
    if backend not in BACKENDS:
        why = PLANNED.get(backend.lower(), f"{backend!r} is not a speech backend")
        raise UsageError(f"--backend {backend}: {why}; the choices are {', '.join(BACKENDS)}", backend=backend)
    if args.lines_file and (args.text or args.out):
        raise UsageError("--lines-file replaces --text/--out; pass one or the other")
    if args.lines_file:
        if not args.out_dir:
            raise UsageError("--lines-file needs --out-dir")
        if args.record:
            raise UsageError("--record names one file; a batch writes <stem>.json beside each WAV")
        out_dir = Path(args.out_dir).resolve()
        jobs = []
        for stem, text in parse_lines_file(args.lines_file):
            if not text:
                raise InputRejected(f"{args.lines_file}: {stem}: the line is empty")
            jobs.append({"text": text, "out": str(out_dir / f"{stem}.wav"), "record": str(out_dir / f"{stem}.json")})
    else:
        if args.text is None or not args.out:
            raise UsageError("either --text TEXT --out WAV, or --lines-file FILE --out-dir DIR")
        if not args.text.strip():
            raise InputRejected("--text is empty — there is nothing to say")
        out = Path(args.out).resolve()
        if out.suffix.lower() != ".wav":
            raise InputRejected(f"--out {args.out}: the generator writes PCM WAV; name it .wav (transcode afterwards if needed)")
        record = Path(args.record).resolve() if args.record else out.with_suffix(".json")
        jobs = [{"text": args.text, "out": str(out), "record": str(record)}]
    reference = check_reference(resolve_voice(args.voice)) if args.voice else None
    voice_record = voice_record_beside(reference)
    if args.voice_text and reference is None:
        raise UsageError("--voice-text describes a --voice clip; there is none")
    if args.voice_text and backend == "moss_tts":
        sys.stderr.write("forge-gen: speech: moss_tts takes no transcript of the reference; --voice-text is recorded, not used\n")
    seed = args.seed
    if seed is not None and seed < 0:
        raise InputRejected(f"--seed must be >= 0, got {seed}")
    if seed is None:
        # The node takes a seed whether or not the caller chose one, so one
        # is drawn and written down. A seed nobody chose is still the seed
        # that spoke the line.
        seed = random.randrange(2**31)
    language = language_name(args.language)
    if language is not None and language not in NODE_LANGUAGES:
        raise InputRejected(
            f"--language {language!r} is not one the host's engine offers: {', '.join(NODE_LANGUAGES)}"
        )
    project = records.project()
    return {
        "backend": backend,
        "model": args.model or model_id(),
        "reference": str(reference) if reference else None,
        "voice_record": str(voice_record) if voice_record else None,
        "language": language,
        "voice_text": args.voice_text or None,
        "seed": seed,
        "sampling": dict(SAMPLING),
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
        "voice": voice_name(spec["reference"]),
        "model": spec["model"],
        "_text": "\n".join(f"[tts] OK {job['out']} -> {job['record']}" for job in rendered),
    }


# ---------------------------------------------------------------- the graph --


def _say(text: str) -> None:
    sys.stderr.write(f"[tts] {text}\n")
    sys.stderr.flush()


def _progress(seconds: float, entry) -> None:
    if seconds >= 30 and int(seconds) % 30 == 0:
        _say(f"{seconds:.0f}s on the host")


def template_inputs(spec: dict, job: dict, *, reference_name: str, prefix: str) -> dict:
    """Every knob ``speech.api.json`` marks, filled from a checked spec.

    ``reference`` is the **name ComfyUI filed the upload under**, not a path:
    ``LoadAudio`` lists its own input directory, and handing this model a
    path is the thing that did not work in the venv.
    """
    sampling = spec["sampling"]
    return {
        "text": job["text"],
        "seed": int(spec["seed"]),
        "reference": reference_name,
        "language": spec["language"] or "Auto",
        "temperature": float(sampling["temperature"]),
        "top_p": float(sampling["top_p"]),
        "top_k": int(sampling["top_k"]),
        "repetition_penalty": float(sampling["repetition_penalty"]),
        "max_new_tokens": int(sampling["max_new_tokens"]),
        "filename_prefix": prefix,
    }


def run(args) -> dict:
    """Validate, upload the reference, run the graph per line, record each."""
    # Imported here and not at the top of the module: `run_fake` must never
    # load the graph client, which is what keeps `just ci-fake` a control
    # for the whole move to the host.
    from forge_gen import comfy  # noqa: PLC0415

    spec = plan(args)
    backend = backends_mod.load_backend(spec["backend"])
    if spec["model"] != DEFAULT_MODEL:
        raise InputRejected(
            f"the host's MOSS-TTS node offers {MODEL_VARIANT} ({DEFAULT_MODEL}), not {spec['model']!r}",
            hint=f"unset ${MODEL_ENV}, or add a template that states another variant",
        )
    if not spec["reference"]:
        raise InputRejected(
            "this graph clones a voice and needs one: --voice <name> for a designed voice, "
            "or --voice <clip.wav> for one you own",
            hint="forge gen voice <name> --describe '…' designs one",
        )
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

    # One upload for the run: every line of a batch clones the same voice.
    reference = Path(spec["reference"])
    reference_name = comfy.upload_image(base, reference, f"forge_voice_{reference.parent.name}_{reference.name}")
    _say(f"reference {reference} uploaded as {reference_name}")

    rendered = []
    blocks = []
    for job in spec["jobs"]:
        out = Path(job["out"])
        inputs = template_inputs(spec, job, reference_name=reference_name, prefix=comfy.output_prefix(out.stem))
        patched = comfy.patch(graph, inputs, where)
        _say(f"seed {spec['seed']}, {spec['language'] or 'Auto'}: {job['text'][:60]}")
        prompt_id = comfy.submit(base, patched, comfy.client_id())
        entry = comfy.wait_for(base, prompt_id, timeout=float(getattr(args, "timeout", GENERATE_TIMEOUT_S)), poll=POLL_S, on_progress=_progress)
        out.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="forge-speech-") as scratch:
            saved = comfy.fetch(base, entry, Path(scratch))
            transcode_wav(ffmpeg, saved[0], out)
        rec = build_record(
            text=job["text"],
            out_path=out,
            model=spec["model"],
            reference=spec["reference"],
            language=spec["language"],
            voice_text=spec["voice_text"],
            voice_record=spec.get("voice_record"),
            seed=spec["seed"],
            sampling=spec["sampling"],
            created_by=spec["created_by"],
            workflow=WORKFLOW,
            **facts,
        )
        records.write(rec, job["record"])
        _say(f"OK {out}")
        rendered.append({"out": str(out), "record": job["record"]})
        blocks.append(
            {
                "template": f"backends/{spec['backend']}/workflows/{WORKFLOW}",
                "template_sha256": template_sha,
                "inputs": inputs,
                "comfyui_commit": facts["comfyui_commit"],
                "packs": facts["packs"],
                "prompt_id": prompt_id,
                "cached": comfy.was_cached(entry, points["filename_prefix"][0]),
            }
        )
    summary = _success(spec, rendered)
    summary["comfy"] = blocks[0]
    if len(blocks) > 1:
        summary["comfy_batch"] = blocks
    return summary


def run_fake(args) -> dict:
    """A placeholder tone where the line would be, and a record that says so — the reference still hashed."""
    spec = plan(args)
    placeholders.refuse_real(*(path for job in spec["jobs"] for path in (job["out"], job["record"])))
    rendered = []
    for job in spec["jobs"]:
        placeholders.placeholder_wav(job["out"])
        rec = build_record(
            text=job["text"],
            out_path=job["out"],
            model=spec["model"],
            reference=spec["reference"],
            language=spec["language"],
            voice_text=spec["voice_text"],
            voice_record=spec.get("voice_record"),
            seed=spec["seed"],
            sampling=spec["sampling"],
            created_by=spec["created_by"],
            fake=True,
        )
        records.write(rec, job["record"])
        rendered.append({"out": job["out"], "record": job["record"]})
    return _success(spec, rendered)
