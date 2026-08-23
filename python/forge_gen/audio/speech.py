"""``forge-gen speech``: one spoken line from text, a voice cloned from a reference clip.

    forge-gen speech --text "Hold the line. They breach on my mark." \\
        --voice assets-src/voices/kessa/ref.wav --out out/audio/kessa_hold_the_line.wav

    forge-gen speech --text "Testing a fresh voice" --out out/audio/test.wav   # no cloning

A voice is a reference clip — 5–15 s of clean speech defines a character's
voice permanently. Same reference in, same voice out; the record carries
the clip's hash so "same" is checkable. Loading the 4B model is most of a
call; ``--lines-file`` (one ``stem|text`` per line, into ``--out-dir``)
renders a session on one load.

The command is backend-agnostic on its face — ``--backend`` names who
speaks — and ``moss_tts`` is the one backend v1 ships. OmniVoice is a
documented v1.1 add; naming it today exits 2 and says so.

Each WAV gets a ``forge_record`` beside it (``<stem>.json``, or ``--record``):
the spoken line is the prompt, because that is what a reader searches for,
and the reference is an input with its sha256, because that is what makes
the same character come back. ``seed`` is null and stays null: MOSS-TTS's
API takes none. ``--seed`` seeds torch's RNG, which its sampler does draw
from, and is recorded only when given — writing a zero there otherwise would
be the same fabrication the animation sidecars used to carry.

``build_record`` is pure — the record from its numbers — so the schema can
be checked without the model resident.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import launcher, placeholders, records
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError

#: The backends that can speak. ``moss_tts`` is the one that exists.
BACKENDS = ("moss_tts",)
DEFAULT_BACKEND = "moss_tts"

#: Names a user may reach for that are not here yet, with the answer.
PLANNED = {"omnivoice": "OmniVoice is a v1.1 add; only moss_tts speaks in this build"}

#: The record's ``tool`` — the sidecar's generator name, which for this backend is also the backend's name.
TOOL = "moss_tts"

#: Local-4B fits comfortably on the 24 GB card (the 8B Delay model + audio
#: tokenizer OOMs); override with ``MOSS_TTS_MODEL=OpenMOSS-Team/MOSS-TTS-v1.5``
#: if the llama.cpp low-VRAM path ever gets set up.
MODEL_ENV = "MOSS_TTS_MODEL"
DEFAULT_MODEL = "OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5"

#: Reference clip containers the processor reads (it decodes through its own audio loader).
REFERENCE_SUFFIXES = (".wav", ".mp3", ".flac", ".m4a")

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
    "sw": "Swahili", "sv": "Swedish", "tl": "Tagalog", "th": "Thai", "tr": "Turkish",
    "vi": "Vietnamese",
}  # fmt: skip

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
    seed: int | None = None,
    sampling: dict | None = None,
    created_by: str | None = None,
    commit: str | None = None,
    python: str | None = None,
    torch: str | None = None,
    model_revision: str | None = None,
    fake: bool = False,
) -> dict:
    """The record for one spoken line.

    Split out of the inner half so the schema can be checked without the
    model resident. The line itself is the ``prompt`` input; the reference
    clip is the ``reference`` input, hashed, and ``params.reference`` repeats
    its path because the Rust projection (``speech_params``) reads it from
    either place. ``seed`` is ``None`` unless one was given to torch's RNG.
    ``voice_text`` is carried as given and unused by this backend: MOSS-TTS
    takes no transcript of the reference; OmniVoice will.
    """
    if fake:
        rec = placeholders.fake_record("speech", TOOL, backend=TOOL, created_by=created_by, model=model)
    else:
        rec = records.new_record("speech", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=TOOL, commit=commit, python=python, torch=torch, model=model, model_revision=model_revision
        )
        rec["note"] = "MOSS-TTS takes no seed; the hash is what identifies this render" if seed is None else None
    records.add_input(rec, "prompt", prompt=text)
    reference_entry = records.add_input(rec, "reference", reference) if reference is not None else None
    params = {
        "model": model,
        "seed": None if seed is None else int(seed),
        "voice": voice_name(reference),
        "reference": reference_entry["path"] if reference_entry else None,
        "language": language,
        "voice_text": voice_text,
    }
    params.update(sampling or {})
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
    parser.add_argument("--voice", metavar="REF", help="reference clip, 5–15 s of clean speech (.wav/.mp3/.flac/.m4a); omit for an uncloned voice")
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

    ``{"backend", "model", "reference", "language", "voice_text", "seed",
    "sampling", "created_by", "project", "jobs": [{"text", "out", "record"}]}``.
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
    reference = check_reference(args.voice) if args.voice else None
    if args.voice_text and reference is None:
        raise UsageError("--voice-text describes a --voice clip; there is none")
    if args.voice_text and backend == "moss_tts":
        sys.stderr.write("forge-gen: speech: moss_tts takes no transcript of the reference; --voice-text is recorded, not used\n")
    seed = args.seed
    if seed is not None and seed < 0:
        raise InputRejected(f"--seed must be >= 0, got {seed}")
    project = records.project()
    return {
        "backend": backend,
        "model": args.model or model_id(),
        "reference": str(reference) if reference else None,
        "language": language_name(args.language),
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


# --------------------------------------------------------------------- outer --


def checkout_commit(backend: backends_mod.Backend) -> str | None:
    """HEAD of the upstream checkout the inner half runs from, or ``None`` when git will not say."""
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
    """Validate, resolve the backend (exit 3 before any GPU work), speak under its venv."""
    spec = plan(args)
    backend = backends_mod.load_backend(spec["backend"])
    launcher.resolve_interpreter(backend)
    spec["commit"] = checkout_commit(backend)
    with tempfile.TemporaryDirectory(prefix="forge-speech-") as scratch:
        spec_path = Path(scratch) / "spec.json"
        spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
        result = launcher.run_inner_checked(backend, "audio.speech", ["--spec", str(spec_path)])
    rendered = result.get("jobs") or []
    if not rendered:
        raise BackendFailed("the inner half returned no spoken lines")
    return _success(spec, rendered)


def run_fake(args) -> dict:
    """Silence where the line would be, and a record that says so — the reference still hashed."""
    spec = plan(args)
    rendered = []
    for job in spec["jobs"]:
        placeholders.silence_wav(job["out"])
        rec = build_record(
            text=job["text"],
            out_path=job["out"],
            model=spec["model"],
            reference=spec["reference"],
            language=spec["language"],
            voice_text=spec["voice_text"],
            seed=spec["seed"],
            sampling=spec["sampling"],
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


def _attn_implementation(torch, device: str, dtype) -> str:
    """flash-attn when it is installed and the card is Ampere or newer; SDPA on CUDA otherwise; eager on CPU."""
    import importlib.util

    if device == "cuda" and importlib.util.find_spec("flash_attn") is not None and dtype in (torch.float16, torch.bfloat16):
        major, _ = torch.cuda.get_device_capability()
        if major >= 8:
            return "flash_attention_2"
    return "sdpa" if device == "cuda" else "eager"


def main_inner(argv: list[str]) -> int:
    """Speak every line in the spec under the backend's venv; torch is imported here and nowhere above.

    Prints progress lines and, last, one JSON object ``{"ok": true, "jobs":
    [{"out", "record"}]}``. Anything the model raises is a backend failure
    (5) with the traceback on stderr; the launcher carries the tail out.
    """
    parser = argparse.ArgumentParser(prog="forge_gen.audio.speech --inner", add_help=True)
    parser.add_argument("--spec", required=True)
    args = parser.parse_args(argv)
    spec = json.loads(Path(args.spec).read_text(encoding="utf-8"))
    if spec.get("project"):
        records.set_project(spec["project"])

    import soundfile
    import torch
    from transformers import AutoModel, AutoProcessor

    torch.backends.cuda.enable_cudnn_sdp(False)  # broken kernel, per model card
    # Kept enabled as fallbacks, as the card does.
    torch.backends.cuda.enable_flash_sdp(True)
    torch.backends.cuda.enable_mem_efficient_sdp(True)
    torch.backends.cuda.enable_math_sdp(True)

    model_name = spec["model"]
    device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    attn = _attn_implementation(torch, device, dtype)
    print(f"[tts] loading {model_name} on {device} ({attn})", flush=True)
    processor = AutoProcessor.from_pretrained(model_name, trust_remote_code=True)
    processor.audio_tokenizer = processor.audio_tokenizer.to(device)
    model = AutoModel.from_pretrained(model_name, trust_remote_code=True, attn_implementation=attn, torch_dtype=dtype).to(device)
    model.eval()
    revision = _snapshot_revision(model_name)

    reference = spec.get("reference")
    language = spec.get("language")
    seed = spec.get("seed")
    sampling = spec.get("sampling") or dict(SAMPLING)
    sample_rate = int(processor.model_config.sampling_rate)

    rendered = []
    for job in spec["jobs"]:
        if seed is not None:
            # Re-seeded per line so re-rendering one line of a batch gives
            # back what it gave inside the batch.
            torch.manual_seed(int(seed))
            if device == "cuda":
                torch.cuda.manual_seed_all(int(seed))
        kwargs = {"text": job["text"]}
        if reference:
            kwargs["reference"] = [reference]
        if language:
            kwargs["language"] = language
        conversation = [processor.build_user_message(**kwargs)]
        batch = processor([conversation], mode="generation")
        print(f"[tts] {language or 'inferred language'}{', cloning ' + voice_name(reference) if reference else ''}: {job['text']}", flush=True)
        with torch.no_grad():
            outputs = model.generate(
                input_ids=batch["input_ids"].to(device),
                attention_mask=batch["attention_mask"].to(device),
                max_new_tokens=int(sampling["max_new_tokens"]),
                do_sample=True,
                audio_temperature=float(sampling["temperature"]),
                audio_top_p=float(sampling["top_p"]),
                audio_top_k=int(sampling["top_k"]),
                audio_repetition_penalty=float(sampling["repetition_penalty"]),
            )
        messages = [message for message in processor.decode(outputs) if message is not None]
        if not messages or not messages[0].audio_codes_list:
            raise BackendFailed(f"the model produced no audio for {job['text']!r}")
        audio = messages[0].audio_codes_list[0]
        out_path = Path(job["out"])
        out_path.parent.mkdir(parents=True, exist_ok=True)
        # soundfile instead of torchaudio.save: torchaudio 2.9 delegates to
        # torchcodec, whose ffmpeg libs collide with system glib on this box.
        # The v1.5 codec returns [channels, samples]; soundfile wants
        # [samples, channels]. PCM_16 so the stdlib wave module measures it.
        wav = audio.detach().float().cpu().numpy()
        soundfile.write(os.fspath(out_path), wav.T if wav.ndim > 1 else wav, sample_rate, subtype="PCM_16")
        rec = build_record(
            text=job["text"],
            out_path=out_path,
            model=model_name,
            reference=reference,
            language=language,
            voice_text=spec.get("voice_text"),
            seed=seed,
            sampling=sampling,
            created_by=spec.get("created_by"),
            commit=spec.get("commit"),
            python=platform.python_version(),
            torch=torch.__version__,
            model_revision=revision,
        )
        records.write(rec, job["record"])
        print(f"[tts] OK {out_path}", flush=True)
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
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen speech ...\n")
    sys.exit(2)
