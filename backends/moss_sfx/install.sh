#!/usr/bin/env bash
# backends/moss_sfx/install.sh — MOSS-SoundEffect v2 in its own venv.
#
#   bash backends/moss_sfx/install.sh [--prefix DIR] [--no-models] [--yes]
#   bash backends/moss_sfx/install.sh --adopt-env ~/src/MOSS-TTS/moss_soundeffect_v2/.venv \
#       --adopt-checkout ~/src/MOSS-TTS/moss_soundeffect_v2
#
# One clone, two venvs (designs/hosting.md, MOSS): the sound-effect model
# lives in moss_soundeffect_v2/ of the MOSS-TTS repository with its own
# pins — torch 2.9.0+cu128, transformers 4.57.1, numpy 1.26 — that the TTS
# model's 2.9.1 / 5.0.0 contradict. The clone goes to $PREFIX/../moss-tts,
# where backends/moss_tts/install.sh finds the same one; this backend's
# .checkout link points at the subdirectory, because that is where the
# inner module runs from and what `pip install -e` is pointed at.
#
# Leaves behind: .env -> the venv, .checkout -> <clone>/moss_soundeffect_v2,
# installed.json. Idempotent; re-running re-links, re-probes, and pip is a
# no-op on a finished env. Weights (~11 GB on disk, Apache-2.0) go to the Hugging
# Face cache unless --no-models, in which case the first `forge gen sfx`
# downloads them and doctor says "partial" until then.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="moss_sfx"
BACKEND_DIR="$here"
. "$here/../_lib/common.sh"
parse_common_flags "$@"

UPSTREAM="https://github.com/OpenMOSS/MOSS-TTS.git"
COMMIT="58b20a0d5fcc6766658d50967a90a9d890009a46"
SUBDIR="moss_soundeffect_v2"
PYVER="3.12"
MODEL_ID="OpenMOSS-Team/MOSS-SoundEffect-v2.0"
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
    # The adopted path is the subdirectory itself (README: --adopt-checkout
    # ~/src/MOSS-TTS/moss_soundeffect_v2); accept the repository root too.
    if [ -f "$ADOPT_CHECKOUT/pipeline_moss_soundeffect.py" ]; then
        CHECKOUT="$ADOPT_CHECKOUT"
    elif [ -f "$ADOPT_CHECKOUT/$SUBDIR/pipeline_moss_soundeffect.py" ]; then
        CHECKOUT="$ADOPT_CHECKOUT/$SUBDIR"
    else
        die "--adopt-checkout $ADOPT_CHECKOUT is neither MOSS-TTS nor its $SUBDIR/ subdirectory"
    fi
    head="$(git -C "$CHECKOUT" rev-parse --verify HEAD 2>/dev/null || echo '?')"
    [ "$head" = "$COMMIT" ] || warn "adopted checkout is at ${head:0:12}, pinned is ${COMMIT:0:12} — doctor will say so"
else
    REPO_DIR="$(dirname "$PREFIX")/moss-tts"
    clone_pinned "$UPSTREAM" "$COMMIT" "$REPO_DIR"
    CHECKOUT="$REPO_DIR/$SUBDIR"
    [ -f "$CHECKOUT/pipeline_moss_soundeffect.py" ] || die "$CHECKOUT has no pipeline_moss_soundeffect.py — the clone is not at $COMMIT"
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
    # The package's own extras carry the pins: [torch-cu128] is torch 2.9.0,
    # torchaudio, torchvision, torchcodec from the cu128 index; the base
    # dependencies pin transformers 4.57.1, numpy 1.26.4 and soundfile.
    # Editable, from the subdirectory: its pyproject maps the package to ".".
    log "pip install -e $CHECKOUT[torch-cu128] (torch 2.9.0+cu128, transformers 4.57.1, numpy<2)"
    pip_install_with_torch_index "$ENV_DIR" -e "$CHECKOUT[torch-cu128]" soundfile
    link_env "$ENV_DIR"
fi

# ------------------------------------------------------------------ models --
if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: $MODEL_ID is fetched by the first \`forge gen sfx\`; doctor says partial until then"
else
    log "downloading $MODEL_ID into the Hugging Face cache (~11 GB, Apache-2.0)"
    if [ -x "$ENV_DIR/bin/hf" ]; then
        PYTHONNOUSERSITE=1 "$ENV_DIR/bin/hf" download "$MODEL_ID" >/dev/null
    else
        PYTHONNOUSERSITE=1 "$(env_python)" -c "from huggingface_hub import snapshot_download; snapshot_download('$MODEL_ID')" >/dev/null
    fi
fi

# ------------------------------------------------------------------- probe --
run_probe
write_installed_json
log "done — \`forge doctor\` for the table; \`forge gen sfx --prompt \"a wooden door slamming\" --out out/audio/door.wav\` for a sound"
