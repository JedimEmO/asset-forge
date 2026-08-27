#!/usr/bin/env bash
# Install (or adopt) the ARDY backend: a python 3.12 venv with the pinned
# clone installed editable, the released `core` model in the HF cache, and
# the hand-assembled LLM2Vec text encoder under the prefix. Idempotent.
#
#   bash backends/ardy/install.sh [--prefix DIR] [--no-models] [--yes]
#   bash backends/ardy/install.sh --adopt-env DIR --adopt-checkout DIR \
#                                 --adopt-text-encoders DIR [--no-models]
#
# What it leaves in backends/ardy/: .env, .checkout, .text-encoders (symlinks,
# gitignored) and installed.json. Everything heavy lives under $PREFIX or
# wherever --adopt-* pointed. The why of every step is designs/hosting.md
# under "ARDY"; the short version:
#   - transformers==5.8.1 and numpy<2 are what the clone pins; nothing floats.
#   - the text encoder is Llama-3-8B-Instruct (NousResearch mirror, same
#     weights, no gate) with the LLM2Vec MNTP adapter merged into a full
#     model, then the supervised adapter on top — assemble_text_encoder.py
#     does it inside the env, ~31 GB on disk, and needs the Llama 3 notice
#     accepted (--yes without a TTY).
#   - ~16 GB VRAM per sweep; never beside TRELLIS, MOSS or the ACE-Step server.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="ardy"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

UPSTREAM="https://github.com/nv-tlabs/ardy"
COMMIT="693f74d13b3d04a0a22ce127ee79c929dd89756b"
PYVER="3.12"
MODEL_ID="nvidia/ARDY-Core-RP-20FPS-Horizon40"
# Same pin trellis2 uses (verified on an RTX 5080/Blackwell): the clone's own
# ">=2.4.0a0" is satisfied by anything recent, so there is no real
# constraint pulling this toward a different version.
ARDY_TORCH_SPEC="torch==2.7.0 torchvision==0.22.0"
ARDY_TORCH_INDEX="https://download.pytorch.org/whl/cu128"

CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/checkout}"
ENV_DIR="${ADOPT_ENV:-$PREFIX/env}"
TEXT_ENCODERS="${ADOPT_TEXT_ENCODERS:-$PREFIX/text-encoders}"

LLAMA_NOTICE='The ARDY text encoder is built from Meta-Llama-3-8B-Instruct (via the
NousResearch mirror) under the Llama 3 Community License, which asks that
anything built with it say "Built with Meta Llama 3". Here the encoder only
conditions generation — it never ships inside an asset — and the backend
notice carries the attribution. The assembly downloads ~16 GB of weights and
writes ~31 GB under '"$TEXT_ENCODERS"'.'

# ------------------------------------------------------------------- clone --

if [ -n "$ADOPT_CHECKOUT" ]; then
    log "adopting checkout $CHECKOUT"
    head="$(git -C "$CHECKOUT" rev-parse HEAD 2>/dev/null || echo '?')"
    [ "$head" = "$COMMIT" ] || warn "adopted checkout is at ${head:0:12}, pinned is ${COMMIT:0:12} (doctor will say so)"
else
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT"
fi

# --------------------------------------------------------------------- env --

if [ -n "$ADOPT_ENV" ]; then
    log "adopting env $ENV_DIR"
else
    make_venv "$ENV_DIR" "$PYVER"
    # `-e` so ardy/assets (skeleton definitions) resolve from the clone, as
    # upstream's own scripts expect; the clone pins transformers==5.8.1 and
    # numpy<2 itself. matplotlib is ours: the review sheets.
    log "pip install -e $CHECKOUT + matplotlib"
    pip_install "$ENV_DIR" -e "$CHECKOUT" "matplotlib>=3.8"
fi

link_env "$ENV_DIR"
link_checkout "$CHECKOUT"
python="$(env_python)"

# torch — the clone's own pyproject.toml pins a bare "torch>=2.4.0a0",
# written for an NGC container that already has a CUDA-matched torch
# preinstalled (the comment there says so); outside one, pip grabs PyPI's
# default CPU-only wheel to satisfy that same constraint, silently, and
# nothing said so until a sweep just ran forever. Checked with a real CUDA
# call, not an import: CPU torch imports fine.
torch_has_cuda() {
    PYTHONNOUSERSITE=1 "$python" -c "import torch, sys; sys.exit(0 if torch.cuda.is_available() else 1)" >/dev/null 2>&1
}
if torch_has_cuda; then
    log "torch $(PYTHONNOUSERSITE=1 "$python" -c 'import torch; print(torch.__version__)') has CUDA"
else
    log "torch has no CUDA (the clone's own pin let pip grab a CPU wheel) — installing $ARDY_TORCH_SPEC from $ARDY_TORCH_INDEX"
    # shellcheck disable=SC2086
    pip_install "$ENV_DIR" $ARDY_TORCH_SPEC --index-url "$ARDY_TORCH_INDEX"
    if torch_has_cuda; then
        log "torch $(PYTHONNOUSERSITE=1 "$python" -c 'import torch; print(torch.__version__)') has CUDA"
    else
        warn "torch still has no CUDA; \`forge gen motion sweep\` will run on CPU (very slow, not refused)"
    fi
fi

# ------------------------------------------------------------------ models --

if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: skipping $MODEL_ID and the text encoder (doctor will say partial)"
else
    log "fetching $MODEL_ID into the Hugging Face cache"
    if command -v hf >/dev/null 2>&1; then
        hf download "$MODEL_ID" >/dev/null
    else
        PYTHONNOUSERSITE=1 "$python" -c "from huggingface_hub import snapshot_download; snapshot_download('$MODEL_ID')"
    fi
    if [ -n "$ADOPT_TEXT_ENCODERS" ]; then
        log "adopting text encoders $TEXT_ENCODERS"
    else
        confirm_license "$LLAMA_NOTICE"
        PYTHONNOUSERSITE=1 "$python" "$here/assemble_text_encoder.py" --dest "$TEXT_ENCODERS"
    fi
fi

if [ -d "$TEXT_ENCODERS" ]; then
    link_extra text-encoders "$TEXT_ENCODERS"
elif [ -n "$ADOPT_TEXT_ENCODERS" ]; then
    die "--adopt-text-encoders $ADOPT_TEXT_ENCODERS is not a directory"
else
    warn "no text encoders at $TEXT_ENCODERS yet (re-run without --no-models, or --adopt-text-encoders DIR)"
fi

# ----------------------------------------------------------------- receipt --

write_installed_json
run_probe
log "done. Next: forge doctor; then forge gen motion sweep --out-dir out/sweeps/try --prompt 'A person is walking.' --duration 2 --samples 1"
