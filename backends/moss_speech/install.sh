#!/usr/bin/env bash
# A separate runtime for speech; never changes the ComfyUI environment.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="moss_speech"
BACKEND_DIR="$here"
. "$here/../_lib/common.sh"
parse_common_flags "$@"
CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/checkout}"
ENV_DIR="${ADOPT_ENV:-$PREFIX/env}"
CHECKPOINTS="${ADOPT_CHECKPOINTS:-$PREFIX/checkpoints}"
if [ -z "$ADOPT_CHECKOUT" ]; then
    clone_pinned https://github.com/OpenMOSS/MOSS-TTS 58b20a0d5fcc6766658d50967a90a9d890009a46 "$CHECKOUT"
fi
if [ -z "$ADOPT_ENV" ]; then
    make_venv "$ENV_DIR" 3.12
    pip_install "$ENV_DIR" torch==2.9.1 torchaudio==2.9.1 --index-url https://download.pytorch.org/whl/cu128
    pip_install "$ENV_DIR" transformers==5.0.0 soundfile librosa einops accelerate scipy
fi
link_env "$ENV_DIR"
link_checkout "$CHECKOUT"
if [ -z "$ADOPT_CHECKPOINTS" ] && [ "$NO_MODELS" != 1 ]; then
    mkdir -p "$CHECKPOINTS"
    PYTHONNOUSERSITE=1 "$(env_python)" - "$CHECKPOINTS" <<'MODELS'
import sys
from pathlib import Path
from huggingface_hub import snapshot_download
for name, revision in [
    ("MOSS-TTS-Local-Transformer", "12aa734e4f11a7b3fdf4eb0ad2aa2029675ffc2e"),
    ("MOSS-Audio-Tokenizer", "3cd226ba2947efa357ef453bcad111b6eafba782"),
]:
    snapshot_download("OpenMOSS-Team/" + name, revision=revision, local_dir=Path(sys.argv[1])/name)
MODELS
fi
if [ -d "$CHECKPOINTS" ]; then link_extra checkpoints "$CHECKPOINTS"; fi
write_installed_json
run_probe
