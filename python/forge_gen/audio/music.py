"""``forge-gen music``: one track from a prompt, as a graph on the ComfyUI host.

    forge-gen music --out boss_ambush.ogg --record boss_ambush.music.json \\
        --prompt "dark ambient boss theme, low strings, taiko" --duration 90
    forge-gen music --out hub_theme.ogg --record hub_theme.music.json \\
        --prompt "hopeful synthwave exploration" --lyrics-file verse.txt --duration 120

ACE-Step 1.5 is native to the pinned ComfyUI, so there is no environment
here, no clone, and — the part that changes how this reads — **no resident
server of its own to start, watch and stop**. What replaces all of it is
``backends/acestep/workflows/music.api.json``, a tracked graph whose
patchable knobs are marked inside it: load, patch, ``POST /prompt``, poll
``/history``, fetch the audio. The card is released by whoever holds the
lease (``forge serve``, or ``forge gen`` itself when no daemon is up), which
is why ``--stop-server`` is gone; a pid file that eventually signals a
stranger went with it.

``SaveAudio`` writes FLAC — ComfyUI v0.34.2 has no WAV save node — so the
track is transcoded here: losslessly to PCM for ``--format wav``, to vorbis
for ``--format ogg``. **ffmpeg is now required for both.**

The record (``forge_record: 2``, kind ``music``) comes out the same shape it
always had — same knobs under ``params``, same ``measured`` read off the
file on disk — plus what a comfy run can say and an env one cannot: the
host's commit, the hash of the *tracked template file*, and the packs it
carried (none: these nodes are ComfyUI's own). The content hash is the rest
of the claim: the model is not bit-reproducible, so "this is the file that
was auditioned" is the strongest thing a record can say about the bytes.

Stdlib only.
"""

from __future__ import annotations

import argparse
import os
import sys
import tempfile
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import placeholders, records
from forge_gen.audio import check_pcm, ffmpeg_bin, transcode_ogg, transcode_wav
from forge_gen.exit_codes import InputRejected, UsageError

#: The backend directory this command runs through.
BACKEND = "acestep"

#: The tool name the sidecar's generator block uses (``forge_library::schema::Generator::AceStep``).
TOOL = "ace_step"

#: The record kind.
KIND = "music"

#: Output containers the command writes. The graph saves FLAC (ComfyUI
#: v0.34.2 has no WAV save node), and both of these are an ffmpeg transcode
#: of it — lossless for wav, vorbis for ogg.
FORMATS = ("ogg", "wav")

#: The tracked graph this command runs, under ``backends/acestep/workflows/``.
WORKFLOW = "music.api.json"

#: What ACE-Step accepts for a duration, seconds. The node's own range is
#: 1–2000; this is the range a game track is worth spending the card on.
DURATION_MIN, DURATION_MAX = 10.0, 600.0

#: Seconds between ``/history`` polls.
POLL_S = 2.0

#: How long one track may take end to end, first load included.
GENERATE_TIMEOUT_S = 3600.0

#: What ``TextEncodeAceStepAudio1.5`` accepts for ``keyscale``, captured
#: from the running host's ``GET /object_info`` on 2026-08-30 — never
#: written from memory, the same rule ``[comfy] nodes`` is held to. It is
#: here so a key the node would refuse is a refusal *before* the card, with
#: the list in the message.
KEYSCALES = (
    "C major", "C# major", "Db major", "D major", "D# major", "Eb major", "E major",
    "F major", "F# major", "Gb major", "G major", "G# major", "Ab major", "A major",
    "A# major", "Bb major", "B major",
    "C minor", "C# minor", "Db minor", "D minor", "D# minor", "Eb minor", "E minor",
    "F minor", "F# minor", "Gb minor", "G minor", "G# minor", "Ab minor", "A minor",
    "A# minor", "Bb minor", "B minor",
)

#: The same, for ``timesignature``.
TIMESIGNATURES = ("2", "3", "4", "6")

#: What the node is given when the run states no key or tempo. These are
#: the node's own defaults, and they are *recorded*, because a record says
#: what the sampler was given — not what the caller happened to type.
DEFAULT_BPM = 120
DEFAULT_KEYSCALE = "C major"
DEFAULT_TIMESIGNATURE = "4"

#: What the server is told when the track has no words.
INSTRUMENTAL = "[instrumental]"


# ---------------------------------------------------------------- parser --


def add_parser(subparsers) -> None:
    """Register ``music``."""
    parser = subparsers.add_parser(
        "music",
        help="One track from a prompt (ACE-Step 1.5, as a graph on the ComfyUI host)",
        description=__doc__,
    )
    parser.add_argument("--out", metavar="FILE", help="where the track goes: .ogg or .wav")
    parser.add_argument("--record", metavar="JSON", help="where the forge_record goes")
    parser.add_argument("--prompt", metavar="TEXT", help="music description (genre, mood, instrumentation)")
    parser.add_argument("--lyrics-file", metavar="FILE", help="lyrics from a file; omit for instrumental")
    parser.add_argument("--duration", type=float, default=30.0, metavar="S", help="seconds, 10-600 (default 30)")
    parser.add_argument("--seed", type=int, default=None, metavar="N", help="a fixed seed; omit for random")
    parser.add_argument("--bpm", type=int, default=None, metavar="N", help=f"tempo (default {DEFAULT_BPM}, the node's own)")
    parser.add_argument("--keyscale", default=None, metavar="KEY", help='e.g. "C major", "E minor" (default "C major")')
    parser.add_argument("--timesignature", default=None, metavar="N", choices=[None, *TIMESIGNATURES], help='beats per bar: 2, 3, 4 or 6 (default "4")')
    # The 5 Hz LM planner the old flag named is, in 1.5, the text model's
    # own audio-code plan — the node's default and what every ComfyUI
    # template ships. So the flag keeps its name and its meaning and gains
    # the way to turn it off, rather than defaulting to a register nobody
    # runs this model in.
    parser.add_argument("--thinking", action="store_true", default=True, help="let the text model plan the audio codes: structure (default)")
    parser.add_argument("--no-thinking", dest="thinking", action="store_false", help="sample straight from the tags and lyrics, with no plan")
    parser.add_argument("--format", choices=FORMATS, default=None, help="container (default: from --out's suffix)")
    # Kept only to be refused by name. Removing it outright would leave
    # argparse saying "unrecognized arguments: --stop-server", which names
    # nothing a caller can do next; this names the door that replaced it.
    parser.add_argument("--stop-server", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--timeout", type=float, default=GENERATE_TIMEOUT_S, metavar="S", help="seconds one track may take (default 3600)")


# ----------------------------------------------------------- validation --


def resolve_format(out: str | os.PathLike | None, requested: str | None) -> str:
    """The container: ``--format`` when given, else the suffix of ``--out``; the two must agree."""
    suffix = Path(out).suffix.lower().lstrip(".") if out else ""
    if requested and suffix and requested != suffix:
        raise UsageError(f"--format {requested} does not match --out's .{suffix}")
    fmt = requested or suffix
    if fmt not in FORMATS:
        raise UsageError(f"--out must end in .ogg or .wav (or say --format); got {out!r}")
    return fmt


def read_lyrics(path: str | os.PathLike | None) -> str:
    """The lyrics text, or ``[instrumental]`` when there is no file."""
    if not path:
        return INSTRUMENTAL
    target = Path(path)
    if not target.is_file():
        raise InputRejected(f"lyrics file not found: {target}")
    text = target.read_text(encoding="utf-8")
    return text if text.strip() else INSTRUMENTAL


def check_inputs(args) -> dict:
    """Everything the request needs, refused before the host is touched.

    The knobs the node takes from a fixed list (key, time signature) are
    checked here against what the running host said they were, so a typo is
    a refusal naming the list rather than a ``POST /prompt`` that fails in
    front of a stranger with the card already leased.
    """
    if not args.out or not args.record:
        raise UsageError("--out and --record are required")
    prompt = (args.prompt or "").strip()
    if not prompt:
        raise InputRejected("the prompt is empty")
    duration = float(args.duration)
    if not DURATION_MIN <= duration <= DURATION_MAX:
        raise InputRejected(f"--duration {duration:g} is outside {DURATION_MIN:g}-{DURATION_MAX:g} s")
    if args.bpm is not None and args.bpm <= 0:
        raise InputRejected(f"--bpm {args.bpm} is not a tempo")
    if args.seed is not None and args.seed < 0:
        raise InputRejected(f"--seed {args.seed} is negative; ACE-Step takes a non-negative seed")
    keyscale = (args.keyscale or "").strip() or None
    if keyscale is not None and keyscale not in KEYSCALES:
        raise InputRejected(
            f"--keyscale {keyscale!r} is not one the model takes; it takes "
            f"{', '.join(KEYSCALES)}"
        )
    timesignature = (getattr(args, "timesignature", None) or "").strip() or None
    if timesignature is not None and timesignature not in TIMESIGNATURES:
        raise InputRejected(f"--timesignature {timesignature!r} is not one of {', '.join(TIMESIGNATURES)}")
    fmt = resolve_format(args.out, args.format)
    return {
        "prompt": prompt,
        "lyrics": read_lyrics(args.lyrics_file),
        "lyrics_file": str(Path(args.lyrics_file).resolve()) if args.lyrics_file else None,
        "duration_s": duration,
        "seed": args.seed,
        "bpm": args.bpm,
        "keyscale": keyscale,
        "timesignature": timesignature,
        "thinking": bool(args.thinking),
        "format": fmt,
        "out": Path(args.out).resolve(),
        "record": Path(args.record).resolve(),
    }


# ------------------------------------------------------------- the record --


def template_inputs(request: dict, *, seed: int, prefix: str) -> dict:
    """Every knob ``music.api.json`` marks, filled from a checked request.

    Every one of them, always: the template refuses a marker nobody fills,
    which is the same rule as "a recipe states every knob". Where the run
    said nothing the node's own default goes in — and it is what the record
    then reports, because a record says what the sampler was given.

    The seed goes in twice on purpose. The text model plans the piece from
    one seed and the sampler denoises from another, and in the graph they
    are two nodes; one ``--seed`` drives both, so a re-run of the same
    record gets the same plan *and* the same noise.
    """
    return {
        "tags": request["prompt"],
        "lyrics": request["lyrics"],
        "plan_seed": seed,
        "seed": seed,
        "bpm": int(request.get("bpm") or DEFAULT_BPM),
        "duration": float(request["duration_s"]),
        "seconds": float(request["duration_s"]),
        "keyscale": request.get("keyscale") or DEFAULT_KEYSCALE,
        "timesignature": request.get("timesignature") or DEFAULT_TIMESIGNATURE,
        "generate_audio_codes": bool(request["thinking"]),
        "filename_prefix": prefix,
    }


def _text_or_none(value) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _int_or_none(value) -> int | None:
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, int):
        return value
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return None


def build_record(
    request: dict,
    result: dict,
    *,
    measured: dict | None = None,
    backend: dict | None = None,
    created_by: str | None = None,
    fake: bool = False,
) -> dict:
    """The record for a finished track, from the request and the server's result.

    Split out of :func:`run` so the schema can be checked without a GPU: a
    captured server response goes in, a record the Rust reader accepts comes
    out. It adds the prompt (and the lyrics text) as inputs; the caller adds
    the lyrics *file* and the output, because those are hashed from disk.

    Everything here is what ACE-Step actually used — both stage seeds, the
    resolved key and tempo, both checkpoints — so this is ``recorded``, not
    reconstructed. ``None`` is written where the server did not say.
    """
    metas = result.get("metas") or {}
    rec = records.new_record(KIND, TOOL, created_by=created_by)
    rec["backend"] = records.backend_block(**(backend or {"name": BACKEND}))
    if rec["backend"].get("model") is None:
        rec["backend"]["model"] = _text_or_none(result.get("dit_model"))
    records.add_input(rec, "prompt", prompt=result.get("prompt") or request["prompt"])
    lyrics = _text_or_none(result.get("lyrics")) or _text_or_none(metas.get("lyrics")) or request.get("lyrics")
    rec["params"] = {
        "lm_model": _text_or_none(result.get("lm_model")),
        "dit_model": _text_or_none(result.get("dit_model")),
        # Two comma-separated integers as the server reports them, kept
        # verbatim because feeding it back is the only thing it is for.
        "seed": _text_or_none(result.get("seed_value")),
        "bpm": _int_or_none(metas.get("bpm")),
        "keyscale": _text_or_none(metas.get("keyscale")),
        "timesignature": _text_or_none(metas.get("timesignature")),
        "genres": _text_or_none(metas.get("genres")),
        "lyrics": lyrics,
        "thinking": bool(request.get("thinking")),
        "format": request["format"],
        # What was asked for; what came out is under measured.
        "duration_s": float(request["duration_s"]),
    }
    rec["measured"] = dict(measured or {})
    rec["fake"] = bool(fake)
    return rec


def measure_wav(path: str | os.PathLike) -> dict:
    """What the ``wave`` module can say about a PCM file: duration, rate, channels."""
    with wave.open(os.fspath(path), "rb") as handle:
        rate = handle.getframerate()
        frames = handle.getnframes()
        return {
            "duration_s": round(frames / rate, 3) if rate else None,
            "sample_rate": rate,
            "channels": handle.getnchannels(),
        }


# ------------------------------------------------------------- the graph --


def _say(text: str) -> None:
    """One progress line on stderr, so the JSON last line stays the last line."""
    sys.stderr.write(f"[music] {text}\n")
    sys.stderr.flush()


def _progress(seconds: float, entry) -> None:
    """What ``wait_for`` prints while the host works — a first load is minutes."""
    if int(seconds) % 30 == 0 and seconds >= 30:
        _say(f"{seconds:.0f}s on the host")


def backend_facts(
    backend: backends_mod.Backend,
    host: backends_mod.Backend,
    *,
    workflow_sha256: str | None,
    model: str | None,
) -> dict:
    """The ``backend`` block's inputs for a run on the host.

    ``commit``, ``python`` and ``torch`` are ``null`` and stay null: this
    backend has no checkout, no interpreter and no environment of its own,
    and a record that answered those questions with the *host's* answers
    would be saying the generator is something it is not. What ran is named
    by ``executor``, ``comfyui_commit`` and the hash of the tracked template
    — and by ``packs``, which is empty here because every node in the graph
    is ComfyUI's own.
    """
    from forge_gen import comfy  # noqa: PLC0415 - only a real run asks the host anything

    return {
        "name": backend.name,
        "commit": None,
        "python": None,
        "torch": None,
        "model": model,
        "model_revision": None,
        "executor": "comfy",
        "comfyui_commit": comfy.host_commit(host),
        "workflow_sha256": workflow_sha256,
        "packs": comfy.packs_block(backend),
    }


def finish(rec: dict, request: dict, out: Path, record_path: Path) -> dict:
    """Hash the file inputs and the output into the record, write it, and shape the result."""
    if request.get("lyrics_file"):
        records.add_input(rec, "lyrics", request["lyrics_file"])
    records.add_output(rec, out)
    records.write(rec, record_path)
    return {
        "ok": True,
        "record": str(record_path),
        "outputs": [str(out)],
        "seed": rec["params"].get("seed"),
        "duration_s": rec["measured"].get("duration_s"),
        "format": request["format"],
    }


# ------------------------------------------------------------------ run --


def refuse_stop_server(args) -> None:
    """``--stop-server`` names a server that no longer exists; say what replaced it.

    Exit 2 with the next command to type. Deleting the flag outright would
    have left argparse saying "unrecognized arguments: --stop-server", which
    tells a caller nothing about where the card went.
    """
    if getattr(args, "stop_server", False):
        raise UsageError(
            "--stop-server is gone with the resident ACE-Step server: this track is a graph on the "
            "ComfyUI host, and the card is released by whoever holds the lease",
            hint="forge gpu --free releases the host's models; systemctl --user stop forge-comfy stops the host",
        )


def run(args) -> dict:
    """Load the tracked graph, patch it, run it on the host, transcode, record."""
    # Imported here and not at the top of the module: `run_fake` must never
    # load the graph client, which is what keeps `just ci-fake` a control
    # for the whole move to the host.
    from forge_gen import comfy  # noqa: PLC0415

    refuse_stop_server(args)
    backend = backends_mod.load_backend(BACKEND)
    request = check_inputs(args)
    # Every failure that can happen before the card is leased happens here:
    # a missing ffmpeg (6), a template that cannot be built (3), a knob the
    # graph does not carry (3).
    ffmpeg = ffmpeg_bin()
    host = comfy.host_backend(backend)
    base = comfy.base_url(backend, records.project())
    graph, template_sha = comfy.load_template(backend, WORKFLOW)
    seed = request["seed"] if request["seed"] is not None else _fresh_seed()
    prefix = comfy.output_prefix(request["out"].stem)
    inputs = template_inputs(request, seed=seed, prefix=prefix)
    where = str(backend.workflow(WORKFLOW))
    graph = comfy.patch(graph, inputs, where)
    save_node = comfy.patch_points(graph, where)["filename_prefix"][0]

    _say(f"{request['duration_s']:g} s at {inputs['bpm']} bpm in {inputs['keyscale']}, seed {seed}")
    prompt_id = comfy.submit(base, graph, comfy.client_id())
    _say(f"prompt {prompt_id} on {base}")
    entry = comfy.wait_for(base, prompt_id, timeout=float(args.timeout), poll=POLL_S, on_progress=_progress)

    out = request["out"]
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="forge-music-") as scratch:
        saved = comfy.fetch(base, entry, Path(scratch))
        wav = out if request["format"] == "wav" else Path(scratch) / "track.wav"
        transcode_wav(ffmpeg, saved[0], wav)
        # On the PCM, before the ogg and before the record: nine renders off
        # this host came back pinned at 0.0 dBFS with runs of 10 to 186
        # full-scale samples and every one of them printed OK (2026-08-30).
        check_pcm(wav, expected_s=request["duration_s"], what="the track")
        measured = measure_wav(wav)
        if request["format"] == "ogg":
            transcode_ogg(ffmpeg, wav, out)

    result = {
        "prompt": request["prompt"],
        "lyrics": inputs["lyrics"],
        # The pair the two stages were given, in the same comma-separated
        # form the old server reported and the Rust reader already parses.
        "seed_value": f"{inputs['plan_seed']},{inputs['seed']}",
        "dit_model": _checkpoint(graph),
        "lm_model": None,
        "metas": {
            "bpm": inputs["bpm"],
            "keyscale": inputs["keyscale"],
            "timesignature": inputs["timesignature"],
            "genres": inputs["tags"],
        },
    }
    facts = backend_facts(backend, host, workflow_sha256=template_sha, model=_checkpoint(graph))
    rec = build_record(request, result, measured=measured, backend=facts, created_by=getattr(args, "created_by", None))
    rec["params"]["workflow"] = WORKFLOW
    summary = finish(rec, request, out, request["record"])
    # The one key the daemon copies into the job row verbatim. The process
    # that patched the graph is the one that says what it patched; nothing
    # in Rust composes this.
    summary["comfy"] = {
        "template": _tracked(backend, WORKFLOW),
        "template_sha256": template_sha,
        "inputs": inputs,
        "comfyui_commit": facts["comfyui_commit"],
        "packs": facts["packs"],
        "prompt_id": prompt_id,
        "cached": comfy.was_cached(entry, save_node),
    }
    _say(f"OK {out} ({out.stat().st_size / 1e6:.1f} MB, {measured['duration_s']} s)")
    return summary


def _fresh_seed() -> int:
    """A seed nobody chose, which is still a seed worth writing down."""
    import random  # noqa: PLC0415 - only when one is actually drawn

    return random.randrange(2**31)


def _checkpoint(graph: dict) -> str | None:
    """The checkpoint file the graph loads, for the record's ``model``."""
    for node in graph.values():
        if isinstance(node, dict) and node.get("class_type") == "CheckpointLoaderSimple":
            return node.get("inputs", {}).get("ckpt_name")
    return None


def _tracked(backend: backends_mod.Backend, name: str) -> str:
    """The template's path as a record and a job row name it: relative to the toolkit.

    Not to the project and not absolute: the file is tracked in this
    repository, and "go and look at it" is the whole reason the hash beside
    it is worth anything.
    """
    return f"backends/{backend.name}/workflows/{name}"


def run_fake(args) -> dict:
    """A short placeholder tone and a record that says ``fake``; no host, no graph.

    It does not import :mod:`forge_gen.comfy`, read a template or resolve a
    URL — which is what keeps ``just ci-fake`` a control for the whole move
    to the host: it proves the same thing on the day this lands as the day
    before.

    The ogg case still needs ffmpeg: Symphonia on the Rust side decodes
    what it is given, and a WAV wearing an ``.ogg`` name would fail there
    instead of here. Without ffmpeg the fake refuses with exit 6 like the
    real path would.
    """
    refuse_stop_server(args)
    request = check_inputs(args)
    out = request["out"]
    placeholders.refuse_real(out, request["record"])
    if request["format"] == "ogg":
        ffmpeg = ffmpeg_bin()
        wav = out.with_name(out.name + ".tmp.wav")
        placeholders.placeholder_wav(wav, seconds=min(request["duration_s"], 2.0))
        check_pcm(wav, what="the placeholder")
        measured = measure_wav(wav)
        try:
            # The vorbis comment is the placeholder mark: the WAV's RIFF
            # chunk does not survive a transcode.
            transcode_ogg(ffmpeg, wav, out, comment=placeholders.FAKE_MARK.decode("ascii"))
        finally:
            wav.unlink(missing_ok=True)
    else:
        placeholders.placeholder_wav(out, seconds=min(request["duration_s"], 2.0))
        check_pcm(out, what="the placeholder")
        measured = measure_wav(out)
    # What the graph would have been given, minus the graph: nothing is
    # invented, the seeds and the checkpoint stay null.
    result = {"prompt": request["prompt"], "lyrics": request["lyrics"], "metas": {"bpm": request["bpm"], "keyscale": request["keyscale"]}}
    backend = {"name": BACKEND, "commit": placeholders.FAKE_COMMIT, "executor": "comfy"}
    rec = build_record(request, result, measured=measured, backend=backend, created_by=getattr(args, "created_by", None), fake=True)
    rec["note"] = "placeholder output from a --fake run; nothing about it is a measurement"
    return finish(rec, request, out, request["record"])
