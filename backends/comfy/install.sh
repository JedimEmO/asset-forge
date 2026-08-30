#!/usr/bin/env bash
# Install the ComfyUI host: a python 3.12 venv, the pinned ComfyUI clone,
# ComfyUI-Manager as the pip package it now is, one custom node pack and a
# systemd --user unit.
#
#   bash backends/comfy/install.sh [--prefix DIR] [--yes]
#   bash backends/comfy/install.sh --no-service          # do not touch systemd
#   bash backends/comfy/install.sh --adopt-env DIR --adopt-checkout DIR
#
# **It downloads no weights.** The host is a place for models, not an owner
# of any: backends/acestep/install.sh fetches the one ACE-Step checkpoint,
# and the three MOSS models come down on the node pack's first run. The
# image-model group this script used to fetch — Qwen-Image fp8 and its Q4
# GGUF, FLUX.1-schnell, both ControlNets — left with the reference door
# (designs/decisions.md, "The reference image stays brought", 2026-08-30);
# what is already on disk from it is named in the closing summary, for a
# human to remove or keep.
#
# Leaves behind, in this directory: .env -> the venv, .checkout -> the
# pinned clone, installed.json. Everything heavy lives under $PREFIX
# (${FORGE_BACKENDS_HOME:-~/.cache/asset-forge/backends}/comfy):
#
#   $PREFIX/ComfyUI/            the clone at $COMMIT (+ extra_model_paths.yaml)
#   $PREFIX/venv/               python 3.12, torch cu130, comfyui_manager
#   $PREFIX/data/               --base-directory: models/ input/ output/ user/
#   $PREFIX/snapshot.json       what the Manager says is installed, now
#
# Idempotent: re-running on a finished install re-links, re-copies the paths
# config, restarts nothing that is already right, and re-probes. `--no-models`
# is accepted and does nothing: there are no weights here to skip.
#
# The traps this encodes, dated, are in designs/hosting.md under "ComfyUI":
#   - the clone is a checkout, so nothing configurable is written into it
#     except extra_model_paths.yaml, which is in ComfyUI's own .gitignore;
#   - --base-directory keeps models/output/user out of the clone;
#   - --cache-none, because a cached node result is a re-roll that never ran;
#   - ComfyUI-Manager is `pip install comfyui_manager` + `--enable-manager`,
#     not a custom_nodes clone, since ComfyUI v0.31;
#   - the service holds ~0.4 GB of the card idle for its CUDA context;
#   - one custom node pack, TTS-Audio-Suite, because the three MOSS models
#     come through it. It is cloned into $PREFIX/data/custom_nodes/ before
#     the unit starts, because packs are scanned once at startup, and ten
#     pips go into the venv with it — see below for which two may not.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="comfy"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

NO_SERVICE=0
for arg in "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"; do
    case "$arg" in
        --no-service) NO_SERVICE=1 ;;
        # --models and --no-flux-controlnet went with the image-model group
        # they selected. Named rather than ignored, because a stranger with
        # an old command line deserves to be told what happened to it.
        --models|--models=*|--no-flux-controlnet)
            die "$arg is gone: this host downloads no weights of its own since the image-model group left it (designs/decisions.md, 2026-08-30). backends/acestep/install.sh fetches the ACE-Step checkpoint; the MOSS models come down on the node pack's first run." ;;
        *) die "unknown flag: $arg (see --help; this backend adds --no-service)" ;;
    esac
done

# Pinned in backend.toml; repeated here so the script stands alone.
UPSTREAM="https://github.com/comfyanonymous/ComfyUI"
COMMIT="169fcf35a2fc163fec31338b816503ddac0d3fcf"   # v0.34.2, 2026-08-27
PYVER="3.12"
TORCH="2.13.0"                                      # PyPI's linux wheel is the cu130 build
PORT=8188
UNIT="forge-comfy.service"
# The one custom node pack, pinned in backend.toml's [[comfy.packs]] and
# repeated here so the script stands alone.
TTS_PACK_URL="https://github.com/diodiogod/TTS-Audio-Suite"
TTS_PACK_COMMIT="fab00263fbdcdaddd4c721d1b560e1a08b6025ea"   # v5.8.7, 2026-08-28
TTS_PACK_DIR="TTS-Audio-Suite"
# What MOSS-SoundEffect v2 imports and the host had not got, measured on the
# first real sfx run (2026-08-30). Unpinned on purpose: what is pinned is the
# pack, and these are the versions its imports resolve against — the versions
# that ran are in every record's `backend` block. diffusers 0.40.0, ftfy 6.3.1
# on this machine.
TTS_PACK_PIPS=(diffusers ftfy flatten-dict julius soundfile ffmpy importlib-resources tensorboard randomname)
# descript-audiotools requires protobuf>=3.9.2,<3.20 and would drag the host's
# protobuf — tensorboard and transformers both want it well above that — down
# with it, so it alone comes in with --no-deps and the seven modules its
# import chain actually reaches are the tail of the list above — flatten_dict, julius, soundfile, ffmpy,
# importlib_resources, tensorboard (audiotools.ml imports it; nothing here
# trains anything) and randomname, each added by running the import until it
# stopped failing and each checked for what it would move first.
TTS_PACK_PIPS_NO_DEPS=(descript-audiotools)
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DEFAULT_PREFIX="$HOME/.cache/asset-forge/backends/comfy"

CHECKOUT="${ADOPT_CHECKOUT:-$PREFIX/ComfyUI}"
ENV_DIR="${ADOPT_ENV:-$PREFIX/venv}"
DATA="$PREFIX/data"

need_cmd git
need_cmd curl

# ---------------------------------------------------------------- checkout --

if [ -n "$ADOPT_CHECKOUT" ]; then
    log "adopting checkout $CHECKOUT"
    [ -f "$CHECKOUT/main.py" ] || die "$CHECKOUT does not look like a ComfyUI clone (no main.py)"
else
    clone_pinned "$UPSTREAM" "$COMMIT" "$CHECKOUT"
fi

# --------------------------------------------------------------------- env --

if [ -n "$ADOPT_ENV" ]; then
    log "adopting env $ENV_DIR"
else
    make_venv "$ENV_DIR" "$PYVER"
    # torch first and pinned: ComfyUI's requirements.txt asks for a bare
    # `torch`, so an unpinned install is a different build every month.
    log "torch $TORCH (+ torchvision, torchaudio) from PyPI — the linux wheel is the CUDA build"
    pip_install "$ENV_DIR" "torch==$TORCH" torchvision torchaudio
    log "ComfyUI requirements"
    pip_install "$ENV_DIR" -r "$CHECKOUT/requirements.txt"
    # ComfyUI-Manager is a pip package now, pinned by the clone itself, and
    # switched on with --enable-manager. There is no custom_nodes clone.
    log "ComfyUI-Manager ($(tr -d '\r' < "$CHECKOUT/manager_requirements.txt" | tr '\n' ' '))"
    pip_install "$ENV_DIR" -r "$CHECKOUT/manager_requirements.txt"
fi
PY_BIN="$ENV_DIR/bin/python"; [ -x "$PY_BIN" ] || PY_BIN="$ENV_DIR/bin/python3"

# ------------------------------------------------------------------- paths --

mkdir -p "$DATA/models" "$DATA/input" "$DATA/output" "$DATA/user" "$DATA/custom_nodes"
# ComfyUI reads extra_model_paths.yaml from beside main.py. It is in the
# clone's own .gitignore, so this is the one file that may live there.
cp "$here/extra_model_paths.yaml" "$CHECKOUT/extra_model_paths.yaml"
log "extra_model_paths.yaml -> $CHECKOUT"

# ------------------------------------------------------------------- packs --

# TTS-Audio-Suite: the three MOSS models (SoundEffect v2, TTS, VoiceGenerator)
# behind moss_sfx and moss_tts. Cloned at its pin, before the unit starts,
# because node packs are scanned once at startup; `uv pip`/`pip` is chosen by
# pip_install, since this venv has no pip of its own when uv built it.
#
# Registration is free — all 58 classes come up with the venv untouched, which
# is why this row said "no pips" until a sound was actually asked for — but **MOSS-SoundEffect v2 does not load without
# these**, measured on the first real `forge gen sfx` (2026-08-30): the engine
# imports diffusers (AutoencoderOobleck, ConfigMixin, ModelMixin), ftfy (the
# WAN prompter) and audiotools (the DAC VAE), each of them a hard import that
# fails at POST /prompt with the card already leased.
#
# Two rules hold this list together, and both are the reason it is not just
# `pip install -r requirements.txt`:
#   - the pack's own requirements.txt asks for numpy<2.3.0, which would
#     downgrade the host's 2.5.2;
#   - descript-audiotools caps protobuf<3.20, which would downgrade the
#     host's — tensorboard and transformers both want it above that — so it
#     goes in with --no-deps and the seven modules its import chain actually
#     reaches follow one by one.
# With those two avoided, nothing moved: torch 2.13.0+cu130, torchaudio
# 2.11.0, transformers 5.16.1, numpy 2.5.2, protobuf 7.36.0 are exactly what
# they were before. designs/hosting.md, "ComfyUI", 2026-08-30.
clone_pinned "$TTS_PACK_URL" "$TTS_PACK_COMMIT" "$DATA/custom_nodes/$TTS_PACK_DIR"
log "pack pips: ${TTS_PACK_PIPS[*]}"
pip_install "$ENV_DIR" "${TTS_PACK_PIPS[@]}"
log "pack pips (--no-deps, its protobuf cap would downgrade the host's): ${TTS_PACK_PIPS_NO_DEPS[*]}"
pip_install "$ENV_DIR" --no-deps "${TTS_PACK_PIPS_NO_DEPS[@]}"

# ----------------------------------------------------------------- service --

if [ "$NO_SERVICE" = 1 ]; then
    log "--no-service: not installing $UNIT (start it by hand from $CHECKOUT)"
else
    need_cmd systemctl "this backend runs as a systemd --user unit; --no-service skips it"
    mkdir -p "$UNIT_DIR"
    # The tracked unit spells the default prefix as %h/...; a --prefix
    # elsewhere is substituted in. No absolute path is written into the
    # tracked copy, and the installed copy is a rendering of it.
    if [ "$PREFIX" = "$DEFAULT_PREFIX" ]; then
        cp "$here/$UNIT" "$UNIT_DIR/$UNIT"
    else
        sed "s|%h/.cache/asset-forge/backends/comfy|$PREFIX|g" "$here/$UNIT" > "$UNIT_DIR/$UNIT"
    fi
    log "$UNIT_DIR/$UNIT"
    systemctl --user daemon-reload
    systemctl --user enable "$UNIT" >/dev/null
    systemctl --user restart "$UNIT"
    log "waiting for http://127.0.0.1:$PORT/system_stats"
    ready=0
    for _ in $(seq 1 120); do
        if curl -fsS --max-time 5 "http://127.0.0.1:$PORT/system_stats" >/dev/null 2>&1; then ready=1; break; fi
        sleep 1
    done
    [ "$ready" = 1 ] || die "the service did not answer within 120 s — journalctl --user -u $UNIT -n 50"
    log "up: ComfyUI $(curl -fsS "http://127.0.0.1:$PORT/system_stats" | "$PY_BIN" -c 'import json,sys; print(json.load(sys.stdin)["system"]["comfyui_version"])')"
    # `enable` is only half of "starts itself": a --user unit runs while the
    # user has a session, so on a headless box it never comes up at boot and
    # dies when the last shell logs out.
    if [ "$(loginctl show-user "$USER" -p Linger --value 2>/dev/null || echo no)" != "yes" ]; then
        warn "lingering is off for $USER: $UNIT starts on login, not at boot, and stops with your last session — loginctl enable-linger $USER"
    fi
fi

# ---------------------------------------------------------------- snapshot --

if [ "$NO_SERVICE" != 1 ]; then
    # `GET /v2/snapshot/get_current` — the Manager's own answer to "what is
    # installed": the ComfyUI commit, every custom node pack and every pip in
    # the venv. Pretty-printed so the committed copy diffs line by line when
    # the environment moves.
    if curl -fsS --max-time 30 "http://127.0.0.1:$PORT/v2/snapshot/get_current" -o "$PREFIX/snapshot.raw.json" 2>/dev/null; then
        python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); json.dump(d, open(sys.argv[2],"w"), indent=2); open(sys.argv[2],"a").write("\n")' \
            "$PREFIX/snapshot.raw.json" "$PREFIX/snapshot.json"
        rm -f "$PREFIX/snapshot.raw.json"
        log "Manager snapshot -> $PREFIX/snapshot.json"
        if ! diff -q "$PREFIX/snapshot.json" "$here/snapshot.json" >/dev/null 2>&1; then
            warn "the live snapshot differs from the committed $here/snapshot.json:"
            diff "$here/snapshot.json" "$PREFIX/snapshot.json" | head -20 >&2 || true
            warn "copy it in and commit if the change was meant"
        fi
    else
        warn "the Manager snapshot endpoint did not answer; is --enable-manager on the running unit?"
    fi
fi

# ------------------------------------------------------------------- links --

link_env "$ENV_DIR"
link_checkout "$CHECKOUT"

write_installed_json
run_probe

# What this script no longer fetches, and therefore no longer maintains. A
# host that once ran the reference spike still has ~60 GB of image weights
# and the ComfyUI-GGUF pack under $PREFIX; nothing here reads either any
# more, and nothing here deletes them either — a weight is the user's, and
# an installer that removes files it did not just write is one nobody can
# re-run safely.
log "the image-model group left this host: $DATA/models/{diffusion_models,controlnet} and the FLUX/Qwen files under $DATA/models/{text_encoders,vae,checkpoints}, plus $DATA/custom_nodes/ComfyUI-GGUF, are no longer read by anything — remove them by hand if you want the ~60 GB back"
log "done — systemctl --user status $UNIT; the daemon drives it over http://127.0.0.1:$PORT"
