#!/usr/bin/env python3
"""Is the ComfyUI host answering, at the pin, with the models it should have? One JSON line.

    python3 backends/comfy/probe.py

Runs under any python (stdlib only, no torch, no requests): ComfyUI has an
environment but doctor never enters it — the thing to check is the service
on 127.0.0.1:8188 and what it says about itself. Two calls do it:
``GET /system_stats`` (version, argv, the card it sees) and
``GET /object_info`` (every node class, with each loader's file list), which
is also how the models are checked — a weight ComfyUI cannot see in its
folders is a weight that is not installed, whatever is on disk.

The last stdout line is the object doctor reads: ``{"tool": "comfy", "ok":
bool, "bin": "http://127.0.0.1:8188", "version": "0.34.2", "build_hash":
<the checkout's HEAD>, "imports": {}, "extras": {...}, "notices": [...],
"hints": [...]}``. Exit 0 when the service answers, is at the pinned commit
and has every model; 1 when it answers but something is absent or adrift
(doctor says ``partial``); 6 (the ``missing_tool`` code) when nothing is
listening.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
TIMEOUT_S = 20.0
#: The node classes the reference spike's workflows load their models with.
#: Every one but ``UnetLoaderGGUF`` is native to the pinned ComfyUI; that one
#: comes from the ComfyUI-GGUF pack, so its absence names a missing pack.
WANTED_NODES = (
    "UNETLoader",
    "CLIPLoader",
    "VAELoader",
    "ControlNetLoader",
    "CheckpointLoaderSimple",
    "TextEncodeQwenImageEdit",
    "ModelSamplingFlux",
    "ControlNetApplyAdvanced",
    "UnetLoaderGGUF",
)
#: Which loader's file list a model folder shows up in, when the model does
#: not name its own (a ``.gguf`` in ``diffusion_models`` is listed by the
#: pack's ``UnetLoaderGGUF``, never by the native ``UNETLoader``).
FOLDER_NODE = {
    "diffusion_models": ("UNETLoader", "unet_name"),
    "text_encoders": ("CLIPLoader", "clip_name"),
    "vae": ("VAELoader", "vae_name"),
    "controlnet": ("ControlNetLoader", "control_net_name"),
    "checkpoints": ("CheckpointLoaderSimple", "ckpt_name"),
}


def config() -> dict:
    """``backend.toml`` beside this file, or an empty dict."""
    try:
        with open(HERE / "backend.toml", "rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return {}


def base_url(conf: dict) -> str:
    """``$FORGE_COMFY_URL``, else ``[server]``'s host and port."""
    override = os.environ.get("FORGE_COMFY_URL")
    if override:
        return override.rstrip("/")
    server = conf.get("server") or {}
    return f"http://{server.get('host', '127.0.0.1')}:{int(server.get('port', 8188))}"


def get(url: str, timeout: float = TIMEOUT_S):
    """One GET, decoded, or ``(None, why)``."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as answer:  # noqa: S310 - a literal http://127.0.0.1
            return json.loads(answer.read().decode("utf-8")), None
    except urllib.error.HTTPError as err:
        return None, f"{url} answered {err.code}"
    except (urllib.error.URLError, OSError) as err:
        return None, f"{url} did not answer: {getattr(err, 'reason', err)}"
    except (json.JSONDecodeError, ValueError) as err:
        return None, f"{url} did not answer JSON: {err}"


def checkout_head() -> str | None:
    """HEAD of the pinned ComfyUI clone behind the ``.checkout`` link."""
    checkout = HERE / ".checkout"
    if not checkout.exists():
        return None
    try:
        done = subprocess.run(
            ["git", "-C", str(checkout.resolve()), "rev-parse", "HEAD"],
            capture_output=True, text=True, timeout=TIMEOUT_S, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() or None


def pack_head(base: Path | None, directory: str) -> str | None:
    """HEAD of a custom node pack's clone under ``<base>/custom_nodes/``."""
    if base is None or not directory:
        return None
    clone = base / "custom_nodes" / directory
    if not (clone / ".git").exists():
        return None
    try:
        done = subprocess.run(
            ["git", "-C", str(clone), "rev-parse", "HEAD"],
            capture_output=True, text=True, timeout=TIMEOUT_S, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() or None


def unit_state(conf: dict) -> str | None:
    """What ``systemctl --user is-active`` says about the unit, when there is one."""
    unit = (conf.get("server") or {}).get("unit")
    if not unit:
        return None
    try:
        done = subprocess.run(
            ["systemctl", "--user", "is-active", unit],
            capture_output=True, text=True, timeout=TIMEOUT_S, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.strip() or None


def main() -> int:
    conf = config()
    url = base_url(conf)
    result: dict = {
        "tool": "comfy",
        "ok": False,
        "bin": None,
        "version": None,
        "build_hash": None,
        "imports": {},
        "extras": {},
        "notices": [],
        "hints": [],
    }
    unit = (conf.get("server") or {}).get("unit")
    state = unit_state(conf)
    if state:
        result["extras"]["unit"] = f"{unit} {state}"

    stats, why = get(f"{url}/system_stats")
    if stats is None:
        result["error"] = why
        if unit:
            result["hints"].append(f"systemctl --user start {unit}   (then: systemctl --user status {unit})")
        result["hints"].append(f"bash {HERE / 'install.sh'}  — installs the venv, the pinned clone and the unit")
        print(json.dumps(result))
        return 6
    result["bin"] = url
    system = stats.get("system") or {}
    result["version"] = system.get("comfyui_version")
    argv = system.get("argv") or []
    result["extras"]["python"] = (system.get("python_version") or "?").split(" ")[0]
    result["extras"]["torch"] = system.get("pytorch_version")
    for device in stats.get("devices") or []:
        if device.get("type") == "cuda":
            free = device.get("vram_free") or 0
            total = device.get("vram_total") or 0
            result["extras"]["vram_free_gb"] = round(free / 1e9, 1)
            result["extras"]["vram_total_gb"] = round(total / 1e9, 1)
            break

    problems: list[str] = []

    # The pin. A service running some other commit is the same defect as a
    # checkout that has drifted, and it is the running one that matters.
    pinned = str(conf.get("commit") or "")
    head = checkout_head()
    result["build_hash"] = (head or "")[:12] or None
    if pinned and head and not head.lower().startswith(pinned.lower()[:12]):
        problems.append(f"the clone is at {head[:12]}, not the pinned {pinned[:12]}")
        result["hints"].append(f"git -C {(HERE / '.checkout').resolve()} checkout {pinned}  (or update backend.toml's commit)")
    elif head is None:
        problems.append("no .checkout link — the pinned clone cannot be checked")

    # The flags the unit is meant to run with. A service started by hand
    # without --base-directory writes into the clone and finds no models.
    for flag in ("--disable-auto-launch", "--disable-api-nodes", "--base-directory", "--cache-none", "--enable-manager"):
        if flag not in argv:
            problems.append(f"the running service has no {flag}")
    base = None
    if "--base-directory" in argv:
        index = argv.index("--base-directory")
        if index + 1 < len(argv):
            base = Path(argv[index + 1])
    result["extras"]["base_directory"] = str(base) if base else "(the clone)"

    # The nodes. /object_info is the whole surface; the loaders' file lists
    # are how ComfyUI answers "is this weight installed".
    info, why = get(f"{url}/object_info", timeout=120.0)
    if info is None:
        problems.append(why or "/object_info did not answer")
        info = {}
    else:
        result["extras"]["node_classes"] = len(info)
    for node in WANTED_NODES:
        result["imports"][node] = node in info
    missing_nodes = sorted(node for node in WANTED_NODES if node not in info)
    if missing_nodes:
        problems.append(f"node classes absent: {', '.join(missing_nodes)}")

    def listed(folder: str, node: str | None = None, field: str | None = None) -> list[str]:
        fallback_node, fallback_field = FOLDER_NODE.get(folder, (None, None))
        node, field = node or fallback_node, field or fallback_field
        try:
            values = info[node]["input"]["required"][field][0]
        except (KeyError, IndexError, TypeError):
            return []
        return [str(v) for v in values] if isinstance(values, list) else []

    # One model list: `[[models]]` with a `comfy:models/<folder>` store. The
    # `[[comfy.models]]` block this used to read existed because `store` had
    # no word for a comfy folder; it has one now, and two lists that can
    # disagree is exactly the shape doctor must not have.
    wanted = [
        model
        for model in (conf.get("models") or [])
        if str(model.get("store") or "").startswith("comfy:")
    ]
    absent: list[str] = []
    for model in wanted:
        folder = str(model["store"]).split("/")[-1]
        name = str(model.get("local") or Path(str(model.get("file") or "")).name)
        if name in listed(folder, model.get("node"), model.get("field")):
            continue
        # A file on disk that ComfyUI does not list is worth saying apart
        # from one that is not there at all: the first is a paths problem.
        on_disk = bool(base and (base / "models" / folder / name).is_file())
        absent.append(f"{folder}/{name}" + (" (on disk, not listed — check extra_model_paths.yaml)" if on_disk else ""))
    result["extras"]["models"] = f"{len(wanted) - len(absent)}/{len(wanted)}"
    if absent:
        problems.append("models absent: " + "; ".join(absent))
        result["hints"].append(f"bash {HERE / 'install.sh'}  — re-running fetches only what is missing")

    # The packs. A pack the tracked description names and the host does not
    # carry (or carries at another commit) is the same defect as a drifted
    # ComfyUI clone: the snapshot beside this file then says something the
    # host does not.
    packs = (conf.get("comfy") or {}).get("packs") or []
    for pack in packs:
        directory = str(pack.get("dir") or "")
        pinned_pack = str(pack.get("commit") or "")
        head = pack_head(base, directory)
        if head is None:
            problems.append(f"custom node pack absent: {directory}")
            result["hints"].append(f"bash {HERE / 'install.sh'}  — clones {directory} at {pinned_pack[:12]} and installs its pips")
        elif pinned_pack and not head.lower().startswith(pinned_pack.lower()[:12]):
            problems.append(f"pack {directory} is at {head[:12]}, not the pinned {pinned_pack[:12]}")
    if packs:
        result["extras"]["packs"] = ", ".join(
            f"{pack.get('dir')}@{str(pack.get('commit') or '')[:12]}" for pack in packs
        )

    for notice in conf.get("notices") or []:
        if isinstance(notice, dict) and notice.get("text"):
            title = notice.get("title")
            result["notices"].append(f"{title}: {notice['text']}" if title else str(notice["text"]))

    result["ok"] = not problems
    if problems:
        result["error"] = "; ".join(problems)
    print(json.dumps(result))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
