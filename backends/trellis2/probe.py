#!/usr/bin/env python
"""Does the trellis2 env hold what `forge gen mesh` will import? One JSON line, no weights.

Run by ``forge doctor`` (and by ``install.sh`` at the end) *inside* the
backend's interpreter, from the upstream checkout, with ``backend.toml``'s
``[env]`` exported. It imports what the inner module imports — the
``trellis2.pipelines`` module, ``o_voxel`` (the exporter), the compiled extensions
it calls into (``nvdiffrast``, ``cumesh``, ``flex_gemm``) — and reports
torch's version and whether it sees a CUDA device. ``flash_attn`` is
optional: its absence is ``extras.attn_backend = "sdpa"``, slower and
correct, not a failure.

Nothing here loads a model or touches the GPU beyond ``is_available``; the
probe is the proof the *install* worked, and doctor's model checks are the
proof the weights are there.

The last stdout line is the JSON object doctor reads::

    {"ok": true, "python": "3.11.15", "torch": "2.6.0+cu124", "torch_cuda": "12.4",
     "cuda_available": true, "imports": {"trellis2.pipelines": true, ...},
     "extras": {"attn_backend": "flash_attn", "nvcc": "12.4", "nvdiffrast": "0.4.0"},
     "notices": [], "hints": [...]}

Exit 0 when every must-import succeeds (CUDA absence is a doctor check,
not a probe failure); 1 otherwise, still with the JSON line so doctor can
say *which* import.
"""

from __future__ import annotations

import importlib
import json
import os
import re
import subprocess
import sys

#: What the inner module cannot run without.
MUST_IMPORT = ("trellis2.pipelines", "o_voxel", "nvdiffrast", "cumesh", "flex_gemm", "PIL", "cv2", "trimesh")

#: Nice to have; the launcher falls back without it.
OPTIONAL = ("flash_attn",)

NVDIFFRAST_HINT = (
    "the texture bake is unavailable without nvdiffrast (non-commercial licence): "
    "bash backends/trellis2/install.sh --yes   after reading the terms it prints"
)


def _try_import(name: str) -> tuple[bool, str | None, str | None]:
    """``(ok, version, error)`` for one module; never raises."""
    try:
        module = importlib.import_module(name)
    except Exception as err:  # noqa: BLE001 - a compiled extension can fail with anything
        return False, None, f"{err.__class__.__name__}: {err}"
    version = None
    try:
        # utils3d's __version__ is a lazy submodule that is not there; any
        # attribute access can raise, so it is read defensively.
        value = getattr(module, "__version__", None)
        version = str(value) if isinstance(value, (str, int, float)) else None
    except Exception:  # noqa: BLE001
        version = None
    return True, version, None


def _nvcc_version() -> str | None:
    """The toolkit in ``$CUDA_HOME`` (the env), from ``nvcc --version``."""
    home = os.environ.get("CUDA_HOME") or sys.prefix
    nvcc = os.path.join(home, "bin", "nvcc")
    if not os.path.exists(nvcc):
        return None
    try:
        out = subprocess.run([nvcc, "--version"], capture_output=True, text=True, timeout=20, check=False).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    match = re.search(r"release (\d+\.\d+)", out)
    return match.group(1) if match else None


def main() -> int:
    # The checkout is the cwd (backend.toml says cwd = "checkout") and the
    # launcher names it in TRELLIS2_DIR; the trellis2 package is a plain
    # directory there, not an installed distribution.
    checkout = os.environ.get("TRELLIS2_DIR") or os.getcwd()
    if checkout not in sys.path:
        sys.path.insert(0, checkout)

    report: dict = {
        "ok": False,
        "python": ".".join(map(str, sys.version_info[:3])),
        "torch": None,
        "torch_cuda": None,
        "cuda_available": False,
        "imports": {},
        "versions": {},
        "errors": {},
        "extras": {},
        "notices": [],
        "hints": [],
    }

    ok, version, error = _try_import("torch")
    if ok:
        import torch  # noqa: PLC0415 - inside the env only

        report["torch"] = torch.__version__
        report["torch_cuda"] = getattr(torch.version, "cuda", None)
        try:
            report["cuda_available"] = bool(torch.cuda.is_available())
        except Exception as err:  # noqa: BLE001
            report["cuda_available"] = False
            report["errors"]["cuda"] = f"{err.__class__.__name__}: {err}"
    else:
        report["errors"]["torch"] = error
    report["imports"]["torch"] = ok

    for name in MUST_IMPORT:
        ok, version, error = _try_import(name)
        report["imports"][name] = ok
        if version:
            report["versions"][name] = version
        if error:
            report["errors"][name] = error

    for name in OPTIONAL:
        ok, version, error = _try_import(name)
        if version:
            report["versions"][name] = version
        if name == "flash_attn":
            report["extras"]["attn_backend"] = "flash_attn" if ok else "sdpa"
            if not ok:
                report["hints"].append("flash-attn is absent; the lift runs with ATTN_BACKEND=sdpa (slower, correct)")

    ok, version, error = _try_import("transformers")
    if version:
        report["versions"]["transformers"] = version
        if not version.startswith("4."):
            report["hints"].append(
                f"transformers {version}: 5.x restructured DINOv3ViTModel and the pipeline indexes model.layer directly; pin 4.57.6"
            )

    nvcc = _nvcc_version()
    if nvcc:
        report["extras"]["nvcc"] = nvcc
    else:
        report["errors"]["cuda_toolchain"] = "no working nvcc under $CUDA_HOME"
        report["hints"].append("no nvcc under $CUDA_HOME — rerun the TRELLIS installer to restore its CUDA toolchain link")

    # backend.toml's [[notices]] carries the non-commercial warning doctor
    # prints while nvdiffrast is installed; the probe only adds the version
    # it saw, and the install line when it is absent.
    if report["imports"].get("nvdiffrast"):
        report["extras"]["nvdiffrast"] = report["versions"].get("nvdiffrast") or "present"
    else:
        report["hints"].append(NVDIFFRAST_HINT)

    for key in ("CC", "CXX"):
        compiler = os.environ.get(key)
        if not compiler or not os.path.isfile(compiler) or not os.access(compiler, os.X_OK):
            report["errors"][key] = f"{key}={compiler!r} is not an executable compiler"
            report["hints"].append(f"restore the TRELLIS toolchain: {key} must name its conda compiler")

    report["ok"] = (all(report["imports"].get(name, False) for name in ("torch", *MUST_IMPORT))
                    and nvcc is not None and not any(key in report["errors"] for key in ("CC", "CXX")))
    print(json.dumps(report))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
