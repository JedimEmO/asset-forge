"""``forge-gen doctor``: is every backend, tool and model where it should be?

Per backend, in the order a fresh clone would hit the problems: the
``backend.toml`` parses → the upstream checkout is at the pinned commit
(warn on a mismatch, note when dirty) → the env's interpreter resolves and
is the declared version → the in-env ``probe.py`` imports what the inner
module will import and says what torch sees → the weights are on disk (HF
cache, checkpoints dir, text encoders; a gated model that is absent gets the
accept URL and the exact login line) → the backend's notices, printed as
warnings, because a licence fact is not a detail.

Then the host: the GPU and who is holding it (``GPU busy: pid … 8.1 GB``
when more than 2 GB is in use — the generators do not share 24 GB), Blender
(≥ 4.2), ffmpeg, conda, python3.

Statuses: ``ok`` everything answered; ``partial`` the env runs but a weight,
an import or CUDA is missing; ``missing`` not installed; ``broken`` present
but unusable (bad toml, no probe, probe fails, checkout gone). Exit 0 when
every backend asked about is ok, 1 when any is not, 3 when there is no
backends directory at all. ``--json`` is what the Rust ``forge doctor``
aggregates.

The probes are each backend's own. This module runs them and never imports
torch itself.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen import exit_codes, launcher
from forge_gen.backends import Backend
from forge_gen.exit_codes import MissingBackend, MissingTool

#: The doctor JSON schema this build writes.
SCHEMA = 1

#: Above this much VRAM in use, somebody is holding the card.
GPU_BUSY_MB = 2048

#: How long a probe may take. Importing torch cold on a slow disk is seconds; a minute is a hang.
PROBE_TIMEOUT_S = 60.0

#: How long a host tool may take to say its version.
TOOL_TIMEOUT_S = 20.0

#: How long ``GET /object_info`` may take. It is the whole node surface —
#: megabytes on a host with packs — and a cold service answers it slowly.
OBJECT_INFO_TIMEOUT_S = 120.0

#: Where the ComfyUI service answers when nobody says otherwise. Mirrors
#: ``forge_library::project::DEFAULT_COMFY_URL``.
DEFAULT_COMFY_URL = "http://127.0.0.1:8188"

#: The environment variable naming where the backends install themselves.
BACKENDS_HOME_ENV = "FORGE_BACKENDS_HOME"

#: The five words. ``off`` is not a probe result: it is ``[make]`` not having
#: chosen the kind, so the row is never probed and never votes on the exit
#: code. The other four are what a probe found.
STATUSES = ("ok", "partial", "missing", "broken", "off")


def add_parser(subparsers) -> None:
    """Register ``doctor``."""
    parser = subparsers.add_parser(
        "doctor",
        help="Is every backend, tool and model where it should be?",
        description=__doc__,
    )
    parser.add_argument("--backend", metavar="NAME", help="check only this backend")
    parser.add_argument(
        "--probe-timeout", type=float, default=PROBE_TIMEOUT_S, metavar="S", help="seconds a probe may take (default 60)"
    )
    parser.add_argument("--no-host", action="store_true", help="skip the host checks (GPU, Blender, ffmpeg)")
    parser.add_argument(
        "--chosen",
        metavar="NAMES",
        help="comma-separated backends the project's [make] chose; every other row reads `off`, "
        "is not probed, and does not vote on the exit code. Unstated, every backend is chosen.",
    )
    parser.add_argument(
        "--off",
        metavar="NAME=REASON",
        action="append",
        default=[],
        help="why an unchosen backend is off, in the words the project uses "
        "(`--off acestep=\'[make] music = false\'`). Repeatable.",
    )
    parser.add_argument(
        "--comfy-url",
        metavar="URL",
        help="where the ComfyUI service answers ([hardware] comfy_url). "
        "Unstated, the comfy backend's [server] says.",
    )


def run(args) -> dict:
    """Diagnose, print the table (unless ``--json``), and carry the exit code out."""
    report = diagnose(
        only=getattr(args, "backend", None),
        probe_timeout=getattr(args, "probe_timeout", PROBE_TIMEOUT_S),
        host=not getattr(args, "no_host", False),
        chosen=getattr(args, "chosen", None),
        off_reasons=getattr(args, "off", None),
        comfy_url_override=getattr(args, "comfy_url", None),
    )
    report["_text"] = render(report)
    report["_exit"] = report.pop("exit_code")
    return report


#: Doctor has no GPU work to fake.
run_fake = run


# ------------------------------------------------------------------ helpers --


def _run(command: list[str], *, timeout: float, env: dict | None = None, cwd: str | os.PathLike | None = None):
    """``(code, stdout, stderr)``, or ``None`` when the program is not there or hangs."""
    try:
        done = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=os.fspath(cwd) if cwd else None,
            check=False,
            encoding="utf-8",
            errors="replace",
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.returncode, done.stdout, done.stderr


def _check(name: str, ok: bool, detail: str) -> dict:
    return {"name": name, "ok": bool(ok), "detail": detail}


def hf_cache_dir() -> Path:
    """Where the hub keeps models: ``$HF_HUB_CACHE``, else ``$HF_HOME/hub``, else ``~/.cache/huggingface/hub``."""
    if os.environ.get("HF_HUB_CACHE"):
        return Path(os.environ["HF_HUB_CACHE"]).expanduser()
    if os.environ.get("HF_HOME"):
        return Path(os.environ["HF_HOME"]).expanduser() / "hub"
    return Path.home() / ".cache" / "huggingface" / "hub"


def hf_token_path() -> Path:
    """Where ``hf auth login --token`` leaves the token."""
    if os.environ.get("HF_TOKEN_PATH"):
        return Path(os.environ["HF_TOKEN_PATH"]).expanduser()
    if os.environ.get("HF_HOME"):
        return Path(os.environ["HF_HOME"]).expanduser() / "token"
    return Path.home() / ".cache" / "huggingface" / "token"


def hf_token_present() -> bool:
    """Whether a token is stored or exported."""
    return bool(os.environ.get("HF_TOKEN")) or hf_token_path().is_file()


def _non_empty_dir(path: Path) -> bool:
    try:
        return path.is_dir() and any(path.iterdir())
    except OSError:
        return False


def hf_model_present(model_id: str) -> tuple[bool, str]:
    """Whether ``models--org--name`` has a non-empty snapshot in the hub cache."""
    folder = hf_cache_dir() / ("models--" + model_id.replace("/", "--"))
    snapshots = folder / "snapshots"
    if not snapshots.is_dir():
        return False, f"not in {hf_cache_dir()}"
    try:
        revisions = [entry for entry in snapshots.iterdir() if _non_empty_dir(entry)]
    except OSError:
        revisions = []
    if not revisions:
        return False, f"{folder.name}: no complete snapshot"
    return True, f"{folder.name}/snapshots/{revisions[0].name}"


def model_present(backend: Backend, model: backends_mod.Model) -> tuple[bool, str]:
    """Whether one model's weights are on disk, by its store."""
    if model.store == "hf":
        return hf_model_present(model.id)
    base = backend.checkpoints if model.store == "checkpoints_dir" else backend.text_encoders
    link = backends_mod.CHECKPOINTS_LINK if model.store == "checkpoints_dir" else backends_mod.TEXT_ENCODERS_LINK
    if not base.exists():
        return False, f"no {link} link under backends/{backend.name}"
    target = base / model.id
    if _non_empty_dir(target):
        return True, str(target.resolve())
    return False, f"{target} is absent or empty"


# ----------------------------------------------------------------- backends --


def _git(checkout: Path, *args: str) -> str | None:
    done = _run(["git", "-C", str(checkout), *args], timeout=TOOL_TIMEOUT_S)
    if done is None or done[0] != 0:
        return None
    return done[1].strip()


def check_checkout(backend: Backend, out: dict) -> None:
    checkout = backend.checkout
    if not checkout.exists():
        if backend.cwd == "checkout":
            out["checks"].append(_check("checkout", False, f"no {backends_mod.CHECKOUT_LINK} link — the inner module runs from the upstream clone"))
            out["hints"].append(backend.install_hint())
        else:
            out["checks"].append(_check("checkout", True, "none needed"))
        return
    head = _git(checkout, "rev-parse", "HEAD")
    if head is None:
        out["checks"].append(_check("checkout", False, f"{checkout.resolve()} is not a git checkout"))
        return
    dirty = _git(checkout, "status", "--porcelain", "--untracked-files=no")
    pinned = backend.commit.lower()
    matches = head.lower().startswith(pinned) or pinned.startswith(head.lower())
    detail = f"{head[:12]}"
    if not matches:
        detail = f"warn: HEAD {head[:12]} is not the pinned {pinned[:12]}"
        out["hints"].append(f"git -C {checkout.resolve()} checkout {backend.commit}  (or update backend.toml's commit)")
    if dirty:
        changed = len(dirty.splitlines())
        detail += f"; dirty ({changed} tracked file{'s' if changed != 1 else ''} modified)"
    out["checks"].append(_check("checkout", True, detail))


def check_python(backend: Backend, out: dict) -> Path | None:
    try:
        interpreter = launcher.resolve_interpreter(backend)
    except MissingBackend as err:
        out["checks"].append(_check("python", False, err.message))
        if err.hint:
            out["hints"].append(err.hint)
        return None
    done = _run([str(interpreter), "-c", "import sys; print('.'.join(map(str, sys.version_info[:3])))"], timeout=TOOL_TIMEOUT_S)
    if done is None or done[0] != 0:
        out["checks"].append(_check("python", False, f"{interpreter} does not run"))
        return None
    version = done[1].strip()
    detail = f"{version} at {interpreter}"
    wanted = str(backend.python or "")
    if wanted and not version.startswith(wanted.rstrip(".") + ".") and version != wanted:
        detail = f"warn: {version} (backend.toml says {wanted}) at {interpreter}"
    out["checks"].append(_check("python", True, detail))
    return interpreter


def check_env_shadowing(backend: Backend, interpreter: Path, out: dict) -> None:
    """A warn row per ambient variable that shadows a plain ``[env]`` value.

    The launcher's setdefault lets the shell win over ``backend.toml`` on
    purpose — but silently, and a shadowed ``CC`` once fed nvdiffrast's JIT
    a mixed CUDA host toolchain. ``[env.force]`` entries cannot be shadowed
    and never appear here.
    """
    try:
        shadowed = launcher.env_shadowing(backend, interpreter)
    except MissingBackend:
        return
    for key, ambient, configured in shadowed:
        out["checks"].append(
            _check(f"env:{key}", True, f"warn: the shell's {key}={ambient} shadows backend.toml's {configured}")
        )


def run_probe(backend: Backend, interpreter: Path, *, timeout: float) -> tuple[dict | None, str]:
    """Run ``probe.py`` under the env; ``(parsed last JSON line, detail)``."""
    probe = backend.probe
    if not probe.is_file():
        return None, "no probe"
    try:
        env = launcher.inner_env(backend, interpreter)
    except MissingBackend as err:
        return None, err.message
    cwd: Path | None = None
    if backend.cwd == "checkout" and backend.checkout.exists():
        cwd = backend.checkout.resolve()
    done = _run([str(interpreter), str(probe)], timeout=timeout, env=env, cwd=cwd or backend.dir)
    if done is None:
        return None, f"probe did not finish within {timeout:.0f} s (or could not start)"
    code, stdout, stderr = done
    lines = [line for line in stdout.splitlines() if line.strip()]
    parsed = None
    if lines:
        try:
            parsed = json.loads(lines[-1])
        except json.JSONDecodeError:
            parsed = None
    if code != 0 or not isinstance(parsed, dict):
        tail = [line for line in (stderr or stdout).splitlines() if line.strip()][-5:]
        why = "; ".join(tail) if tail else "no output"
        return None, f"probe exited {code} without a JSON line: {why}" if code != 0 else f"probe printed no JSON line: {why}"
    return parsed, "ran"


def check_probe(backend: Backend, interpreter: Path, out: dict, *, timeout: float) -> bool:
    """Append the probe check; return whether the probe ran (not whether all was well)."""
    probe, detail = run_probe(backend, interpreter, timeout=timeout)
    if probe is None:
        out["checks"].append(_check("probe", False, detail))
        if detail == "no probe":
            out["hints"].append(f"{backend.probe} is missing — this checkout is incomplete")
        return False
    out["probe"] = probe
    torch = probe.get("torch")
    cuda = probe.get("cuda_available")
    torch_cuda = probe.get("torch_cuda")
    imports = probe.get("imports") or {}
    failed = sorted(name for name, ok in imports.items() if not ok)
    summary = f"torch {torch or '?'}" + (f" cu{torch_cuda}" if torch_cuda else "") + f", cuda {'yes' if cuda else 'NO'}"
    if imports:
        summary += f", imports {len(imports) - len(failed)}/{len(imports)}"
    extras = probe.get("extras") or {}
    if isinstance(extras, dict) and extras:
        summary += "; " + ", ".join(f"{key}={value}" for key, value in extras.items())
    out["checks"].append(_check("probe", True, summary))
    if not torch:
        out["checks"].append(_check("torch", False, "probe saw no torch"))
    if not cuda:
        out["checks"].append(_check("cuda", False, "torch sees no CUDA device"))
        out["hints"].append("the env's torch cannot see the GPU: driver, CUDA build of torch, or another process holding the card")
    for name in failed:
        out["checks"].append(_check(f"import:{name}", False, f"{name} does not import in the env"))
    for notice in probe.get("notices") or []:
        if isinstance(notice, str):
            out["notices"].append(notice)
    for hint in probe.get("hints") or []:
        if isinstance(hint, str):
            out["hints"].append(hint)
    return True


def check_models(backend: Backend, out: dict) -> None:
    for model in backend.models:
        present, detail = model_present(backend, model)
        label = f"model:{model.id}"
        if present:
            out["checks"].append(_check(label, True, detail))
            continue
        if model.gated:
            out["checks"].append(_check(label, False, f"gated and absent: {detail}"))
            if model.accept_url:
                out["hints"].append(f"accept the licence for {model.id} at {model.accept_url}")
            token = "a token is stored" if hf_token_present() else f"no token at {hf_token_path()}"
            out["hints"].append(f"hf auth login --token <tok>   ({token}; never the interactive login — no TTY under an agent)")
        else:
            out["checks"].append(_check(label, False, f"absent: {detail}"))
            out["hints"].append(f"the first run downloads {model.id}; or: bash {backend.install_script}")


def _status(out: dict, *, env_missing: bool, broken: bool) -> str:
    if broken:
        return "broken"
    if env_missing:
        return "missing"
    if all(check["ok"] for check in out["checks"]):
        return "ok"
    return "partial"


def diagnose_tool(backend: Backend, out: dict, *, timeout: float) -> str:
    """A tool backend (Blender): its probe runs under this python and decides the status.

    The probe's exit is the verdict — 0 answered and new enough (``ok``),
    1 present but too old (``partial``), 6 not there (``missing``); no probe
    or no JSON line is ``broken``.
    """
    probe = backend.probe
    if not probe.is_file():
        out["checks"].append(_check("probe", False, "no probe"))
        out["hints"].append(f"{probe} is missing — this checkout is incomplete")
        return "broken"
    done = _run([sys.executable, str(probe)], timeout=timeout, cwd=backend.dir)
    if done is None:
        out["checks"].append(_check("probe", False, f"probe did not finish within {timeout:.0f} s (or could not start)"))
        return "broken"
    code, stdout, stderr = done
    lines = [line for line in stdout.splitlines() if line.strip()]
    parsed = None
    if lines:
        try:
            parsed = json.loads(lines[-1])
        except json.JSONDecodeError:
            parsed = None
    if not isinstance(parsed, dict):
        tail = [line for line in (stderr or stdout).splitlines() if line.strip()][-5:]
        out["checks"].append(_check("probe", False, f"probe exited {code} without a JSON line: {'; '.join(tail) or 'no output'}"))
        return "broken"
    out["probe"] = parsed
    # The toml's notice and the probe's say the same thing under the same
    # title; the file's wording is the one that is reviewed, so it wins.
    titles = {notice.split(":", 1)[0] for notice in out["notices"]}
    for notice in parsed.get("notices") or []:
        if isinstance(notice, str) and notice.split(":", 1)[0] not in titles:
            out["notices"].append(notice)
    for hint in parsed.get("hints") or []:
        if isinstance(hint, str):
            out["hints"].append(hint)
    if code == exit_codes.MISSING_TOOL or not parsed.get("bin"):
        out["checks"].append(_check("binary", False, str(parsed.get("error") or f"{backend.name} is not there")))
        return "missing"
    version = parsed.get("version") or "?"
    build = parsed.get("build_hash")
    detail = f"{version} at {parsed['bin']}" + (f" (build {build})" if build else "")
    if parsed.get("ok"):
        out["checks"].append(_check("binary", True, detail))
        return "ok"
    out["checks"].append(_check("binary", False, f"{detail}: {parsed.get('error') or 'too old'}"))
    return "partial"


# -------------------------------------------------------------------- comfy --


def executor_of(backend: Backend) -> str:
    """Which executor runs this backend: ``env``, ``comfy`` or ``tool``.

    Read from the parser, never re-parsed here — ``backend.toml``'s second
    form states it outright, and a v1 file that does not is derived from its
    ``env_kind``: a backend with no interpreter is a host tool, everything
    else is the per-backend launcher this repo has always had.
    """
    stated = getattr(backend, "executor", None) or backend.extra.get("executor")
    if isinstance(stated, str) and stated in ("env", "comfy", "tool"):
        return stated
    return "tool" if backend.is_tool else "env"


def comfy_table(backend: Backend) -> dict:
    """The backend's ``[comfy]`` table, from the parser (or, on a v1 parser, its ``extra``)."""
    table = getattr(backend, "comfy", None)
    if table is None:
        table = backend.extra.get("comfy")
    return table if isinstance(table, dict) else {}


class ComfyView:
    """The ComfyUI host, fetched **once per doctor run** and shared by every
    backend the ``comfy`` executor hosts.

    A per-backend fetch would be six ``GET /object_info`` on a cold host —
    the response is the whole node surface, megabytes of it — for six
    answers that cannot differ, because there is one service. So: one
    ``/system_stats``, one ``/object_info``, memoised, and every comfy row
    reads them.

    Nothing here enters ComfyUI's environment. The thing to check is the
    service: that it answers, that it is the commit we pinned, that the node
    classes a workflow names exist in it, and that the weights are on disk.
    """

    def __init__(self, url: str, host: Backend | None = None) -> None:
        #: Where the service answers.
        self.url = url.rstrip("/")
        #: The ``comfy`` backend, when the directory describes one: its
        #: pinned commit, its packs, its unit name.
        self.host = host
        self._fetched = False
        #: ``GET /system_stats``, or ``None`` when it did not answer.
        self.stats: dict | None = None
        #: ``GET /object_info``, or ``None``.
        self.info: dict | None = None
        #: Why it did not answer, when it did not.
        self.error: str | None = None
        #: ``--base-directory`` as the running service was given it.
        self.base: Path | None = None

    # -- the one fetch --------------------------------------------------

    def fetch(self) -> None:
        """One ``/system_stats`` and one ``/object_info``, at most once."""
        if self._fetched:
            return
        self._fetched = True
        self.stats, self.error = _get_json(f"{self.url}/system_stats", timeout=TOOL_TIMEOUT_S)
        if self.stats is None:
            return
        argv = (self.stats.get("system") or {}).get("argv") or []
        if "--base-directory" in argv:
            index = argv.index("--base-directory")
            if index + 1 < len(argv):
                self.base = Path(argv[index + 1])
        if self.base is None and self.host is not None:
            named = comfy_table(self.host).get("base_directory")
            prefix = _prefix_of(self.host)
            if named and prefix:
                self.base = prefix / str(named)
        self.info, why = _get_json(f"{self.url}/object_info", timeout=OBJECT_INFO_TIMEOUT_S)
        if self.info is None:
            self.error = why

    @property
    def answered(self) -> bool:
        """Whether the service answered at all."""
        self.fetch()
        return self.stats is not None

    @property
    def classes(self) -> set[str]:
        """Every node class the running service offers."""
        self.fetch()
        return set(self.info or ())

    @property
    def version(self) -> str | None:
        """What the service calls itself."""
        self.fetch()
        return ((self.stats or {}).get("system") or {}).get("comfyui_version")

    def commit_state(self) -> tuple[bool, str]:
        """``(matches, detail)`` for the pinned ComfyUI commit.

        The *running* clone is what matters, so this reads the checkout the
        unit execs, not the description beside it.
        """
        if self.host is None:
            return True, "no comfy backend describes the host — its pin cannot be checked"
        pinned = (self.host.commit or "").lower()
        head = _git_head(self.host.checkout)
        if head is None:
            return False, f"no {backends_mod.CHECKOUT_LINK} link under backends/{self.host.name}"
        if pinned and not head.lower().startswith(pinned[:12]):
            return False, f"the service's clone is at {head[:12]}, not the pinned {pinned[:12]}"
        return True, head[:12]

    def pack_states(self) -> list[tuple[str, bool, str]]:
        """``(name, at_its_pin, detail)`` for every node pack the host names."""
        out: list[tuple[str, bool, str]] = []
        if self.host is None:
            return out
        for pack in comfy_table(self.host).get("packs") or []:
            if not isinstance(pack, dict):
                continue
            directory = str(pack.get("dir") or "")
            pinned = str(pack.get("commit") or "")
            base = self.base or _prefix_of(self.host)
            clone = None
            if base is not None and directory:
                clone = base / "custom_nodes" / directory
            if clone is None or not (clone / ".git").exists():
                out.append((directory or "?", False, f"no clone at {clone or '(unknown)'}"))
                continue
            head = _git_head(clone)
            if head is None:
                out.append((directory, False, f"{clone} is not a git checkout"))
            elif pinned and not head.lower().startswith(pinned.lower()[:12]):
                out.append((directory, False, f"at {head[:12]}, not the pinned {pinned[:12]}"))
            else:
                out.append((directory, True, head[:12]))
        return out

    def unit_hint(self) -> str | None:
        """``systemctl --user status forge-comfy`` — the line that says why it is not answering."""
        unit = ((self.host.server or {}) if self.host else {}).get("unit")
        return f"systemctl --user status {unit}" if unit else None

    def start_hint(self) -> str | None:
        """The line that starts it."""
        unit = ((self.host.server or {}) if self.host else {}).get("unit")
        return f"systemctl --user start {unit}" if unit else None


def _get_json(url: str, *, timeout: float) -> tuple[dict | None, str | None]:
    """``(parsed, why not)`` for one GET. Stdlib only: doctor never enters an env."""
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as err:
        return None, f"{url} answered {err.code}"
    except (urllib.error.URLError, OSError) as err:
        return None, f"{url} did not answer ({getattr(err, 'reason', err)})"
    except (json.JSONDecodeError, UnicodeDecodeError):
        return None, f"{url} answered with something that is not JSON"


def _git_head(checkout: Path | None) -> str | None:
    """HEAD of a clone, or ``None`` when there is not one there."""
    if checkout is None or not checkout.exists():
        return None
    return _git(checkout.resolve(), "rev-parse", "HEAD")


def _prefix_of(backend: Backend) -> Path | None:
    """Where a backend installed itself: the ``.env`` link's parent, else
    ``$FORGE_BACKENDS_HOME/<name>``, else the default cache."""
    link = backend.env_link
    if link.exists():
        return link.resolve().parent
    home = os.environ.get(BACKENDS_HOME_ENV)
    base = Path(home).expanduser() if home else Path.home() / ".cache" / "asset-forge" / "backends"
    candidate = base / backend.name
    return candidate if candidate.exists() else None


def comfy_url(base: Path, override: str | None = None) -> str:
    """Where the service answers: ``--comfy-url`` (from ``[hardware]``), else
    the ``comfy`` backend's ``[server]``, else the default."""
    if override:
        return override.rstrip("/")
    try:
        host = backends_mod.load_backend("comfy", base)
    except MissingBackend:
        return DEFAULT_COMFY_URL
    server = host.server or {}
    return f"http://{server.get('host', '127.0.0.1')}:{server.get('port', 8188)}"


def workflow_classes(backend: Backend) -> dict[str, list[str]]:
    """Every node class each tracked workflow names, by file name.

    A workflow in the tree that names a class the service does not have is a
    graph that cannot run — ``broken``, not ``partial``: the file is wrong or
    the pack it needs is gone, and no download fixes either.
    """
    out: dict[str, list[str]] = {}
    for name in comfy_table(backend).get("workflows") or []:
        path = backend.dir / "workflows" / str(name)
        if not path.is_file():
            out[str(name)] = []
            continue
        try:
            with open(path, encoding="utf-8") as handle:
                graph = json.load(handle)
        except (OSError, json.JSONDecodeError):
            out[str(name)] = []
            continue
        classes = sorted(
            {node["class_type"] for node in graph.values() if isinstance(node, dict) and node.get("class_type")}
        )
        out[str(name)] = classes
    return out


def comfy_weights(backend: Backend) -> list[dict]:
    """Every weight a comfy backend needs, from either shape it can be written in.

    ``[[comfy.models]]`` — ``{repo, file, folder, local?, gb?}`` — is what
    the tracked descriptions use, because ``store`` had no word for a
    ComfyUI model folder when they were written. ``[[models]]`` with
    ``store = "comfy:models/<folder>"`` is that word; both are read here, and
    a backend may use either.
    """
    out: list[dict] = []
    for entry in comfy_table(backend).get("models") or []:
        if not isinstance(entry, dict):
            continue
        name = str(entry.get("local") or Path(str(entry.get("file") or "")).name)
        out.append(
            {
                "id": f"{entry.get('repo', '?')}/{name}" if name else str(entry.get("repo", "?")),
                "folder": f"models/{entry.get('folder', '')}".rstrip("/"),
                "file": name,
                "gb": entry.get("gb"),
            }
        )
    for model in backend.models:
        if not str(model.store).startswith("comfy:"):
            continue
        out.append(
            {
                "id": model.id,
                "folder": str(model.store).split(":", 1)[1],
                "file": Path(model.id).name,
                "gb": getattr(model, "gb", None),
            }
        )
    return out


def comfy_model_present(view: ComfyView, weight: dict) -> tuple[bool, str]:
    """Whether one weight is on disk under the host's base directory.

    ComfyUI reads its models from ``<base>/models/<folder>``, so that is
    where a weight either is or is not; the hub cache is irrelevant to a
    service that was never told about it.
    """
    base = view.base
    if base is None and view.host is not None:
        prefix = _prefix_of(view.host)
        named = comfy_table(view.host).get("base_directory") or "data"
        base = prefix / str(named) if prefix else None
    if base is None:
        return False, "the comfy base directory is not known — is the service running?"
    target = base / weight["folder"] / weight["file"]
    if target.is_file():
        return True, str(target)
    return False, f"{target} is absent"


def diagnose_comfy(backend: Backend, out: dict, view: ComfyView) -> str:
    """One ``comfy`` backend's five words, against the shared view.

    ``ok`` the service answers, is at its pin, has every class the
    backend's ``[comfy] nodes`` and its tracked workflows name, every pack
    clone is at its pinned commit and every ``[[models]]`` file is on disk;
    ``partial`` it answers and the packs are right but a class or a weight
    is absent; ``missing`` nothing is listening; ``broken`` it answers as
    another commit, or a pack is off its pin, or a tracked workflow names a
    class that does not exist.
    """
    out["checks"].append(_check("comfy_url", True, view.url))
    if not view.answered:
        out["checks"].append(_check("service", False, view.error or "no answer"))
        for hint in (view.start_hint(), view.unit_hint()):
            if hint:
                out["hints"].append(hint)
        out["hints"].append("the comfy executor cannot run anything while the service is down")
        return "missing"
    out["checks"].append(_check("service", True, f"ComfyUI {view.version or '?'} at {view.url}"))

    broken = False
    matches, detail = view.commit_state()
    out["checks"].append(_check("commit", matches, detail))
    if not matches:
        broken = True
        if view.unit_hint():
            out["hints"].append(view.unit_hint())

    for name, at_pin, detail in view.pack_states():
        out["checks"].append(_check(f"pack:{name}", at_pin, detail))
        if not at_pin:
            broken = True

    classes = view.classes
    wanted = [str(node) for node in comfy_table(backend).get("nodes") or []]
    missing_nodes = [node for node in wanted if node not in classes]
    if wanted:
        out["checks"].append(
            _check(
                "nodes",
                not missing_nodes,
                ", ".join(wanted) if not missing_nodes else f"absent from /object_info: {', '.join(missing_nodes)}",
            )
        )

    for file_name, named in workflow_classes(backend).items():
        absent = [node for node in named if node not in classes]
        if not named:
            out["checks"].append(_check(f"workflow:{file_name}", False, "not tracked here, or not JSON"))
            broken = True
        elif absent:
            # A graph naming a class the service does not have cannot run,
            # and no download makes it run: the file or the pack is wrong.
            out["checks"].append(
                _check(f"workflow:{file_name}", False, f"names {', '.join(absent)}, which /object_info does not list")
            )
            broken = True
        else:
            out["checks"].append(_check(f"workflow:{file_name}", True, f"{len(named)} classes, all present"))

    for weight in comfy_weights(backend):
        present, detail = comfy_model_present(view, weight)
        label = f"model:{weight['id']}"
        if present:
            out["checks"].append(_check(label, True, detail))
        else:
            gb = f" ({weight['gb']} GB to fetch)" if weight.get("gb") else ""
            out["checks"].append(_check(label, False, f"{detail}{gb}"))
    for model in backend.models:
        # A comfy backend may still name a weight in one of the three
        # non-comfy stores — the hub cache, a checkpoints dir — and those
        # are checked the way they always were.
        if str(model.store).startswith("comfy:"):
            continue
        present, detail = model_present(backend, model)
        out["checks"].append(_check(f"model:{model.id}", present, detail))

    if broken:
        return "broken"
    if all(check["ok"] for check in out["checks"]):
        return "ok"
    return "partial"

def off_row(name: str, reason: str, *, root: str | os.PathLike | None = None) -> dict:
    """A row for a kind the project did not choose.

    Never probed — which is what makes doctor fast on a props-only project —
    and never a reason to exit 1. The ``backend.toml`` is still *read*, which
    is a file read and not a probe, so the row can say which executor would
    have run it; a backend that is not even described here reads ``off`` all
    the same, because a kind you did not choose cannot be missing.
    """
    row: dict = {
        "status": "off",
        "chosen": False,
        "executor": None,
        "reason": reason,
        "checks": [],
        "notices": [],
        "hints": [],
        "dir": None,
    }
    try:
        backend = backends_mod.load_backend(name, root)
    except MissingBackend:
        return row
    row["dir"] = str(backend.dir)
    row["executor"] = executor_of(backend)
    return row


def diagnose_backend(
    name: str,
    *,
    root: str | os.PathLike | None = None,
    probe_timeout: float = PROBE_TIMEOUT_S,
    comfy: ComfyView | None = None,
) -> dict:
    """One backend's report: ``{status, executor, chosen, checks, notices, hints, dir, ...}``."""
    out: dict = {
        "status": "missing",
        "chosen": True,
        "executor": None,
        "checks": [],
        "notices": [],
        "hints": [],
        "dir": None,
    }
    try:
        backend = backends_mod.load_backend(name, root)
    except backends_mod.BackendConfigError as err:
        out["checks"].append(_check("toml", False, err.message))
        out["status"] = "broken"
        return out
    except MissingBackend as err:
        out["checks"].append(_check("toml", False, err.message))
        out["status"] = "missing"
        if err.hint:
            out["hints"].append(err.hint)
        return out
    out["dir"] = str(backend.dir)
    out["executor"] = executor_of(backend)
    if out["executor"] == "comfy":
        out["checks"].append(
            _check("toml", True, f"{backend.role or 'backend'}, comfy executor, {backend.license or 'licence unstated'}")
        )
        out["notices"].extend(backend.notices)
        if comfy is None:
            out["checks"].append(_check("service", False, "no ComfyUI host was resolved for this run"))
            out["status"] = "missing"
            return out
        out["status"] = diagnose_comfy(backend, out, comfy)
        out["hints"] = list(dict.fromkeys(out["hints"]))
        return out
    if backend.is_tool:
        out["checks"].append(_check("toml", True, f"{backend.role or 'tool'}, a host program, {backend.license or 'licence unstated'}"))
        out["notices"].extend(backend.notices)
        out["status"] = diagnose_tool(backend, out, timeout=probe_timeout)
        return out
    out["checks"].append(
        _check("toml", True, f"{backend.role or 'backend'}, {backend.env_kind} py{backend.python}, commit {backend.commit[:12]}, {backend.license or 'licence unstated'}")
    )
    out["notices"].extend(backend.notices)
    receipt = backend.installed()
    if receipt:
        out["installed"] = receipt

    check_checkout(backend, out)
    interpreter = check_python(backend, out)
    env_missing = interpreter is None
    broken = False
    if interpreter is not None:
        check_env_shadowing(backend, interpreter, out)
        if backend.cwd == "checkout" and not backend.checkout.exists():
            broken = True
        if not check_probe(backend, interpreter, out, timeout=probe_timeout):
            broken = True
    check_models(backend, out)
    out["status"] = _status(out, env_missing=env_missing, broken=broken)
    # One hint each: a backend with two FAILs used to print the same
    # install line twice (ardy, with its model trio, up to six times).
    out["hints"] = list(dict.fromkeys(out["hints"]))
    return out


# --------------------------------------------------------------------- host --


def gpu_report() -> dict:
    """What ``nvidia-smi`` says: the card, its memory, and who holds it."""
    binary = shutil.which("nvidia-smi")
    if not binary:
        return {"ok": False, "error": "nvidia-smi is not on PATH", "busy": False, "apps": []}
    done = _run([binary, "--query-gpu=name,memory.total,memory.used", "--format=csv,noheader,nounits"], timeout=TOOL_TIMEOUT_S)
    if done is None or done[0] != 0:
        return {"ok": False, "error": "nvidia-smi did not answer", "busy": False, "apps": []}
    cards = []
    for line in done[1].splitlines():
        parts = [part.strip() for part in line.split(",")]
        if len(parts) >= 3:
            try:
                cards.append({"name": parts[0], "total_mb": int(float(parts[1])), "used_mb": int(float(parts[2]))})
            except ValueError:
                continue
    apps = []
    done = _run([binary, "--query-compute-apps=pid,process_name,used_memory", "--format=csv,noheader,nounits"], timeout=TOOL_TIMEOUT_S)
    if done is not None and done[0] == 0:
        for line in done[1].splitlines():
            parts = [part.strip() for part in line.split(",")]
            if len(parts) >= 3:
                try:
                    apps.append({"pid": int(parts[0]), "name": parts[1], "used_mb": int(float(parts[2]))})
                except ValueError:
                    continue
    if not cards:
        return {"ok": False, "error": "nvidia-smi listed no GPU", "busy": False, "apps": apps}
    first = cards[0]
    busy = first["used_mb"] > GPU_BUSY_MB
    warn = None
    if busy:
        holders = ", ".join(f"pid {app['pid']} {app['name']} {app['used_mb'] / 1024:.1f} GB" for app in apps) or f"{first['used_mb'] / 1024:.1f} GB in use"
        warn = f"GPU busy: {holders}"
    return {"ok": True, "name": first["name"], "total_mb": first["total_mb"], "used_mb": first["used_mb"], "gpus": cards, "apps": apps, "busy": busy, "warn": warn}


def blender_report() -> dict:
    """``$BLENDER_BIN`` or PATH, its version, and whether it is new enough."""
    try:
        binary = launcher.blender_bin()
    except MissingTool as err:
        return {"ok": False, "error": err.message, "hint": err.hint}
    version = launcher.blender_version(binary, timeout=TOOL_TIMEOUT_S)
    if version is None:
        return {"ok": False, "bin": str(binary), "error": "blender --version did not answer"}
    text = ".".join(str(v) for v in version)
    new_enough = tuple(version[:2]) >= launcher.BLENDER_MIN
    return {
        "ok": new_enough,
        "bin": str(binary),
        "version": text,
        "error": None if new_enough else f"Blender {text} is older than {launcher.BLENDER_MIN[0]}.{launcher.BLENDER_MIN[1]}",
    }


def tool_report(name: str, version_args: list[str]) -> dict:
    """A host tool's presence and first version line."""
    binary = shutil.which(name)
    if not binary:
        return {"ok": False, "error": f"{name} is not on PATH"}
    done = _run([binary, *version_args], timeout=TOOL_TIMEOUT_S)
    if done is None:
        return {"ok": False, "bin": binary, "error": f"{name} did not answer"}
    first = (done[1] or done[2]).strip().splitlines()
    return {"ok": True, "bin": binary, "version": first[0] if first else "?"}


def host_report() -> dict:
    """GPU, conda, python3 — Blender and ffmpeg are their own sections."""
    return {
        "gpu": gpu_report(),
        "conda": tool_report("conda", ["--version"]),
        "python3": {"bin": sys.executable, "version": ".".join(map(str, sys.version_info[:3])), "ok": sys.version_info >= (3, 11)},
    }


# ------------------------------------------------------------------ overall --


def parse_off_reasons(entries: list[str] | None) -> dict[str, str]:
    """``--off name=reason`` into a map. A malformed entry is dropped rather
    than refused: a missing reason costs a good sentence, not the table."""
    out: dict[str, str] = {}
    for entry in entries or []:
        name, sep, reason = str(entry).partition("=")
        if sep and name.strip():
            out[name.strip()] = reason.strip()
    return out


def diagnose(
    *,
    only: str | None = None,
    root: str | os.PathLike | None = None,
    probe_timeout: float = PROBE_TIMEOUT_S,
    host: bool = True,
    chosen: str | list[str] | None = None,
    off_reasons: list[str] | dict[str, str] | None = None,
    comfy_url_override: str | None = None,
) -> dict:
    """The whole report, with ``exit_code`` decided.

    ``chosen`` is the backend set the project's ``[make]`` implies; every
    other row reads ``off``, is not probed, and does not vote. Unstated,
    every backend is chosen — which is what ``python3 python/forge_gen
    doctor`` on its own means, and what a project with no ``[make]`` reads
    as.

    **Exit 1 only while a chosen backend is not ok.** ``--make none`` and
    tier ``fake`` choose nothing, so every row is ``off`` and the exit is 0:
    that is how this gate stays green on a machine with no card.
    """
    base = Path(root).expanduser().resolve() if root else backends_mod.backends_dir()
    report: dict = {
        "schema": SCHEMA,
        "ok": False,
        "backends_dir": str(base),
        "host": host_report() if host else {},
        "blender": blender_report() if host else {},
        "ffmpeg": tool_report("ffmpeg", ["-version"]) if host else {},
        "backends": {},
        "exit_code": exit_codes.OK,
    }
    if not base.is_dir():
        report["error"] = f"no backends directory at {base} — set {backends_mod.BACKENDS_ENV} or run from a toolkit checkout"
        report["exit_code"] = exit_codes.MISSING_BACKEND
        return report
    names = backends_mod.list_names(base)
    if only:
        if only not in names:
            report["error"] = f"{only} is not a backend here ({', '.join(names)})"
            report["exit_code"] = exit_codes.USAGE
            return report
        names = [only]

    if isinstance(chosen, str):
        chosen_set: set[str] | None = {word.strip() for word in chosen.split(",") if word.strip()}
    elif chosen is None:
        chosen_set = None
    else:
        chosen_set = {str(word) for word in chosen}
    report["chosen"] = sorted(chosen_set) if chosen_set is not None else None
    reasons = off_reasons if isinstance(off_reasons, dict) else parse_off_reasons(off_reasons)

    # One view of the host per run: ``GET /object_info`` is the whole node
    # surface, and six comfy backends asking six times on a cold host is six
    # times the wait for one answer that cannot differ.
    url = comfy_url(base, comfy_url_override)
    try:
        comfy_host: Backend | None = backends_mod.load_backend("comfy", base)
    except MissingBackend:
        comfy_host = None
    view = ComfyView(url, comfy_host)
    report["comfy_url"] = url

    for name in names:
        if chosen_set is not None and name not in chosen_set:
            report["backends"][name] = off_row(name, reasons.get(name, "not needed by [make]"), root=base)
            continue
        report["backends"][name] = diagnose_backend(name, root=base, probe_timeout=probe_timeout, comfy=view)

    # ``off`` never votes: a kind the project did not choose is not a defect.
    voting = [entry for entry in report["backends"].values() if entry["status"] != "off"]
    all_ok = all(entry["status"] == "ok" for entry in voting)
    report["ok"] = all_ok
    report["exit_code"] = exit_codes.OK if all_ok else 1
    return report


def render(report: dict) -> str:
    """The human table."""
    lines: list[str] = []
    host = report.get("host") or {}
    gpu = host.get("gpu")
    if gpu:
        if gpu.get("ok"):
            lines.append(f"gpu       {gpu['name']}  {gpu['used_mb']} / {gpu['total_mb']} MiB in use")
            if gpu.get("warn"):
                lines.append(f"          warn: {gpu['warn']}")
        else:
            lines.append(f"gpu       {gpu.get('error')}")
    blender = report.get("blender") or {}
    if blender:
        if blender.get("version"):
            lines.append(f"blender   {blender['version']} {blender.get('bin', '')}  {'ok' if blender.get('ok') else blender.get('error')}")
        else:
            lines.append(f"blender   missing  {blender.get('error')}")
        if not blender.get("ok") and blender.get("hint"):
            lines.append(f"          hint: {blender['hint']}")
    ffmpeg = report.get("ffmpeg") or {}
    if ffmpeg:
        lines.append(f"ffmpeg    {ffmpeg.get('version', ffmpeg.get('error', '?'))}")
    for tool in ("conda", "python3"):
        entry = host.get(tool)
        if entry:
            lines.append(f"{tool:<9} {entry.get('version', entry.get('error', '?'))}")
    lines.append(f"backends  {report.get('backends_dir')}")
    if report.get("error"):
        lines.append(f"  error: {report['error']}")
    for name, entry in (report.get("backends") or {}).items():
        executor = entry.get("executor") or "?"
        if entry["status"] == "off":
            # Dimmed, with the reason on the same line: an `off` row is not
            # a problem to be fixed and should not read like one.
            lines.append(_dim(f"  {name:<10} {'off':<8} off — {entry.get('reason') or 'not chosen'}"))
            continue
        lines.append(f"  {name:<10} {entry['status']:<8} [{executor}] {entry.get('dir') or ''}")
        for check in entry["checks"]:
            mark = "ok  " if check["ok"] else "FAIL"
            if check["ok"] and check["detail"].startswith("warn:"):
                mark = "warn"
            lines.append(f"    {mark} {check['name']:<28} {check['detail']}")
        for notice in entry["notices"]:
            lines.append(f"    warn notice: {notice}")
        for hint in entry["hints"]:
            lines.append(f"    hint: {hint}")
    entries = report.get("backends") or {}
    off = [name for name, entry in entries.items() if entry["status"] == "off"]
    chosen = [name for name, entry in entries.items() if entry["status"] != "off"]
    if not chosen:
        verdict = "nothing is chosen — every backend is off, and that is not a problem"
    elif report.get("ok"):
        verdict = f"every chosen backend ok ({', '.join(chosen)})"
    else:
        bad = [name for name in chosen if entries[name]["status"] != "ok"]
        verdict = f"not every chosen backend is ok: {', '.join(bad)}"
    if off:
        verdict += f"; {len(off)} off ({', '.join(off)})"
    lines.append(f"doctor: {verdict} (exit {report.get('exit_code', report.get('_exit', '?'))})")
    return "\n".join(lines) + "\n"


def _dim(text: str) -> str:
    """Dimmed, when something is watching. A pipe gets the plain words: the
    Rust side parses the JSON, and a log with escape codes in it is worse
    than one without."""
    if not sys.stdout.isatty() or os.environ.get("NO_COLOR"):
        return text
    return f"\x1b[2m{text}\x1b[0m"
