"""``forge-gen music``: one track from a prompt through the local ACE-Step 1.5 API server.

    forge-gen music --out boss_ambush.ogg --record boss_ambush.music.json \\
        --prompt "dark ambient boss theme, low strings, taiko" --duration 90
    forge-gen music --out hub_theme.ogg --record hub_theme.music.json \\
        --prompt "hopeful synthwave exploration" --lyrics-file verse.txt --duration 120
    forge-gen music --stop-server

Starts the server (first call loads models — allow a few minutes) if it is
not already up, and leaves it up: ACE-Step is a resident server at ~8 GB,
and a second track a minute later should not pay the load again. It does not
co-reside with a lift or a sweep; ``--stop-server`` (alone, or after a
track) sends it SIGTERM through the pid file this module wrote.

The outer half is the whole of it: a stdlib HTTP client (``/health``,
``/release_task``, ``/query_result`` polling, the WAV download) and the
server's lifecycle. Nothing of torch is imported on this side — the server
runs under the backend's own interpreter, resolved by the launcher before
anything else happens, so a missing backend is exit 3 in about 100 ms.

The record (``forge_record: 1``, kind ``music``) carries what ACE-Step
actually used — both seeds verbatim, the resolved key and tempo, both
checkpoints — so it is ``recorded``, not reconstructed. The content hash is
the other half of that claim: the model is not bit-reproducible, so "this is
the file that was auditioned" is the strongest thing a record can say about
the bytes.

Stdlib only.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import launcher, placeholders, records
from forge_gen.exit_codes import BackendFailed, InputRejected, MissingTool, UsageError

#: The backend directory this command runs through.
BACKEND = "acestep"

#: The tool name the sidecar's generator block uses (``forge_library::schema::Generator::AceStep``).
TOOL = "ace_step"

#: The record kind.
KIND = "music"

#: Output containers the command writes. The server renders WAV in every
#: case — it only saves wav/flac without torchcodec, whose ffmpeg libraries
#: are the broken ones on this box — and ogg is a local transcode.
FORMATS = ("ogg", "wav")

#: What ACE-Step accepts for ``audio_duration``, seconds.
DURATION_MIN, DURATION_MAX = 10.0, 600.0

#: The environment variable that overrides where the server is.
URL_ENV = "ACESTEP_API_URL"

#: Fallbacks when ``backend.toml`` has no ``[server]`` table.
DEFAULT_HOST, DEFAULT_PORT, DEFAULT_HEALTH = "127.0.0.1", 8001, "/health"

#: How long to wait for ``/health`` after starting the server; the first
#: start loads every model and takes minutes.
READY_TIMEOUT_S = 600.0

#: Seconds between readiness probes while the server starts.
READY_POLL_S = 3.0

#: Seconds between ``/query_result`` polls.
POLL_S = 5.0

#: How long one track may take end to end; the server's own task timeout is an hour.
GENERATE_TIMEOUT_S = 3600.0

#: How long ``--stop-server`` waits after SIGTERM before SIGKILL.
STOP_TIMEOUT_S = 30.0

#: The server log and pid file, under the state directory.
LOG_NAME = "acestep-server.log"
PID_NAME = "acestep-server.pid"

#: How many trailing lines of the server log a failure carries.
LOG_TAIL = 40

#: What the server is told when the track has no words.
INSTRUMENTAL = "[instrumental]"

#: Vorbis quality for the ogg transcode (~192 kb/s).
VORBIS_QUALITY = "6"


# ---------------------------------------------------------------- parser --


def add_parser(subparsers) -> None:
    """Register ``music``."""
    parser = subparsers.add_parser(
        "music",
        help="One track from a prompt (ACE-Step; the server stays resident)",
        description=__doc__,
    )
    parser.add_argument("--out", metavar="FILE", help="where the track goes: .ogg or .wav")
    parser.add_argument("--record", metavar="JSON", help="where the forge_record goes")
    parser.add_argument("--prompt", metavar="TEXT", help="music description (genre, mood, instrumentation)")
    parser.add_argument("--lyrics-file", metavar="FILE", help="lyrics from a file; omit for instrumental")
    parser.add_argument("--duration", type=float, default=30.0, metavar="S", help="seconds, 10-600 (default 30)")
    parser.add_argument("--seed", type=int, default=None, metavar="N", help="a fixed seed; omit for random")
    parser.add_argument("--bpm", type=int, default=None, metavar="N", help="tempo")
    parser.add_argument("--keyscale", default=None, metavar="KEY", help='e.g. "C Major", "Am"')
    parser.add_argument("--thinking", action="store_true", help="use the 5Hz LM planner (slower, better structure)")
    parser.add_argument("--format", choices=FORMATS, default=None, help="container (default: from --out's suffix)")
    parser.add_argument("--stop-server", action="store_true", help="SIGTERM the resident server (alone, or after the track)")
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
    """Everything the request needs, refused before any server is touched."""
    if not args.out or not args.record:
        raise UsageError("--out and --record are required (unless --stop-server is all you want)")
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
    fmt = resolve_format(args.out, args.format)
    return {
        "prompt": prompt,
        "lyrics": read_lyrics(args.lyrics_file),
        "lyrics_file": str(Path(args.lyrics_file).resolve()) if args.lyrics_file else None,
        "duration_s": duration,
        "seed": args.seed,
        "bpm": args.bpm,
        "keyscale": (args.keyscale or "").strip() or None,
        "thinking": bool(args.thinking),
        "format": fmt,
        "out": Path(args.out).resolve(),
        "record": Path(args.record).resolve(),
    }


def ffmpeg_bin() -> Path:
    """``ffmpeg`` on PATH, or :class:`MissingTool` (exit 6)."""
    found = shutil.which("ffmpeg")
    if not found:
        raise MissingTool("ffmpeg is not on PATH", tool="ffmpeg", hint="install ffmpeg, or ask for --format wav")
    return Path(found)


# ------------------------------------------------------------- the record --


def request_payload(request: dict) -> dict:
    """The ``/release_task`` body for a checked request."""
    payload = {
        "prompt": request["prompt"],
        "lyrics": request["lyrics"],
        "audio_duration": request["duration_s"],
        # Always WAV: the server only saves wav/flac without torchcodec; the
        # ogg is a local transcode (see FORMATS).
        "audio_format": "wav",
        "thinking": request["thinking"],
    }
    if request.get("bpm"):
        payload["bpm"] = request["bpm"]
    if request.get("keyscale"):
        payload["key_scale"] = request["keyscale"]
    if request.get("seed") is not None:
        payload["seed"] = request["seed"]
        payload["use_random_seed"] = False
    return payload


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


# --------------------------------------------------------------- the server --


def state_dir() -> Path:
    """``$XDG_STATE_HOME/asset-forge`` (default ``~/.local/state/asset-forge``)."""
    base = os.environ.get("XDG_STATE_HOME") or os.path.join(os.path.expanduser("~"), ".local", "state")
    return Path(base) / "asset-forge"


def log_path() -> Path:
    return state_dir() / LOG_NAME


def pid_path() -> Path:
    return state_dir() / PID_NAME


def server_settings(backend: backends_mod.Backend | None) -> dict:
    """Host, port, health path and readiness timeout from ``[server]``."""
    table = dict(backend.server or {}) if backend is not None else {}
    return {
        "host": str(table.get("host", DEFAULT_HOST)),
        "port": int(table.get("port", DEFAULT_PORT)),
        "health": str(table.get("health", DEFAULT_HEALTH)),
        "ready_timeout_s": float(table.get("ready_timeout_s", READY_TIMEOUT_S)),
    }


def base_url(settings: dict) -> str:
    """``$ACESTEP_API_URL`` when set, else ``http://host:port`` from the settings."""
    override = os.environ.get(URL_ENV)
    if override:
        return override.rstrip("/")
    return f"http://{settings['host']}:{settings['port']}"


def api(url: str, path: str, payload: dict | None = None, timeout: float = 30.0) -> dict:
    """One JSON round trip with the server."""
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url + path, data=data, headers={"Content-Type": "application/json"} if data else {})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def server_up(url: str, health: str = DEFAULT_HEALTH) -> bool:
    """Whether ``/health`` answers ``status: ok``."""
    try:
        return api(url, health, timeout=5).get("data", {}).get("status") == "ok"
    except Exception:  # noqa: BLE001 - down, refusing, or not there: all "no"
        return False


def _log_tail(path: Path, lines: int = LOG_TAIL) -> list[str]:
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            return [line.rstrip("\n") for line in handle.readlines()[-lines:]]
    except OSError:
        return []


def _say(text: str) -> None:
    sys.stdout.write(f"[music] {text}\n")
    sys.stdout.flush()


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _proc_start(pid: int) -> str | None:
    """The kernel's start time of ``pid`` (field 22 of ``/proc/<pid>/stat``), or ``None`` without /proc."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    # The command name sits in parentheses and may hold spaces; split after it.
    fields = stat[stat.rindex(")") + 2 :].split()
    return fields[19] if len(fields) > 19 else None


def _pid_is_ours(pid: int, start: str | None) -> bool:
    """Whether ``pid`` is the server the pid file describes — so a stale file never kills a stranger.

    The pid file carries the process's kernel start time; a pid reused after
    a reboot or a crash has a different one. Without a recorded start time
    (an older file) or without /proc, the command line is the evidence.
    """
    if not _pid_alive(pid):
        return False
    current = _proc_start(pid)
    if start and current:
        return current == start
    try:
        cmdline = Path(f"/proc/{pid}/cmdline").read_bytes().replace(b"\0", b" ")
    except OSError:
        return True
    return b"acestep.api_server" in cmdline or b"acestep-api" in cmdline


def write_pid(pid: int) -> None:
    """``<pid> <start time>`` — enough to tell this process from one that inherited its pid."""
    pid_path().parent.mkdir(parents=True, exist_ok=True)
    start = _proc_start(pid)
    pid_path().write_text(f"{pid} {start}\n" if start else f"{pid}\n")


def read_pid() -> tuple[int, str | None] | None:
    """``(pid, start time or None)`` from the pid file, or ``None`` when there is no usable file."""
    try:
        words = pid_path().read_text().split()
    except OSError:
        return None
    if not words:
        return None
    try:
        return int(words[0]), (words[1] if len(words) > 1 else None)
    except ValueError:
        return None


def start_server(backend: backends_mod.Backend, interpreter: Path, settings: dict, url: str) -> int:
    """Spawn ``acestep.api_server`` under the backend env and wait for ``/health``.

    Runs ``<env python> -m acestep.api_server --host --port`` — ``python -m``
    rather than the console script, whose shebang in an adopted venv may
    point at a pre-move path — from the upstream checkout, with ``[env]``
    from ``backend.toml`` exported (``ACESTEP_CHECKPOINTS_DIR`` above all),
    in its own session so it outlives this command, stdout+stderr appended
    to the state log. Returns the pid, which is also written to the pid file.
    """
    env = launcher.inner_env(backend, interpreter)
    cwd = launcher.inner_cwd(backend)
    state_dir().mkdir(parents=True, exist_ok=True)
    log = log_path()
    command = [
        str(interpreter),
        "-m",
        "acestep.api_server",
        "--host",
        settings["host"],
        "--port",
        str(settings["port"]),
    ]
    _say(f"ACE-Step server not running — starting it (model load takes a few minutes; log: {log})")
    with open(log, "ab") as sink:
        sink.write(f"\n=== forge-gen music: starting {' '.join(command)} at {time.strftime('%Y-%m-%d %H:%M:%S')}\n".encode())
        try:
            process = subprocess.Popen(
                command,
                cwd=os.fspath(cwd) if cwd else None,
                env=env,
                stdout=sink,
                stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL,
                start_new_session=True,
            )
        except OSError as err:
            raise BackendFailed(f"could not start the ACE-Step server: {err}") from err
    write_pid(process.pid)
    started = time.monotonic()
    deadline = started + settings["ready_timeout_s"]
    while time.monotonic() < deadline:
        if server_up(url, settings["health"]):
            _say(f"server up after {time.monotonic() - started:.0f}s (pid {process.pid})")
            return process.pid
        if process.poll() is not None:
            pid_path().unlink(missing_ok=True)
            raise BackendFailed(
                f"the ACE-Step server exited with {process.returncode} while starting — see {log}",
                log_tail=_log_tail(log),
            )
        time.sleep(READY_POLL_S)
    # Still running but not answering: leave it and its pid file alone, so
    # --stop-server can end it, and say where to look.
    raise BackendFailed(
        f"the ACE-Step server did not answer {settings['health']} within {settings['ready_timeout_s']:.0f} s — see {log}",
        log_tail=_log_tail(log),
    )


def ensure_server(backend: backends_mod.Backend, interpreter: Path, settings: dict, url: str) -> bool:
    """Make sure the server answers; returns whether this call started it."""
    if server_up(url, settings["health"]):
        return False
    start_server(backend, interpreter, settings, url)
    return True


def stop_server(settings: dict | None = None, url: str | None = None) -> dict:
    """SIGTERM the server the pid file names; SIGKILL after :data:`STOP_TIMEOUT_S`.

    A pid file whose process is not an ``acestep.api_server`` is stale and is
    removed, not signalled. A server that answers ``/health`` but has no pid
    file was not started by this command, and is reported rather than
    hunted for.
    """
    settings = settings or server_settings(None)
    url = url or base_url(settings)
    found = read_pid()
    out: dict = {"pid": found[0] if found else None, "stopped": False, "was_running": False}
    if found is None:
        if server_up(url, settings["health"]):
            out["note"] = f"a server answers at {url} but {pid_path()} is absent — not started by forge-gen; stop it yourself"
            out["was_running"] = True
        else:
            out["note"] = "no server running"
        return out
    pid, start = found
    if not _pid_is_ours(pid, start):
        pid_path().unlink(missing_ok=True)
        out["note"] = f"pid {pid} is not the server this file described (stale pid file removed)"
        return out
    out["was_running"] = True
    _say(f"stopping server pid {pid}")
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pid_path().unlink(missing_ok=True)
        out["note"] = "already gone"
        return out
    deadline = time.monotonic() + STOP_TIMEOUT_S
    while time.monotonic() < deadline:
        if not _pid_alive(pid):
            break
        time.sleep(0.5)
    else:
        _say(f"pid {pid} ignored SIGTERM for {STOP_TIMEOUT_S:.0f}s; SIGKILL")
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        time.sleep(1.0)
        out["killed"] = True
    pid_path().unlink(missing_ok=True)
    out["stopped"] = not _pid_alive(pid)
    return out


# ------------------------------------------------------------- generation --


def submit(url: str, payload: dict) -> str:
    """``/release_task`` → the task id."""
    try:
        data = api(url, "/release_task", payload)["data"]
    except (urllib.error.URLError, OSError, KeyError, ValueError) as err:
        raise BackendFailed(f"/release_task failed: {err}", log_tail=_log_tail(log_path())) from err
    if isinstance(data, dict):
        data = data.get("task_id", data)
    return str(data)


def wait_for(url: str, task_id: str, *, timeout: float) -> dict:
    """Poll ``/query_result`` until the task is done; the first result item."""
    started = time.monotonic()
    next_report = started + 30.0
    while True:
        if time.monotonic() - started > timeout:
            raise BackendFailed(f"task {task_id} did not finish within {timeout:.0f} s", log_tail=_log_tail(log_path()))
        try:
            rows = api(url, "/query_result", {"task_id_list": [task_id]})["data"]
        except Exception as err:  # noqa: BLE001 - a busy server can stall a poll; just retry
            _say(f"poll retry ({type(err).__name__})")
            time.sleep(POLL_S)
            continue
        row = rows[0] if rows else {}
        status = row.get("status")
        if status == 1:
            break
        if status == 2:
            raise BackendFailed(f"generation failed: {json.dumps(row, ensure_ascii=False)[:800]}", log_tail=_log_tail(log_path()))
        time.sleep(POLL_S)
        if time.monotonic() >= next_report:
            _say(f"... {time.monotonic() - started:.0f}s" + (f" ({row.get('progress_text')})" if row.get("progress_text") else ""))
            next_report = time.monotonic() + 30.0
    try:
        result = json.loads(row["result"])[0]
    except (KeyError, ValueError, IndexError, TypeError) as err:
        raise BackendFailed(f"task {task_id} finished with an unreadable result: {err}", log_tail=_log_tail(log_path())) from err
    if not result.get("file"):
        raise BackendFailed(f"server reported success but no audio file — see {log_path()}", log_tail=_log_tail(log_path()))
    return result


def download(url: str, file_url: str, dest: Path) -> Path:
    """Fetch the rendered WAV from ``/v1/audio?path=…``."""
    if not file_url.startswith(("http://", "https://", "/")):
        file_url = "/" + file_url
    source = file_url if file_url.startswith("http") else url + file_url
    dest.parent.mkdir(parents=True, exist_ok=True)
    try:
        with urllib.request.urlopen(source, timeout=300) as resp, open(dest, "wb") as sink:
            shutil.copyfileobj(resp, sink)
    except (urllib.error.URLError, OSError) as err:
        raise BackendFailed(f"downloading {source} failed: {err}") from err
    return dest


def transcode_ogg(ffmpeg: Path, wav: Path, out: Path, *, comment: str | None = None) -> Path:
    """WAV → Ogg Vorbis through the ffmpeg CLI (``-q:a 6``); ``comment`` lands as a vorbis tag."""
    out.parent.mkdir(parents=True, exist_ok=True)
    metadata = ["-metadata", f"comment={comment}"] if comment else []
    done = subprocess.run(
        [str(ffmpeg), "-y", "-loglevel", "error", "-i", str(wav), "-c:a", "libvorbis", "-q:a", VORBIS_QUALITY, *metadata, str(out)],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise BackendFailed(f"ffmpeg exited {done.returncode} transcoding {wav.name}", log_tail=done.stderr.splitlines()[-LOG_TAIL:])
    return out


def _checkout_commit(backend: backends_mod.Backend) -> str | None:
    """HEAD of the ``.checkout`` link, or ``None`` when it will not say."""
    checkout = backend.checkout
    if not checkout.exists():
        return None
    try:
        done = subprocess.run(
            ["git", "-C", str(checkout.resolve()), "rev-parse", "--verify", "HEAD"],
            capture_output=True, text=True, timeout=20, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return done.stdout.strip() or None


def _interpreter_version(interpreter: Path) -> str | None:
    try:
        done = subprocess.run(
            [str(interpreter), "-c", "import sys; print('.'.join(map(str, sys.version_info[:3])))"],
            capture_output=True, text=True, timeout=20, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return done.stdout.strip() or None


def backend_facts(backend: backends_mod.Backend, interpreter: Path) -> dict:
    """The ``backend`` block's inputs: name, the checkout's commit, the env's python, torch from the receipt.

    Torch's version comes from ``installed.json`` — importing torch to ask
    it would cost seconds on this side of the launcher, and the receipt is
    what the installer measured of the same env. ``None`` when absent.
    """
    receipt = backend.installed() or {}
    return {
        "name": backend.name,
        "commit": _checkout_commit(backend),
        "python": _interpreter_version(interpreter),
        "torch": receipt.get("torch") if isinstance(receipt.get("torch"), str) else None,
        "model": None,
        "model_revision": None,
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


def run(args) -> dict:
    """Resolve the backend, make sure the server is up, render, transcode, record."""
    backend = backends_mod.load_backend(BACKEND)
    settings = server_settings(backend)
    url = base_url(settings)

    if args.stop_server and not (args.prompt or args.out or args.record):
        outcome = stop_server(settings, url)
        return {"ok": True, "server": outcome, "outputs": [], "record": None, "_text": f"music: {outcome.get('note') or ('stopped pid %s' % outcome['pid'])}\n"}

    request = check_inputs(args)
    interpreter = launcher.resolve_interpreter(backend)  # exit 3 here, before anything costs
    ffmpeg = ffmpeg_bin() if request["format"] == "ogg" else None
    facts = backend_facts(backend, interpreter)

    started_here = ensure_server(backend, interpreter, settings, url)
    payload = request_payload(request)
    task_id = submit(url, payload)
    _say(f"task {task_id} submitted ({request['duration_s']:g} s, {'thinking' if request['thinking'] else 'direct'})")
    result = wait_for(url, task_id, timeout=float(args.timeout))

    out = request["out"]
    out.parent.mkdir(parents=True, exist_ok=True)
    wav = out if request["format"] == "wav" else out.with_name(out.name + ".tmp.wav")
    download(url, str(result["file"]), wav)
    measured = measure_wav(wav)
    if request["format"] == "ogg":
        try:
            transcode_ogg(ffmpeg, wav, out)
        finally:
            wav.unlink(missing_ok=True)

    rec = build_record(request, result, measured=measured, backend=facts, created_by=getattr(args, "created_by", None))
    summary = finish(rec, request, out, request["record"])
    summary["server"] = {"url": url, "started": started_here, "stopped": False}
    if args.stop_server:
        summary["server"].update(stop_server(settings, url))
    _say(f"OK {out} ({out.stat().st_size / 1e6:.1f} MB, {measured['duration_s']} s)")
    return summary


def run_fake(args) -> dict:
    """A short placeholder tone and a record that says ``fake``; no server, no env.

    The ogg case still needs ffmpeg: Symphonia on the Rust side decodes
    what it is given, and a WAV wearing an ``.ogg`` name would fail there
    instead of here. Without ffmpeg the fake refuses with exit 6 like the
    real path would.
    """
    if args.stop_server and not (args.prompt or args.out or args.record):
        return {"ok": True, "server": {"pid": None, "stopped": False, "note": "fake: no server to stop"}, "outputs": [], "record": None}
    request = check_inputs(args)
    out = request["out"]
    placeholders.refuse_real(out, request["record"])
    if request["format"] == "ogg":
        ffmpeg = ffmpeg_bin()
        wav = out.with_name(out.name + ".tmp.wav")
        placeholders.placeholder_wav(wav, seconds=min(request["duration_s"], 2.0))
        measured = measure_wav(wav)
        try:
            # The vorbis comment is the placeholder mark: the WAV's RIFF
            # chunk does not survive a transcode.
            transcode_ogg(ffmpeg, wav, out, comment=placeholders.FAKE_MARK.decode("ascii"))
        finally:
            wav.unlink(missing_ok=True)
    else:
        placeholders.placeholder_wav(out, seconds=min(request["duration_s"], 2.0))
        measured = measure_wav(out)
    # What a server would have said, minus the server: nothing is invented,
    # the seeds and checkpoints stay null.
    result = {"prompt": request["prompt"], "lyrics": request["lyrics"], "metas": {"bpm": request["bpm"], "keyscale": request["keyscale"]}}
    backend = {"name": BACKEND, "commit": placeholders.FAKE_COMMIT}
    rec = build_record(request, result, measured=measured, backend=backend, created_by=getattr(args, "created_by", None), fake=True)
    rec["note"] = "placeholder output from a --fake run; nothing about it is a measurement"
    summary = finish(rec, request, out, request["record"])
    summary["server"] = {"started": False, "stopped": False, "fake": True}
    return summary
