# Shared by every backends/<name>/install.sh — source it, do not run it.
#
#   set -euo pipefail
#   here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   . "$here/../_lib/common.sh"
#   parse_common_flags "$@"
#
# What an installer leaves behind in its own directory is four gitignored
# things: the .env symlink to the interpreter prefix (what the launcher
# execs), the .checkout symlink to the upstream clone, optional .text-encoders
# / .checkpoints links, and installed.json, the receipt. Everything heavy
# lives under $PREFIX — by default ${FORGE_BACKENDS_HOME:-~/.cache/asset-forge/
# backends}/<name> — or wherever --adopt-* says an existing install already
# is. No absolute path is written into anything tracked.
#
# Installers are idempotent: re-running one on a finished install re-links
# and re-probes and touches nothing else. Every trap an installer encodes is
# dated in designs/hosting.md; the script is the how, that file is the why.

# ------------------------------------------------------------------ output --

log() { printf '%s: %s\n' "${BACKEND_NAME:-install}" "$*" >&2; }
warn() { printf '%s: warning: %s\n' "${BACKEND_NAME:-install}" "$*" >&2; }
die() { printf '%s: FATAL %s\n' "${BACKEND_NAME:-install}" "$*" >&2; exit 1; }

# need_cmd NAME [HINT] — fail early if a host tool is missing.
need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is not on PATH${2:+ — $2}"
}

# ------------------------------------------------------------------- flags --

# parse_common_flags "$@" — every installer's flags, into these globals:
#   PREFIX            where the env and the clone go (--prefix DIR)
#   ADOPT_ENV         an existing interpreter prefix to link instead of making one
#   ADOPT_CHECKOUT    an existing upstream clone to link instead of cloning
#   ADOPT_TEXT_ENCODERS, ADOPT_CHECKPOINTS   likewise for the weight dirs
#   NO_MODELS         1 to skip weight downloads (doctor will say "partial")
#   YES               1 to accept every licence prompt without a TTY
#   EXTRA_ARGS        whatever the installer itself wants to handle
# Requires BACKEND_NAME and BACKEND_DIR to be set by the caller first.
parse_common_flags() {
    : "${BACKEND_NAME:?parse_common_flags: set BACKEND_NAME first}"
    : "${BACKEND_DIR:?parse_common_flags: set BACKEND_DIR first}"
    PREFIX="${FORGE_BACKENDS_HOME:-$HOME/.cache/asset-forge/backends}/$BACKEND_NAME"
    ADOPT_ENV=""
    ADOPT_CHECKOUT=""
    ADOPT_TEXT_ENCODERS=""
    ADOPT_CHECKPOINTS=""
    NO_MODELS=0
    YES=0
    EXTRA_ARGS=()
    while [ $# -gt 0 ]; do
        case "$1" in
            --prefix) PREFIX="$2"; shift 2 ;;
            --prefix=*) PREFIX="${1#*=}"; shift ;;
            --adopt-env) ADOPT_ENV="$2"; shift 2 ;;
            --adopt-env=*) ADOPT_ENV="${1#*=}"; shift ;;
            --adopt-checkout) ADOPT_CHECKOUT="$2"; shift 2 ;;
            --adopt-checkout=*) ADOPT_CHECKOUT="${1#*=}"; shift ;;
            --adopt-text-encoders) ADOPT_TEXT_ENCODERS="$2"; shift 2 ;;
            --adopt-text-encoders=*) ADOPT_TEXT_ENCODERS="${1#*=}"; shift ;;
            --adopt-checkpoints) ADOPT_CHECKPOINTS="$2"; shift 2 ;;
            --adopt-checkpoints=*) ADOPT_CHECKPOINTS="${1#*=}"; shift ;;
            --no-models) NO_MODELS=1; shift ;;
            --yes|-y) YES=1; shift ;;
            -h|--help)
                cat >&2 <<EOF
usage: bash backends/$BACKEND_NAME/install.sh [flags]
  --prefix DIR                 where the env and clone go (default \$FORGE_BACKENDS_HOME or ~/.cache/asset-forge/backends/$BACKEND_NAME)
  --adopt-env DIR              link an existing interpreter prefix (conda env or venv) instead of creating one
  --adopt-checkout DIR         link an existing upstream clone instead of cloning
  --adopt-text-encoders DIR    link an existing text-encoder directory (ardy)
  --adopt-checkpoints DIR      link an existing checkpoints directory (acestep)
  --no-models                  skip weight downloads; doctor will report "partial"
  --yes                        accept licence prompts without a TTY (nvdiffrast, Llama 3)
EOF
                exit 0 ;;
            *) EXTRA_ARGS+=("$1"); shift ;;
        esac
    done
    for adopted in "$ADOPT_ENV" "$ADOPT_CHECKOUT" "$ADOPT_TEXT_ENCODERS" "$ADOPT_CHECKPOINTS"; do
        [ -z "$adopted" ] || [ -d "$adopted" ] || die "--adopt-* path is not a directory: $adopted"
    done
    ADOPTED=0
    [ -z "$ADOPT_ENV$ADOPT_CHECKOUT$ADOPT_TEXT_ENCODERS$ADOPT_CHECKPOINTS" ] || ADOPTED=1
    export PREFIX ADOPT_ENV ADOPT_CHECKOUT ADOPT_TEXT_ENCODERS ADOPT_CHECKPOINTS NO_MODELS YES ADOPTED
}

# ------------------------------------------------------------------- clone --

# clone_pinned URL COMMIT DEST [--recursive] — a clone at exactly COMMIT.
# Idempotent: an existing DEST is fetched and reset to the commit (warning
# if it is dirty, never discarding edits). --recursive for a repo that
# vendors submodules (o-voxel's eigen): a shallow clone builds nothing and
# says so late.
clone_pinned() {
    local url="$1" commit="$2" dest="$3" recursive="${4:-}"
    need_cmd git
    if [ -d "$dest/.git" ]; then
        local head
        head="$(git -C "$dest" rev-parse HEAD)"
        if [ "$head" != "$commit" ]; then
            if [ -n "$(git -C "$dest" status --porcelain --untracked-files=no)" ]; then
                warn "$dest is at $head with local edits; leaving it (pinned: $commit)"
            else
                log "$dest is at $head; moving to the pinned $commit"
                git -C "$dest" fetch -q origin "$commit" || git -C "$dest" fetch -q origin
                git -C "$dest" checkout -q "$commit"
            fi
        fi
    else
        log "cloning $url @ ${commit:0:12} -> $dest"
        mkdir -p "$(dirname "$dest")"
        git clone -q "$url" "$dest"
        git -C "$dest" checkout -q "$commit"
    fi
    if [ "$recursive" = "--recursive" ]; then
        git -C "$dest" submodule update -q --init --recursive
    fi
}

# -------------------------------------------------------------------- envs --

# make_venv DEST PYVER — a venv at DEST with python PYVER (e.g. 3.12), via uv
# when it is on PATH (it fetches the interpreter if the host lacks it), else
# python<PYVER> -m venv. Idempotent.
make_venv() {
    local dest="$1" pyver="$2"
    if [ -x "$dest/bin/python" ]; then
        log "venv exists: $dest"
        return 0
    fi
    if command -v uv >/dev/null 2>&1; then
        log "uv venv --python $pyver $dest"
        uv venv -q --python "$pyver" "$dest"
    else
        need_cmd "python$pyver" "install uv (https://astral.sh/uv) or a python$pyver"
        log "python$pyver -m venv $dest"
        "python$pyver" -m venv "$dest"
        "$dest/bin/python" -m pip install -q --upgrade pip
    fi
}

# pip_install PREFIX ARGS... — pip into an env, through uv when present
# (faster, same resolver semantics for pinned wheels), with user site off.
pip_install() {
    local prefix="$1"; shift
    if command -v uv >/dev/null 2>&1; then
        PYTHONNOUSERSITE=1 uv pip install -q --python "$prefix/bin/python" "$@"
    else
        PYTHONNOUSERSITE=1 "$prefix/bin/python" -m pip install -q "$@"
    fi
}

# ------------------------------------------------------------------- links --

# _link TARGET LINK — a symlink, replaced if it points elsewhere.
_link() {
    local target="$1" link="$2"
    target="$(cd "$target" && pwd -P)" || die "not a directory: $1"
    if [ -L "$link" ]; then
        [ "$(readlink -f "$link")" = "$target" ] && return 0
        rm -f "$link"
    elif [ -e "$link" ]; then
        die "$link exists and is not a symlink — remove it by hand"
    fi
    ln -s "$target" "$link"
    log "$(basename "$link") -> $target"
}

# link_env DIR — the interpreter prefix the launcher execs (<backend>/.env).
link_env() {
    [ -x "$1/bin/python" ] || [ -x "$1/bin/python3" ] || die "$1 has no bin/python — not an interpreter prefix"
    _link "$1" "$BACKEND_DIR/.env"
}

# link_checkout DIR — the upstream clone (<backend>/.checkout).
link_checkout() { _link "$1" "$BACKEND_DIR/.checkout"; }

# link_extra NAME DIR — another gitignored link: link_extra text-encoders DIR
# writes <backend>/.text-encoders.
link_extra() { _link "$2" "$BACKEND_DIR/.$1"; }

# env_python — the interpreter behind .env.
env_python() {
    local link="$BACKEND_DIR/.env"
    if [ -x "$link/bin/python" ]; then echo "$link/bin/python"; else echo "$link/bin/python3"; fi
}

# _wsl_marker — write <backend>/.wsl-distro (just the distro name) whenever
# this install is running inside WSL2 ($WSL_DISTRO_NAME, which WSL sets in
# every session). A native-Windows `forge` uses its presence to route the
# interpreter through `wsl.exe -d <distro>`, which resolves the .env/.checkout
# symlinks above itself from inside the distro — they are POSIX symlinks to a
# Linux path and a Windows process cannot read them directly. Nothing else
# about link_env/link_checkout changes, and on real Linux/macOS
# $WSL_DISTRO_NAME is never set, so this is a no-op there.
_wsl_marker() {
    [ -n "${WSL_DISTRO_NAME:-}" ] || return 0
    printf '%s\n' "$WSL_DISTRO_NAME" > "$BACKEND_DIR/.wsl-distro"
}

# ----------------------------------------------------------------- receipt --

# write_installed_json — installed.json beside backend.toml: the commit the
# checkout is at, the env's python and torch versions, the date, and whether
# any part was adopted rather than made. Doctor reports it; nothing parses
# it for correctness — the probe is the proof.
write_installed_json() {
    local python commit="" pyver="?" torch="null"
    python="$(env_python)"
    if [ -d "$BACKEND_DIR/.checkout" ]; then
        commit="$(git -C "$BACKEND_DIR/.checkout" rev-parse --verify HEAD 2>/dev/null || true)"
    fi
    pyver="$(PYTHONNOUSERSITE=1 "$python" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))' 2>/dev/null || echo '?')"
    torch="$(PYTHONNOUSERSITE=1 "$python" -c 'import torch; print(torch.__version__)' 2>/dev/null || true)"
    if [ -n "$torch" ]; then torch="\"$torch\""; else torch="null"; fi
    local adopted=false
    [ "${ADOPTED:-0}" = 1 ] && adopted=true
    cat > "$BACKEND_DIR/installed.json" <<EOF
{
  "backend": "$BACKEND_NAME",
  "commit": "${commit}",
  "python": "$pyver",
  "torch": $torch,
  "date": "$(date -u +%Y-%m-%d)",
  "adopted": $adopted,
  "prefix": "$(readlink -f "$BACKEND_DIR/.env")"
}
EOF
    log "wrote installed.json (python $pyver, torch ${torch//\"/}, adopted=$adopted)"
    _wsl_marker
}

# ---------------------------------------------------------------- hf token --

# hf_token_present — 0 when a Hugging Face token is stored or exported.
# `hf auth login --token …`, never the interactive login: under an agent's
# shell there is no TTY and the prompt hangs or stores nothing.
hf_token_present() {
    [ -n "${HF_TOKEN:-}" ] && return 0
    local path="${HF_TOKEN_PATH:-${HF_HOME:-$HOME/.cache/huggingface}/token}"
    [ -s "$path" ]
}

# hf_gate REPO_ID — probe a gated repo with the env's python; prints the
# four login steps and returns 1 when access is not granted.
hf_gate() {
    PYTHONNOUSERSITE=1 "$(env_python)" "$BACKEND_DIR/../_lib/hf_gate.py" "$1"
}

# ------------------------------------------------------------------- probe --

# run_probe — run backends/<name>/probe.py under the env with the [env] from
# backend.toml (via the launcher, so the installer and doctor agree on the
# environment), and fail the install if it does. The probe is the proof the
# install worked; the receipt is written after it.
run_probe() {
    local repo
    repo="$(cd "$BACKEND_DIR/../.." && pwd)"
    [ -f "$BACKEND_DIR/probe.py" ] || die "no probe.py beside backend.toml"
    log "probing the env"
    python3 "$repo/python/forge_gen" doctor --backend "$BACKEND_NAME" --no-host --json > "$BACKEND_DIR/.probe.out" 2>&1 || true
    local line
    line="$(grep -E '^\{' "$BACKEND_DIR/.probe.out" | tail -1)"
    if [ -z "$line" ]; then
        # The real error (a too-old python3, an import crash) is in the
        # captured output; discarding it once told a user "no JSON line"
        # at the end of a multi-GB install. Show the tail before dying.
        warn "doctor printed no JSON line for $BACKEND_NAME; its last lines were:"
        tail -n 15 "$BACKEND_DIR/.probe.out" >&2
        rm -f "$BACKEND_DIR/.probe.out"
        die "doctor printed no JSON line for $BACKEND_NAME (its output is above)"
    fi
    rm -f "$BACKEND_DIR/.probe.out"
    local status
    status="$(printf '%s' "$line" | python3 -c 'import json,sys; print(json.load(sys.stdin)["backends"]["'"$BACKEND_NAME"'"]["status"])')"
    case "$status" in
        ok) log "probe ok" ;;
        partial) warn "probe ran; doctor says partial (weights or CUDA missing) — \`forge doctor\` has the detail" ;;
        *) die "probe failed: doctor says $status — \`python3 $repo/python/forge_gen doctor --backend $BACKEND_NAME\`" ;;
    esac
}

# ----------------------------------------------------------------- licence --

# confirm_license TEXT — print TEXT and require --yes or an interactive "y".
# Used for anything whose licence is not a plain open-source one (nvdiffrast
# is non-commercial; Llama 3 asks for attribution). Without a TTY and
# without --yes the install stops here, on purpose.
confirm_license() {
    printf '\n%s\n\n' "$1" >&2
    if [ "${YES:-0}" = 1 ]; then
        log "accepted via --yes"
        return 0
    fi
    if [ ! -t 0 ]; then
        die "this needs consent: re-run with --yes after reading the text above (no TTY to ask)"
    fi
    local answer
    printf 'Continue? [y/N] ' >&2
    read -r answer
    case "$answer" in
        y|Y|yes|YES) return 0 ;;
        *) die "declined" ;;
    esac
}
