#!/usr/bin/env bash
# Install what the ComfyUI host needs to run `forge gen music`: one file.
#
#   bash backends/acestep/install.sh [--prefix DIR]
#
# There is no environment here to make. ACE-Step 1.5 is native to the pinned
# ComfyUI (v0.34.2), so this backend is a tracked graph plus a checkpoint in
# the host's own model tree, and the host's installer is what made that tree:
# run `bash backends/comfy/install.sh` first. Idempotent — a file already
# there is one line and no download.
#
# What this replaced, and why it is not here any more: a pinned clone of
# ACE-Step-1.5, a python 3.12 venv with torch cu128, the soundfile patch
# around torchaudio's segfault, a ~7.3 GB checkpoint directory of its own
# and a resident API server on 127.0.0.1:8001 with a pid file. All of it was
# one model behind one HTTP door; the host is that door for four models now.
# The old path is in git history at the commit before this one, and
# `backends/acestep/{patches,fetch_models.py,probe.py}` are still here until
# a real track has come out of the host — nothing is deleted on a promise.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="acestep"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

# Pinned in backend.toml; repeated here so the script stands alone.
WEIGHTS_REPO="Comfy-Org/ace_step_1.5_ComfyUI_files"
WEIGHTS_FILE="checkpoints/ace_step_1.5_turbo_aio.safetensors"
WEIGHTS_LOCAL="ace_step_1.5_turbo_aio.safetensors"
WEIGHTS_GB="10.03"

HOST_DIR="$here/../comfy"
[ -d "$HOST_DIR" ] || die "backends/comfy is not here: acestep runs on that host and nothing else"
HOST_ENV="$HOST_DIR/.env"
[ -e "$HOST_ENV" ] || die "the ComfyUI host is not installed — bash $HOST_DIR/install.sh first (acestep has no environment of its own)"

# $PREFIX/data is the host's --base-directory: the venv is $PREFIX/venv, and
# the data tree is its sibling. $FORGE_COMFY_DATA overrides for an install
# that put it somewhere else.
HOST_PREFIX="$(dirname "$(readlink -f "$HOST_ENV")")"
DATA="${FORGE_COMFY_DATA:-$HOST_PREFIX/data}"
DEST="$DATA/models/checkpoints"

if [ -f "$DEST/$WEIGHTS_LOCAL" ]; then
    log "have checkpoints/$WEIGHTS_LOCAL"
else
    log "fetching $WEIGHTS_REPO :: $WEIGHTS_FILE (${WEIGHTS_GB} GB) -> $DEST"
    mkdir -p "$DEST"
    # --local-dir keeps the weights out of the HF blob cache: stored once,
    # not twice. The CLI recreates the repo's subdirectories under it.
    HF_XET_HIGH_PERFORMANCE=1 PYTHONNOUSERSITE=1 \
        "$(readlink -f "$HOST_ENV")/bin/hf" download "$WEIGHTS_REPO" "$WEIGHTS_FILE" --local-dir "$DEST.dl"
    mv "$DEST.dl/$WEIGHTS_FILE" "$DEST/$WEIGHTS_LOCAL"
    rm -rf "$DEST.dl"
fi

# No .env, no .checkout, no installed.json with a python and a torch in it:
# this backend runs no interpreter, and a receipt claiming one would be the
# first thing to read as a lie. What proves it works is `forge doctor`,
# which asks the host.
log "done — the file is at checkpoints/$WEIGHTS_LOCAL; just doctor says whether the host lists it"
