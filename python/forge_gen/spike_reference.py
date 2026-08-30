"""Draw a reference image through ComfyUI — the Phase 0 spike, Qwen-Image vs FLUX.1-schnell.

    python3 python/forge_gen/spike_reference.py --model qwen_image|flux_schnell
            [--seeds 7 11 42 1234] [--pose out/spike/tpose_pose.png]
            [--out-dir out/spike/reference] [--free-first] [--json]

**A Phase 0 spike, not a door.** This is the prototype of the plan's
``comfy`` executor and of ``generate_reference`` (designs/forge2.md, Phase
1 and 3): it is deliberately not registered in ``cli.py``, it writes no
record, nothing in ``just ci`` runs it, and it promotes nothing. What it
proves is which of the two candidate image models lands a strict T-pose
under pose conditioning often enough for the fit gate downstream, and what
each one costs in seconds and VRAM.

Stdlib only for everything that talks to ComfyUI — ``urllib`` against
``POST /upload/image``, ``POST /prompt``, ``GET /history/{id}``, ``GET
/view``, ``POST /free`` and ``GET /system_stats`` — because that is the
whole surface Phase 1's client is cut from, and a spike that needs a
dependency to prove a protocol proves the wrong thing. Pillow is imported
inside :func:`contact_sheet` only, the way ``mesh.py`` imports cv2.

**The templates are the deliverable.** ``backends/comfy/workflows/
reference_{qwen,flux}.api.json`` are API-format graphs with every knob
written down; this file patches five inputs into them (prompt, negative
prompt, pose image, seed, filename prefix) and nothing else. Node ids are
stable and the patch points carry ``PATCH`` in their ``_meta.title``.

**VRAM is measured, not asked for.** ComfyUI's ``/system_stats`` reports
what torch has allocated, which is not the peak of a run; a sampler thread
polls ``nvidia-smi`` at 10 Hz for the whole prompt and the peak of that is
what the notes carry — the same method the SkinTokens spike used, so the
two numbers are comparable.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

#: The log prefix; the spike notes quote these lines.
TAG = "reference"

#: Where ComfyUI answers unless ``$FORGE_COMFY_URL`` or ``--url`` says otherwise.
DEFAULT_URL = "http://127.0.0.1:8188"

#: The workflow template per model, under ``backends/comfy/workflows/``.
TEMPLATES = {
    "qwen_image": "reference_qwen.api.json",
    "qwen_image_gguf": "reference_qwen_gguf.api.json",
    "flux_schnell": "reference_flux.api.json",
}

#: The node ids this file patches. They are stable in both templates.
NODE_PROMPT = "5"
NODE_NEGATIVE = "6"
NODE_POSE = "7"
NODE_CONTROLNET_APPLY = "9"
NODE_LATENT = "10"
NODE_SAMPLER = "11"
NODE_SAVE = "13"

#: The name the pose image is uploaded under, in ComfyUI's input directory.
POSE_INPUT_NAME = "forge_tpose_pose.png"

#: The four seeds the spike draws, as forge2.md's reference door will.
DEFAULT_SEEDS = (7, 11, 42, 1234)


# ------------------------------------------------------------------ HTTP --


def _get(url: str, timeout: float = 30.0):
    with urllib.request.urlopen(url, timeout=timeout) as response:  # noqa: S310
        return json.load(response)


def _get_bytes(url: str, timeout: float = 120.0) -> bytes:
    with urllib.request.urlopen(url, timeout=timeout) as response:  # noqa: S310
        return response.read()


def _post_json(url: str, payload: dict, timeout: float = 60.0):
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
        text = response.read().decode("utf-8")
    return json.loads(text) if text.strip() else {}


def upload_image(base: str, path: Path, name: str) -> str:
    """``POST /upload/image`` with overwrite — a multipart body by hand, stdlib only."""
    boundary = f"----forge{uuid.uuid4().hex}"
    parts = []
    for key, value in (("overwrite", "true"), ("type", "input")):
        parts.append(
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n".encode()
        )
    parts.append(
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{name}\"\r\n"
        f"Content-Type: image/png\r\n\r\n".encode()
    )
    parts.append(path.read_bytes())
    parts.append(f"\r\n--{boundary}--\r\n".encode())
    body = b"".join(parts)
    request = urllib.request.Request(
        f"{base}/upload/image",
        data=body,
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:  # noqa: S310
        answer = json.loads(response.read().decode("utf-8"))
    return answer.get("name", name)


def free(base: str, *, unload_models: bool = True, free_memory: bool = True) -> None:
    """``POST /free``. Native models honour it; a wrapper pack's may not, which is the point of measuring after."""
    _post_json(f"{base}/free", {"unload_models": unload_models, "free_memory": free_memory})


def vram_free_gb(base: str) -> float:
    """What ComfyUI says is free on the card, in GB."""
    stats = _get(f"{base}/system_stats")
    device = stats["devices"][0]
    return device["vram_free"] / (1024**3)


# ---------------------------------------------------------------- sampler --


class VramSampler:
    """Poll ``nvidia-smi`` at 10 Hz; the peak of the whole card, in MiB."""

    def __init__(self, hz: float = 10.0) -> None:
        self.interval = 1.0 / hz
        self.peak = 0
        self.baseline = self._used()
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, daemon=True)

    @staticmethod
    def _used() -> int:
        try:
            out = subprocess.run(
                ["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return 0
        line = out.stdout.strip().splitlines()
        return int(line[0]) if line else 0

    def _loop(self) -> None:
        while not self._stop.is_set():
            self.peak = max(self.peak, self._used())
            self._stop.wait(self.interval)

    def __enter__(self) -> VramSampler:
        self._thread.start()
        return self

    def __exit__(self, *_exc) -> None:
        self._stop.set()
        self._thread.join(timeout=5)


# --------------------------------------------------------------- workflow --


def load_template(model: str, root: Path) -> dict:
    path = root / "backends" / "comfy" / "workflows" / TEMPLATES[model]
    return json.loads(path.read_text(encoding="utf-8"))


def patch(template: dict, *, prompt: str, negative: str, pose_name: str, seed: int, prefix: str) -> dict:
    graph = json.loads(json.dumps(template))
    graph[NODE_PROMPT]["inputs"]["text"] = prompt
    graph[NODE_NEGATIVE]["inputs"]["text"] = negative
    graph[NODE_POSE]["inputs"]["image"] = pose_name
    graph[NODE_SAMPLER]["inputs"]["seed"] = seed
    graph[NODE_SAVE]["inputs"]["filename_prefix"] = prefix
    return graph


def run_one(base: str, graph: dict, client_id: str, *, poll: float = 1.0, timeout_s: float = 1800.0) -> tuple[dict, float]:
    """Submit, wait, return ``(history entry, wall seconds)``. Raises on a failed prompt."""
    started = time.monotonic()
    answer = _post_json(f"{base}/prompt", {"prompt": graph, "client_id": client_id})
    if "prompt_id" not in answer:
        raise RuntimeError(f"ComfyUI refused the prompt: {json.dumps(answer)[:2000]}")
    prompt_id = answer["prompt_id"]
    while True:
        history = _get(f"{base}/history/{prompt_id}")
        entry = history.get(prompt_id)
        if entry is not None:
            status = entry.get("status", {})
            if status.get("completed") or status.get("status_str") in ("success", "error"):
                if status.get("status_str") == "error":
                    raise RuntimeError(f"prompt {prompt_id} failed: {json.dumps(status)[:4000]}")
                return entry, time.monotonic() - started
        if time.monotonic() - started > timeout_s:
            raise TimeoutError(f"prompt {prompt_id} did not finish in {timeout_s:.0f}s")
        time.sleep(poll)


def fetch_images(base: str, entry: dict) -> list[tuple[str, bytes]]:
    out = []
    for node_output in entry.get("outputs", {}).values():
        for image in node_output.get("images", []):
            query = urllib.parse.urlencode(
                {"filename": image["filename"], "subfolder": image.get("subfolder", ""), "type": image.get("type", "output")}
            )
            out.append((image["filename"], _get_bytes(f"{base}/view?{query}")))
    return out


# ---------------------------------------------------------- contact sheet --


def contact_sheet(cells: list[tuple[str, Path]], out: Path, *, cell: int = 512, label_h: int = 34, cols: int | None = None) -> Path:
    """A labelled grid of the drawn images. Pillow, imported here only."""
    from PIL import Image, ImageDraw, ImageFont  # noqa: PLC0415

    cols = cols or min(4, max(1, len(cells)))
    rows = (len(cells) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cell, rows * (cell + label_h)), (24, 24, 26))
    draw = ImageDraw.Draw(sheet)
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 20)
    except OSError:
        font = ImageFont.load_default()
    for index, (label, path) in enumerate(cells):
        column, row = index % cols, index // cols
        x, y = column * cell, row * (cell + label_h)
        with Image.open(path) as image:
            sheet.paste(image.convert("RGB").resize((cell, cell), Image.LANCZOS), (x, y + label_h))
        draw.text((x + 8, y + 7), label, fill=(235, 235, 235), font=font)
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out)
    return out


# ------------------------------------------------------------------ main --


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--model", choices=sorted(TEMPLATES), required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--negative", default="")
    parser.add_argument("--seeds", type=int, nargs="+", default=list(DEFAULT_SEEDS))
    parser.add_argument("--pose", default="out/spike/tpose_pose.png")
    parser.add_argument("--out-dir", default="out/spike/reference")
    parser.add_argument("--url", default=None)
    parser.add_argument("--root", default=None, help="the toolkit checkout (where backends/comfy/workflows lives)")
    parser.add_argument("--free-first", action="store_true", help="POST /free before the first seed")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    import os

    base = (args.url or os.environ.get("FORGE_COMFY_URL") or DEFAULT_URL).rstrip("/")
    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parents[2]
    pose = Path(args.pose).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    if args.free_first:
        free(base)
        time.sleep(2)
    before_gb = vram_free_gb(base)
    print(f"{TAG}: {args.model}, free VRAM before {before_gb:.2f} GB", flush=True)

    pose_name = upload_image(base, pose, POSE_INPUT_NAME)
    template = load_template(args.model, root)
    client_id = uuid.uuid4().hex

    results = []
    for seed in args.seeds:
        graph = patch(
            template,
            prompt=args.prompt,
            negative=args.negative,
            pose_name=pose_name,
            seed=seed,
            prefix=f"forge/{args.model}_{seed}",
        )
        with VramSampler() as sampler:
            entry, seconds = run_one(base, graph, client_id)
        images = fetch_images(base, entry)
        written = []
        for index, (name, blob) in enumerate(images):
            suffix = "" if index == 0 else f"_{index}"
            path = out_dir / f"{args.model}_{seed}{suffix}.png"
            path.write_bytes(blob)
            written.append(str(path))
            print(f"{TAG}: seed {seed} -> {path} ({name})", flush=True)
        results.append(
            {
                "seed": seed,
                "seconds": round(seconds, 2),
                "peak_vram_mib": sampler.peak,
                "vram_baseline_mib": sampler.baseline,
                "images": written,
            }
        )
        print(f"{TAG}: seed {seed} took {seconds:.1f}s, peak {sampler.peak} MiB", flush=True)

    free(base)
    time.sleep(3)
    after_gb = vram_free_gb(base)
    print(f"{TAG}: POST /free, free VRAM after {after_gb:.2f} GB (before {before_gb:.2f} GB)", flush=True)

    payload = {
        "model": args.model,
        "template": TEMPLATES[args.model],
        "prompt": args.prompt,
        "negative": args.negative,
        "pose_image": str(pose),
        "url": base,
        "runs": results,
        "s_per_image": round(sum(r["seconds"] for r in results) / max(1, len(results)), 2),
        "peak_vram_mib": max((r["peak_vram_mib"] for r in results), default=0),
        "vram_free_gb_before": round(before_gb, 2),
        "vram_free_gb_after_free": round(after_gb, 2),
        "free_returned_the_card": after_gb >= before_gb - 0.5,
    }
    (out_dir / f"{args.model}.run.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    if args.json:
        print(json.dumps(payload), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
