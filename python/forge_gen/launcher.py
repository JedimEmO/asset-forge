"""Finding a backend's interpreter and running the inner half under it — or Blender.

The outer ``forge_gen`` runs under the system python and must never import
torch; the inner modules run under each backend's own env and may. The
launcher is the seam: it resolves the interpreter (an override, the ``.env``
link, or exit 3 in about 100 ms), builds the environment ``backend.toml``
asks for, and execs ``<python> -m forge_gen.<module> --inner ...`` with this
package on ``PYTHONPATH`` so the inner half is the same source tree.

Resolution order for the interpreter::

    $FORGE_BACKEND_<NAME_UPPER>_PYTHON   a binary or a prefix
    backends/<name>/.env/bin/python      the symlink install.sh wrote
    → MissingBackend (exit 3, with the install line as the hint)

Blender is a host tool, not a backend: ``$BLENDER_BIN``, else ``blender`` on
PATH, else exit 6. It is run ``--background --factory-startup`` so a user's
add-ons and start-up file never enter a bake, and ``--python-exit-code 5``
so an uncaught exception in the script is a backend failure rather than a
clean exit with nothing written.
"""

from __future__ import annotations

import collections
import json
import os
import shutil
import string
import subprocess
import sys
import threading
from dataclasses import dataclass, field
from pathlib import Path

from forge_gen import exit_codes
from forge_gen.backends import Backend
from forge_gen.exit_codes import BackendFailed, MissingBackend, MissingTool

#: The environment variable naming a Blender binary.
BLENDER_ENV = "BLENDER_BIN"

#: The oldest Blender the scripts are written against.
BLENDER_MIN = (4, 2)

#: How many trailing lines of a failed run's output a refusal carries.
TAIL_LINES = 40

#: What an uncaught exception in a Blender script exits with.
BLENDER_EXCEPTION_EXIT = exit_codes.BACKEND_FAILED


def python_dir() -> Path:
    """The directory holding this package: what goes on the inner ``PYTHONPATH``."""
    return Path(__file__).resolve().parent.parent


def override_var(backend_name: str) -> str:
    """``FORGE_BACKEND_<NAME>_PYTHON`` for a backend."""
    return f"FORGE_BACKEND_{backend_name.upper()}_PYTHON"


def _python_under(prefix: Path) -> Path | None:
    for candidate in ("bin/python", "bin/python3"):
        path = prefix / candidate
        if path.is_file() or (path.is_symlink() and path.exists()):
            return path
    return None


def resolve_interpreter(backend: Backend) -> Path:
    """The interpreter that runs this backend's inner half, or :class:`MissingBackend`.

    The override may name the binary or its prefix. The ``.env`` link is
    always a prefix; a link with no ``bin/python`` under it is an install
    that did not finish, and says so.
    """
    override = os.environ.get(override_var(backend.name))
    if override:
        path = Path(override).expanduser()
        if path.is_file():
            return path
        found = _python_under(path) if path.is_dir() else None
        if found is not None:
            return found
        raise MissingBackend(
            f"{override_var(backend.name)}={override} is neither a python binary nor a prefix with bin/python",
            backend=backend.name,
            hint=f"unset it, or point it at the env: {backend.install_hint()}",
        )
    link = backend.env_link
    if link.is_symlink() and not link.exists():
        raise MissingBackend(
            f"{link} points at {os.readlink(link)}, which is not there — the environment moved or was deleted",
            backend=backend.name,
            hint=backend.install_hint(),
        )
    if link.exists():
        found = _python_under(link)
        if found is not None:
            return found
        raise MissingBackend(
            f"{link} has no bin/python — the install did not finish, or the environment moved",
            backend=backend.name,
            hint=backend.install_hint(),
        )
    raise MissingBackend(
        f"{backend.name} is not installed — generation through it is off",
        backend=backend.name,
        hint=backend.install_hint(),
    )


def prefix_of(backend: Backend, interpreter: Path | None = None) -> Path:
    """The env prefix (``${PREFIX}`` in ``[env]``): the directory above ``bin/``."""
    interpreter = interpreter or resolve_interpreter(backend)
    resolved = interpreter.resolve()
    if resolved.parent.name == "bin":
        return resolved.parent.parent
    return resolved.parent


def _expansion_mapping(backend: Backend, interpreter: Path) -> dict[str, str]:
    """What ``${...}`` in an ``[env]`` value may name: the placeholders, plus the ambient environment."""
    mapping = dict(os.environ)
    mapping.update(
        {
            "PREFIX": str(prefix_of(backend, interpreter)),
            "CHECKOUT": str(backend.checkout.resolve() if backend.checkout.exists() else backend.checkout),
            "BACKEND_DIR": str(backend.dir),
            "TEXT_ENCODERS": str(backend.text_encoders.resolve() if backend.text_encoders.exists() else backend.text_encoders),
            "CHECKPOINTS": str(backend.checkpoints.resolve() if backend.checkpoints.exists() else backend.checkpoints),
        }
    )
    return mapping


def inner_env(backend: Backend, interpreter: Path | None = None) -> dict[str, str]:
    """The environment the inner process runs with.

    ``os.environ`` with this package prepended to ``PYTHONPATH``, then every
    ``[env]`` entry of ``backend.toml`` with ``${PREFIX}``, ``${CHECKOUT}``,
    ``${BACKEND_DIR}``, ``${TEXT_ENCODERS}`` and ``${CHECKPOINTS}`` expanded
    (plus anything already in the environment), applied with *setdefault*
    semantics so a value the user exported wins over the file's — except
    ``[env.force]`` entries, which are set unconditionally: those are the
    values the backend does not work without (trellis2's CUDA host
    toolchain, where an anaconda-base ``CC`` in the shell fed the JIT a
    mixed toolchain). ``PYTHONNOUSERSITE=1`` is set the same way for every
    backend: a ``~/.local`` that has seen years of experiments carries
    ``.pth`` hooks.
    """
    env = dict(os.environ)
    python_path = str(python_dir())
    existing = env.get("PYTHONPATH")
    env["PYTHONPATH"] = python_path if not existing else os.pathsep.join([python_path, existing])
    interpreter = interpreter or resolve_interpreter(backend)
    mapping = _expansion_mapping(backend, interpreter)
    env.setdefault("PYTHONNOUSERSITE", "1")
    env.setdefault("FORGE_BACKEND", backend.name)
    for key, value in backend.env.items():
        env.setdefault(key, string.Template(value).safe_substitute(mapping))
    for key, value in backend.env_force.items():
        env[key] = string.Template(value).safe_substitute(mapping)
    return env


def env_shadowing(backend: Backend, interpreter: Path | None = None) -> list[tuple[str, str, str]]:
    """``(key, ambient, configured)`` for every plain ``[env]`` entry the shell overrides.

    Exactly the cases where :func:`inner_env`'s setdefault lets the ambient
    value win over ``backend.toml``'s. Doctor turns each into a warn row;
    ``[env.force]`` entries cannot be shadowed and are not listed.
    """
    interpreter = interpreter or resolve_interpreter(backend)
    mapping = _expansion_mapping(backend, interpreter)
    out: list[tuple[str, str, str]] = []
    for key, value in backend.env.items():
        ambient = os.environ.get(key)
        if ambient is None:
            continue
        configured = string.Template(value).safe_substitute(mapping)
        if ambient != configured:
            out.append((key, ambient, configured))
    return out


def inner_cwd(backend: Backend) -> Path | None:
    """Where the inner process stands: the upstream checkout when ``cwd = "checkout"``."""
    if backend.cwd != "checkout":
        return None
    checkout = backend.checkout
    if not checkout.exists():
        raise MissingBackend(
            f"{backend.name} runs from its upstream checkout, and {checkout} is not there",
            backend=backend.name,
            hint=backend.install_hint(),
        )
    return checkout.resolve()


@dataclass
class RunResult:
    """What a streamed subprocess left behind."""

    #: Its exit code.
    code: int
    #: The last lines of stdout and stderr, interleaved as they arrived.
    tail: list[str] = field(default_factory=list)
    #: The JSON object on its last stdout line, when there was one.
    result: dict | None = None


def stream(
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    cwd: str | os.PathLike | None = None,
    hold_json: bool = False,
    timeout: float | None = None,
) -> RunResult:
    """Run a command, relaying its output line by line and keeping the tail.

    With ``hold_json`` the last stdout line is held back one line at a time;
    when the process ends, a held line that parses as a JSON object is
    returned as ``result`` instead of printed, so an inner module's
    ``--json`` line reaches the outer as data and not as a stray line before
    the outer's own.
    """
    tail: collections.deque[str] = collections.deque(maxlen=TAIL_LINES)
    held: list[str | None] = [None]
    lock = threading.Lock()

    def pump(pipe, sink, hold: bool) -> None:
        for raw in iter(pipe.readline, ""):
            line = raw.rstrip("\n")
            with lock:
                tail.append(line)
                if hold:
                    previous, held[0] = held[0], line
                    if previous is not None:
                        sink.write(previous + "\n")
                        sink.flush()
                else:
                    sink.write(line + "\n")
                    sink.flush()
        pipe.close()

    try:
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            cwd=os.fspath(cwd) if cwd else None,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
    except OSError as err:
        raise BackendFailed(f"could not start {command[0]}: {err}") from err
    threads = [
        threading.Thread(target=pump, args=(process.stdout, sys.stdout, hold_json), daemon=True),
        threading.Thread(target=pump, args=(process.stderr, sys.stderr, False), daemon=True),
    ]
    for thread in threads:
        thread.start()
    try:
        code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
        for thread in threads:
            thread.join(timeout=1)
        raise BackendFailed(f"{command[0]} did not finish within {timeout:.0f} s and was killed", log_tail=list(tail)) from None
    for thread in threads:
        thread.join()
    result = None
    last = held[0]
    if last is not None:
        parsed = None
        stripped = last.strip()
        if stripped.startswith("{") and stripped.endswith("}"):
            try:
                parsed = json.loads(stripped)
            except json.JSONDecodeError:
                parsed = None
        if isinstance(parsed, dict):
            result = parsed
        else:
            sys.stdout.write(last + "\n")
            sys.stdout.flush()
    return RunResult(code=code, tail=list(tail), result=result)


def inner_command(backend: Backend, module: str, argv: list[str], interpreter: Path | None = None) -> list[str]:
    """``<python> -m forge_gen.<module> --inner <argv>``."""
    interpreter = interpreter or resolve_interpreter(backend)
    name = module if module.startswith("forge_gen.") else f"forge_gen.{module}"
    return [str(interpreter), "-m", name, "--inner", *argv]


def run_inner(backend: Backend, module: str, argv: list[str], *, timeout: float | None = None) -> int:
    """Exec the inner half of a command under the backend's env, output streamed through.

    Returns the exit code; the inner module speaks the same codes, so a
    caller that wants the refusal as an exception uses :func:`run_inner_checked`.
    """
    return run_inner_result(backend, module, argv, timeout=timeout).code


def run_inner_result(
    backend: Backend, module: str, argv: list[str], *, hold_json: bool = False, timeout: float | None = None
) -> RunResult:
    """As :func:`run_inner`, returning the tail (and the held JSON line) as well."""
    interpreter = resolve_interpreter(backend)
    env = inner_env(backend, interpreter)
    cwd = inner_cwd(backend)
    command = inner_command(backend, module, argv, interpreter)
    return stream(command, env=env, cwd=cwd, hold_json=hold_json, timeout=timeout)


def run_inner_checked(backend: Backend, module: str, argv: list[str], *, timeout: float | None = None) -> dict:
    """Run the inner half and return its JSON result, relaying any refusal unchanged.

    An inner exit of 3/4/5/6 becomes the matching exception, rebuilt from the
    JSON line the inner printed; any other non-zero exit is a backend
    failure carrying the output tail. A zero exit with no JSON line is a
    success with an empty result.
    """
    outcome = run_inner_result(backend, module, argv, hold_json=True, timeout=timeout)
    if outcome.code == exit_codes.OK:
        return outcome.result or {}
    payload = outcome.result or {}
    payload.setdefault("log_tail", outcome.tail)
    raise exit_codes.from_payload(
        outcome.code,
        payload,
        fallback=f"{backend.name}: forge_gen.{module} exited {outcome.code}",
    )


def blender_bin() -> Path:
    """``$BLENDER_BIN``, else ``blender`` on PATH, else :class:`MissingTool` (exit 6)."""
    override = os.environ.get(BLENDER_ENV)
    if override:
        path = Path(override).expanduser()
        if path.is_file():
            return path
        raise MissingTool(
            f"{BLENDER_ENV}={override} is not a file",
            tool="blender",
            hint=f"point {BLENDER_ENV} at the binary, or put blender on PATH",
        )
    found = shutil.which("blender")
    if found:
        return Path(found)
    raise MissingTool(
        "blender is not on PATH",
        tool="blender",
        hint=f"install Blender >= {BLENDER_MIN[0]}.{BLENDER_MIN[1]} and put it on PATH, or set {BLENDER_ENV}",
    )


def blender_version(binary: Path | None = None, *, timeout: float = 30) -> tuple[int, ...] | None:
    """``(major, minor, patch)`` from ``blender --version``, or ``None`` when it will not say."""
    binary = binary or blender_bin()
    try:
        out = subprocess.run(
            [str(binary), "--version"], capture_output=True, text=True, timeout=timeout, check=False
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in out.splitlines():
        if line.startswith("Blender "):
            words = line.split()[1].split(".")
            try:
                return tuple(int(w) for w in words[:3])
            except ValueError:
                return None
    return None


def blender_command(module_path: str | os.PathLike, argv: list[str], binary: Path | None = None) -> list[str]:
    """``blender --background --factory-startup --python-exit-code 5 --python <module> -- <argv>``."""
    binary = binary or blender_bin()
    return [
        str(binary),
        "--background",
        "--factory-startup",
        "--python-exit-code",
        str(BLENDER_EXCEPTION_EXIT),
        "--python",
        os.fspath(module_path),
        "--",
        *argv,
    ]


def run_blender(module_path: str | os.PathLike, argv: list[str], *, timeout: float | None = None) -> int:
    """Run a Blender module headless, output streamed through; returns Blender's exit code.

    The script's ``sys.exit(code)`` comes back as that code, so a module
    that refuses with 4 is seen refusing with 4; an uncaught exception comes
    back as 5.
    """
    return run_blender_result(module_path, argv, timeout=timeout).code


def run_blender_result(
    module_path: str | os.PathLike, argv: list[str], *, hold_json: bool = False, timeout: float | None = None
) -> RunResult:
    """As :func:`run_blender`, with the tail and the held JSON line."""
    env = dict(os.environ)
    env.setdefault("PYTHONNOUSERSITE", "1")
    return stream(blender_command(module_path, argv), env=env, hold_json=hold_json, timeout=timeout)


def run_blender_checked(module_path: str | os.PathLike, argv: list[str], *, timeout: float | None = None) -> dict:
    """Run a Blender module and return its JSON result, relaying any refusal unchanged."""
    outcome = run_blender_result(module_path, argv, hold_json=True, timeout=timeout)
    if outcome.code == exit_codes.OK:
        return outcome.result or {}
    payload = outcome.result or {}
    payload.setdefault("log_tail", outcome.tail)
    raise exit_codes.from_payload(
        outcome.code, payload, fallback=f"blender exited {outcome.code} running {Path(module_path).name}"
    )
