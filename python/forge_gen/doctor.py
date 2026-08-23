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

STATUSES = ("ok", "partial", "missing", "broken")


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


def run(args) -> dict:
    """Diagnose, print the table (unless ``--json``), and carry the exit code out."""
    report = diagnose(
        only=getattr(args, "backend", None),
        probe_timeout=getattr(args, "probe_timeout", PROBE_TIMEOUT_S),
        host=not getattr(args, "no_host", False),
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


def diagnose_backend(name: str, *, root: str | os.PathLike | None = None, probe_timeout: float = PROBE_TIMEOUT_S) -> dict:
    """One backend's report: ``{status, checks, notices, hints, dir, ...}``."""
    out: dict = {"status": "missing", "checks": [], "notices": [], "hints": [], "dir": None}
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


def diagnose(*, only: str | None = None, root: str | os.PathLike | None = None, probe_timeout: float = PROBE_TIMEOUT_S, host: bool = True) -> dict:
    """The whole report, with ``exit_code`` decided."""
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
    for name in names:
        report["backends"][name] = diagnose_backend(name, root=base, probe_timeout=probe_timeout)
    all_ok = all(entry["status"] == "ok" for entry in report["backends"].values())
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
        lines.append(f"  {name:<10} {entry['status']:<8} {entry.get('dir') or ''}")
        for check in entry["checks"]:
            mark = "ok  " if check["ok"] else "FAIL"
            if check["ok"] and check["detail"].startswith("warn:"):
                mark = "warn"
            lines.append(f"    {mark} {check['name']:<28} {check['detail']}")
        for notice in entry["notices"]:
            lines.append(f"    warn notice: {notice}")
        for hint in entry["hints"]:
            lines.append(f"    hint: {hint}")
    verdict = "every backend ok" if report.get("ok") else "not every backend is ok"
    lines.append(f"doctor: {verdict} (exit {report.get('exit_code', report.get('_exit', '?'))})")
    return "\n".join(lines) + "\n"
