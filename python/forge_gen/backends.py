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

#: Where a model's weights live, by ``store``. Beside these three fixed
#: words there is one prefix form, :data:`COMFY_STORE`: ``comfy:models/tts``
#: names a folder inside the ComfyUI host's model tree.
STORES = ("hf", "checkpoints_dir", "text_encoders_dir")

#: The prefix a comfy store carries. What follows is a relative path with no
#: ``..`` and no leading ``/``, resolved under the host backend's
#: ``$PREFIX/<base_directory>/`` — which is what ``extra_model_paths.yaml``
#: already points at, so doctor has one model list rather than two that can
#: disagree.
COMFY_STORE = "comfy:"

#: The environment variable that names the ComfyUI host's data directory
#: outright, for an install that is somewhere this module cannot derive.
COMFY_DATA_ENV = "FORGE_COMFY_DATA"

#: How a backend is run. ``env`` execs the inner half under the backend's
#: own interpreter; ``comfy`` posts a graph to the ComfyUI host named by
#: ``host``; ``tool`` is a host program (Blender) or the host service
#: itself. Mirrors ``forge_library::backends::ExecutorKind``.
EXECUTORS = ("env", "comfy", "tool")

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

    #: ``org/name`` for ``hf`` and for a comfy store (the repo the file comes
    #: from); a subdirectory name for the two directory stores.
    id: str
    #: One of :data:`STORES`, or ``comfy:<relative path>``.
    store: str
    #: The weights' licence, for the table and the notices.
    license: str | None = None
    #: Whether the hub gates it behind a click-through.
    gated: bool = False
    #: Where to click through, when gated.
    accept_url: str | None = None
    #: Why it is needed, one line.
    note: str | None = None
    #: The file inside the repo, for a store that holds files rather than
    #: whole snapshots. ``None`` for an ``hf`` snapshot.
    file: str | None = None
    #: The name it takes on disk, when that differs from the file's basename.
    local: str | None = None
    #: The hub revision pinned, when one is.
    revision: str | None = None
    #: The download in GB, for ``forge setup``'s screen before a byte moves.
    gb: float | None = None
    #: The node class that lists this file, when it is not the folder's usual
    #: one (a ``.gguf`` is listed by ``UnetLoaderGGUF``, not ``UNETLoader``).
    node: str | None = None
    #: The input field on that node.
    field: str | None = None

    @property
    def is_comfy(self) -> bool:
        """Whether the weights live in the ComfyUI host's model tree."""
        return self.store.startswith(COMFY_STORE)

    @property
    def comfy_path(self) -> str | None:
        """The path under the host's base directory: ``models/tts`` for ``comfy:models/tts``."""
        return self.store[len(COMFY_STORE) :] if self.is_comfy else None

    @property
    def comfy_folder(self) -> str | None:
        """The ComfyUI model folder name — ``tts`` for ``comfy:models/tts``."""
        path = self.comfy_path
        return path.split("/")[-1] if path else None

    @property
    def filename(self) -> str | None:
        """What the file is called on disk, when the row names one."""
        if self.local:
            return self.local
        return self.file.rsplit("/", 1)[-1] if self.file else None


@dataclass
class ComfyPack:
    """One custom node pack the ComfyUI host carries.

    A pack lives in four places or nowhere — here, in ``install.sh``, in
    ``snapshot.json`` and in ``designs/hosting.md``'s pins row
    (hosting.md, 2026-08-30). This is the first of the four, and
    ``probe.py`` holds the clone to :attr:`commit`.
    """

    repo: str
    commit: str
    #: The directory name under ``$PREFIX/<base_directory>/custom_nodes/``.
    dir: str
    license: str | None = None
    #: What the pack needs in the host's venv beyond ComfyUI's own requirements.
    pips: list[str] = field(default_factory=list)
    #: The node classes it contributes, captured from ``GET /object_info``.
    nodes: list[str] = field(default_factory=list)
    note: str | None = None


@dataclass
class ComfySpec:
    """The ``[comfy]`` table: what a backend needs of the host to run.

    ``nodes`` are class names **captured from the running host's
    ``/object_info``**, never written from memory: doctor turns a class the
    host does not have into a ``partial`` row naming the pack, so a template
    that cannot run is a doctor line and not a ``POST /prompt`` failure in
    front of a stranger.
    """

    #: The tracked API-format graphs, under ``backends/<name>/workflows/``.
    workflows: list[str] = field(default_factory=list)
    #: Every node class the workflows use.
    nodes: list[str] = field(default_factory=list)
    #: The node that unloads the model at the end of a graph, for a wrapper
    #: pack that loads outside ComfyUI's memory manager. ``None`` for native
    #: nodes, which honour ``POST /free``.
    unload_node: str | None = None
    #: The packs this backend's workflows need.
    packs: list[ComfyPack] = field(default_factory=list)
    #: Everything else the host's own table says (manager package, snapshot,
    #: base directory, frontend) — read by ``install.sh`` and ``probe.py``.
    extra: dict = field(default_factory=dict)


@dataclass
class Backend:
    """One ``backend.toml``, parsed and checked."""

    name: str
    dir: Path
    role: str
    upstream: str
    #: The generator's own upstream commit. ``None`` for a comfy backend with
    #: no checkout of its own — the version that ran is the host's, and
    #: writing it twice under two names is the four-places-or-nowhere trap in
    #: miniature.
    commit: str | None
    license: str
    #: One of :data:`EXECUTORS`. Stated, or derived from ``env_kind``.
    executor: str
    #: ``""`` for a backend with no environment (comfy, tool).
    env_kind: str
    python: str
    cuda: str | None
    vram_gb: float | None
    entry: str
    cwd: str
    resident: bool
    #: Which ``backends/<name>`` is the service, for ``executor = "comfy"``.
    host: str | None = None
    #: The ``[comfy]`` table, when there is one.
    comfy: ComfySpec | None = None
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
        """A host program or service described beside the generators (Blender, ComfyUI): no env of its own to exec."""
        return self.executor == "tool"

    @property
    def is_comfy(self) -> bool:
        """Whether this backend runs as a graph on the ComfyUI host."""
        return self.executor == "comfy"

    @property
    def workflows(self) -> Path:
        """Where this backend's tracked API-format graphs live."""
        return self.dir / "workflows"

    def workflow(self, name: str) -> Path:
        """One tracked template by the name ``[comfy] workflows`` lists it under."""
        return self.workflows / name

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
        if self.is_comfy:
            # Nothing is installed under this directory: what it needs is the
            # host, its packs and its weights, and one script does all three.
            return f"bash {self.dir.parent / (self.host or 'comfy') / 'install.sh'}  — {self.name} runs on the {self.host} host"
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


def _executor(data: dict, name: str) -> tuple[str, str]:
    """``(executor, env_kind)`` — the second form's rule, and the derive that keeps the first form working.

    ``executor`` present wins, and ``env_kind`` is required only for
    ``"env"``. ``executor`` absent is derived from ``env_kind``
    (``"none"`` → ``tool``, anything else → ``env``), so every
    ``backend.toml`` written before this keeps working unedited. Both
    present and disagreeing is a refusal naming both words: a file that
    says two things about how it is run is one nobody can act on.
    """
    stated = _string(data, "executor", name, required=False)
    kind = data.get("env_kind")
    if kind is not None and not isinstance(kind, str):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: env_kind must be a string", backend=name)
    if kind is not None and kind not in ENV_KINDS:
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: env_kind {kind!r} is not one of {', '.join(ENV_KINDS)}", backend=name)
    if stated is None:
        if kind is None:
            raise BackendConfigError(f"{name}/{BACKEND_FILE} has no 'executor'", backend=name)
        return ("tool" if kind == "none" else "env"), kind
    if stated not in EXECUTORS:
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: executor {stated!r} is not one of {', '.join(EXECUTORS)}", backend=name)
    if kind is not None:
        derived = "tool" if kind == "none" else "env"
        if derived != stated:
            raise BackendConfigError(
                f"{name}/{BACKEND_FILE}: executor {stated!r} and env_kind {kind!r} disagree "
                f"(env_kind {kind!r} reads as executor {derived!r}) — say it once",
                backend=name,
            )
    elif stated == "env":
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: executor \"env\" needs an env_kind", backend=name)
    if stated == "comfy":
        # A comfy backend has no interpreter, and saying it has one is a
        # half-truth doctor should refuse at parse time rather than resolve
        # into a MissingBackend the day someone installs the host. `entry`
        # is *not* part of this rule: it is a module path relative to
        # forge_gen and the module runs in the outer process.
        for key in ("env", "python"):
            if data.get(key):
                what = "an [env] table" if key == "env" else "a python key"
                raise BackendConfigError(
                    f"{name}/{BACKEND_FILE}: executor \"comfy\" runs inside the host and has no interpreter, "
                    f"but the file carries {what}",
                    backend=name,
                )
    return stated, (kind or "")


def _store(value: str, model_id: str, name: str) -> str:
    """One ``store`` word, or the ``comfy:`` prefix form, validated."""
    if value.startswith(COMFY_STORE):
        relative = value[len(COMFY_STORE) :]
        bad = (
            not relative
            or relative.startswith("/")
            or ".." in relative.split("/")
            or relative.endswith("/")
        )
        if bad:
            raise BackendConfigError(
                f"{name}/{BACKEND_FILE}: model {model_id!r} store {value!r} must be {COMFY_STORE} plus a "
                "relative path inside the host's base directory (no leading /, no ..)",
                backend=name,
            )
        return value
    if value not in STORES:
        raise BackendConfigError(
            f"{name}/{BACKEND_FILE}: model {model_id!r} store {value!r} is not one of "
            f"{', '.join(STORES)} or {COMFY_STORE}<path>",
            backend=name,
        )
    return value


def _comfy(data: dict, name: str) -> ComfySpec | None:
    """The ``[comfy]`` table, when there is one."""
    table = data.get("comfy")
    if table is None:
        return None
    if not isinstance(table, dict):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: [comfy] must be a table", backend=name)

    def _strings(key: str) -> list[str]:
        items = table.get(key, []) or []
        if not isinstance(items, list) or not all(isinstance(i, str) for i in items):
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: [comfy] {key} must be a list of strings", backend=name)
        return list(items)

    packs: list[ComfyPack] = []
    for index, item in enumerate(table.get("packs", []) or []):
        if not isinstance(item, dict):
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: [[comfy.packs]] #{index} is not a table", backend=name)
        for key in ("repo", "commit", "dir"):
            if not item.get(key):
                raise BackendConfigError(f"{name}/{BACKEND_FILE}: [[comfy.packs]] #{index} has no {key!r}", backend=name)
        packs.append(
            ComfyPack(
                repo=str(item["repo"]),
                commit=str(item["commit"]),
                dir=str(item["dir"]),
                license=item.get("license"),
                pips=[str(p) for p in (item.get("pips") or [])],
                nodes=[str(n) for n in (item.get("nodes") or [])],
                note=item.get("note"),
            )
        )
    unload = table.get("unload_node")
    if unload is not None and not isinstance(unload, str):
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: [comfy] unload_node must be a string or absent", backend=name)
    known = {"workflows", "nodes", "unload_node", "packs"}
    return ComfySpec(
        workflows=_strings("workflows"),
        nodes=_strings("nodes"),
        unload_node=unload,
        packs=packs,
        extra={key: value for key, value in table.items() if key not in known},
    )


def parse_backend(data: dict, name: str, directory: Path) -> Backend:
    """Turn a parsed ``backend.toml`` into a :class:`Backend`, refusing what does not fit.

    The checks are the ones whose failure would otherwise surface three
    steps later with a worse message: the file calls itself by its
    directory's name, ``executor``/``env_kind``/``cwd``/``store`` are words
    the launcher knows and agree with each other, the pinned commit looks
    like one, every ``[env]`` value is text.
    """
    declared = _string(data, "name", name)
    if declared != name:
        raise BackendConfigError(f"{name}/{BACKEND_FILE} calls itself {declared!r} but lives in {name}/", backend=name)
    executor, env_kind = _executor(data, name)
    cwd = _string(data, "cwd", name, required=False, default="none")
    if cwd not in CWDS:
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: cwd {cwd!r} is not one of {', '.join(CWDS)}", backend=name)
    # A tool backend is a binary release, not a pinned checkout: "none" is
    # the honest value and its probe reports the build hash instead. A comfy
    # backend need not name a commit at all — it has no checkout of its own,
    # and the version that ran is the host's.
    commit = _string(data, "commit", name, required=executor != "comfy")
    if commit is not None:
        is_hash = 7 <= len(commit) <= 40 and all(c in "0123456789abcdef" for c in commit.lower())
        if not is_hash and not (executor == "tool" and commit == "none"):
            raise BackendConfigError(f"{name}/{BACKEND_FILE}: commit {commit!r} is not a git hash", backend=name)
    entry = _string(data, "entry", name)
    if not entry.replace("_", "").replace(".", "").isalnum():
        raise BackendConfigError(f"{name}/{BACKEND_FILE}: entry {entry!r} is not a module name", backend=name)
    host = _string(data, "host", name, required=False)
    if executor == "comfy" and not host:
        raise BackendConfigError(
            f"{name}/{BACKEND_FILE}: executor \"comfy\" must name the host backend it runs on (host = \"comfy\")",
            backend=name,
        )

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
        store = _store(str(item.get("store", "hf")), str(item["id"]), name)
        gb = item.get("gb")
        models.append(
            Model(
                id=str(item["id"]),
                store=store,
                license=item.get("license"),
                gated=bool(item.get("gated", False)),
                accept_url=item.get("accept_url") or None,
                note=item.get("note"),
                file=item.get("file") or None,
                local=item.get("local") or None,
                revision=item.get("revision") or None,
                gb=None if gb is None else float(gb),
                node=item.get("node") or None,
                field=item.get("field") or None,
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
        "name", "role", "upstream", "commit", "license", "executor", "env_kind", "python", "cuda", "vram_gb",
        "entry", "cwd", "resident", "host", "comfy", "env", "models", "notices", "server",
    }
    return Backend(
        name=name,
        dir=directory,
        role=_string(data, "role", name, required=False, default="") or "",
        upstream=_string(data, "upstream", name),
        commit=commit,
        license=_string(data, "license", name, required=False, default="") or "",
        executor=executor,
        env_kind=env_kind,
        python=str(data.get("python", "")),
        cuda=None if data.get("cuda") in (None, "") else str(data["cuda"]),
        vram_gb=None if vram is None else float(vram),
        entry=entry,
        cwd=cwd,
        resident=bool(data.get("resident", False)),
        host=host,
        comfy=_comfy(data, name),
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


def comfy_data_dir(host: Backend) -> Path:
    """The host's ``--base-directory``: where ``models/``, ``input/`` and ``custom_nodes/`` live.

    In order: ``$FORGE_COMFY_DATA``; the ``data_dir`` the installer wrote
    into its receipt; ``<the .env link's prefix>/../<base_directory>``,
    which is the default layout (``$PREFIX/venv`` beside ``$PREFIX/data``);
    else the backend directory's own, which is where a test tree puts it.
    Never guessed silently into a check — a caller that needs the directory
    to exist looks.
    """
    override = os.environ.get(COMFY_DATA_ENV)
    if override:
        return Path(override).expanduser()
    receipt = host.installed() or {}
    if receipt.get("data_dir"):
        return Path(str(receipt["data_dir"])).expanduser()
    base = str((host.comfy.extra.get("base_directory") if host.comfy else None) or "data")
    link = host.env_link
    if link.exists():
        # $PREFIX/venv/bin/python's prefix is $PREFIX/venv; the data tree is
        # its sibling, because that is the layout install.sh lays down.
        return link.resolve().parent / base
    return host.dir / base


def comfy_model_path(model: Model, host: Backend) -> Path | None:
    """Where a ``comfy:`` model's file sits on disk, or ``None`` for another store.

    ``comfy:models/tts`` + ``file = "…/x.safetensors"`` resolves to
    ``<data>/models/tts/x.safetensors``. A row with no ``file`` names a
    directory, and that is what comes back.
    """
    relative = model.comfy_path
    if relative is None:
        return None
    folder = comfy_data_dir(host) / relative
    name = model.filename
    return folder / name if name else folder


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
