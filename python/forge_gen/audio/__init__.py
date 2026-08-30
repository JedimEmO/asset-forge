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

import os
import shutil
import subprocess
from pathlib import Path

from forge_gen.exit_codes import BackendFailed, MissingTool

#: How many trailing lines of a failed transcode a refusal carries.
LOG_TAIL = 40

#: Vorbis quality for the ogg transcode (~192 kb/s).
VORBIS_QUALITY = "6"


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
