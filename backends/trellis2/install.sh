#!/usr/bin/env bash
# backends/trellis2/install.sh — host TRELLIS.2 for `forge gen mesh`.
#
#   bash backends/trellis2/install.sh [--prefix DIR] [--yes] [--no-models]
#   bash backends/trellis2/install.sh --adopt-env ~/anaconda3/envs/trellis2 \
#                                     --adopt-checkout ~/src/TRELLIS.2 [--no-models]
#
# The one conda backend: torch 2.6/cu124 plus five compiled extensions
# (nvdiffrast, CuMesh, FlexGEMM, o-voxel, flash-attn) need a CUDA 12.4
# toolkit and a gcc 13 that the host is not assumed to have, and conda's
# label channel is the one place both are pinned. Every step below is a
# dated trap in designs/hosting.md under "TRELLIS.2"; this file is the how,
# that file is the why. Idempotent: each step checks whether the env already
# holds what it would install and skips; re-running a finished install
# re-links, re-writes the receipt and re-probes.
#
# What it leaves in this directory (all gitignored): .env -> the conda
# prefix, .checkout -> the TRELLIS.2 clone at the pinned commit,
# installed.json. Everything heavy goes under $PREFIX (default
# ${FORGE_BACKENDS_HOME:-~/.cache/asset-forge/backends}/trellis2): env/,
# TRELLIS.2/, src/ for the extension sources.
#
# Two things it will never install: nvdiffrec's renderutils (non-commercial,
# texturing-only, nothing here needs it) and briaai/RMBG-2.0 (gated and
# commercially restrictive; the inner module keys alpha itself and stubs the
# class). One thing it installs only with consent: nvdiffrast 0.4.0, the
# texture baker, under the NVIDIA Source Code License (non-commercial). It
# prints the terms and needs --yes or an interactive "y"; declining leaves
# an env that does everything but bake, doctor says "partial", and
# `forge gen mesh` exits 6 naming this script.

set -euo pipefail

BACKEND_NAME=trellis2
BACKEND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../_lib/common.sh
. "$BACKEND_DIR/../_lib/common.sh"
parse_common_flags "$@"
[ "${#EXTRA_ARGS[@]}" -eq 0 ] || die "unknown flag(s): ${EXTRA_ARGS[*]}  (--help lists them)"

# ------------------------------------------------------------------- pins --
# These match backend.toml and the verified-facts table of 2026-08-23.

UPSTREAM="https://github.com/microsoft/TRELLIS.2"
COMMIT="75fbf0183001ed9876c8dbb35de6b68552ee08bd"
PYVER="3.11"
CUDA_LABEL="nvidia/label/cuda-12.8.1"   # the label channel: plain cuda-toolkit=12.4 floats components to 13.x
CUDA_RELEASE="12.8"
GCC_MAJOR="13"                          # CUDA 12.4's host_config.h refuses gcc > 13; 12.8's ceiling is the same
# EXPERIMENTAL 2026-08-27: bumped from the upstream-pinned torch==2.6.0+cu124
# because stable PyTorch has no sm_120 (RTX 50-series/Blackwell) kernels
# before 2.7.0+cu128 — cu124 detects the GPU but cannot run a kernel on it.
# This moves off Microsoft's verified combination; watch for anything
# torch-2.6-specific in TRELLIS.2's own pipeline code, not just the
# extensions (which rebuild from source against whatever torch is present).
TORCH_SPEC="torch==2.7.0 torchvision==0.22.0"
TORCH_INDEX="https://download.pytorch.org/whl/cu128"
# What `pip show torch`'s Version ends up as, e.g. "2.7.0+cu128" — derived,
# not re-typed, so the idempotency check below never drifts from the pins
# above the way a hand-copied literal did (it used to re-run the whole torch
# + flash-attn install, flash-attn's --no-build-isolation compile included,
# on every single re-run once the pins moved past what it still compared
# against).
TORCH_VERSION_EXPECT="$(printf '%s' "$TORCH_SPEC" | sed -n 's/^torch==\([0-9.]*\).*/\1/p')+$(printf '%s' "$TORCH_INDEX" | sed -n 's#.*/##p')"
TRANSFORMERS_SPEC="transformers==4.57.6" # 5.x restructured DINOv3ViTModel; the pipeline indexes model.layer directly
UTILS3D_SPEC="utils3d @ git+https://github.com/EasternJournalist/utils3d.git@9a4eb15e4021b67b12c460c7057d642626897ec8"
FLASH_ATTN_SPEC="flash-attn==2.7.3"
# trellis2's sparse sampler has no sdpa fallback (see below); this is what
# runs when flash-attn's from-source build fails. Pinned to a release whose
# own PyPI metadata pairs it with $TORCH_SPEC, installed --no-deps from
# $TORCH_INDEX so it never floats the torch pin as a side effect.
XFORMERS_SPEC="xformers==0.0.30"
# The bundled version torch pulls in as its own dependency; PyPI's 3.3.0
# wheel predates consumer Blackwell (sm_120) in its tensor-core lowering
# pass specifically (a hard C++ assertion, not a Python-catchable one:
# "getMMAVersionSafe: computeCapability not supported") — measured on an
# RTX 5080, fixed by this version upstream. Installed --no-deps for the
# same reason as xformers above.
TRITON_SPEC="triton==3.4.0"
NVDIFFRAST_URL="https://github.com/NVlabs/nvdiffrast.git"
NVDIFFRAST_TAG="v0.4.0"
CUMESH_URL="https://github.com/JeffreyXiang/CuMesh.git"
FLEXGEMM_URL="https://github.com/JeffreyXiang/FlexGEMM.git"
# Pinned 2026-08-23 to what the adopted trellis2 env actually runs: the
# 2026-08-18 build cloned both at unpinned HEAD, and these are the upstream
# heads of that day — verified by matching every tracked .py in the env's
# installed cumesh/flex_gemm packages byte-for-byte against these commits.
# Override with CUMESH_COMMIT=/FLEXGEMM_COMMIT= env vars to move a rebuild.
CUMESH_COMMIT="${CUMESH_COMMIT:-12289e1062f0603f2f0d0771b02e1395d247f26f}"
FLEXGEMM_COMMIT="${FLEXGEMM_COMMIT:-6dd94a859c26ee8246888502eada3dd8ad85532e}"
MODEL_MAIN="microsoft/TRELLIS.2-4B"
MODEL_GATED="facebook/dinov3-vitl16-pretrain-lvd1689m"

read -r -d '' NVDIFFRAST_TERMS <<'EOF' || true
nvdiffrast 0.4.0 — NVIDIA Source Code License (1-Way Commercial)

  TRELLIS.2's texture bake (o_voxel.postprocess.to_glb) rasterises through
  nvdiffrast. Nothing in `forge gen mesh` produces a textured mesh without
  it. Its licence, section 3.3, Use Limitation:

    "The Work and any derivative works thereof only may be used or intended
     for use non-commercially. [...] As used herein, 'non-commercially'
     means for research or evaluation purposes only and not for any direct
     or indirect monetary gain."

  If your project is commercial, a lifted mesh's texture came through
  software you are not licensed to use for it. `forge doctor` will warn
  for as long as nvdiffrast is installed, and every lift record carries
  texture_baker naming it. Full text: https://github.com/NVlabs/nvdiffrast/blob/v0.4.0/LICENSE.txt

  Declining leaves an env that does everything but bake: doctor reports
  "partial" and `forge gen mesh` exits 6 pointing back here.
EOF

# ---------------------------------------------------------------- helpers --

ENV_DIR="${ADOPT_ENV:-$PREFIX/env}"
CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/TRELLIS.2}"
SRC_DIR="$PREFIX/src"

# py — the env's interpreter, user site off, the build trio exported.
py() { PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" "$@"; }

# pipi ARGS... — pip into the env. The stock pip is used on purpose (not uv):
# the cu124 index trap below is a pip trap and the fix is sequenced for pip.
pipi() { py -m pip install "$@"; }

# have_mod NAME — 0 when the env imports NAME.
have_mod() { py -c "import $1" >/dev/null 2>&1; }

# mod_version NAME — its __version__ or empty.
mod_version() { py -c "import $1 as m; print(getattr(m, '__version__', ''))" 2>/dev/null || true; }

conda_bin() { echo "${CONDA_EXE:-conda}"; }

# export_build_env — what nvcc and the extension builds need, from the env
# itself: CUDA_HOME at the prefix (no system CUDA root is assumed) and the
# env's gcc 13 fronting for whatever the host has. The launcher exports the
# same trio on every run because nvdiffrast JIT-compiles its kernels on
# first use.
export_build_env() {
    export PYTHONNOUSERSITE=1
    export CUDA_HOME="$ENV_DIR"
    export CC="$ENV_DIR/bin/x86_64-conda-linux-gnu-gcc"
    export CXX="$ENV_DIR/bin/x86_64-conda-linux-gnu-g++"
    export CUDAHOSTCXX="$CXX"
    export PATH="$ENV_DIR/bin:$PATH"
}

# ---------------------------------------------------------------- checkout --

if [ -n "$ADOPT_CHECKOUT" ]; then
    [ -d "$CHECKOUT/trellis2" ] && [ -d "$CHECKOUT/o-voxel" ] || die "--adopt-checkout $CHECKOUT has no trellis2/ and o-voxel/ — not a TRELLIS.2 clone"
    head="$(git -C "$CHECKOUT" rev-parse HEAD 2>/dev/null || echo '?')"
    [ "$head" = "$COMMIT" ] || warn "adopted checkout is at ${head:0:12}, pinned is ${COMMIT:0:12}; doctor will say so"
    log "adopting checkout $CHECKOUT"
else
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT" --recursive
fi
# o-voxel vendors eigen as a submodule; a clone without it builds nothing
# and says so late. Adopted clones get the same check.
if [ -z "$(ls -A "$CHECKOUT/o-voxel/third_party/eigen" 2>/dev/null)" ]; then
    log "o-voxel/third_party/eigen is empty; initialising submodules"
    git -C "$CHECKOUT" submodule update -q --init --recursive
fi

# ---------------------------------------------------------------- the env --

if [ -n "$ADOPT_ENV" ]; then
    [ -x "$ENV_DIR/bin/python" ] || die "--adopt-env $ENV_DIR has no bin/python"
    log "adopting env $ENV_DIR ($(py -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')); installing nothing"
    if ! have_mod nvdiffrast; then
        warn "the adopted env has no nvdiffrast — the texture bake is unavailable; run without --adopt-env (with --yes) to install it after the licence"
    fi
else
    need_cmd "$(conda_bin)" "install Miniconda/Anaconda or set CONDA_EXE"
    need_cmd git
    mkdir -p "$PREFIX" "$SRC_DIR"

    # Python 3.11, not upstream's 3.10: a conda 3.10 build crashed in
    # sre_compile on torch's hipify trie regex.
    if [ -x "$ENV_DIR/bin/python" ]; then
        log "env exists: $ENV_DIR"
    else
        log "conda create -p $ENV_DIR python=$PYVER"
        "$(conda_bin)" create -y -q -p "$ENV_DIR" "python=$PYVER"
    fi

    # CUDA toolkit from the label channel. `-c nvidia cuda-toolkit=12.4`
    # installs the 12.4.1 metapackage and floats every component to 13.x;
    # --override-channels against the label is what held.
    if [ -x "$ENV_DIR/bin/nvcc" ]; then
        release="$("$ENV_DIR/bin/nvcc" --version | sed -n 's/.*release \([0-9]*\.[0-9]*\).*/\1/p')"
        if [ "$release" != "$CUDA_RELEASE" ]; then
            die "env nvcc is $release, want $CUDA_RELEASE — the components floated. Fix: conda remove -p $ENV_DIR 'cuda-*' and re-run"
        fi
        log "cuda-toolkit $release present"
    else
        log "conda install --override-channels -c $CUDA_LABEL -c defaults cuda-toolkit"
        # defaults, alongside the label: 12.8.1's cuda-nvml-dev needs
        # libstdcxx-ng >=11.2.0, which the label channel alone does not
        # carry (12.4.1's did, or it was already cached) — --override-channels
        # still keeps out anything from the user's own conda config.
        "$(conda_bin)" install -y -q -p "$ENV_DIR" --override-channels -c "$CUDA_LABEL" -c defaults cuda-toolkit
    fi
    # 12.8.1's package keeps its headers only under targets/x86_64-linux/
    # include/ (12.4.1's landed them at the top level too, or a prior conda
    # release did the linking) — every downstream build (nvdiffrast et al.)
    # expects $CUDA_HOME/include/cuda_runtime.h directly. Symlinked in,
    # never overwriting a header conda's other packages already placed there.
    if [ -d "$ENV_DIR/targets/x86_64-linux/include" ] && [ ! -e "$ENV_DIR/include/cuda_runtime.h" ]; then
        for header in "$ENV_DIR"/targets/x86_64-linux/include/*; do
            name="$(basename "$header")"
            [ -e "$ENV_DIR/include/$name" ] || ln -s "$header" "$ENV_DIR/include/$name"
        done
        log "linked $(ls "$ENV_DIR/targets/x86_64-linux/include" | wc -l) CUDA headers into $ENV_DIR/include"
    fi

    # gcc 13 in the env, exported at build time and at run time (the JIT).
    if [ -x "$ENV_DIR/bin/x86_64-conda-linux-gnu-gcc" ]; then
        log "gcc $("$ENV_DIR/bin/x86_64-conda-linux-gnu-gcc" -dumpversion) present"
    else
        log "conda install -c conda-forge gcc_linux-64=$GCC_MAJOR gxx_linux-64=$GCC_MAJOR"
        "$(conda_bin)" install -y -q -p "$ENV_DIR" -c conda-forge "gcc_linux-64=$GCC_MAJOR" "gxx_linux-64=$GCC_MAJOR"
    fi
    export_build_env

    # pip first, typing-extensions second, then torch: the cu124 index serves
    # a typing_extensions wheel whose metadata name the stock pip
    # mis-normalises, and the sdist fallback cannot see flit_core because
    # --index-url replaced PyPI.
    if have_mod torch && [ "$(mod_version torch)" = "$TORCH_VERSION_EXPECT" ]; then
        log "torch $TORCH_VERSION_EXPECT present"
    else
        log "pip: pip, typing-extensions, then $TORCH_SPEC from $TORCH_INDEX"
        pipi -q -U pip
        pipi -q typing-extensions
        # shellcheck disable=SC2086
        pipi -q $TORCH_SPEC --index-url "$TORCH_INDEX"
    fi

    if have_mod transformers && [ "$(mod_version transformers)" = "${TRANSFORMERS_SPEC#*==}" ] && have_mod kornia && have_mod psutil && have_mod utils3d; then
        log "transformers ${TRANSFORMERS_SPEC#*==} and the basics present"
    else
        log "pip: $TRANSFORMERS_SPEC and the basics"
        # psutil and ninja are here because flash-attn's build needs them
        # present before it starts; wheel/setuptools for FlexGEMM's bdist_wheel.
        # Pinned 2026-08-23 to the versions the adopted env runs (its
        # `pip list`): the line used to float on every fresh install while
        # backends/README.md claimed licences "verified from the files on
        # disk" — a claim about a moving target.
        pipi -q "$TRANSFORMERS_SPEC" \
            imageio==2.37.4 imageio-ffmpeg==0.6.0 tqdm==4.70.0 easydict==1.13 \
            opencv-python-headless==5.0.0.93 ninja==1.13.0 trimesh==5.0.0 pillow==12.3.0 \
            kornia==0.8.3 timm==1.0.28 zstandard==0.25.0 psutil==7.2.2 \
            wheel==0.47.0 setuptools==83.0.0 huggingface_hub==0.36.2 "$UTILS3D_SPEC"
    fi

    # flash-attn: --no-build-isolation (its metadata imports torch). have_mod
    # actually imports it, so it catches a stale build too (a torch bump
    # leaves the old flash_attn_2_cuda.so ABI-broken, "undefined symbol",
    # even though `pip show` still calls it 2.7.3 present) — but pip's own
    # version check does not know that, and would silently no-op a
    # reinstall of the exact same pinned version, so a broken import is
    # uninstalled first to force a real rebuild.
    #
    # sdpa is not a fallback here — trellis2/modules/sparse/config.py's own
    # ATTN whitelist is only xformers/flash_attn/flash_attn_3; the sparse
    # diffusion sampler has no sdpa path at all. xformers is the fallback,
    # pinned to a version its own PyPI metadata pairs with $TORCH_SPEC and
    # installed --no-deps: an unconstrained `pip install xformers` pulls in
    # whatever torch it wants as a dependency, silently floating the pinned
    # version everything else here was built against.
    if have_mod flash_attn; then
        log "flash-attn $(mod_version flash_attn) present"
    else
        if "$ENV_DIR/bin/python" -m pip show flash-attn >/dev/null 2>&1; then
            log "flash-attn is installed but broken (stale build against a prior torch) — uninstalling before rebuild"
            PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip uninstall -q -y flash-attn
        fi
        log "pip: $FLASH_ATTN_SPEC --no-build-isolation (minutes; a failure here is a warning)"
        pipi -q "$FLASH_ATTN_SPEC" --no-build-isolation || true
        # pip reports success even when the built extension is ABI-broken
        # (it does not run an import check) — have_mod is what actually
        # proves it, same as the check this whole branch started from.
        if ! have_mod flash_attn; then
            warn "flash-attn did not build a working import"
            # Left half-installed, it poisons xformers too:
            # xformers/ops/fmha/flash.py imports flash_attn itself at module
            # load, unconditionally. Gone entirely, that import fails cleanly
            # (ModuleNotFoundError, which xformers' own optional-backend
            # guard expects) instead of hitting the same broken .so again.
            PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip uninstall -q -y flash-attn 2>/dev/null || true
        fi
    fi
    # xformers_runs — not have_mod, and not just xformers.ops importing:
    # a prebuilt wheel for a GPU generation newer than its own kernels
    # cover (Blackwell/sm_120 against a Hopper-only build, measured)
    # imports fine and only fails "no kernel image is available" at the
    # first real launch. This is the only check that actually proves it.
    xformers_runs() {
        PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -c '
import torch, xformers.ops as xops
q = torch.randn(1, 8, 16, 64, device="cuda", dtype=torch.float16)
xops.memory_efficient_attention(q, q, q)
torch.cuda.synchronize()
' >/dev/null 2>&1
    }

    if xformers_runs; then
        log "xformers $(mod_version xformers) present and runs on this GPU"
    elif have_mod flash_attn; then
        log "flash-attn is present; xformers not needed"
    else
        if "$ENV_DIR/bin/python" -m pip show xformers >/dev/null 2>&1; then
            log "xformers is installed but does not run on this GPU — uninstalling before reinstall"
            PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip uninstall -q -y xformers
        fi
        log "pip: $XFORMERS_SPEC --no-deps --index-url $TORCH_INDEX (flash-attn's fallback — trellis2's sparse sampler has no sdpa path)"
        pipi -q "$XFORMERS_SPEC" --no-deps --index-url "$TORCH_INDEX" || true
        if xformers_runs; then
            log "xformers $(mod_version xformers) present and runs on this GPU"
        else
            # The prebuilt wheel's own kernels do not cover this GPU's
            # compute capability (a fresh GPU generation ahead of what
            # PyPI has published wheels for is the case this was written
            # for) — built from source instead, for exactly the
            # capability this GPU reports, not a guess.
            PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip uninstall -q -y xformers 2>/dev/null || true
            arch="$(PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -c 'import torch; c = torch.cuda.get_device_capability(); print(f"{c[0]}.{c[1]}")' 2>/dev/null || true)"
            if [ -z "$arch" ]; then
                warn "no CUDA device visible to determine TORCH_CUDA_ARCH_LIST — skipping the source build; \`forge gen mesh\` will refuse to run without flash-attn or xformers"
            else
                xf_src="$SRC_DIR/xformers"
                if [ ! -d "$xf_src/.git" ]; then
                    log "cloning xformers ${XFORMERS_SPEC#*==} (recursive — pulls cutlass and flash-attention's own source, a large checkout)"
                    git clone -q --recursive -b "v${XFORMERS_SPEC#*==}" https://github.com/facebookresearch/xformers.git "$xf_src"
                fi
                log "building xformers from source for TORCH_CUDA_ARCH_LIST=$arch (minutes; needs a real compiler and disk — a failure here is a warning)"
                if ! (cd "$xf_src" && TORCH_CUDA_ARCH_LIST="$arch" MAX_JOBS="${MAX_JOBS:-8}" CUDA_HOME="$ENV_DIR" CC="$ENV_DIR/bin/x86_64-conda-linux-gnu-gcc" CXX="$ENV_DIR/bin/x86_64-conda-linux-gnu-g++" CUDAHOSTCXX="$ENV_DIR/bin/x86_64-conda-linux-gnu-g++" PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip install --no-build-isolation -q .); then
                    warn "xformers did not build from source either; \`forge gen mesh\` will refuse to run without flash-attn or xformers"
                elif ! xformers_runs; then
                    warn "xformers built from source but still does not run a kernel on this GPU"
                fi
            fi
        fi
    fi

    # nvdiffrast: the texture baker, non-commercial, only with consent. The
    # prompt runs in a subshell so a decline (or no TTY without --yes) is a
    # warning and the rest of the env still gets built.
    if have_mod nvdiffrast; then
        log "nvdiffrast $(mod_version nvdiffrast) present (NVIDIA Source Code License, non-commercial)"
    elif (confirm_license "$NVDIFFRAST_TERMS"); then
        log "cloning nvdiffrast $NVDIFFRAST_TAG"
        if [ ! -d "$SRC_DIR/nvdiffrast/.git" ]; then
            git clone -q -b "$NVDIFFRAST_TAG" "$NVDIFFRAST_URL" "$SRC_DIR/nvdiffrast"
        fi
        pipi -q "$SRC_DIR/nvdiffrast" --no-build-isolation
    else
        warn "nvdiffrast declined: the texture bake is unavailable, doctor will say partial, \`forge gen mesh\` exits 6. Re-run with --yes to install it."
    fi

    # CuMesh: the mesh extraction/cleanup extension. --recursive: submodules.
    if have_mod cumesh; then
        log "cumesh present"
    else
        log "building CuMesh"
        if [ ! -d "$SRC_DIR/CuMesh/.git" ]; then
            git clone -q --recursive "$CUMESH_URL" "$SRC_DIR/CuMesh"
        fi
        [ -z "$CUMESH_COMMIT" ] || git -C "$SRC_DIR/CuMesh" checkout -q "$CUMESH_COMMIT"
        pipi -q "$SRC_DIR/CuMesh" --no-build-isolation
    fi

    # FlexGEMM: the pip install path dies in metadata generation (a
    # str-subclass JSON key deep in the hooks); build the wheel and install
    # it. Its CUDA lives in triton/JIT, so the wheel is cheap.
    if have_mod flex_gemm; then
        log "flex_gemm present"
    else
        log "building FlexGEMM via setup.py bdist_wheel"
        if [ ! -d "$SRC_DIR/FlexGEMM/.git" ]; then
            git clone -q --recursive "$FLEXGEMM_URL" "$SRC_DIR/FlexGEMM"
        fi
        [ -z "$FLEXGEMM_COMMIT" ] || git -C "$SRC_DIR/FlexGEMM" checkout -q "$FLEXGEMM_COMMIT"
        (cd "$SRC_DIR/FlexGEMM" && rm -rf dist && py setup.py -q bdist_wheel)
        pipi -q "$SRC_DIR"/FlexGEMM/dist/*.whl
    fi

    # o-voxel: in-tree under the checkout, with eigen as a submodule (checked
    # above). --no-build-isolation so it builds against the env's torch.
    if have_mod o_voxel; then
        log "o_voxel present"
    else
        log "building o-voxel from $CHECKOUT/o-voxel"
        pipi -q "$CHECKOUT/o-voxel" --no-build-isolation
    fi
fi

# --------------------------------------------------------------- triton --
# Checked last, after every other pip install above (not right after torch,
# where it was first written): FlexGEMM's own pyproject.toml declares an
# unpinned "triton>=3.2.0", and something in this env's dependency
# resolution — traced to FlexGEMM's install specifically, not confirmed
# beyond that — was silently re-landing torch's bundled 3.3.0 over a
# correctly-upgraded 3.4.0 checked earlier in the script. This is the
# single point nothing installed above can still undo: real tl.dot call,
# not an import, since every FlexGEMM kernel (this backend's whole sparse
# conv path) is Triton, and a version that predates this GPU's tensor-core
# generation imports fine, only crashing the first time a kernel needs it.
cat > "$SRC_DIR/_triton_dot_probe.py" <<'PYEOF'
import torch
import triton
import triton.language as tl

@triton.jit
def _dot_probe(a_ptr, b_ptr, c_ptr, BLOCK: tl.constexpr):
    offs = tl.arange(0, BLOCK)
    a = tl.load(a_ptr + offs[:, None] * BLOCK + offs[None, :])
    b = tl.load(b_ptr + offs[:, None] * BLOCK + offs[None, :])
    acc = tl.zeros((BLOCK, BLOCK), dtype=tl.float32)
    acc = tl.dot(a, b, acc)
    tl.store(c_ptr + offs[:, None] * BLOCK + offs[None, :], acc)

BLOCK = 32
a = torch.randn(BLOCK, BLOCK, device="cuda", dtype=torch.float16)
b = torch.randn(BLOCK, BLOCK, device="cuda", dtype=torch.float16)
c = torch.empty(BLOCK, BLOCK, device="cuda", dtype=torch.float32)
_dot_probe[(1,)](a, b, c, BLOCK=BLOCK)
torch.cuda.synchronize()
PYEOF
triton_dot_works() {
    PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" "$SRC_DIR/_triton_dot_probe.py" >/dev/null 2>&1
}
if triton_dot_works; then
    log "triton $(mod_version triton) runs tl.dot on this GPU"
else
    log "triton $(mod_version triton) does not run tl.dot on this GPU — installing $TRITON_SPEC"
    PYTHONNOUSERSITE=1 "$ENV_DIR/bin/python" -m pip uninstall -q -y triton 2>/dev/null || true
    pipi -q "$TRITON_SPEC" --no-deps
    if triton_dot_works; then
        log "triton $(mod_version triton) runs tl.dot on this GPU"
    else
        warn "triton still does not run tl.dot on this GPU; FlexGEMM's sparse conv kernels will fail at generation time"
    fi
fi

# ------------------------------------------------------------------ links --
# Linked before the models: hf_gate runs under .env's python.

link_env "$ENV_DIR"
link_checkout "$CHECKOUT"

# ----------------------------------------------------------------- models --

if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: skipping $MODEL_MAIN and $MODEL_GATED (doctor will say partial until they are cached)"
else
    # hf_download REPO — the CLI when it is on PATH, else the env's own hub client.
    hf_download() {
        if command -v hf >/dev/null 2>&1; then
            hf download "$1" >/dev/null
        else
            py -c "from huggingface_hub import snapshot_download; snapshot_download('$1')" >/dev/null
        fi
    }
    log "downloading $MODEL_MAIN (MIT)"
    hf_download "$MODEL_MAIN"
    # The image conditioner is gated: access on the model page plus a token
    # login (`hf auth login --token`, never interactive — no TTY under an
    # agent). hf_gate prints the four steps when it is refused.
    if hf_gate "$MODEL_GATED"; then
        log "downloading $MODEL_GATED (DINOv3 License, gated)"
        hf_download "$MODEL_GATED"
    else
        warn "$MODEL_GATED is not reachable; the first lift will fail until the steps above are done (doctor repeats them)"
    fi
fi

# ---------------------------------------------------------------- receipt --

write_installed_json
run_probe
log "done — \`forge doctor\` for the table; nvdiffrast, when installed, is non-commercial and doctor will keep saying so"
