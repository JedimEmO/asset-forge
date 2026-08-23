"""``forge-gen motion sweep``: batch-generate many ARDY takes in one process — the audition step.

Generating a single clip pays the model + text-encoder load (~1 min,
~16 GB). Prompt engineering needs dozens of takes, so this loads once and
runs the whole grid, batching every (prompt × sample) pair that shares a
duration and CFG weight into a single forward pass.

    forge-gen motion sweep --out-dir out/sweeps/walk --prompt "A person is walking." --duration 4 --samples 8
    forge-gen motion sweep --out-dir out/sweeps/run --prompts-file prompts.txt --seeds 0 1 --cfg 2.0 3.0
    forge-gen motion review out/sweeps/walk/*.npz --sheet out/sweeps/walk/sheet.png

A prompts file gives each variant a stable label (``label<TAB>prompt`` per
line, ``#`` comments ignored), which is what the output filenames are built
from; ``--prompt`` takes its label from ``--label`` or a slug of the text.

What lands in ``--out-dir``:

* ``<label>__d<dur>_c<cfg>_s<seed>_<k>.npz`` — one take per cell
  (``k`` is the sample index, always present);
* ``<take>.take.json`` — the generator record beside each take: every knob,
  the prompt as an input, the file hashed;
* ``sweep.json`` — the manifest: model, fps and the list of takes with
  their records, in generation order.

The out-dir's *name* is the caller's business. The convention the skills
and the MCP tool use is ``out/sweeps/<slug>-s<seed>-<8 hex of sha256(prompt)>``
so two auditions of the same prompt with different seeds sit side by side
and a re-run of the same pair lands on the same directory; nothing here
depends on it, and nothing under ``out/`` is a source.

GPU note: ~16 GB VRAM — never run alongside TRELLIS, MOSS or the ACE-Step
server. ``--fake`` writes still figures in the rest pose through
``forge_gen.npz.write_take`` and real records with ``fake: true``.
"""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import os
import re
import sys
import time
from pathlib import Path

from forge_gen import records
from forge_gen.exit_codes import InputRejected, UsageError
from forge_gen.motion import session

#: The file the manifest is written to.
MANIFEST = "sweep.json"

#: ARDY core's frame rate; the fake path writes at it, the real one reads it from the model.
DEFAULT_FPS = 20

#: Nickname → released folder, for the fake path only (the real one asks ardy).
KNOWN_MODELS = {
    "core": "ARDY-Core-RP-20FPS-Horizon40",
    "core40": "ARDY-Core-RP-20FPS-Horizon40",
    "core8": "ARDY-Core-RP-20FPS-Horizon8",
}


# ---------------------------------------------------------------- prompts --


def slug(text: str, maxlen: int = 28) -> str:
    """A filename-safe label from a prompt: lower-case words joined by ``_``, cut at ``maxlen``."""
    s = re.sub(r"[^a-z0-9]+", "_", text.lower()).strip("_")
    return s[:maxlen].rstrip("_")


def prompt_hash(prompt: str) -> str:
    """Eight hex characters of the prompt's sha256 — the out-dir naming convention's tail."""
    return hashlib.sha256(prompt.encode("utf-8")).hexdigest()[:8]


def read_prompts(path: str | os.PathLike) -> list[tuple[str, str]]:
    """``label<TAB>prompt`` lines (or bare prompts, labelled by slug); ``#`` comments and blanks ignored."""
    out = []
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            label, _, prompt = line.partition("\t")
            if not prompt:
                label, prompt = slug(line), line
            out.append((label.strip(), prompt.strip()))
    return out


def variants_from(args) -> list[tuple[str, str]]:
    """The (label, prompt) list from ``--prompts-file`` and/or ``--prompt``; refuses an empty one."""
    variants: list[tuple[str, str]] = []
    if args.prompts_file:
        path = Path(args.prompts_file)
        if not path.is_file():
            raise InputRejected(f"--prompts-file {path} is not a file")
        variants += read_prompts(path)
    prompts = list(args.prompt or [])
    if args.label and len(prompts) != 1:
        raise UsageError("--label names one --prompt; give one prompt or use a prompts file for labels")
    for text in prompts:
        text = text.strip()
        if not text:
            raise InputRejected("an empty prompt generates nothing worth auditioning")
        variants.append((args.label.strip() if args.label else slug(text), text))
    if not variants:
        raise UsageError("no prompts: give --prompt TEXT or --prompts-file FILE")
    labels = [label for label, _ in variants]
    dupes = sorted({label for label in labels if labels.count(label) > 1})
    if dupes:
        raise InputRejected(f"two prompts share a label and would overwrite each other's takes: {', '.join(dupes)}")
    return variants


# ------------------------------------------------------------------ cells --


def plan_cells(variants, seeds, cfgs, durations, samples: int) -> list[dict]:
    """One cell per motion to generate, in the order the grid runs. Cells sharing (duration, cfg, seed) batch together."""
    return [
        {"label": label, "prompt": prompt, "seed": int(seed), "cfg": float(cfg), "duration": float(dur), "sample": k}
        for seed, cfg, dur in itertools.product(seeds, cfgs, durations)
        for label, prompt in variants
        for k in range(samples)
    ]


def take_name(cell: dict) -> str:
    """``<label>__d<dur>_c<cfg>_s<seed>_<k>``."""
    return f"{cell['label']}__d{cell['duration']:g}_c{cell['cfg']:g}_s{cell['seed']}_{cell['sample']}"


def group_cells(cells: list[dict]) -> dict[tuple, list[dict]]:
    groups: dict[tuple, list[dict]] = {}
    for cell in cells:
        groups.setdefault((cell["duration"], cell["cfg"], cell["seed"]), []).append(cell)
    return groups


def params_for(cell: dict, args, *, history_frames: int | None, diffusion_steps: int | None) -> dict:
    """The record's params for one cell: every knob stated."""
    return {
        "model": args.model,
        "model_repo": None,  # filled by the caller, who knows the resolved name
        "repo": None,
        "prompt": cell["prompt"],
        "label": cell["label"],
        "seed": cell["seed"],
        "cfg": cell["cfg"],
        "cfg_constraint": 2.0,  # what the model is handed as the pair's second weight; no constraint set, so it does nothing
        "duration_s": cell["duration"],
        "sample": cell["sample"],
        "diffusion_steps": diffusion_steps,
        "history_frames": history_frames,
        "postprocess": not args.no_postprocess,
        "keys_file": None,
        "keys_sha256": None,
        "preset": None,
    }


# --------------------------------------------------------------- argparse --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--out-dir", required=True, metavar="DIR", help="where the takes, their records and sweep.json go")
    parser.add_argument("--prompt", action="append", metavar="TEXT", help="a prompt (repeatable); its label is --label or a slug of the text")
    parser.add_argument("--label", metavar="LABEL", help="the label for a single --prompt")
    parser.add_argument("--prompts-file", metavar="FILE", help="file of 'label<TAB>prompt' lines (or bare prompts)")
    parser.add_argument("--model", default="core", help="ARDY model nickname (default core)")
    parser.add_argument("--duration", type=float, nargs="+", default=[4.0], metavar="S", help="one or more durations to sweep (default 4)")
    parser.add_argument("--samples", type=int, default=8, metavar="N", help="takes per (prompt, seed, cfg, duration) cell (default 8)")
    parser.add_argument("--seeds", type=int, nargs="+", default=[0], metavar="SEED", help="seeds; each seed reruns the whole grid (default 0)")
    parser.add_argument("--cfg", type=float, nargs="+", default=[2.0], metavar="W", help="one or more text CFG weights to sweep (ARDY default 2.0)")
    parser.add_argument("--batch-size", type=int, default=8, metavar="N", help="max motions per forward pass (default 8)")
    parser.add_argument("--diffusion-steps", type=int, default=None, metavar="N", help="denoising steps (default: the model's)")
    parser.add_argument("--history-frames", type=int, default=None, metavar="N", help="crop_history_length (default: ARDY's own)")
    parser.add_argument("--no-postprocess", action="store_true", help="skip ARDY's foot-skate correction (to see raw output)")


def add_parser(subparsers) -> None:
    """Register ``motion sweep``."""
    parser = subparsers.add_parser(
        "sweep",
        help="Audition animation prompts (ARDY): many takes, one model load",
        description=__doc__,
    )
    _add_arguments(parser)


def _inner_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="forge_gen.motion.sweep --inner", description="inner half of motion sweep")
    _add_arguments(parser)
    parser.add_argument("--project", default=None)
    parser.add_argument("--created-by", default=None)
    return parser


def _forward_argv(args, out_dir: Path) -> list[str]:
    """The inner's argv, rebuilt from the parsed outer args with paths made absolute."""
    argv = ["--out-dir", str(out_dir), "--model", args.model]
    for text in args.prompt or []:
        argv += ["--prompt", text]
    if args.label:
        argv += ["--label", args.label]
    if args.prompts_file:
        argv += ["--prompts-file", str(Path(args.prompts_file).resolve())]
    argv += ["--duration", *[f"{d:g}" for d in args.duration]]
    argv += ["--samples", str(args.samples)]
    argv += ["--seeds", *[str(s) for s in args.seeds]]
    argv += ["--cfg", *[f"{c:g}" for c in args.cfg]]
    argv += ["--batch-size", str(args.batch_size)]
    if args.diffusion_steps is not None:
        argv += ["--diffusion-steps", str(args.diffusion_steps)]
    if args.history_frames is not None:
        argv += ["--history-frames", str(args.history_frames)]
    if args.no_postprocess:
        argv.append("--no-postprocess")
    project = getattr(args, "project", None)
    if project:
        argv += ["--project", str(Path(project).resolve())]
    created_by = getattr(args, "created_by", None)
    if created_by:
        argv += ["--created-by", created_by]
    return argv


def _validate(args) -> tuple[Path, list[tuple[str, str]]]:
    """Everything the outer can refuse before the GPU: prompts, grid sizes, the out-dir."""
    variants = variants_from(args)
    if args.samples < 1:
        raise UsageError("--samples must be at least 1")
    if args.batch_size < 1:
        raise UsageError("--batch-size must be at least 1")
    if any(d <= 0 for d in args.duration):
        raise UsageError("--duration must be positive")
    out_dir = Path(args.out_dir).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    return out_dir, variants


# ------------------------------------------------------------------ outer --


def run(args) -> dict:
    """Outer: validate, resolve the backend, hand the grid to the inner, return its manifest."""
    from forge_gen import backends as backends_mod
    from forge_gen import launcher

    out_dir, _ = _validate(args)
    backend = backends_mod.load_backend("ardy")
    result = launcher.run_inner_checked(backend, "motion.sweep", _forward_argv(args, out_dir))
    return _summary(result, out_dir)


def _summary(result: dict, out_dir: Path) -> dict:
    takes = result.get("takes", [])
    return {
        "record": result.get("sweep", str(out_dir / MANIFEST)),
        "outputs": [take["path"] for take in takes],
        "records": [take["record"] for take in takes],
        "takes": takes,
        "out_dir": str(out_dir),
        "fake": bool(result.get("fake", False)),
        "_text": f"wrote {len(takes)} takes to {out_dir}\n"
        + "\n".join(f"  {take['name']}" for take in takes)
        + f"\nreview: forge-gen motion review {out_dir}/*.npz --sheet {out_dir}/sheet.png",
    }


def write_manifest(out_dir: Path, *, model: str, model_repo: str | None, fps, takes: list[dict], fake: bool) -> Path:
    """``sweep.json``: what the sweep was and every take it wrote."""
    path = out_dir / MANIFEST
    payload = {
        "sweep": 1,
        "created": records.today(),
        "model": model,
        "model_repo": model_repo,
        "fps": float(fps),
        "out_dir": str(out_dir),
        "fake": fake,
        "takes": takes,
    }
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
    return path


# ------------------------------------------------------------------- fake --


def run_fake(args) -> dict:
    """Still figures in the rest pose, one per cell, with records that say ``fake: true``."""
    from forge_gen import npz, placeholders

    out_dir, variants = _validate(args)
    cells = plan_cells(variants, args.seeds, args.cfg, args.duration, args.samples)
    folder = KNOWN_MODELS.get(args.model)
    model_repo = f"{session.HF_ORG}/{folder}" if folder else None
    backend = records.backend_block(name=session.BACKEND, commit=placeholders.FAKE_COMMIT, model=model_repo)
    upstream = session.backend_upstream()
    takes = []
    for cell in cells:
        name = take_name(cell)
        path = out_dir / f"{name}.npz"
        frames = max(1, int(cell["duration"] * DEFAULT_FPS))
        npz.write_take(path, frames=frames, fps=DEFAULT_FPS, prompt=cell["prompt"])
        params = params_for(cell, args, history_frames=args.history_frames, diffusion_steps=args.diffusion_steps)
        params["model_repo"] = model_repo
        params["repo"] = upstream
        rec = session.take_record(
            npz_path=path,
            prompt=cell["prompt"],
            model_name=args.model,
            params=params,
            frames=frames,
            fps=DEFAULT_FPS,
            created_by=getattr(args, "created_by", None),
            backend=backend,
            note="placeholder take from a --fake run: a still figure in the rest pose; nothing about it is a measurement",
        )
        rec["fake"] = True
        record = records.write(rec, session.record_path_for(path))
        takes.append(dict(cell, name=name, path=str(path), record=str(record), fps=float(DEFAULT_FPS), frames=frames))
    manifest = write_manifest(out_dir, model=args.model, model_repo=model_repo, fps=DEFAULT_FPS, takes=takes, fake=True)
    return _summary({"takes": takes, "sweep": str(manifest), "fake": True}, out_dir)


# ------------------------------------------------------------------ inner --


def main_inner(argv: list[str]) -> dict:
    """Inner: load once, run the grid, write takes + records + sweep.json."""
    import numpy as np  # noqa: F401 - asserts the env before the model load
    import torch
    from ardy.motion_rep.tools import length_to_mask
    from ardy.postprocess import post_process_motion
    from ardy.tools import seed_everything, to_numpy

    args = _inner_parser().parse_args(argv)
    if args.project:
        records.set_project(args.project)
    out_dir, variants = _validate(args)

    model = session.load_model(args.model)
    device = session.device()
    fps = model.motion_rep.fps
    history_frames = args.history_frames or session.default_history_frames(model)
    diffusion_steps = args.diffusion_steps or int(model.diffusion.num_base_steps)
    repo = session.model_repo(args.model)
    upstream = session.backend_upstream()
    backend = session.backend_block(args.model)

    cells = plan_cells(variants, args.seeds, args.cfg, args.duration, args.samples)
    print(
        f"{len(cells)} takes: {len(variants)} prompts x {len(args.seeds)} seeds x {len(args.cfg)} cfg "
        f"x {len(args.duration)} durations x {args.samples} samples",
        flush=True,
    )

    takes: list[dict] = []
    for (dur, cfg, seed), group in group_cells(cells).items():
        num_frames = int(dur * fps)
        for i in range(0, len(group), args.batch_size):
            chunk = group[i : i + args.batch_size]
            # One seed per forward pass: the grid is reproducible per
            # (duration, cfg, seed, chunk), which is what the record states.
            seed_everything(seed)
            texts = [c["prompt"] for c in chunk]
            lengths = torch.tensor([num_frames] * len(chunk), device=device)
            started = time.time()
            with torch.no_grad():
                motion = model(
                    texts,
                    num_frames,
                    num_denoising_steps=diffusion_steps,
                    pad_mask=length_to_mask(lengths),
                    first_heading_angle=torch.zeros(len(chunk), device=device),
                    motion_mask=None,
                    observed_motion=None,
                    cfg_weight=(float(cfg), 2.0),
                    crop_history_length=history_frames,
                )
                output = model.motion_rep.inverse(motion, is_normalized=True)
            if not args.no_postprocess:
                output.update(
                    post_process_motion(output["local_rot_mats"], output["root_positions"], output["foot_contacts"], model.skeleton)
                )
            output = to_numpy(output)
            count = int(output["posed_joints"].shape[0])
            for sample, cell in zip(session.split_batch(output, count), chunk):
                name = take_name(cell)
                path = out_dir / f"{name}.npz"
                session.write_take(path, sample, fps=fps, prompt=cell["prompt"])
                frames = session.take_frames(sample)
                params = params_for(cell, args, history_frames=history_frames, diffusion_steps=diffusion_steps)
                params["model_repo"] = repo
                params["repo"] = upstream
                record = session.write_take_record(
                    path,
                    prompt=cell["prompt"],
                    model_name=args.model,
                    params=params,
                    frames=frames,
                    fps=fps,
                    created_by=args.created_by,
                    backend=backend,
                )
                takes.append(dict(cell, name=name, path=str(path), record=str(record), fps=float(fps), frames=frames))
            print(
                f"  {chunk[0]['label']}... x{len(chunk)}  d={dur:g} cfg={cfg:g} seed={seed}  ({time.time() - started:.0f}s)",
                flush=True,
            )

    manifest = write_manifest(out_dir, model=args.model, model_repo=repo, fps=fps, takes=takes, fake=False)
    print(f"wrote {len(takes)} takes to {out_dir}", flush=True)
    return {"takes": takes, "sweep": str(manifest), "fake": False}


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--inner":
        sys.exit(session.inner_entry(main_inner, sys.argv[2:]))
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen motion sweep ...\n")
    sys.exit(2)
