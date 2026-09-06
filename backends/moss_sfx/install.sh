#!/usr/bin/env bash
# Check that the ComfyUI host can run `forge gen sfx`. It installs nothing.
#
#   bash backends/moss_sfx/install.sh
#
# There is no environment here to make. MOSS-SoundEffect v2 runs inside the
# host through the TTS-Audio-Suite pack, which `backends/comfy/install.sh`
# clones at its pin, and the weights (~11 GB) are downloaded by the node
# itself on its first run, into the same HF cache every other backend fills.
# So this script's whole job is to say whether the host is there, whether the
# pack is at the pin this backend names, and whether the node class the
# tracked graph needs is registered — and to name the fix when it is not.
#
# What this replaced, and why it is not here any more: a python 3.12 venv
# with its own torch pin, a clone of OpenMOSS/MOSS-TTS shared with moss_tts,
# and TORCHDYNAMO_DISABLE=1 to stop the DiT compiling itself for minutes on
# every one-shot call. The first two are the host's now. The third is a
# choice the host has to make in forge-comfy.service, and until it does the
# first effect of a ComfyUI session pays the compile — the pack says the
# artifact is cached across sessions, which the old venv could not do.
# The old path is in git history at the commit before this one.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="moss_sfx"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

HOST_DIR="$here/../comfy"
[ -d "$HOST_DIR" ] || die "backends/comfy is not here: moss_sfx runs on that host and nothing else"
[ -e "$HOST_DIR/.env" ] || die "the ComfyUI host is not installed — bash $HOST_DIR/install.sh first (moss_sfx has no environment of its own)"

PACK_DIR_NAME="TTS-Audio-Suite"
PACK_COMMIT="fab00263fbdcdaddd4c721d1b560e1a08b6025ea"
HOST_PREFIX="$(dirname "$(readlink -f "$HOST_DIR/.env")")"
DATA="${FORGE_COMFY_DATA:-$HOST_PREFIX/data}"
PACK="$DATA/custom_nodes/$PACK_DIR_NAME"

if [ ! -d "$PACK/.git" ]; then
    die "$PACK_DIR_NAME is not in the host's custom_nodes — bash $HOST_DIR/install.sh clones it at its pin"
fi
have="$(git -C "$PACK" rev-parse --verify HEAD 2>/dev/null || echo unknown)"
if [ "$have" != "$PACK_COMMIT" ]; then
    warn "$PACK_DIR_NAME is at ${have:0:12}, not the pinned ${PACK_COMMIT:0:12} — the tracked graph was captured against the pin"
fi
log "$PACK_DIR_NAME at ${have:0:12} in $DATA/custom_nodes"

# The node the tracked graph loads the model with. A class the host does not
# have is a doctor line and not a POST /prompt failure in front of a stranger.
URL="${FORGE_COMFY_URL:-http://127.0.0.1:8188}"
if curl -fsS --max-time 60 "$URL/object_info/MossSoundEffectV2EngineNode" 2>/dev/null | grep -q MossSoundEffectV2EngineNode; then
    log "the host registers MossSoundEffectV2EngineNode"
else
    warn "the host does not answer for MossSoundEffectV2EngineNode at $URL — systemctl --user status forge-comfy, and packs are scanned once at startup"
fi
command -v ffmpeg >/dev/null 2>&1 || warn "ffmpeg is not on PATH: the host saves FLAC and every audio verb transcodes it here (exit 6 without it)"
log "done — the weights come down on the first render; just doctor says what the host can run"
