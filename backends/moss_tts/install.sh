#!/usr/bin/env bash
# backends/moss_tts/install.sh — MOSS-TTS (Local-Transformer 4B) in its own venv.
#
#   bash backends/moss_tts/install.sh [--prefix DIR] [--no-models] [--yes]
#   bash backends/moss_tts/install.sh --adopt-env ~/src/MOSS-TTS/.venv --adopt-checkout ~/src/MOSS-TTS
#
# One clone, two venvs (designs/hosting.md, MOSS): this backend pins torch
# 2.9.1+cu128 and transformers 5.0.0 through the repository's [torch-runtime]
# extra; the sound-effect model in moss_soundeffect_v2/ pins 2.9.0 and
# 4.57.1, so the two share the clone at $PREFIX/../moss-tts and nothing
# else. The model code itself arrives with the weights (trust_remote_code);
# the clone is here for its pins and its commit.
#
# Leaves behind: .env -> the venv, .checkout -> the clone root,
# installed.json. Idempotent. Weights (~8 GB, Apache-2.0) go to the Hugging
# Face cache unless --no-models; the 8B Delay model is never fetched — it
# OOMs on 24 GB with the audio tokenizer resident, and the 4B fits.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="moss_tts"
BACKEND_DIR="$here"
. "$here/../_lib/common.sh"
parse_common_flags "$@"

UPSTREAM="https://github.com/OpenMOSS/MOSS-TTS.git"
COMMIT="58b20a0d5fcc6766658d50967a90a9d890009a46"
PYVER="3.12"
MODEL_ID="OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5"
TORCH_INDEX="https://download.pytorch.org/whl/cu128"

# pip_install_with_torch_index PREFIX ARGS... — as common.sh's pip_install,
# with the cu128 wheel index beside PyPI. uv's default index strategy takes
# a package from the first index that lists it at all, and PyPI lists torch
# without the +cu128 builds; unsafe-best-match lets the local-version pin
# find its wheel. pip merges indexes by itself.
pip_install_with_torch_index() {
    local prefix="$1"; shift
    if command -v uv >/dev/null 2>&1; then
        PYTHONNOUSERSITE=1 uv pip install -q --python "$prefix/bin/python" \
            --index-strategy unsafe-best-match --extra-index-url "$TORCH_INDEX" "$@"
    else
        PYTHONNOUSERSITE=1 "$prefix/bin/python" -m pip install -q --extra-index-url "$TORCH_INDEX" "$@"
    fi
}

# ---------------------------------------------------------------- checkout --
if [ -n "$ADOPT_CHECKOUT" ]; then
    CHECKOUT="$ADOPT_CHECKOUT"
    [ -f "$CHECKOUT/pyproject.toml" ] && [ -d "$CHECKOUT/moss_soundeffect_v2" ] \
        || die "--adopt-checkout $CHECKOUT does not look like the MOSS-TTS repository root"
    head="$(git -C "$CHECKOUT" rev-parse --verify HEAD 2>/dev/null || echo '?')"
    [ "$head" = "$COMMIT" ] || warn "adopted checkout is at ${head:0:12}, pinned is ${COMMIT:0:12} — doctor will say so"
else
    CHECKOUT="$(dirname "$PREFIX")/moss-tts"
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT"
fi
link_checkout "$CHECKOUT"

# --------------------------------------------------------------------- env --
if [ -n "$ADOPT_ENV" ]; then
    link_env "$ADOPT_ENV"
    ENV_DIR="$(readlink -f "$BACKEND_DIR/.env")"
    log "adopted env $ENV_DIR; installing nothing into it"
else
    ENV_DIR="$PREFIX/venv"
    make_venv "$ENV_DIR" "$PYVER"
    # [torch-runtime] is the repository's own runtime stack: torch 2.9.1+cu128,
    # torchaudio, torchcodec, transformers 5.0.0, accelerate. The root
    # pyproject installs no modules of its own (py-modules = []); it is the
    # dependency carrier, and the model code comes with the weights.
    # soundfile is the writer: torchaudio.save → torchcodec → an ffmpeg that
    # collides with the system glib.
    log "pip install -e $CHECKOUT[torch-runtime] (torch 2.9.1+cu128, transformers 5.0.0)"
    pip_install_with_torch_index "$ENV_DIR" -e "$CHECKOUT[torch-runtime]" soundfile
    link_env "$ENV_DIR"
fi

# ------------------------------------------------------------------ models --
if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: $MODEL_ID is fetched by the first \`forge gen speech\`; doctor says partial until then"
else
    log "downloading $MODEL_ID into the Hugging Face cache (~8 GB, Apache-2.0)"
    if [ -x "$ENV_DIR/bin/hf" ]; then
        PYTHONNOUSERSITE=1 "$ENV_DIR/bin/hf" download "$MODEL_ID" >/dev/null
    else
        PYTHONNOUSERSITE=1 "$(env_python)" -c "from huggingface_hub import snapshot_download; snapshot_download('$MODEL_ID')" >/dev/null
    fi
fi

# ------------------------------------------------------------------- probe --
run_probe
write_installed_json
log "done — \`forge doctor\` for the table; \`forge gen speech --text \"Stand down.\" --voice ref.wav --out out/audio/line.wav\` for a line"
