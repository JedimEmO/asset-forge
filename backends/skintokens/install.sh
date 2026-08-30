#!/usr/bin/env bash
# Install (or adopt) the SkinTokens backend: a python 3.11 venv with torch
# 2.7.0+cu128, the pinned clone with both patches applied, and the ~1.6 GB
# of released checkpoints under the prefix. Idempotent.
#
#   bash backends/skintokens/install.sh [--prefix DIR] [--no-models]
#   bash backends/skintokens/install.sh --adopt-env DIR --adopt-checkout DIR \
#                                       [--adopt-checkpoints DIR]
#
# What it leaves in backends/skintokens/: .env, .checkout, .checkpoints
# (symlinks, gitignored) and installed.json. Everything heavy lives under
# $PREFIX or wherever --adopt-* pointed. The why of every step is
# designs/hosting.md under "SkinTokens"; the short version:
#   - flash-attn is NOT installed. patches/0001 rewrites the two hard-coded
#     attn_implementation="flash_attention_2" sites to "sdpa" and gives each
#     flash_attn import a scaled_dot_product_attention fallback; without it
#     src.model.tokenrig does not import at all.
#   - patches/0002 is upstream issue #8: make_asset() counted every child
#     twice, so no joint ever had exactly one child and the exported bone
#     tails were wrong.
#   - download.py --model writes experiments/ and models/ into the working
#     directory, so it is run inside $PREFIX/weights and the clone gets a
#     symlink by each of those two names (src/model/tokenrig.py resolves
#     "models/Qwen3-0.6B" against the working directory). Both names are in
#     upstream's own .gitignore, so the checkout stays clean.
#   - ~14 GB VRAM; never beside TRELLIS.2, ARDY, MOSS or the ACE-Step server.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="skintokens"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

for arg in "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"; do
    die "unknown flag: $arg (see --help)"
done

# Pinned in backend.toml; repeated here so the script stands alone.
UPSTREAM="https://github.com/VAST-AI-Research/SkinTokens"
COMMIT="273b691d35989d71cd17ff2895fdc735097b92d1"
PYVER="3.11"
TORCH_INDEX="https://download.pytorch.org/whl/cu128"

CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/checkout}"
ENV_DIR="${ADOPT_ENV:-$PREFIX/env}"
WEIGHTS="${ADOPT_CHECKPOINTS:-$PREFIX/weights}"

PATCHES=(
    "$here/patches/0001-sdpa-instead-of-flash-attn.patch"
    "$here/patches/0002-make_asset-sons-counted-once.patch"
)

# ----------------------------------------------------------------- patches --

# patch_applied DIR PATCH — 0 when it is already in the tree (it reverses).
patch_applied() { git -C "$1" apply --check -R "$2" >/dev/null 2>&1; }

# apply_patches DIR — apply each once; refuse a tree where one neither is
# nor fits, because that is a clone at another commit and the next failure
# would be a traceback three steps later.
apply_patches() {
    local dir="$1" patch
    for patch in "${PATCHES[@]}"; do
        [ -f "$patch" ] || die "missing $patch — this checkout of the toolkit is incomplete"
        if patch_applied "$dir" "$patch"; then
            log "$(basename "$patch") already applied"
        elif git -C "$dir" apply --check "$patch" >/dev/null 2>&1; then
            log "applying $(basename "$patch")"
            git -C "$dir" apply "$patch"
        else
            die "$(basename "$patch") neither applies to nor is present in $dir — is it at $COMMIT?"
        fi
    done
}

# ---------------------------------------------------------------- checkout --

if [ -n "$ADOPT_CHECKOUT" ]; then
    log "adopting checkout $CHECKOUT"
    [ -d "$CHECKOUT/src/model" ] || die "$CHECKOUT does not look like a SkinTokens clone (no src/model/)"
    head="$(git -C "$CHECKOUT" rev-parse HEAD 2>/dev/null || echo '?')"
    [ "$head" = "$COMMIT" ] || warn "adopted checkout is at ${head:0:12}, pinned is ${COMMIT:0:12} (doctor will say so)"
    # Adoption installs nothing and edits nothing; an unpatched clone is the
    # user's to patch, and the probe will say which one is missing.
    for patch in "${PATCHES[@]}"; do
        patch_applied "$CHECKOUT" "$patch" \
            || warn "the adopted checkout lacks $(basename "$patch"): git -C $CHECKOUT apply $patch"
    done
else
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT"
    apply_patches "$CHECKOUT"
fi

# --------------------------------------------------------------------- env --

if [ -n "$ADOPT_ENV" ]; then
    log "adopting env $ENV_DIR"
else
    make_venv "$ENV_DIR" "$PYVER"
    # torch first, from the cu128 index at the build upstream's README pins.
    # Its own index has no numpy/pillow, so it is --extra-index-url and the
    # exact +cu128 version is what keeps PyPI's plain 2.7.0 from winning.
    log "torch 2.7.0+cu128 (+ torchvision, torchaudio) from $TORCH_INDEX"
    pip_install "$ENV_DIR" --extra-index-url "$TORCH_INDEX" \
        "torch==2.7.0+cu128" "torchvision==0.22.0+cu128" "torchaudio==2.7.0+cu128"
    # requirements.txt as upstream wrote it: transformers>=4.57,
    # diffusers>=0.35, lightning, the pip bpy>=4.2 wheel (its own Blender,
    # nothing to do with $BLENDER_BIN), trimesh, open3d, fast-simplification,
    # bottle and tornado (the bpy server demo.py talks to over HTTP).
    # flash-attn is deliberately NOT here — see patches/0001.
    log "requirements.txt (no flash-attn: patches/0001 makes it SDPA)"
    pip_install "$ENV_DIR" -r "$CHECKOUT/requirements.txt"
fi

link_env "$ENV_DIR"
link_checkout "$CHECKOUT"
python="$(env_python)"

# ------------------------------------------------------------------ models --

mkdir -p "$WEIGHTS"
if [ -n "$ADOPT_CHECKPOINTS" ]; then
    log "adopting weights $WEIGHTS"
elif [ "$NO_MODELS" = 1 ]; then
    log "--no-models: skipping the ~1.6 GB of checkpoints (doctor will say partial)"
else
    # download.py hard-codes local_dir="." and the directory names the
    # checkpoint loader joins onto; run it where those names should land
    # rather than moving anything afterwards.
    log "fetching the TokenRig and FSQ-CVAE checkpoints + the Qwen3-0.6B config into $WEIGHTS"
    (cd "$WEIGHTS" && PYTHONNOUSERSITE=1 "$python" "$CHECKOUT/download.py" --model)
fi
link_extra checkpoints "$WEIGHTS"

# src/model/tokenrig.py resolves models/Qwen3-0.6B against the working
# directory, and every run stands in the checkout. Link, do not copy: the
# weights have one home. Both names are in upstream's .gitignore.
for name in experiments models; do
    target="$WEIGHTS/$name"
    link="$CHECKOUT/$name"
    if [ ! -d "$target" ]; then
        warn "no $target yet — re-run without --no-models"
        continue
    fi
    if [ -L "$link" ]; then
        [ "$(readlink -f "$link")" = "$(readlink -f "$target")" ] || { rm -f "$link"; ln -s "$target" "$link"; }
    elif [ -e "$link" ]; then
        warn "$link exists and is not a symlink — leaving it (the checkout already has its own $name/)"
    else
        ln -s "$target" "$link"
        log "$link -> $target"
    fi
done

# ----------------------------------------------------------------- receipt --

write_installed_json
run_probe
log "done. Next: forge doctor --backend skintokens"
