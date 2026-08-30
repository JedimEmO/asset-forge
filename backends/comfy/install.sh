#!/usr/bin/env bash
# Install the ComfyUI host: a python 3.12 venv, the pinned ComfyUI clone,
# ComfyUI-Manager as the pip package it now is, a systemd --user unit, and
# the models the Phase 0 reference spike needs.
#
#   bash backends/comfy/install.sh [--prefix DIR] [--no-models] [--yes]
#   bash backends/comfy/install.sh --models qwen_image   # only that group's weights
#   bash backends/comfy/install.sh --no-service          # do not touch systemd
#   bash backends/comfy/install.sh --adopt-env DIR --adopt-checkout DIR
#
# --models <all|none|qwen_image|flux>, comma-separated, default all. The
# venv, the clone, the packs and the unit are the host and are always made;
# what --models decides is which weights come down. `forge setup` names the
# group the chosen kinds need — `--models qwen_image` for props and
# characters, `--models none` for the audio kinds, whose weights are
# backends/acestep/install.sh's one file and the node pack's own download.
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
# config, restarts nothing that is already right, fetches only the weights
# that are missing, and re-probes.
#
# The traps this encodes, dated, are in designs/hosting.md under "ComfyUI":
#   - the clone is a checkout, so nothing configurable is written into it
#     except extra_model_paths.yaml, which is in ComfyUI's own .gitignore;
#   - --base-directory keeps models/output/user out of the clone;
#   - --cache-none, because a cached node result is a re-roll that never ran;
#   - ComfyUI-Manager is `pip install comfyui_manager` + `--enable-manager`,
#     not a custom_nodes clone, since ComfyUI v0.31;
#   - the service holds ~0.4 GB of the card idle for its CUDA context;
#   - black-forest-labs/FLUX.1-schnell is gated "auto": an HF token is
#     needed for ae.safetensors even though the licence is Apache-2.0;
#   - the FLUX pose ControlNet is FLUX.1-dev NON-COMMERCIAL and is asked for;
#   - two custom node packs. ComfyUI-GGUF, because the lean tier's reference
#     image is a Q4_K_M GGUF and no native loader reads one (two pips in the
#     venv); and TTS-Audio-Suite, because the three MOSS models come through
#     it (no pips at all — see below). Both are cloned into
#     $PREFIX/data/custom_nodes/ before the unit starts.

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_NAME="comfy"
BACKEND_DIR="$here"
# shellcheck source=../_lib/common.sh
. "$here/../_lib/common.sh"
parse_common_flags "$@"

NO_SERVICE=0
NO_FLUX_CONTROLNET=0
# Which model groups to fetch. `all` is what a hand-run install means and
# what this script always did; `forge setup` names the group the chosen
# kinds actually need, because "one screen before a byte downloads" was a
# 9.5 GB screen in front of a 73.7 GB download — `forge setup music` pulled
# the whole image stack, FLUX included, for a backend whose one file is
# fetched by backends/acestep/install.sh (2026-08-30).
MODELS="all"
_next_is_models=0
for arg in "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"; do
    if [ "$_next_is_models" = 1 ]; then MODELS="$arg"; _next_is_models=0; continue; fi
    case "$arg" in
        --no-service) NO_SERVICE=1 ;;
        --no-flux-controlnet) NO_FLUX_CONTROLNET=1 ;;
        --models) _next_is_models=1 ;;
        --models=*) MODELS="${arg#*=}" ;;
        *) die "unknown flag: $arg (see --help; this backend adds --no-service, --no-flux-controlnet, --models <all|none|qwen_image|flux>)" ;;
    esac
done
[ "$_next_is_models" = 0 ] || die "--models needs a value: all | none | qwen_image | flux (comma-separated)"
case ",$MODELS," in
    *,all,*|*,none,*|*,qwen_image,*|*,flux,*) ;;
    *) die "--models $MODELS: the groups are all, none, qwen_image, flux (comma-separated)" ;;
esac

# wants GROUP — whether this run fetches that model group.
wants() {
    case ",$MODELS," in
        *,all,*) return 0 ;;
        *",$1,"*) return 0 ;;
        *) return 1 ;;
    esac
}

# Pinned in backend.toml; repeated here so the script stands alone.
UPSTREAM="https://github.com/comfyanonymous/ComfyUI"
COMMIT="169fcf35a2fc163fec31338b816503ddac0d3fcf"   # v0.34.2, 2026-08-27
PYVER="3.12"
TORCH="2.13.0"                                      # PyPI's linux wheel is the cu130 build
PORT=8188
UNIT="forge-comfy.service"
# The one custom node pack, pinned in backend.toml's [[comfy.packs]] and
# repeated here so the script stands alone. It is what loads a .gguf.
GGUF_PACK_URL="https://github.com/city96/ComfyUI-GGUF"
GGUF_PACK_COMMIT="6ea2651e7df66d7585f6ffee804b20e92fb38b8a"   # 2026-01-12
GGUF_PACK_DIR="ComfyUI-GGUF"
GGUF_PACK_PIPS=(gguf==0.19.0 protobuf==7.36.0)
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
# 7.36.0 (ComfyUI-GGUF's pin) down with it, so it alone comes in with
# --no-deps and the seven modules its import chain actually reaches are the
# tail of the list above — flatten_dict, julius, soundfile, ffmpy,
# importlib_resources, tensorboard (audiotools.ml imports it; nothing here
# trains anything) and randomname, each added by running the import until it
# stopped failing and each checked for what it would move first.
TTS_PACK_PIPS_NO_DEPS=(descript-audiotools)
GGUF_WEIGHTS_REPO="city96/Qwen-Image-gguf"
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

# ComfyUI-GGUF: the lean tier's Qwen-Image is a Q4_K_M GGUF and UNETLoader
# does not read one. Cloned at its pin, before the unit starts, because node
# packs are scanned once at startup. Two pips go into the venv with it;
# `uv pip`/`pip` is chosen by pip_install, since this venv has no pip of its
# own when uv built it.
clone_pinned "$GGUF_PACK_URL" "$GGUF_PACK_COMMIT" "$DATA/custom_nodes/$GGUF_PACK_DIR"
log "pack pips: ${GGUF_PACK_PIPS[*]}"
pip_install "$ENV_DIR" "${GGUF_PACK_PIPS[@]}"

# TTS-Audio-Suite: the three MOSS models (SoundEffect v2, TTS, VoiceGenerator)
# behind moss_sfx and moss_tts. Registration is free — all 58 classes come up
# with the venv untouched, which is why this row said "no pips" until a sound
# was actually asked for — but **MOSS-SoundEffect v2 does not load without
# these**, measured on the first real `forge gen sfx` (2026-08-30): the engine
# imports diffusers (AutoencoderOobleck, ConfigMixin, ModelMixin), ftfy (the
# WAN prompter) and audiotools (the DAC VAE), each of them a hard import that
# fails at POST /prompt with the card already leased.
#
# Two rules hold this list together, and both are the reason it is not just
# `pip install -r requirements.txt`:
#   - the pack's own requirements.txt asks for numpy<2.3.0, which would
#     downgrade the host's 2.5.2 under every image template;
#   - descript-audiotools caps protobuf<3.20, which would downgrade the
#     7.36.0 ComfyUI-GGUF pins above — so it goes in with --no-deps and the
#     seven modules its import chain actually reaches follow one by one.
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

# ------------------------------------------------------------------ models --

M="$DATA/models"
fetch() {  # fetch REPO FILE FOLDER [LOCALNAME]
    local repo="$1" file="$2" folder="$3" local_name="${4:-}"
    [ -n "$local_name" ] || local_name="$(basename "$file")"
    local dest="$M/$folder"
    mkdir -p "$dest"
    if [ -f "$dest/$local_name" ]; then log "have $folder/$local_name"; return 0; fi
    log "fetching $repo :: $file -> $folder/$local_name"
    # --local-dir keeps the weights out of the HF blob cache: 55 GB stored
    # once, not twice. The download lands under a temp tree because the CLI
    # recreates the repo's own subdirectories under it.
    HF_XET_HIGH_PERFORMANCE=1 PYTHONNOUSERSITE=1 \
        "$ENV_DIR/bin/hf" download "$repo" "$file" --local-dir "$dest.dl"
    mv "$dest.dl/$file" "$dest/$local_name"
    rm -rf "$dest.dl"
}

if [ "$NO_MODELS" = 1 ]; then
    log "--no-models: skipping weights (doctor will say partial until they are there)"
elif ! wants qwen_image && ! wants flux; then
    log "--models $MODELS: no image weights (the audio backends fetch their own, or the node pack does on first run)"
else
    if wants flux; then
        hf_token_present || warn "no Hugging Face token: black-forest-labs/FLUX.1-schnell is gated \"auto\" and ae.safetensors will 401 — hf auth login --token <tok>"
    fi
fi

if [ "$NO_MODELS" != 1 ] && wants qwen_image; then
    log "--models: qwen_image (33.60 GB fp8 + 13.07 GB Q4_K_M GGUF)"
    # Qwen-Image, the fp8 split form ComfyUI documents (Apache-2.0).
    fetch Comfy-Org/Qwen-Image_ComfyUI split_files/diffusion_models/qwen_image_fp8_e4m3fn.safetensors diffusion_models
    fetch Comfy-Org/Qwen-Image_ComfyUI split_files/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors text_encoders
    fetch Comfy-Org/Qwen-Image_ComfyUI split_files/vae/qwen_image_vae.safetensors vae
    # The pose ControlNet for it: InstantX's Union, repackaged by Comfy-Org
    # into the single file ControlNetLoader takes. Apache-2.0, on-base.
    fetch Comfy-Org/Qwen-Image-InstantX-ControlNets split_files/controlnet/Qwen-Image-InstantX-ControlNet-Union.safetensors controlnet
    # The lean tier's Qwen-Image, for the ComfyUI-GGUF pack above: 16.2 GB
    # at 1024 against fp8's 23.3, at the same style, pose and second.
    fetch "$GGUF_WEIGHTS_REPO" qwen-image-Q4_K_M.gguf diffusion_models
fi

if [ "$NO_MODELS" != 1 ] && wants flux; then
    # FLUX.1-schnell lost the Phase 0 reference spike (decisions.md,
    # 2026-08-30: it drew a photograph in 4 of 4 and would not take the
    # style line), so no kind in the map needs any of this and `forge setup`
    # never asks for the group. It stays fetchable by hand because
    # `workflows/reference_flux.api.json` is tracked as the losing side's
    # evidence, and a tracked template a stranger cannot run is the trap
    # this repository already paid for once.
    log "--models: flux (22.72 GB, the Phase 0 spike's losing candidate)"
    # The fp8 form ComfyUI documents for schnell is the all-in-one
    # checkpoint; there is no fp8 UNET-only file, only a 23.8 GB bf16 one,
    # which the budget does not have room for.
    fetch Comfy-Org/flux1-schnell flux1-schnell-fp8.safetensors checkpoints
    # Its text encoders and VAE as separate files, for the split and GGUF
    # paths a FLUX lean form would want.
    fetch comfyanonymous/flux_text_encoders clip_l.safetensors text_encoders
    fetch comfyanonymous/flux_text_encoders t5xxl_fp8_e4m3fn.safetensors text_encoders
    fetch black-forest-labs/FLUX.1-schnell ae.safetensors vae
    # And the one pose ControlNet the FLUX family has that is maintained —
    # under a licence that is not open source, on a base that is not
    # schnell. `forge setup` always passes --no-flux-controlnet: a licence
    # is accepted at a door by name, and this one has no id in the toolkit's
    # table, so a `--yes` about nvdiffrast must never be able to accept it.
    if [ "$NO_FLUX_CONTROLNET" = 1 ]; then
        log "--no-flux-controlnet: skipping the FLUX pose ControlNet (the FLUX arm of the spike then has no pose conditioning)"
    elif [ -f "$M/controlnet/FLUX.1-dev-ControlNet-Union-Pro-2.0.safetensors" ]; then
        log "have controlnet/FLUX.1-dev-ControlNet-Union-Pro-2.0.safetensors"
    else
        confirm_license "Shakker-Labs/FLUX.1-dev-ControlNet-Union-Pro-2.0 is licensed under the
FLUX.1-dev Non-Commercial License — NOT an open-source licence — and was
trained on FLUX.1-dev, not on the Apache-2.0 FLUX.1-schnell it will be
applied to. It is fetched only so the Phase 0 spike can condition both
candidate image models on a pose; nothing may ship a reference that names
it until that licence has been weighed. --no-flux-controlnet skips it."
        fetch Shakker-Labs/FLUX.1-dev-ControlNet-Union-Pro-2.0 diffusion_pytorch_model.safetensors controlnet FLUX.1-dev-ControlNet-Union-Pro-2.0.safetensors
    fi
fi

# ---------------------------------------------------------------- snapshot --

if [ "$NO_SERVICE" != 1 ]; then
    # `GET /v2/snapshot/get_current` — the Manager's own answer to "what is
    # installed": the ComfyUI commit, every custom node pack (one: the GGUF
    # loader) and every pip in the venv. Pretty-printed so the committed copy diffs
    # line by line when the environment moves.
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
log "done — systemctl --user status $UNIT; the daemon drives it over http://127.0.0.1:$PORT"
