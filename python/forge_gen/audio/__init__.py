"""forge_gen.audio — the four audio commands, and the one thing all four now do.

Every one of them runs as a graph on the ComfyUI host, and **ComfyUI v0.34.2
cannot write a WAV**: ``SaveAudio`` saves FLAC and ``SaveAudioAdvanced``
offers flac, mp3 and opus (read off the node schemas on the running host,
2026-08-30). The library's audio is PCM WAV — ``measure_wav`` reads one with
the stdlib ``wave`` module, and a game engine decodes 16-bit PCM with no
float-WAV path — so each verb fetches what the graph saved and transcodes it
here before anything measures it or hashes it into a record.

That is why ffmpeg is now a dependency of `sfx`, `speech` and `voice` as
well as `music`, where it used to be needed only for `music --format ogg`:
the old venv path wrote the WAV itself with ``soundfile``. A missing ffmpeg
is exit 6, refused **before** the card is leased rather than after a
four-minute render.

Stdlib only, like everything on this side of the launcher.
"""

from __future__ import annotations

import array
import os
import shutil
import subprocess
import wave
from pathlib import Path

from forge_gen.exit_codes import BackendFailed, MissingTool

#: How many trailing lines of a failed transcode a refusal carries.
LOG_TAIL = 40

#: Vorbis quality for the ogg transcode (~192 kb/s).
VORBIS_QUALITY = "6"

# ------------------------------------------------------------------- the gate --
#
# The three numbers below are `crates/forge_audio/src/metrics.rs`'s, read off
# it and not invented here: one door must not call a file defective that the
# other calls fine.

#: At or beyond this magnitude a sample sits at full scale. Not 1.0: a lossy
#: codec overshoots by a hair on reconstruction.
CLIP_THRESHOLD = 0.999

#: Consecutive full-scale samples, in one channel, before it is clipping
#: rather than normalisation. Peak-normalising puts one sample at the rail
#: by construction; audible distortion is a run.
CLIP_RUN = 3

#: Below this magnitude a sample is silence (about -60 dBFS).
SILENCE_FLOOR = 0.001

#: How much shorter than the length that was *asked for* a render may come
#: back before it is a truncation rather than a rounding. Three quarters is
#: loose on purpose — a 3 s effect measures 3.0 s and a 30 s track 30.01 s,
#: so nothing this repository has rendered is anywhere near it, and the case
#: it catches is a graph that stopped early.
SHORT_OF_REQUEST = 0.75


def check_pcm(path: str | os.PathLike, *, expected_s: float | None = None, what: str = "the render") -> dict:
    """Refuse a PCM WAV that ``forge audio inspect`` would refuse.

    ``measure_wav`` is not a gate: it reads a duration, a rate and a channel
    count, all of which are true of one second of digital zeros. On
    2026-08-30 the TTS-Audio-Suite node caught its own ``AttributeError``,
    logged it with an emoji, returned a silent tensor and let the graph
    complete — so ComfyUI reported success, ``forge gen speech`` printed
    ``OK``, and a ``forge_record: 2`` was written for a file whose peak was
    -120 dBFS. Every number in that record was true and the file was
    worthless. The same afternoon nine ACE-Step renders came off the host
    pinned at 0.0 dBFS with runs of 10 to 186 full-scale samples, and every
    one of them printed ``OK`` too.

    So this runs on the transcoded PCM **before the record is written**, in
    the one place that knows the run happened, and raises
    :class:`BackendFailed` (exit 5) naming the measurement. Nothing is
    recorded for a file that fails it: a record is what this repository
    treats as the truth, and the fix for a bad render is upstream of it —
    another seed, another prompt, a pin that works.

    Returns what it measured, so a caller can say it out loud.
    """
    path = Path(path)
    try:
        with wave.open(os.fspath(path), "rb") as handle:
            channels = handle.getnchannels()
            width = handle.getsampwidth()
            rate = handle.getframerate()
            frames = handle.getnframes()
            raw = handle.readframes(frames)
    except (wave.Error, EOFError, OSError) as err:
        raise BackendFailed(
            f"{what} is not a PCM WAV this side can read ({path.name}: {err}) — nothing was recorded",
        ) from err
    if frames == 0 or not raw:
        raise BackendFailed(
            f"{what} came back empty: {path.name} has no audio frames at all — nothing was recorded",
        )
    typecode = {1: "b", 2: "h", 4: "i"}.get(width)
    if typecode is None:
        raise BackendFailed(
            f"{what} is {width * 8}-bit PCM, which this gate cannot read; the transcode writes "
            f"16-bit and something else wrote {path.name} — nothing was recorded",
        )
    samples = array.array(typecode)
    samples.frombytes(raw[: frames * channels * width])
    full = float(1 << (width * 8 - 1))
    peak = max(abs(min(samples)), abs(max(samples))) / full
    duration = frames / rate if rate else 0.0

    if peak <= SILENCE_FLOOR:
        raise BackendFailed(
            f"{what} is silent: peak {_dbfs(peak)} over {duration:.3f} s in {path.name}. the graph "
            f"completed and produced nothing audible — nothing was recorded",
            hint="re-run with another seed, or read the host's journal: a node that catches its "
            "own error returns a silent tensor and lets the graph succeed",
        )
    run = _longest_full_scale_run(samples, channels, full)
    if run >= CLIP_RUN:
        raise BackendFailed(
            f"{what} is clipped: {run} consecutive samples pinned at full scale in {path.name} "
            f"(peak {_dbfs(peak)}) — audible distortion, and nothing was recorded",
            hint="a peak that lands on 0.0 dBFS whatever the content is a normalise-to-peak on "
            "the generator's side; state a gain knob in the graph rather than editing the file",
        )
    if expected_s and duration < expected_s * SHORT_OF_REQUEST:
        raise BackendFailed(
            f"{what} is truncated: {duration:.3f} s came back where {expected_s:g} s was asked "
            f"for ({path.name}) — nothing was recorded",
        )
    return {"peak": peak, "peak_dbfs": _dbfs(peak), "duration_s": duration, "clip_run": run}


def _longest_full_scale_run(samples: array.array, channels: int, full: float) -> int:
    """The longest run of full-scale samples in any one channel.

    Measured per channel, like the Rust side: interleaved stereo would
    otherwise turn one loud sample per channel into a run of two.
    """
    level = CLIP_THRESHOLD * full
    if max(abs(min(samples)), abs(max(samples))) < level:
        return 0
    longest = 0
    for channel in range(channels):
        run = 0
        for index in range(channel, len(samples), channels):
            if abs(samples[index]) >= level:
                run += 1
                longest = max(longest, run)
            else:
                run = 0
    return longest


def _dbfs(peak: float) -> str:
    """A linear peak as dBFS, floored so silence does not print as -inf."""
    import math  # noqa: PLC0415 - one call, on a path that is about to raise

    return f"{max(20.0 * math.log10(peak), -120.0) if peak > 0 else -120.0:.1f} dBFS"


def ffmpeg_bin() -> Path:
    """``ffmpeg`` on PATH, or :class:`MissingTool` (exit 6)."""
    found = shutil.which("ffmpeg")
    if not found:
        raise MissingTool(
            "ffmpeg is not on PATH",
            tool="ffmpeg",
            hint="install ffmpeg — the host saves FLAC and every audio verb transcodes it here",
        )
    return Path(found)


def transcode_wav(ffmpeg: Path, source: str | os.PathLike, out: str | os.PathLike) -> Path:
    """Whatever the graph saved → 16-bit PCM WAV, losslessly from FLAC."""
    out = Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    done = subprocess.run(
        [str(ffmpeg), "-y", "-loglevel", "error", "-i", os.fspath(source), "-c:a", "pcm_s16le", str(out)],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise BackendFailed(
            f"ffmpeg exited {done.returncode} decoding {Path(source).name} to PCM",
            log_tail=done.stderr.splitlines()[-LOG_TAIL:],
        )
    return out


def transcode_ogg(ffmpeg: Path, wav: str | os.PathLike, out: str | os.PathLike, *, comment: str | None = None) -> Path:
    """WAV → Ogg Vorbis through the ffmpeg CLI (``-q:a 6``); ``comment`` lands as a vorbis tag."""
    out = Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    metadata = ["-metadata", f"comment={comment}"] if comment else []
    done = subprocess.run(
        [str(ffmpeg), "-y", "-loglevel", "error", "-i", os.fspath(wav), "-c:a", "libvorbis", "-q:a", VORBIS_QUALITY, *metadata, str(out)],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise BackendFailed(
            f"ffmpeg exited {done.returncode} transcoding {Path(wav).name}",
            log_tail=done.stderr.splitlines()[-LOG_TAIL:],
        )
    return out
