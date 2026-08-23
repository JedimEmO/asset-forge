#!/usr/bin/env bash
# Install the ACE-Step 1.5 backend: a pinned clone with the soundfile patch,
# a python 3.12 venv with torch cu128, and the minimal checkpoint set.
#
#   bash backends/acestep/install.sh [--prefix DIR] [--no-models] [--all-models]
#   bash backends/acestep/install.sh --adopt-env DIR --adopt-checkout DIR --adopt-checkpoints DIR
#
# Leaves behind, in this directory: .env -> the venv, .checkout -> the clone,
# .checkpoints -> the weights, installed.json. Everything heavy lives under
# $PREFIX (or wherever --adopt-* says it already is). Idempotent: re-running
# on a finished install re-links, re-probes and touches nothing else.
#
# The traps this encodes, dated, are in designs/hosting.md under "ACE-Step":
#   - torchaudio/torchcodec segfault on save against the system glib, so
#     patches/0001 routes WAV/FLAC saves through soundfile and the client
#     transcodes with the ffmpeg CLI;
#   - it is a server, resident until `forge gen music --stop-server`;
#   - ACESTEP_CHECKPOINTS_DIR points it at the minimal ~7.3 GB set, else it
#     downloads its own (bigger) one into the clone.
#
# --all-models adds the two XL DiTs (~38 GB); nothing in the toolkit asks
# for them, so they are off by default.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="acestep"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

ALL_MODELS=0
for arg in "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"; do
    case "$arg" in
        --all-models) ALL_MODELS=1 ;;
        *) die "unknown flag: $arg (see --help)" ;;
    esac
done

# Pinned in backend.toml; repeated here so the script stands alone.
UPSTREAM="https://github.com/ACE-Step/ACE-Step-1.5.git"
COMMIT="82252c2418de6cb8b3ca99b05592aaf539cc7fb3"
PYVER="3.12"
TORCH_INDEX="https://download.pytorch.org/whl/cu128"
PATCH="$here/patches/0001-audio_utils-soundfile.patch"

CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/checkout}"
ENV_DIR="${ADOPT_ENV:-$PREFIX/env}"
CKPT="${ADOPT_CHECKPOINTS:-$PREFIX/checkpoints}"

# ------------------------------------------------------------------- patch --

# patch_applied DIR — 0 when the soundfile patch is already in the tree.
patch_applied() { git -C "$1" apply --check -R "$PATCH" >/dev/null 2>&1; }

# apply_patch DIR — apply it once; refuse a tree where it neither is nor fits.
apply_patch() {
    if patch_applied "$1"; then
        log "soundfile patch already applied"
    elif git -C "$1" apply --check "$PATCH" >/dev/null 2>&1; then
        log "applying $(basename "$PATCH")"
        git -C "$1" apply "$PATCH"
    else
        die "$(basename "$PATCH") neither applies to nor is present in $1 — is it at $COMMIT?"
    fi
}

# ---------------------------------------------------------------- checkout --

if [ -n "$ADOPT_CHECKOUT" ]; then
    log "adopting checkout $CHECKOUT"
    [ -d "$CHECKOUT/acestep" ] || die "$CHECKOUT does not look like an ACE-Step clone (no acestep/ package)"
    if patch_applied "$CHECKOUT"; then
        log "soundfile patch present in the adopted checkout"
    else
        # Adoption installs nothing and edits nothing; an unpatched clone is
        # the user's to patch, and the server will segfault on its first
        # save until they do.
        warn "the adopted checkout lacks the soundfile patch: git -C $CHECKOUT apply $PATCH"
    fi
else
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT"
    apply_patch "$CHECKOUT"
fi

# --------------------------------------------------------------------- env --

if [ -n "$ADOPT_ENV" ]; then
    log "adopting env $ENV_DIR"
    # An adopted venv may carry an editable ace-step whose recorded path is
    # where the clone used to be. Every launch stands in the checkout, so
    # `python -m acestep.api_server` still finds the package through the
    # working directory; say so rather than let doctor be the first to.
    adopted_python="$ENV_DIR/bin/python"; [ -x "$adopted_python" ] || adopted_python="$ENV_DIR/bin/python3"
    editable="$(PYTHONNOUSERSITE=1 "$adopted_python" - <<'PY' 2>/dev/null || true
import importlib.metadata as m, json
try:
    print(json.loads(m.distribution("ace-step").read_text("direct_url.json") or "{}").get("url", ""))
except Exception:
    pass
PY
)"
    case "$editable" in
        file://*) [ -d "${editable#file://}" ] || warn "the venv's editable ace-step points at ${editable#file://} (gone); imports resolve through the checkout only — pip install -e $CHECKOUT into it to fix" ;;
        "") warn "ace-step is not installed in $ENV_DIR; the server will import it from the checkout only if the checkout is the working directory" ;;
    esac
else
    make_venv "$ENV_DIR" "$PYVER"
    # torch first, from the cu128 index, at the exact build the pyproject
    # pins for linux/x86_64 — PyPI's torch 2.10.0 is a different wheel and
    # the `+cu128` pin would otherwise be unresolvable.
    log "torch 2.10.0+cu128 (+ torchaudio, torchvision) from $TORCH_INDEX"
    pip_install "$ENV_DIR" --extra-index-url "$TORCH_INDEX" \
        "torch==2.10.0+cu128" "torchaudio==2.10.0+cu128" "torchvision==0.25.0+cu128"
    # nano-vllm is vendored under the checkout and named through
    # [tool.uv.sources], which `pip install -e` does not read; install the
    # path first so the package's own requirement is already satisfied.
    log "nano-vllm (vendored)"
    pip_install "$ENV_DIR" "$CHECKOUT/acestep/third_parts/nano-vllm"
    # The package, editable, with the two pins the hosting log names:
    # transformers 4.57 (5.x breaks the checkpoint loaders) and soundfile for
    # the patched save path.
    log "ace-step (editable) + transformers==4.57.6 + soundfile"
    pip_install "$ENV_DIR" --extra-index-url "$TORCH_INDEX" \
        -e "$CHECKOUT" "transformers==4.57.6" "soundfile>=0.13.1"
fi

# The client renders WAV and transcodes to ogg itself; the server's own
# encoders go through torchcodec, which is what the patch routes around.
need_cmd ffmpeg "the music command transcodes WAV to ogg with it"

# ------------------------------------------------------------------ models --

if [ -n "$ADOPT_CHECKPOINTS" ]; then
    log "adopting checkpoints $CKPT"
fi
if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: skipping weights (doctor will say partial until they are there)"
    mkdir -p "$CKPT"
else
    mkdir -p "$CKPT"
    python_bin="$ENV_DIR/bin/python"
    [ -x "$python_bin" ] || python_bin="$ENV_DIR/bin/python3"
    fetch_args=(--dir "$CKPT")
    [ "$ALL_MODELS" = 1 ] && fetch_args+=(--all)
    log "fetching checkpoints into $CKPT"
    # From the checkout: the fetcher's code sync imports the upstream package,
    # which an adopted env may only resolve through the working directory.
    (cd "$CHECKOUT" && PYTHONNOUSERSITE=1 "$python_bin" "$here/fetch_models.py" "${fetch_args[@]}")
fi

# ------------------------------------------------------------------- links --

link_env "$ENV_DIR"
link_checkout "$CHECKOUT"
link_extra checkpoints "$CKPT"

write_installed_json
run_probe
log "done — forge gen music starts the server on first use; stop it with --stop-server"
