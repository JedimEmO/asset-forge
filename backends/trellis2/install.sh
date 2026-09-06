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
# Machine-local links: .env -> the runtime, .toolchain -> the conda dependency
# prefix, .checkout -> TRELLIS.2, plus installed.json. Heavy files live under
# $PREFIX (default ~/.cache/asset-forge/backends/trellis2): env/, TRELLIS.2/,
# src/ and python-3.11.16-20260901/. An adopted dependency env stays in place.
# Fresh installs use the checksum-pinned standalone runtime; adoption adds it
# only with --with-pinned-runtime. See install_runtime.py for the archive pin.
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
# Fresh installs use the pinned runtime. Adoption keeps the requested env
# unless --with-pinned-runtime explicitly asks to add the runtime layer.
for arg in "$@"; do
    case "$arg" in
        --help|-h) printf '%s\n' 'TRELLIS extra: --with-pinned-runtime adds the verified CPython runtime when adopting an existing dependency env.' >&2 ;;
    esac
done
parse_common_flags "$@"
PINNED_RUNTIME=0
[ -n "$ADOPT_ENV" ] || PINNED_RUNTIME=1
for arg in "${EXTRA_ARGS[@]}"; do
    case "$arg" in
        --with-pinned-runtime) PINNED_RUNTIME=1 ;;
        *) die "unknown flag: $arg  (--help lists them)" ;;
    esac
done

# ------------------------------------------------------------------- pins --
# These match backend.toml and the verified-facts table of 2026-08-23.

UPSTREAM="https://github.com/microsoft/TRELLIS.2"
COMMIT="75fbf0183001ed9876c8dbb35de6b68552ee08bd"
PYVER="3.11"
CUDA_LABEL="nvidia/label/cuda-12.4.1"   # the label channel: plain cuda-toolkit=12.4 floats components to 13.x
CUDA_RELEASE="12.4"
GCC_MAJOR="13"                          # CUDA 12.4's host_config.h refuses gcc > 13
TORCH_SPEC="torch==2.6.0 torchvision==0.21.0"
TORCH_INDEX="https://download.pytorch.org/whl/cu124"
TRANSFORMERS_SPEC="transformers==4.57.6" # 5.x restructured DINOv3ViTModel; the pipeline indexes model.layer directly
UTILS3D_SPEC="utils3d @ git+https://github.com/EasternJournalist/utils3d.git@9a4eb15e4021b67b12c460c7057d642626897ec8"
FLASH_ATTN_SPEC="flash-attn==2.7.3"
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
    log "adopting env $ENV_DIR ($(py -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')); leaving its packages unchanged"
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
        log "conda install --override-channels -c $CUDA_LABEL cuda-toolkit"
        "$(conda_bin)" install -y -q -p "$ENV_DIR" --override-channels -c "$CUDA_LABEL" cuda-toolkit
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
    if have_mod torch && [ "$(mod_version torch)" = "2.6.0+cu124" ]; then
        log "torch 2.6.0+cu124 present"
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

    # flash-attn: --no-build-isolation (its metadata imports torch). Absent,
    # the launcher runs with ATTN_BACKEND=sdpa — slower, correct.
    if have_mod flash_attn; then
        log "flash-attn $(mod_version flash_attn) present"
    else
        log "pip: $FLASH_ATTN_SPEC --no-build-isolation (minutes; a failure here is a warning)"
        if ! pipi -q "$FLASH_ATTN_SPEC" --no-build-isolation; then
            warn "flash-attn did not install; the lift will run with ATTN_BACKEND=sdpa (slower, correct)"
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

# ------------------------------------------------------------------ links --
# Linked before the models: hf_gate runs under .env's python.

RUNTIME_DIR="$ENV_DIR"
if [ "$PINNED_RUNTIME" = 1 ]; then
    RUNTIME_DIR="$PREFIX/python-3.11.16-20260901"
    log "installing/checking the pinned CPython runtime; dependencies stay in $ENV_DIR"
    python3 "$BACKEND_DIR/install_runtime.py" --libraries "$ENV_DIR" --destination "$RUNTIME_DIR"
fi
# CUDA/JIT uses the dependency prefix even when Python runs elsewhere.
link_extra toolchain "$ENV_DIR"
link_env "$RUNTIME_DIR"
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
