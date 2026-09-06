#!/usr/bin/env bash
# Check the ComfyUI voice designer. Speech has its own moss_speech installer.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="moss_tts"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

HOST_DIR="$here/../comfy"
[ -d "$HOST_DIR" ] || die "backends/comfy is not here: moss_tts runs on that host and nothing else"
[ -e "$HOST_DIR/.env" ] || die "the ComfyUI host is not installed — bash $HOST_DIR/install.sh first (moss_tts has no environment of its own)"

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
    warn "$PACK_DIR_NAME is at ${have:0:12}, not the pinned ${PACK_COMMIT:0:12} — the tracked graphs were captured against the pin"
fi
log "$PACK_DIR_NAME at ${have:0:12} in $DATA/custom_nodes"

# The node both tracked graphs configure the model with. A class the host
# does not have is a doctor line and not a POST /prompt failure in front of
# a stranger.
URL="${FORGE_COMFY_URL:-http://127.0.0.1:8188}"
if curl -fsS --max-time 60 "$URL/object_info/MossTTSEngineNode" 2>/dev/null | grep -q MossTTSEngineNode; then
    log "the host registers MossTTSEngineNode"
else
    warn "the host does not answer for MossTTSEngineNode at $URL — systemctl --user status forge-comfy, and packs are scanned once at startup"
fi
command -v ffmpeg >/dev/null 2>&1 || warn "ffmpeg is not on PATH: the host saves FLAC and every audio verb transcodes it here (exit 6 without it)"
log "done — the weights come down on the first render; just doctor says what the host can run"
