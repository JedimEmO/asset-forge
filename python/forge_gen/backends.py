"""Where the generator backends are, and what each ``backend.toml`` says.

A backend is a directory under ``backends/`` — ``trellis2``, ``ardy``,
``acestep``, ``moss_sfx``, ``moss_tts`` — holding ``backend.toml`` (what it
is: upstream, pinned commit, licence, interpreter kind, entry module, the
environment the launcher exports, the weights it needs), ``install.sh``
(what sets it up) and ``probe.py`` (what doctor runs inside the env). Once
installed it also holds gitignored symlinks — ``.env`` to the interpreter
prefix, ``.checkout`` to the upstream clone, optionally ``.text-encoders``
and ``.checkpoints`` — and ``installed.json``, the receipt.

This module reads the directory. It runs nothing: resolving an interpreter
is ``launcher.py``'s job, and whether the env works is ``doctor.py``'s.
No absolute path lives in source; the directory comes from
``$FORGE_BACKENDS`` or from where this package is checked out.
"""

from __future__ import annotations

import json
import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from forge_gen.exit_codes import MissingBackend

#: The backends the toolkit knows, in the order doctor lists them. Mirrors
#: ``forge_library::backends::KNOWN``.
KNOWN = ("trellis2", "ardy", "acestep", "moss_sfx", "moss_tts")

#: The environment variable naming the backends directory.
BACKENDS_ENV = "FORGE_BACKENDS"

#: The file that describes a backend.
BACKEND_FILE = "backend.toml"

#: The symlinks ``install.sh`` leaves.
ENV_LINK = ".env"
CHECKOUT_LINK = ".checkout"
TEXT_ENCODERS_LINK = ".text-encoders"
CHECKPOINTS_LINK = ".checkpoints"

#: The receipt ``install.sh`` writes.
INSTALLED_FILE = "installed.json"

#: Where a model's weights live, by ``store``.
STORES = ("hf", "checkpoints_dir", "text_encoders_dir")

#: The interpreter kinds an installer can make — and ``none`` for a tool
#: backend (Blender), which has no environment: the launcher finds its
#: binary on the host and doctor runs its probe under the system python.
ENV_KINDS = ("conda", "venv", "none")

#: What ``cwd`` may say: run the inner module from the upstream checkout, or from wherever the caller stands.
CWDS = ("checkout", "none")


class BackendConfigError(MissingBackend):
    """``backend.toml`` is present but does not say what it must.

    Exit 3 like a missing backend — generation through it is off either
    way — with the defect named so the fix is a line in the file, not a
    reinstall.
    """

    error = "broken_backend"


@dataclass
class Model:
    """One set of weights a backend needs."""

    #: ``org/name`` for ``hf``; a subdirectory name for the two directory stores.
    id: str
    #: One of :data:`STORES`.
    store: str
    #: The weights' licence, for the table and the notices.
    license: str | None = None
    #: Whether the hub gates it behind a click-through.
    gated: bool = False
    #: Where to click through, when gated.
    accept_url: str | None = None
    #: Why it is needed, one line.
    note: str | None = None


@dataclass
class Backend:
    """One ``backend.toml``, parsed and checked."""

    name: str
    dir: Path
    role: str
    upstream: str
    commit: str
    license: str
    env_kind: str
    python: str
    cuda: str | None
    vram_gb: float | None
    entry: str
    cwd: str
    resident: bool
    env: dict[str, str] = field(default_factory=dict)
    #: ``[env.force]``: entries the launcher sets unconditionally — the
    #: ambient shell must not be able to shadow them (setdefault covers the
    #: rest of ``[env]``).
    env_force: dict[str, str] = field(default_factory=dict)
    models: list[Model] = field(default_factory=list)
    notices: list[str] = field(default_factory=list)
    server: dict | None = None
    #: The rest of the file, for a backend that needs a knob this module does not name.
    extra: dict = field(default_factory=dict)

    @property
    def env_link(self) -> Path:
        """The ``.env`` symlink to the interpreter prefix."""
        return self.dir / ENV_LINK

    @property
    def checkout(self) -> Path:
        """The ``.checkout`` symlink to the upstream clone."""
        return self.dir / CHECKOUT_LINK

    @property
    def text_encoders(self) -> Path:
        """The ``.text-encoders`` symlink (ARDY)."""
        return self.dir / TEXT_ENCODERS_LINK

    @property
    def checkpoints(self) -> Path:
        """The ``.checkpoints`` symlink (ACE-Step)."""
        return self.dir / CHECKPOINTS_LINK

    @property
    def is_tool(self) -> bool:
        """A host tool described beside the generators (Blender): no env, no install, no weights."""
        return self.env_kind == "none"

    @property
    def probe(self) -> Path:
        """The in-env probe doctor runs."""
        return self.dir / "probe.py"

    @property
    def install_script(self) -> Path:
        """What installs it."""
        return self.dir / "install.sh"

    def installed(self) -> dict | None:
        """The receipt ``install.sh`` wrote, or ``None``."""
        path = self.dir / INSTALLED_FILE
        if not path.is_file():
            return None
        try:
            with open(path, encoding="utf-8") as handle:
                return json.load(handle)
        except (OSError, json.JSONDecodeError):
            return None

    def install_hint(self) -> str:
        """The command that installs it — the script's resolved path, so the line works from any cwd."""
        if self.is_tool:
            return f"install {self.name} and put it on PATH, or set {self.extra.get('bin_env', self.name.upper() + '_BIN')}"
        return f"bash {self.install_script}  (or --adopt-env <prefix> --adopt-checkout <clone>)"


def backends_dir() -> Path:
    """``$FORGE_BACKENDS`` when set, else ``<repo>/backends`` beside this package."""
    override = os.environ.get(BACKENDS_ENV)
    if override:
        return Path(override).expanduser().resolve()
    return Path(__file__).resolve().parent.parent.parent / "backends"


def _string(data: dict, key: str, name: str, *, required: bool = True, default: str | None = None) -> str | None:
    value = data.get(key, default)
    if value is None:
        if required:
            raise BackendConfigError(f"{name}/{BACKEND_FILE} has no {key!r}", backend=name)
        return None
    if not isinstance(value, str):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: {key!r} must be a string", backend=name)
    return value


def parse_backend(data: dict, name: str, directory: Path) -> Backend:
    """Turn a parsed ``backend.toml`` into a :class:`Backend`, refusing what does not fit.

    The checks are the ones whose failure would otherwise surface three
    steps later with a worse message: the file calls itself by its
    directory's name, ``env_kind``/``cwd``/``store`` are words the launcher
    knows, the pinned commit looks like one, every ``[env]`` value is text.
    """
    declared = _string(data, "name", name)
    if declared != name:
        raise BackendConfigError(f"{name}/{BACKEND_FILE} calls itself {declared!r} but lives in {name}/", backend=name)
    env_kind = _string(data, "env_kind", name)
    if env_kind not in ENV_KINDS:
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: env_kind {env_kind!r} is not one of {', '.join(ENV_KINDS)}", backend=name)
    cwd = _string(data, "cwd", name, required=False, default="none")
    if cwd not in CWDS:
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: cwd {cwd!r} is not one of {', '.join(CWDS)}", backend=name)
    commit = _string(data, "commit", name)
    # A tool backend is a binary release, not a pinned checkout: "none" is
    # the honest value and its probe reports the build hash instead.
    is_hash = 7 <= len(commit) <= 40 and all(c in "0123456789abcdef" for c in commit.lower())
    if not is_hash and not (env_kind == "none" and commit == "none"):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: commit {commit!r} is not a git hash", backend=name)
    entry = _string(data, "entry", name)
    if not entry.replace("_", "").replace(".", "").isalnum():
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: entry {entry!r} is not a module name", backend=name)

    env_table = data.get("env", {})
    if not isinstance(env_table, dict):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: [env] must be a table", backend=name)

    def _env_entries(table: dict, section: str) -> dict[str, str]:
        out: dict[str, str] = {}
        for key, value in table.items():
            if isinstance(value, bool):
                value = "1" if value else "0"
            elif isinstance(value, (int, float)):
                value = str(value)
            if not isinstance(value, str):
                raise BackendConfigError(f"{name}/{BACKEND_FILE}: [{section}] {key} must be text", backend=name)
            out[str(key)] = value
        return out

    # [env.force]: values the launcher exports unconditionally; the rest of
    # [env] is setdefault, so the shell may override it (and doctor warns).
    force_table = env_table.pop("force", {})
    if not isinstance(force_table, dict):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: [env.force] must be a table", backend=name)
    env = _env_entries(env_table, "env")
    env_force = _env_entries(force_table, "env.force")
    overlap = sorted(set(env) & set(env_force))
    if overlap:
        raise BackendConfigError(
            f"{name}/{BACKEND_FILE}: {', '.join(overlap)} in both [env] and [env.force] — say once whether the shell may override it",
            backend=name,
        )

    models: list[Model] = []
    for index, item in enumerate(data.get("models", []) or []):
        if not isinstance(item, dict) or "id" not in item:
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: [[models]] #{index} has no id", backend=name)
        store = str(item.get("store", "hf"))
        if store not in STORES:
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: model {item['id']!r} store {store!r} is not one of {', '.join(STORES)}", backend=name)
        models.append(
            Model(
                id=str(item["id"]),
                store=store,
                license=item.get("license"),
                gated=bool(item.get("gated", False)),
                accept_url=item.get("accept_url") or None,
                note=item.get("note"),
            )
        )

    notices: list[str] = []
    for item in data.get("notices", []) or []:
        if isinstance(item, str):
            notices.append(item)
        elif isinstance(item, dict) and "text" in item:
            title = item.get("title")
            notices.append(f"{title}: {item['text']}" if title else str(item["text"]))
        else:
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: a notice must be text or a table with text", backend=name)

    server = data.get("server")
    if server is not None and not isinstance(server, dict):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: [server] must be a table", backend=name)

    vram = data.get("vram_gb")
    known = {
        "name", "role", "upstream", "commit", "license", "env_kind", "python", "cuda", "vram_gb",
        "entry", "cwd", "resident", "env", "models", "notices", "server",
    }
    return Backend(
        name=name,
        dir=directory,
        role=_string(data, "role", name, required=False, default="") or "",
        upstream=_string(data, "upstream", name),
        commit=commit,
        license=_string(data, "license", name, required=False, default="") or "",
        env_kind=env_kind,
        python=str(data.get("python", "")),
        cuda=None if data.get("cuda") in (None, "") else str(data["cuda"]),
        vram_gb=None if vram is None else float(vram),
        entry=entry,
        cwd=cwd,
        resident=bool(data.get("resident", False)),
        env=env,
        env_force=env_force,
        models=models,
        notices=notices,
        server=dict(server) if server else None,
        extra={key: value for key, value in data.items() if key not in known},
    )


def load_backend(name: str, root: str | os.PathLike | None = None) -> Backend:
    """Read ``backends/<name>/backend.toml``.

    Raises :class:`MissingBackend` when the directory or file is absent and
    :class:`BackendConfigError` when the file does not parse or does not say
    what it must — both exit 3, both before anything is run.
    """
    base = Path(root).expanduser().resolve() if root else backends_dir()
    directory = base / name
    path = directory / BACKEND_FILE
    if not base.is_dir():
        raise MissingBackend(f"no backends directory at {base}", backend=name, hint=f"set {BACKENDS_ENV} or run from a toolkit checkout")
    if not path.is_file():
        if name in KNOWN:
            raise MissingBackend(
                f"{name} is not described here: no {directory}/{BACKEND_FILE}",
                backend=name,
                hint=f"this checkout lacks backends/{name}/ — a known backend; restore it from the toolkit",
            )
        raise MissingBackend(
            f"{name} is not a backend this toolkit knows: no {directory}/{BACKEND_FILE}",
            backend=name,
            hint=f"known backends: {', '.join(KNOWN)}",
        )
    try:
        with open(path, "rb") as handle:
            data = tomllib.load(handle)
    except tomllib.TOMLDecodeError as err:
        raise BackendConfigError(f"{name}/{BACKEND_FILE} does not parse: {err}", backend=name) from err
    except OSError as err:
        raise BackendConfigError(f"{name}/{BACKEND_FILE} cannot be read: {err}", backend=name) from err
    return parse_backend(data, name, directory)


def list_names(root: str | os.PathLike | None = None) -> list[str]:
    """Every backend name in doctor's order: :data:`KNOWN` first, then any other described directory, sorted."""
    base = Path(root).expanduser().resolve() if root else backends_dir()
    names = list(KNOWN)
    if base.is_dir():
        extra = sorted(
            entry.name
            for entry in base.iterdir()
            if entry.is_dir() and (entry / BACKEND_FILE).is_file() and entry.name not in KNOWN
        )
        names.extend(extra)
    return names


def all_backends(root: str | os.PathLike | None = None) -> dict[str, Backend | MissingBackend]:
    """Every backend by name, parsed — or the :class:`MissingBackend` that explains why not."""
    out: dict[str, Backend | MissingBackend] = {}
    for name in list_names(root):
        try:
            out[name] = load_backend(name, root)
        except MissingBackend as err:
            out[name] = err
    return out
