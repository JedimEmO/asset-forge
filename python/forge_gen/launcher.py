"""Finding a backend's interpreter and running the inner half under it — or Blender.

The outer ``forge_gen`` runs under the system python and must never import
torch; the inner modules run under each backend's own env and may. The
launcher is the seam: it resolves the interpreter (an override, the ``.env``
link, or exit 3 in about 100 ms), builds the environment ``backend.toml``
asks for, and execs ``<python> -m forge_gen.<module> --inner ...`` with this
package on ``PYTHONPATH`` so the inner half is the same source tree.

Resolution order for the interpreter::

    $FORGE_BACKEND_<NAME_UPPER>_PYTHON   a binary or a prefix
    backends/<name>/.wsl-distro           a WSL2 install, native Windows only
    backends/<name>/.env/bin/python      the symlink install.sh wrote
    → MissingBackend (exit 3, with the install line as the hint)

A backend whose CUDA extensions need a Linux host toolchain (trellis2, on
Windows) is installed by running its unmodified install.sh inside a WSL2
distro instead — nothing about install.sh changes for that, and native
Linux/macOS installs are untouched either way. What differs is only how a
native-Windows `forge` reaches it afterwards: install.sh's usual `.env`/
`.checkout` symlinks point at a Linux path a Windows process cannot read, so
`.wsl-distro` (just the distro name) tells the launcher to go through
`wsl.exe -d <distro>` instead, which resolves those same symlinks itself
from inside the distro. See :class:`WslInterpreter`.

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
import re
import shlex
import shutil
import string
import subprocess
import sys
import threading
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

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


@dataclass(frozen=True)
class WslInterpreter:
    """A backend whose env was installed inside WSL2, reached through ``wsl.exe``.

    ``install.sh`` writes ``.env``/``.checkout`` as POSIX symlinks to a Linux
    path; a native Windows process cannot read those directly (they resolve
    to nothing it understands), so this bridges by handing the *repo-relative*
    path to ``wsl.exe`` and letting the distro resolve its own symlinks —
    they are valid there, since it wrote them.
    """

    #: The distro named in ``<backend>/.wsl-distro``.
    distro: str
    #: ``<backend-dir>``, translated to its ``/mnt/<drive>/...`` form.
    backend_dir_wsl: str


def _win_to_wsl_path(path: Path) -> str:
    """``H:\\src\\asset-forge`` -> ``/mnt/h/src/asset-forge`` — the default WSL2 drive mount.

    Only meaningful on Windows; a custom ``automount root`` in the distro's
    ``/etc/wsl.conf`` would need a different prefix, which this does not
    detect.
    """
    resolved = str(path.resolve())
    drive, rest = os.path.splitdrive(resolved)
    if not drive:
        return resolved.replace("\\", "/")
    return f"/mnt/{drive.rstrip(':').lower()}{rest.replace(chr(92), '/')}"


#: A Windows absolute path, e.g. ``H:\src\asset-forge\...`` or ``H:/...``.
_WINDOWS_ABS_PATH = re.compile(r"^[A-Za-z]:[\\/]")


def _translate_argv_paths(argv: list[str]) -> list[str]:
    """``argv``, with every Windows-absolute-looking value mapped to its WSL form.

    The outer launcher runs natively and resolves file arguments (a
    reference image, ``--out``, ``--record``, ...) to absolute Windows paths
    before handing off to the inner half; a bare ``H:\\...`` string means
    nothing inside the distro and is read back as a relative path joined
    onto whatever the inner process's cwd happens to be — the exact bug this
    fixes (a lift refusing to find its own reference image). Only values
    that already look like a Windows absolute path are touched; flags,
    presets and relative paths pass through unchanged.
    """
    return [_win_to_wsl_path(Path(arg)) if _WINDOWS_ABS_PATH.match(arg) else arg for arg in argv]


def _wsl_interpreter(backend: Backend) -> WslInterpreter | None:
    """``.wsl-distro``, when this is a native-Windows process and it exists."""
    if os.name != "nt":
        return None
    marker = backend.wsl_marker
    if not marker.is_file():
        return None
    distro = marker.read_text(encoding="utf-8").strip()
    if not distro:
        return None
    return WslInterpreter(distro=distro, backend_dir_wsl=_win_to_wsl_path(backend.dir))


def resolve_interpreter(backend: Backend) -> Path | WslInterpreter:
    """The interpreter that runs this backend's inner half, or :class:`MissingBackend`.

    The override may name the binary or its prefix. The ``.env`` link is
    always a prefix; a link with no ``bin/python`` under it is an install
    that did not finish, and says so. A WSL2 install (see
    :class:`WslInterpreter`) is checked before the native ``.env`` link,
    since that link is unreadable from native Windows anyway.
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
    wsl = _wsl_interpreter(backend)
    if wsl is not None:
        return wsl
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


def prefix_of(backend: Backend, interpreter: Path | WslInterpreter | None = None) -> Path | PurePosixPath:
    """The env prefix (``${PREFIX}`` in ``[env]``): the directory above ``bin/``."""
    interpreter = interpreter or resolve_interpreter(backend)
    if isinstance(interpreter, WslInterpreter):
        return PurePosixPath(f"{interpreter.backend_dir_wsl}/.env")
    resolved = interpreter.resolve()
    if resolved.parent.name == "bin":
        return resolved.parent.parent
    return resolved.parent


def _expansion_mapping(backend: Backend, interpreter: Path | WslInterpreter) -> dict[str, str]:
    """What ``${...}`` in an ``[env]`` value may name: the placeholders, plus the ambient environment.

    For a :class:`WslInterpreter`, every placeholder is the WSL-side symlink
    itself (``.env``, ``.checkout``, ...) rather than its resolved target —
    the distro resolves those; a native Windows process cannot.
    """
    mapping = dict(os.environ)
    if isinstance(interpreter, WslInterpreter):
        base = interpreter.backend_dir_wsl
        mapping.update(
            {
                "PREFIX": f"{base}/.env",
                "CHECKOUT": f"{base}/.checkout",
                "BACKEND_DIR": base,
                "TEXT_ENCODERS": f"{base}/.text-encoders",
                "CHECKPOINTS": f"{base}/.checkpoints",
            }
        )
        return mapping
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


def inner_env(backend: Backend, interpreter: Path | WslInterpreter | None = None) -> dict[str, str]:
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
    interpreter = interpreter or resolve_interpreter(backend)
    if isinstance(interpreter, WslInterpreter):
        # The ambient Windows PYTHONPATH (if any) is semicolon-separated
        # Windows paths, meaningless to the Linux side — dropped rather than
        # joined onto the translated toolkit path.
        env["PYTHONPATH"] = _win_to_wsl_path(python_dir())
    else:
        python_path = str(python_dir())
        existing = env.get("PYTHONPATH")
        env["PYTHONPATH"] = python_path if not existing else os.pathsep.join([python_path, existing])
    mapping = _expansion_mapping(backend, interpreter)
    env.setdefault("PYTHONNOUSERSITE", "1")
    env.setdefault("FORGE_BACKEND", backend.name)
    for key, value in backend.env.items():
        env.setdefault(key, string.Template(value).safe_substitute(mapping))
    for key, value in backend.env_force.items():
        env[key] = string.Template(value).safe_substitute(mapping)
    return env


def env_shadowing(backend: Backend, interpreter: Path | WslInterpreter | None = None) -> list[tuple[str, str, str]]:
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


def external_command(
    interpreter: Path | WslInterpreter,
    argv: list[str],
    *,
    env: dict[str, str] | None = None,
    wsl_cwd: str | None = None,
) -> list[str]:
    """``<interpreter> argv...``, wrapped through ``wsl.exe -d <distro>`` for a :class:`WslInterpreter`.

    ``wsl.exe`` does not forward the Windows environment into the distro, so
    ``env`` — when given — is threaded through explicitly as
    ``env KEY=VALUE ...`` inside it; ``wsl_cwd``, a WSL-side POSIX directory,
    becomes a ``cd`` wrapped around the same command (``env --chdir`` is not
    old enough to assume every distro has it).
    """
    if isinstance(interpreter, WslInterpreter):
        python = f"{interpreter.backend_dir_wsl}/.env/bin/python"
        assignments = [f"{key}={value}" for key, value in (env or {}).items()]
        inner = [python, *argv]
        if wsl_cwd:
            shell = "cd " + shlex.quote(wsl_cwd) + " && exec env " + " ".join(shlex.quote(a) for a in [*assignments, *inner])
            return ["wsl.exe", "-d", interpreter.distro, "--", "sh", "-c", shell]
        return ["wsl.exe", "-d", interpreter.distro, "--", "env", *assignments, *inner]
    return [str(interpreter), *argv]


def inner_command(
    backend: Backend,
    module: str,
    argv: list[str],
    interpreter: Path | WslInterpreter | None = None,
    *,
    env: dict[str, str] | None = None,
) -> list[str]:
    """``<python> -m forge_gen.<module> --inner <argv>`` via :func:`external_command`.

    The env forwarded into a WSL child is restricted to what the inner
    process actually needs: ``PYTHONPATH``, ``PYTHONNOUSERSITE``,
    ``FORGE_BACKEND``, and every key ``backend.toml``'s ``[env]``/``[env.force]``
    declares — not the rest of the Windows environment. ``argv`` itself is
    translated the same way (see :func:`_translate_argv_paths`): the outer
    half resolves file arguments to absolute Windows paths before this is
    ever called, and those mean nothing inside the distro.
    """
    interpreter = interpreter or resolve_interpreter(backend)
    name = module if module.startswith("forge_gen.") else f"forge_gen.{module}"
    if isinstance(interpreter, WslInterpreter):
        argv = _translate_argv_paths(argv)
    inner_argv = ["-m", name, "--inner", *argv]
    if isinstance(interpreter, WslInterpreter):
        wanted = ["PYTHONPATH", "PYTHONNOUSERSITE", "FORGE_BACKEND", *backend.env, *backend.env_force]
        restricted = {key: env[key] for key in dict.fromkeys(wanted) if env and key in env}
        wsl_cwd = f"{interpreter.backend_dir_wsl}/.checkout" if backend.cwd == "checkout" else None
        return external_command(interpreter, inner_argv, env=restricted, wsl_cwd=wsl_cwd)
    return external_command(interpreter, inner_argv)


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
    if isinstance(interpreter, WslInterpreter):
        # Everything the inner process needs is threaded through the
        # command line itself (see inner_command); inner_cwd's existence
        # check is native-only and unreliable against a WSL-written symlink.
        command = inner_command(backend, module, argv, interpreter, env=env)
        return stream(command, env=None, cwd=None, hold_json=hold_json, timeout=timeout)
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
