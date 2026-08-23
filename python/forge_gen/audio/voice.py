"""``forge-gen voice``: design a character's voice from a description, with no reference clip.

    forge-gen voice crypt_warden \\
        --describe "Deep, slow, weathered male voice, English, low pitch, unhurried, grave and calm"

    forge-gen voice crypt_warden --describe "..." --seed 7 --line "State your business."

A project never has to bring a voice. MOSS-VoiceGenerator (1.7B, the
``MossTTSDelay`` architecture, Apache-2.0) speaks one audition line in a
timbre it designs from the description, and what it speaks becomes a
durable **source**: ``assets-src/voices/<name>/ref.wav`` beside
``voice.json``, a generator record of kind ``voice`` that names the
description, the line, the seed and every sampling knob. Every spoken line
of that character is then cloned from ``ref.wav`` by ``forge-gen speech
--voice <name>`` (MOSS-TTS) — one speech path, the voice designed once, so
two lines a month apart are the same person.

The seed is the voice. A description is a region of timbres, not a point in
one; two runs of the same words at different seeds are two people. Every
run is seeded — a fresh random one when none is given — and the seed is in
the record, so a voice is reproducible from description + seed for as long
as the model and its sampler hold still. Refusing to overwrite an existing
``ref.wav`` without ``--overwrite`` is not fussiness: replacing the source
changes every line cloned from it afterwards.

The default audition line is :data:`DEFAULT_LINE` — one neutral English
sentence of about eight seconds at a speaking pace, inside the 5–15 s band
the cloner wants — chosen to show pitch, pace and texture without asking for
an emotion the description did not name. ``--line`` replaces it; the record
carries whichever was spoken.

The outer half (``run``) validates, resolves the ``moss_tts`` backend (the
same checkout and venv MOSS-TTS runs in; exit 3 before any GPU work) and
execs the inner half under it; the inner half (``main_inner``) imports
torch. ``build_record`` is pure so the schema can be checked with nothing
resident.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import launcher, placeholders, records
from forge_gen.audio.speech import REFERENCE_GOOD_S, measure_wav
from forge_gen.exit_codes import BackendFailed, InputRejected, UsageError

#: The backend directory this command runs through: the MOSS-TTS env hosts both models.
BACKEND = "moss_tts"

#: The record's ``tool`` — the generator's name, as distinct from the backend's.
TOOL = "moss_voice_generator"

#: The weights, overridable for a fine-tune: ``MOSS_VOICE_MODEL=<dir or hub id>``.
MODEL_ENV = "MOSS_VOICE_MODEL"
DEFAULT_MODEL = "OpenMOSS-Team/MOSS-VoiceGenerator"

#: Where designed voices live, relative to the project root.
VOICES_DIR = "assets-src/voices"

#: The audition clip's name inside its voice directory, and the record's.
REFERENCE_FILE = "ref.wav"
RECORD_FILE = "voice.json"

#: The audition line when ``--line`` is not given: neutral, English, about
#: eight seconds at a speaking pace. Long enough to hear pitch, pace and
#: texture; short enough to sit inside the cloner's 5–15 s band.
DEFAULT_LINE = (
    "The river runs past the old mill at dawn, and the bells across the valley "
    "ring twice before the market opens."
)

#: The model card's recommended decoding hyperparameters. The card says the
#: model is sensitive to them; these are the defaults and every run records
#: what it used.
SAMPLING = {"audio_temperature": 1.5, "audio_top_p": 0.6, "audio_top_k": 50, "audio_repetition_penalty": 1.1}
MAX_NEW_TOKENS = 4096

#: A voice name keys a directory and a ``--voice`` argument: lower-case stem, nothing else.
NAME_RE = re.compile(r"^[a-z0-9_]+$")


def model_id() -> str:
    """The weights this run will load: ``$MOSS_VOICE_MODEL`` or the published id."""
    return os.environ.get(MODEL_ENV) or DEFAULT_MODEL


def voices_root(out_dir: str | os.PathLike | None = None) -> Path:
    """Where voices go: ``--out-dir`` as given, else ``<project>/assets-src/voices``, else the same under the cwd."""
    if out_dir:
        return Path(out_dir).expanduser().resolve()
    root = records.project()
    return (root / VOICES_DIR).resolve() if root else (Path.cwd() / VOICES_DIR).resolve()


def voice_paths(name: str, out_dir: str | os.PathLike | None = None) -> tuple[Path, Path]:
    """``(ref.wav, voice.json)`` for a voice of this name."""
    folder = voices_root(out_dir) / name
    return folder / REFERENCE_FILE, folder / RECORD_FILE


def check_name(name: str) -> str:
    """A voice name, or a refusal that says what one looks like."""
    text = (name or "").strip()
    if not NAME_RE.match(text):
        raise InputRejected(f"voice name {name!r} is not [a-z0-9_]+ — it names a directory under {VOICES_DIR}/ and a --voice argument")
    return text


# --------------------------------------------------------------- the record --


def build_record(
    *,
    name: str,
    instruction: str,
    text: str,
    out_path: str | os.PathLike,
    model: str,
    seed: int,
    sampling: dict | None = None,
    created_by: str | None = None,
    commit: str | None = None,
    python: str | None = None,
    torch: str | None = None,
    model_revision: str | None = None,
    fake: bool = False,
) -> dict:
    """The record for one designed voice: no inputs, every knob in ``params``.

    The description is ``params.instruction`` and the audition line
    ``params.text`` — both are knobs of the design, not files it was handed —
    and the seed is always a number, because every run is seeded. The output
    is the audition clip, hashed, and ``measured`` is what the clip is.
    """
    if fake:
        rec = placeholders.fake_record("voice", TOOL, backend=BACKEND, created_by=created_by, model=model)
    else:
        rec = records.new_record("voice", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=BACKEND, commit=commit, python=python, torch=torch, model=model, model_revision=model_revision
        )
    params = {
        "name": name,
        "model": model,
        "instruction": instruction,
        "text": text,
        "seed": int(seed),
    }
    params.update(sampling or dict(SAMPLING))
    rec["params"] = params
    records.add_output(rec, out_path)
    measured = measure_wav(out_path)
    rec["measured"] = {"duration_s": measured["duration_s"], "sample_rate": measured["sample_rate"], "channels": measured["channels"]}
    return rec


# ------------------------------------------------------------------- parsing --


def add_parser(subparsers) -> None:
    """Register ``voice``."""
    parser = subparsers.add_parser(
        "voice",
        help="Design a character's voice from a description (MOSS-VoiceGenerator)",
        description=__doc__,
    )
    parser.add_argument("name", metavar="NAME", help=f"the voice's name, [a-z0-9_]+; becomes {VOICES_DIR}/NAME/")
    parser.add_argument("--describe", required=True, metavar="TEXT", help="the voice: gender, age, pitch, pace, accent, texture, mood")
    parser.add_argument("--line", default=None, metavar="TEXT", help=f"the audition sentence (default: {DEFAULT_LINE!r})")
    parser.add_argument("--seed", type=int, default=None, metavar="N", help="the sampler seed (default: a fresh random one, recorded) — the seed is the voice")
    parser.add_argument("--temperature", type=float, default=None, metavar="X", help=f"audio_temperature (default {SAMPLING['audio_temperature']:g})")
    parser.add_argument("--top-p", type=float, default=None, metavar="X", help=f"audio_top_p (default {SAMPLING['audio_top_p']:g})")
    parser.add_argument("--top-k", type=int, default=None, metavar="N", help=f"audio_top_k (default {SAMPLING['audio_top_k']})")
    parser.add_argument("--rep-penalty", type=float, default=None, metavar="X", help=f"audio_repetition_penalty (default {SAMPLING['audio_repetition_penalty']:g})")
    parser.add_argument("--out-dir", default=None, metavar="DIR", help=f"where NAME/ goes (default: <project>/{VOICES_DIR})")
    parser.add_argument("--overwrite", action="store_true", help="replace an existing NAME/ref.wav — every line cloned from it afterwards changes")
    parser.add_argument("--model", default=None, metavar="ID", help=f"weights (default ${MODEL_ENV} or {DEFAULT_MODEL})")


def plan(args) -> dict:
    """Turn the arguments into the spec the inner half designs from: validated, absolute.

    ``{"name", "model", "instruction", "text", "seed", "sampling",
    "created_by", "project", "out", "record"}``. The seed is drawn here when
    none was given: a seed nobody chose is still the one fact that brings
    the same voice back, and it goes in the record either way.
    """
    name = check_name(args.name)
    instruction = (args.describe or "").strip()
    if not instruction:
        raise InputRejected("--describe is empty — say who speaks: gender, age, pitch, pace, accent, texture, mood")
    text = (args.line if args.line is not None else DEFAULT_LINE).strip()
    if not text:
        raise InputRejected("--line is empty — the audition needs a sentence to speak")
    seed = args.seed if args.seed is not None else random.randrange(2**31)
    if seed < 0:
        raise InputRejected(f"--seed must be >= 0, got {seed}")
    sampling = dict(SAMPLING)
    if args.temperature is not None:
        if args.temperature <= 0:
            raise InputRejected(f"--temperature must be > 0, got {args.temperature:g}")
        sampling["audio_temperature"] = float(args.temperature)
    if args.top_p is not None:
        if not 0 < args.top_p <= 1:
            raise InputRejected(f"--top-p must be in (0, 1], got {args.top_p:g}")
        sampling["audio_top_p"] = float(args.top_p)
    if args.top_k is not None:
        if args.top_k < 1:
            raise InputRejected(f"--top-k must be >= 1, got {args.top_k}")
        sampling["audio_top_k"] = int(args.top_k)
    if args.rep_penalty is not None:
        if args.rep_penalty <= 0:
            raise InputRejected(f"--rep-penalty must be > 0, got {args.rep_penalty:g}")
        sampling["audio_repetition_penalty"] = float(args.rep_penalty)
    out, record = voice_paths(name, args.out_dir)
    if out.exists() and not args.overwrite:
        raise InputRejected(
            f"{out} exists — a designed voice is a source, and replacing it changes every line cloned from it "
            f"afterwards; pass --overwrite to replace it, or design under another name",
            voice=name,
        )
    project = records.project()
    return {
        "name": name,
        "model": args.model or model_id(),
        "instruction": instruction,
        "text": text,
        "seed": int(seed),
        "sampling": sampling,
        "created_by": getattr(args, "created_by", None),
        "project": str(project) if project else None,
        "out": str(out),
        "record": str(record),
    }


def _success(spec: dict, rendered: dict) -> dict:
    return {
        "ok": True,
        "record": rendered["record"],
        "outputs": [rendered["out"]],
        "voice": spec["name"],
        "seed": spec["seed"],
        "model": spec["model"],
        "_text": f"[voice] OK {rendered['out']} (seed {spec['seed']}) -> {rendered['record']}",
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
    """Validate, resolve the backend (exit 3 before any GPU work), design under its venv."""
    spec = plan(args)
    backend = backends_mod.load_backend(BACKEND)
    launcher.resolve_interpreter(backend)
    spec["commit"] = checkout_commit(backend)
    with tempfile.TemporaryDirectory(prefix="forge-voice-") as scratch:
        spec_path = Path(scratch) / "spec.json"
        spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
        result = launcher.run_inner_checked(backend, "audio.voice", ["--spec", str(spec_path)])
    rendered = result.get("voice")
    if not rendered:
        raise BackendFailed("the inner half returned no designed voice")
    return _success(spec, rendered)


def run_fake(args) -> dict:
    """Silence where the audition would be, and a record that says so.

    The placeholder is as long as a reference should be — the cloner refuses
    a clip under 3 s, and a fake voice has to pass the same gate the real one
    passes so ``speech --fake --voice <name>`` runs through it.
    """
    spec = plan(args)
    placeholders.silence_wav(spec["out"], seconds=float(sum(REFERENCE_GOOD_S) / 2))
    rec = build_record(
        name=spec["name"],
        instruction=spec["instruction"],
        text=spec["text"],
        out_path=spec["out"],
        model=spec["model"],
        seed=spec["seed"],
        sampling=spec["sampling"],
        created_by=spec["created_by"],
        fake=True,
    )
    records.write(rec, spec["record"])
    return _success(spec, {"out": spec["out"], "record": spec["record"]})


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
    """Design the voice in the spec under the backend's venv; torch is imported here and nowhere above.

    Prints progress lines and, last, one JSON object ``{"ok": true, "voice":
    {"out", "record"}}``. Anything the model raises is a backend failure (5)
    with the traceback on stderr; the launcher carries the tail out.
    """
    parser = argparse.ArgumentParser(prog="forge_gen.audio.voice --inner", add_help=True)
    parser.add_argument("--spec", required=True)
    args = parser.parse_args(argv)
    spec = json.loads(Path(args.spec).read_text(encoding="utf-8"))
    if spec.get("project"):
        records.set_project(spec["project"])

    import soundfile
    import torch
    from transformers import AutoModel, AutoProcessor

    torch.backends.cuda.enable_cudnn_sdp(False)  # broken kernel, per model card
    torch.backends.cuda.enable_flash_sdp(True)
    torch.backends.cuda.enable_mem_efficient_sdp(True)
    torch.backends.cuda.enable_math_sdp(True)

    model_name = spec["model"]
    device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    attn = _attn_implementation(torch, device, dtype)
    print(f"[voice] loading {model_name} on {device} ({attn})", flush=True)
    # normalize_inputs: the processor tidies punctuation in the text and the
    # instruction the way the model was trained to see them.
    processor = AutoProcessor.from_pretrained(model_name, trust_remote_code=True, normalize_inputs=True)
    processor.audio_tokenizer = processor.audio_tokenizer.to(device)
    model = AutoModel.from_pretrained(model_name, trust_remote_code=True, attn_implementation=attn, torch_dtype=dtype).to(device)
    model.eval()
    revision = _snapshot_revision(model_name)
    sample_rate = int(processor.model_config.sampling_rate)

    seed = int(spec["seed"])
    sampling = spec.get("sampling") or dict(SAMPLING)
    torch.manual_seed(seed)
    if device == "cuda":
        torch.cuda.manual_seed_all(seed)
    conversation = [processor.build_user_message(text=spec["text"], instruction=spec["instruction"])]
    batch = processor([conversation], mode="generation")
    print(f"[voice] {spec['name']} seed {seed}: {spec['instruction']}", flush=True)
    print(f"[voice] line: {spec['text']}", flush=True)
    with torch.no_grad():
        outputs = model.generate(
            input_ids=batch["input_ids"].to(device),
            attention_mask=batch["attention_mask"].to(device),
            max_new_tokens=MAX_NEW_TOKENS,
            audio_temperature=float(sampling["audio_temperature"]),
            audio_top_p=float(sampling["audio_top_p"]),
            audio_top_k=int(sampling["audio_top_k"]),
            audio_repetition_penalty=float(sampling["audio_repetition_penalty"]),
        )
    messages = [message for message in processor.decode(outputs) if message is not None]
    if not messages or not messages[0].audio_codes_list:
        raise BackendFailed(f"the model produced no audio for {spec['text']!r}")
    audio = messages[0].audio_codes_list[0]
    out_path = Path(spec["out"])
    out_path.parent.mkdir(parents=True, exist_ok=True)
    # soundfile, as speech.py: torchaudio.save goes through torchcodec here.
    # PCM_16 so the stdlib wave module — and the cloner's length gate —
    # can read it.
    wav = audio.detach().float().cpu().numpy()
    soundfile.write(os.fspath(out_path), wav.T if wav.ndim > 1 else wav, sample_rate, subtype="PCM_16")
    rec = build_record(
        name=spec["name"],
        instruction=spec["instruction"],
        text=spec["text"],
        out_path=out_path,
        model=model_name,
        seed=seed,
        sampling=sampling,
        created_by=spec.get("created_by"),
        commit=spec.get("commit"),
        python=platform.python_version(),
        torch=torch.__version__,
        model_revision=revision,
    )
    records.write(rec, spec["record"])
    seconds = rec["measured"]["duration_s"]
    low, high = REFERENCE_GOOD_S
    if seconds is not None and not low <= seconds <= high:
        sys.stderr.write(
            f"forge-gen: voice: the audition is {seconds:.1f} s; {low:g}–{high:g} s clones best — "
            f"another --seed, or a longer/shorter --line\n"
        )
    print(f"[voice] OK {out_path} ({seconds} s at {sample_rate} Hz)", flush=True)
    sys.stdout.write(json.dumps({"ok": True, "voice": {"out": str(out_path), "record": spec["record"]}}) + "\n")
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
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen voice ...\n")
    sys.exit(2)
