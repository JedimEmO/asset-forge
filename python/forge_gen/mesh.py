"""One reference image in, one static textured .glb out, via TRELLIS.2 (a *lift*).

    forge-gen mesh <ref.png> --out <glb> --record <json>
                   [--preset character|prop] [--resolution 512|1024] [--verts N]
                   [--texture 512|1024] [--seed N] [--source TEXT] [--prompt TEXT]
                   [--created-by WHO] [--project DIR] [--json] [--fake]

Two layers, like every command in this package: the outer runs under the
system python, owns the arguments, the record and the only check that
matters — the finished file parses as one self-contained GLB
(``glb.verify_glb``, the same gate a Blender export passes, because a
TRELLIS mesh enters the same ingest gates) — and the inner re-execs under
the ``trellis2`` backend's own interpreter through the launcher. TRELLIS.2's
stack (torch 2.6/cu124 plus five compiled extensions) lives in its own env
and nothing of it may leak into the system python: torch is imported here
inside ``run_in_trellis_env`` and nowhere else.

What comes out is a *shape with a painted skin*, nothing more: no skeleton,
no skinning, arbitrary UV islands, PBR channels mostly discarded downstream.
Characters go on to ``forge-gen rig``, props to ``forge-gen prop``; this
command deliberately knows nothing about either.

The knobs and their defaults are game knobs, not showcase knobs. Upstream's
example exports a million vertices and a 4096 texture; the two presets are
the library's two registers — 25 000 vertices for a ``character`` and 6 000
for a ``prop``, both at 1024³ with a 1024² map (the 900-vertex / 512² table
that came first melted hands and sheared rivets). ``--verts`` is upstream's
``decimation_target``, a VERTEX count, not faces: ~900 vertices is ~1 800
triangles on a closed surface. Decimation happens inside TRELLIS's own
export, *before* its texture bake, so the map is baked against near-final
topology instead of being sheared by a later decimate. ``remesh=True``
always: it is what keeps the output closed enough for bone-heat weighting
downstream.

Resolution names the voxel grid (512 or 1024; 1536 exists upstream and is
refused here — it does not fit a 24 GB card). ``--seed`` makes the *local*
half reproducible: the committed reference image plus this record
re-generates the same mesh, which is as much reproducibility as a pipeline
with a cloud image model in front can honestly claim — the record says
exactly that, and it also names the texture baker, because nvdiffrast is
non-commercial and that is a licence fact, not a detail.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import traceback
from dataclasses import dataclass
from pathlib import Path

if __package__ in (None, ""):
    # Run as a file rather than as `-m forge_gen.mesh`: put the package's
    # parent on the path so the "run me through forge-gen" line below can
    # print instead of an import error.
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from forge_gen import exit_codes, glb, launcher, placeholders, records  # noqa: E402
from forge_gen.backends import load_backend  # noqa: E402
from forge_gen.exit_codes import BackendFailed, ForgeGenError, InputRejected, MissingTool, UsageError  # noqa: E402

#: The backend directory and the tool name the record carries.
BACKEND = "trellis2"
TOOL = "trellis2"

#: The pipeline weights on the hub (MIT).
MODEL_ID = "microsoft/TRELLIS.2-4B"

#: Voxel grid → upstream ``pipeline_type``. 1536 is listed so the refusal can name it.
PIPELINE_TYPES = {512: "512", 1024: "1024_cascade", 1536: "1536_cascade"}

#: What a 24 GB card runs.
RESOLUTIONS = (512, 1024)

#: Texture atlas sizes the presets speak.
TEXTURE_SIZES = (512, 1024)

#: nvdiffrast's own vertex limit, upstream's cap before the bake.
SIMPLIFY_CAP = 16_777_216

#: Named in every lift record. A licence fact: keep it there.
TEXTURE_BAKER = "nvdiffrast (NVIDIA Source Code License, non-commercial)"

#: The eight bytes every PNG starts with.
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

#: The alpha keyer's gates: how flat a border must be, how close to it a
#: pixel must be to count as backdrop, and the band of subject coverage that
#: is still "an isolated subject on a flat background".
BORDER_SPREAD_MAX = 24
KEY_TOLERANCE = 28
SUBJECT_BAND = (0.05, 0.95)


@dataclass(frozen=True)
class Preset:
    """One register: voxel grid, decimation target in vertices, texture size."""

    resolution: int
    verts: int
    texture: int


#: The library's two registers. Stated in vertices on purpose (see the docstring).
PRESETS = {
    "character": Preset(resolution=1024, verts=25000, texture=1024),
    "prop": Preset(resolution=1024, verts=6000, texture=1024),
}

#: The preset when none is named: the old tool's defaults, which were the prop's.
DEFAULT_PRESET = "prop"


# ------------------------------------------------------------------ parser --


def add_parser(subparsers) -> None:
    """Register ``mesh``."""
    parser = subparsers.add_parser(
        "mesh",
        help="Reference PNG -> textured mesh via TRELLIS.2 (a lift)",
        description=__doc__,
    )
    parser.add_argument("image", help="the reference PNG (a committed source; flat background, subject alone)")
    parser.add_argument("--out", required=True, metavar="GLB", help="where the lifted .glb goes")
    parser.add_argument("--record", required=True, metavar="JSON", help="where the lift record goes (beside the PNG as <name>.lift.json is where promote looks)")
    parser.add_argument("--preset", choices=sorted(PRESETS), default=DEFAULT_PRESET, help=f"the register: {', '.join(f'{k} = {v.resolution}³/{v.verts} verts/{v.texture}²' for k, v in PRESETS.items())} (default {DEFAULT_PRESET})")
    parser.add_argument("--resolution", type=int, choices=sorted(PIPELINE_TYPES), metavar="512|1024", help="voxel grid; 1536 is refused (24 GB)")
    parser.add_argument("--verts", type=int, metavar="N", help="decimation target, VERTICES, before the bake (the preset's unless given)")
    parser.add_argument("--texture", type=int, choices=TEXTURE_SIZES, metavar="512|1024", help="texture atlas size (the preset's unless given)")
    parser.add_argument("--seed", type=int, default=42, help="the sampler seed; one front view underdetermines a shape, and the seed is the knob (default 42)")
    parser.add_argument("--source", metavar="TEXT", help="where the image came from, for the record's input (e.g. 'grok image_edit from the style board')")
    parser.add_argument("--prompt", metavar="TEXT", help="the prompt the image was made with; default: the first paragraph of a sibling <ref>.txt when one exists")


# --------------------------------------------------------------- settling --


@dataclass
class Settled:
    """The arguments after validation: absolute paths, the knobs resolved from the preset."""

    image: Path
    out: Path
    record: Path
    preset: str
    resolution: int
    verts: int
    texture: int
    seed: int
    source: str | None
    prompt: str | None
    prompt_from: str | None

    @property
    def pipeline_type(self) -> str:
        return PIPELINE_TYPES[self.resolution]


def check_png(path: Path) -> None:
    """The reference must exist and be a PNG — the signature, not the suffix."""
    if not path.is_file():
        raise InputRejected(f"reference image {path} does not exist", image=str(path))
    with open(path, "rb") as handle:
        head = handle.read(len(PNG_SIGNATURE))
    if head != PNG_SIGNATURE:
        raise InputRejected(f"{path} is not a PNG (the file does not start with the PNG signature)", image=str(path))


def sibling_prompt(image: Path) -> str | None:
    """The first paragraph of ``<ref>.txt`` beside the image, when there is one.

    The reference skill writes the prompt it used beside the PNG; the record
    carries it so a later reader knows what the picture was asked to be.
    """
    sibling = image.with_suffix(".txt")
    if not sibling.is_file():
        return None
    try:
        text = sibling.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    for paragraph in text.replace("\r\n", "\n").split("\n\n"):
        lines = [line.strip() for line in paragraph.splitlines() if line.strip()]
        if lines:
            return " ".join(lines)
    return None


def settle(args) -> Settled:
    """Validate and resolve: paths absolute, knobs from the preset unless given, 1536 refused."""
    image = Path(args.image).expanduser().resolve()
    check_png(image)
    out = Path(args.out).expanduser().resolve()
    if out.suffix.lower() != ".glb":
        raise UsageError(f"--out {out} must end in .glb — the exporter picks its format by extension")
    record = Path(args.record).expanduser().resolve()
    preset = PRESETS[args.preset]
    resolution = args.resolution if args.resolution is not None else preset.resolution
    if resolution not in RESOLUTIONS:
        raise UsageError(
            f"{resolution}³ is refused: 512³ and 1024³ both complete on a 24 GB card beside a desktop (~0.8 GB); "
            "the full-run peak at 1024³ was never pinned down and 1536³ does not fit. Measure one before being tempted."
        )
    verts = args.verts if args.verts is not None else preset.verts
    if verts < 3:
        raise UsageError(f"--verts {verts}: a decimation target is a vertex count (the presets say {PRESETS['prop'].verts} and {PRESETS['character'].verts})")
    texture = args.texture if args.texture is not None else preset.texture
    prompt = args.prompt
    prompt_from = "flag" if prompt else None
    if not prompt:
        prompt = sibling_prompt(image)
        prompt_from = "sibling" if prompt else None
    return Settled(
        image=image,
        out=out,
        record=record,
        preset=args.preset,
        resolution=resolution,
        verts=verts,
        texture=texture,
        seed=args.seed,
        source=args.source,
        prompt=prompt,
        prompt_from=prompt_from,
    )


# ------------------------------------------------------------------ record --


def _git_head(checkout: Path) -> str | None:
    """The checkout's commit, or ``None`` — a record says null before it guesses."""
    try:
        probe = subprocess.run(
            ["git", "-C", str(checkout), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    head = probe.stdout.strip()
    return head or None


def lift_params(settled: Settled, *, attn_backend: str | None) -> dict:
    """Every knob, stated. The keys are what ``forge_library``'s ``lift_params`` reads, plus the ones it tolerates."""
    return {
        "preset": settled.preset,
        "resolution": settled.resolution,
        "pipeline_type": settled.pipeline_type,
        "seed": settled.seed,
        "decimation_target_vertices": settled.verts,
        "texture_size": settled.texture,
        "remesh": True,
        "attn_backend": attn_backend,
        "texture_baker": TEXTURE_BAKER,
        "prompt_from": settled.prompt_from,
    }


def build_record(settled: Settled, *, created_by: str | None, backend: dict, measured: dict, attn_backend: str | None, fake: bool) -> dict:
    """The lift record: kind ``lift``, tool ``trellis2``, the image as its one input, the glb as its one output.

    The input's role is ``image`` — the Rust reader's ``lift_params`` looks
    that role up by name for the sidecar's ``image``/``image_sha256``.
    """
    if fake:
        rec = placeholders.fake_record("lift", TOOL, backend=BACKEND, created_by=created_by, model=MODEL_ID)
    else:
        rec = records.new_record("lift", TOOL, created_by=created_by)
        rec["backend"] = records.backend_block(
            name=BACKEND,
            commit=backend.get("commit"),
            python=backend.get("python"),
            torch=backend.get("torch"),
            model=backend.get("model") or MODEL_ID,
            model_revision=backend.get("model_revision"),
        )
    records.add_input(rec, "image", settled.image, source=settled.source, prompt=settled.prompt)
    rec["params"] = lift_params(settled, attn_backend=attn_backend)
    records.add_output(rec, settled.out)
    rec["measured"] = dict(measured)
    return rec


def _summary(settled: Settled, measured: dict, *, fake: bool) -> str:
    what = "placeholder (--fake)" if fake else "lifted"
    return (
        f"mesh: {what} {settled.image.name} -> {settled.out} — "
        f"{measured.get('vertices')} verts, {measured.get('triangles')} tris, {measured.get('images')} image(s); "
        f"record {settled.record}"
    )


# ------------------------------------------------------------- outer layer --


def run(args) -> dict:
    """Validate, resolve the backend (exit 3 before any GPU work), run the inner half, verify, record."""
    settled = settle(args)
    backend = load_backend(BACKEND)
    launcher.resolve_interpreter(backend)  # MissingBackend here, in ~100 ms, never after a model load
    settled.out.parent.mkdir(parents=True, exist_ok=True)

    inner_argv = [
        str(settled.image),
        "--out",
        str(settled.out),
        "--resolution",
        str(settled.resolution),
        "--verts",
        str(settled.verts),
        "--texture",
        str(settled.texture),
        "--seed",
        str(settled.seed),
    ]
    result = launcher.run_inner_checked(backend, "mesh", inner_argv)

    # The only claim trusted: the file the *next* tool will read is one
    # self-contained GLB. Same check, same code, as the export gate.
    try:
        info = glb.verify_glb(settled.out)
    except glb.GlbError as err:
        raise BackendFailed(f"the lift wrote a file that is not one self-contained glb: {err}") from err

    measured = dict(result.get("measured") or {})
    measured["images"] = info["images"]
    backend_facts = dict(result.get("backend") or {})
    backend_facts.setdefault("commit", _git_head(backend.checkout) if backend.checkout.exists() else None)
    rec = build_record(
        settled,
        created_by=getattr(args, "created_by", None),
        backend=backend_facts,
        measured=measured,
        attn_backend=result.get("attn_backend"),
        fake=False,
    )
    records.write(rec, settled.record)
    print(glb.report(settled.out, info), flush=True)
    return {
        "record": str(settled.record),
        "outputs": [str(settled.out)],
        "measured": measured,
        "_text": _summary(settled, measured, fake=False),
    }


def run_fake(args) -> dict:
    """No env, no torch: a placeholder glb that passes ``verify_glb`` and a record that says ``fake``."""
    settled = settle(args)
    placeholders.placeholder_glb(settled.out, name=settled.image.stem)
    info = glb.verify_glb(settled.out)
    measured = {
        "vertices": 3,
        "triangles": 1,
        "images": info["images"],
        "keyed_subject_fraction": None,
    }
    rec = build_record(
        settled,
        created_by=getattr(args, "created_by", None),
        backend={},
        measured=measured,
        attn_backend=None,
        fake=True,
    )
    records.write(rec, settled.record)
    return {
        "record": str(settled.record),
        "outputs": [str(settled.out)],
        "measured": measured,
        "_text": _summary(settled, measured, fake=True),
    }


# ------------------------------------------------------------- inner layer --


def _inner_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="forge_gen.mesh --inner", add_help=True)
    parser.add_argument("image")
    parser.add_argument("--out", required=True)
    parser.add_argument("--resolution", type=int, required=True)
    parser.add_argument("--verts", type=int, required=True)
    parser.add_argument("--texture", type=int, required=True)
    parser.add_argument("--seed", type=int, default=42)
    return parser


def _model_revision() -> str | None:
    """The hub snapshot the pipeline loaded, from the cache path of its ``pipeline.json``; ``None`` when unknowable."""
    try:
        from huggingface_hub import hf_hub_download  # noqa: PLC0415 - inner only

        path = Path(hf_hub_download(MODEL_ID, "pipeline.json"))
    except Exception:  # noqa: BLE001 - a missing revision is null, not a failure
        return None
    parts = path.parts
    if "snapshots" in parts:
        index = parts.index("snapshots")
        if index + 1 < len(parts):
            return parts[index + 1]
    return None


def keyed(image):
    """Return ``(image with a real alpha channel, subject fraction or None)``.

    An image that already carries one is trusted as-is. Otherwise the flat
    background the reference prompts demand is keyed away: border pixels vote
    for the background color, everything within tolerance of it that is
    *connected to the border* becomes transparent. Interior regions survive
    even if they match the background color, so a grey shield boss does not
    get punched out of a grey-backed image. Fails loudly when the border is
    not flat — that is a reference image defect, and the fix is the prompt
    ("plain flat light-grey background"), not a looser tolerance here.
    """
    import cv2  # noqa: PLC0415 - inner only
    import numpy as np  # noqa: PLC0415
    from PIL import Image  # noqa: PLC0415

    rgba = np.array(image.convert("RGBA"))
    if not np.all(rgba[:, :, 3] == 255):
        return image, None  # real alpha already present

    rgb = rgba[:, :, :3].astype(np.int16)
    border = np.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]])
    background = np.median(border, axis=0)
    background_rgb = tuple(int(v) for v in background)
    spread = np.abs(border - background).max(axis=1)
    if np.percentile(spread, 90) > BORDER_SPREAD_MAX:
        raise InputRejected(
            f"the image border is not a flat background (median RGB {background_rgb}, 90th-percentile spread "
            f"{float(np.percentile(spread, 90)):.0f} > {BORDER_SPREAD_MAX}) — cannot key alpha. "
            "Regenerate the reference with a plain flat backdrop.",
            background_rgb=list(background_rgb),
        )

    candidate = (np.abs(rgb - background).max(axis=2) <= KEY_TOLERANCE).astype(np.uint8)
    count, labels = cv2.connectedComponents(candidate, connectivity=4)
    edge_labels = set(np.unique(np.concatenate([labels[0], labels[-1], labels[:, 0], labels[:, -1]]))) - {0}
    is_background = np.isin(labels, list(edge_labels)) & (candidate == 1)

    alpha = np.where(is_background, 0, 255).astype(np.uint8)
    alpha = cv2.morphologyEx(alpha, cv2.MORPH_OPEN, np.ones((3, 3), np.uint8))
    fraction = float((alpha > 0).mean())
    low, high = SUBJECT_BAND
    if not low <= fraction <= high:
        raise InputRejected(
            f"alpha keying kept {fraction:.0%} of the image as subject against a background of RGB {background_rgb} — "
            "that is not an isolated subject on a flat background",
            background_rgb=list(background_rgb),
            keyed_subject_fraction=fraction,
        )
    print(f"mesh: keyed background, subject covers {fraction:.0%}", flush=True)
    rgba[:, :, 3] = alpha
    return Image.fromarray(rgba), fraction


def run_in_trellis_env(ns: argparse.Namespace) -> dict:
    """Runs inside the trellis2 env. Env vars first, imports second —
    the attention backend and the allocator both read the environment at
    import time, not at call time."""
    os.environ["OPENCV_IO_ENABLE_OPENEXR"] = "1"
    os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
    # nvdiffrast JIT-compiles its kernels on first use; the CUDA toolkit lives
    # inside this conda env (no system CUDA root is assumed), and the env's
    # gcc 13 must front for a system gcc 14, which CUDA 12.4 refuses. The
    # launcher exports the same trio from backend.toml; this is for a hand
    # run under the env's python.
    os.environ.setdefault("CUDA_HOME", sys.prefix)
    conda_gcc = os.path.join(sys.prefix, "bin", "x86_64-conda-linux-gnu-gcc")
    conda_gxx = os.path.join(sys.prefix, "bin", "x86_64-conda-linux-gnu-g++")
    if os.path.exists(conda_gcc):
        os.environ.setdefault("CC", conda_gcc)
        os.environ.setdefault("CXX", conda_gxx)
        os.environ.setdefault("CUDAHOSTCXX", conda_gxx)
    if "ATTN_BACKEND" not in os.environ:
        try:
            import flash_attn  # noqa: F401, PLC0415
        except ImportError:
            os.environ["ATTN_BACKEND"] = "sdpa"
    attn_backend = os.environ.get("ATTN_BACKEND", "flash_attn")

    # The texture baker is consent-gated at install; without it there is no
    # bake and nothing to lift. Said here, before any weight is loaded.
    try:
        import nvdiffrast  # noqa: F401, PLC0415
    except ImportError as err:
        raise MissingTool(
            "nvdiffrast is not installed in the trellis2 env — the texture bake is unavailable",
            tool="nvdiffrast",
            hint="bash backends/trellis2/install.sh --yes   (nvdiffrast is under the NVIDIA Source Code License, "
            "non-commercial; the installer prints the terms and needs consent)",
        ) from err

    # The trellis2 package is a plain directory in the checkout, not an
    # installed distribution; the launcher runs from there and names it.
    trellis_dir = os.environ.get("TRELLIS2_DIR") or os.getcwd()
    if trellis_dir not in sys.path:
        sys.path.insert(0, trellis_dir)
    from PIL import Image  # noqa: PLC0415
    import torch  # noqa: PLC0415
    from trellis2.pipelines import Trellis2ImageTo3DPipeline  # noqa: PLC0415
    from trellis2.pipelines import rembg as trellis_rembg  # noqa: PLC0415
    import o_voxel  # noqa: PLC0415

    # The pipeline eagerly loads briaai/RMBG-2.0 (gated, commercially
    # restrictive) purely to cut backgrounds. Our references are flat-background
    # by construction and the pipeline's own preprocess skips rembg whenever the
    # input carries real alpha — so the alpha is keyed here (deterministic
    # border flood) and the model is stubbed out before it can be downloaded.
    class _AlphaOnly:
        def __init__(self, *arguments, **keywords):
            pass

        def to(self, *arguments, **keywords):
            return self

        def cuda(self):
            return self

        def cpu(self):
            return self

        def __call__(self, image):
            raise RuntimeError(
                "mesh: rembg is stubbed out — the input reached the "
                "pipeline without alpha, which means background keying failed"
            )

    trellis_rembg.BiRefNet = _AlphaOnly

    image, fraction = keyed(Image.open(ns.image))
    print(f"mesh: loading {MODEL_ID}", flush=True)
    pipeline = Trellis2ImageTo3DPipeline.from_pretrained(MODEL_ID)
    pipeline.cuda()
    pipeline_type = PIPELINE_TYPES[ns.resolution]

    print(f"mesh: running {pipeline_type} seed {ns.seed} (attn {attn_backend})", flush=True)
    mesh = pipeline.run(
        image,
        seed=ns.seed,
        pipeline_type=pipeline_type,
    )[0]
    mesh.simplify(SIMPLIFY_CAP)  # nvdiffrast limit, upstream's own cap

    print(f"mesh: baking — decimating to {ns.verts} vertices, {ns.texture}² texture, remesh", flush=True)
    out = o_voxel.postprocess.to_glb(
        vertices=mesh.vertices,
        faces=mesh.faces,
        attr_volume=mesh.attrs,
        coords=mesh.coords,
        attr_layout=mesh.layout,
        voxel_size=mesh.voxel_size,
        aabb=[[-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]],
        decimation_target=ns.verts,
        texture_size=ns.texture,
        remesh=True,
        verbose=True,
    )
    os.makedirs(os.path.dirname(os.path.abspath(ns.out)), exist_ok=True)
    # trimesh's default GLB embedding is PNG; upstream demos pass
    # extension_webp=True, which bevy_gltf cannot decode — deliberately not that.
    out.export(ns.out)
    vertices = int(len(out.vertices))
    triangles = int(len(out.faces))
    print(f"mesh: inner wrote {ns.out} — {vertices} verts, {triangles} tris", flush=True)

    info = glb.verify_glb(ns.out)
    return {
        "ok": True,
        "glb": os.path.abspath(ns.out),
        "measured": {
            "vertices": vertices,
            "triangles": triangles,
            "images": info["images"],
            "keyed_subject_fraction": fraction,
        },
        "backend": {
            "python": ".".join(map(str, sys.version_info[:3])),
            "torch": torch.__version__,
            "model": MODEL_ID,
            "model_revision": _model_revision(),
        },
        "attn_backend": attn_backend,
    }


def main_inner(argv: list[str]) -> int:
    """The inner entry: run under the backend's interpreter, speak the exit-code table, last stdout line JSON."""
    ns = _inner_parser().parse_args(argv)
    ns.image = os.path.abspath(ns.image)
    ns.out = os.path.abspath(ns.out)
    if ns.resolution not in PIPELINE_TYPES:
        err = UsageError(f"resolution {ns.resolution} is not one of {sorted(PIPELINE_TYPES)}")
        print(json.dumps(err.payload()), flush=True)
        return err.code
    try:
        result = run_in_trellis_env(ns)
    except ForgeGenError as err:
        sys.stderr.write(f"mesh: {err.error}: {err.message}\n")
        print(json.dumps(err.payload()), flush=True)
        return err.code
    except glb.GlbError as err:
        failure = BackendFailed(f"the lift wrote a file that is not one self-contained glb: {err}")
        sys.stderr.write(f"mesh: {failure.message}\n")
        print(json.dumps(failure.payload()), flush=True)
        return failure.code
    except Exception as err:  # noqa: BLE001 - the last line must still be JSON
        traceback.print_exc()
        message = f"{err.__class__.__name__}: {err}"
        if "out of memory" in str(err).lower():
            message += " — TRELLIS.2 at 1024³ needs the card alone (~22 GB); `forge gpu` says who else holds it"
        failure = BackendFailed(message)
        print(json.dumps(failure.payload()), flush=True)
        return failure.code
    print(json.dumps(result), flush=True)
    return exit_codes.OK


if __name__ == "__main__":
    _argv = sys.argv[1:]
    if _argv and _argv[0] == "--inner":
        sys.exit(main_inner(_argv[1:]))
    sys.stderr.write("forge_gen.mesh: run me through forge-gen (python3 python/forge_gen mesh <ref.png> --out ... --record ...)\n")
    sys.exit(exit_codes.USAGE)
