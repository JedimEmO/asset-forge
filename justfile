# asset-forge — local generation and previews of game assets, for any game.
#
# Every generator runs on your GPU; every output is an engine-agnostic file
# (glTF, WAV/OGG, PNG, JSON) plus one manifest. Paths come from forge.toml at
# the root, so nothing here needs plumbing. `just` on its own lists everything.
#
# A recipe that answers "not yet: lands in P<n>" is a promise, not a bug: the
# phases are in the plan, and the name is reserved here so the skills can be
# written against it before the code exists.

stage_body := "bodies/vex_runner.glb"
forge := "target/debug/forge"

default:
    @just --list

# The binary is wiring and printing; every recipe below that starts with
# {{forge}} builds it first. forge_capture rides along so `smoke` and the
# P3 renders find their crate already compiled.
_build:
    cargo build -q -p forge -p forge_capture

# ------------------------------------------------------------------ setup --

# `--adopt-env DIR --adopt-checkout DIR` onboard an install that already
# exists; `--no-models` leaves the weights to the first run; `--yes` accepts
# nvdiffrast's non-commercial licence without a prompt (it is printed either
# way). `all` runs every backends/*/install.sh in turn with the same flags.
#
# Install one backend under backends/<name>/: `just setup trellis2 --yes`
setup backend="all" *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{backend}}" = all ]; then
        for script in backends/*/install.sh; do
            echo "== $(basename "$(dirname "$script")")"
            bash "$script" {{flags}}
        done
    else
        [ -f backends/{{backend}}/install.sh ] || { echo "no backends/{{backend}}/install.sh — known: $(ls backends/*/install.sh | xargs -n1 dirname | xargs -n1 basename | tr '\n' ' ')" >&2; exit 2; }
        bash backends/{{backend}}/install.sh {{flags}}
    fi

# ok | partial | missing | broken per backend; exits 1 if any is not ok —
# partial means the env runs but a weight is not cached, and the first
# generate through it would download for minutes. `--json` for a machine,
# `--quick` to skip the in-env probes (seconds each).
#
# Every backend, Blender, ffmpeg, the GPU and the rig profile in one table.
[no-exit-message]
doctor *flags: _build
    {{forge}} doctor {{flags}}

# Look before you spend: the generators do not share 24 GB, and a second one
# started blind ends in an OOM, not a queue. Exits 1 when the largest backend
# (TRELLIS.2 at 1024³, 22 GB) would not fit in what is free, naming who holds
# the rest — `forge gen music --stop-server` is the usual answer.
#
# Who holds the GPU right now.
[no-exit-message]
gpu *flags: _build
    {{forge}} gpu {{flags}}

# Not an asset, a law — the one mesh-shaped thing in the library nobody lifts.
# Until P2 brings `forge gen rig-build` (Blender) this is the data half: the
# contract re-derived from the committed rig.glb — a diff here means the rig
# changed — and the fixture mannequin every test stands on.
#
# Re-export the profile's contract and write its mannequin (rigs/humanoid/).
rig: _build
    cargo run -q -p forge_rig --example export_contract -- rigs/humanoid
    {{forge}} rig fixture out/fixture/mannequin.glb

# --------------------------------------------------------------- generate --

# Every recipe here is `forge gen <cmd>`: the Python launcher resolves the
# backend before any GPU work (a missing one exits 3 in ~100 ms), writes
# where --out says plus a forge_record beside it, and never touches assets/.
# `FORGE_FAKE=1` in front of any of them writes placeholders that pass the
# same validators, with no backend and no Blender — what `ci-fake` runs.

# The seed is a real knob: one front view underdetermines the back of a
# shape, and a seed can leave the rear of a skull absent. Look with `views`
# before rigging anything. The preset is the register — 1024³, 25 000
# vertices, a 1024² texture, seed 42 — and `--seed N`, `--verts N`,
# `--resolution 512`, `--texture 512` land on top of it. The lift record
# lands beside the PNG as <name>.lift.json; `just character vex_runner --seed 7`.
#
# Reference PNG -> textured character mesh via TRELLIS.2, to out/lifts/.
character name *flags: _build
    mkdir -p out/lifts
    {{forge}} gen mesh assets-src/refs/characters/{{name}}.png --preset character \
        --out out/lifts/{{name}}.glb --record assets-src/refs/characters/{{name}}.lift.json {{flags}}

# The prop register: 1024³, 6 000 vertices, 1024² texture, seed 42; the same
# flags land on top. `just prop barrel --seed 3 --resolution 512`
#
# Reference PNG -> textured prop mesh via TRELLIS.2, to out/lifts/: `just prop barrel`
prop name *flags: _build
    mkdir -p out/lifts
    {{forge}} gen mesh assets-src/refs/props/{{name}}.png --preset prop \
        --out out/lifts/{{name}}.glb --record assets-src/refs/props/{{name}}.lift.json {{flags}}

# Refuses a mesh that is not near the T-pose; the fix is always the reference
# image, never the weights. Writes assets-src/blender/<name>.blend and its
# rig record beside it. Then `just promote-mesh <name>`.
#
# Lifted glb -> rigged .blend + rig record in headless Blender.
rig-mesh name *flags: _build
    mkdir -p assets-src/blender
    {{forge}} gen rig out/lifts/{{name}}.glb --out assets-src/blender/{{name}}.blend \
        --record assets-src/blender/{{name}}.rig.json --name {{name}} {{flags}}

# Metres; floor, ceiling or grip at the origin; matte — then straight into
# the library as a model with both records. `--height`/`--length` is the one
# flag it needs; `--hang`, `--held`, `--grip M` move the origin.
#
# Normalize a lifted glb into a prop and file it: `just prop-import barrel --height 0.9`
prop-import name *flags: _build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p out/props
    {{forge}} gen prop out/lifts/{{name}}.glb --out out/props/{{name}}.glb \
        --record out/props/{{name}}.prop.json {{flags}}
    lift=assets-src/refs/props/{{name}}.lift.json
    if [ -f "$lift" ]; then
        {{forge}} promote model out/props/{{name}}.glb {{name}} --lift-record "$lift" --prop-record out/props/{{name}}.prop.json
    else
        echo "no $lift — the model will say reconstructed, not recorded" >&2
        {{forge}} promote model out/props/{{name}}.glb {{name}} --prop-record out/props/{{name}}.prop.json
    fi

# Needs the GPU: ARDY is ~16 GB, so nothing else large may be resident. One
# prompt, a grid of seeds and samples, into out/sweeps/<seed>-<8 chars of the
# prompt's sha>/ — then the review table and sheet over every take there.
# `just sweep "a person walks forward" --duration 2 --samples 4 --seeds 0 1`
#
# Audition an animation prompt: generate the takes, then review them.
sweep prompt *flags: _build
    #!/usr/bin/env bash
    set -euo pipefail
    seed=$(printf '%s\n' "{{flags}}" | sed -n 's/.*--seeds \([0-9][0-9]*\).*/\1/p'); seed="${seed:-0}"
    tag=$(printf '%s' "{{prompt}}" | sha256sum | cut -c1-8)
    dir="out/sweeps/${seed}-${tag}"
    mkdir -p "$dir"
    {{forge}} gen motion sweep --out-dir "$dir" --prompt "{{prompt}}" {{flags}}
    just review "$dir"

# The metrics table (foot contact, drift, frozen joints) and a contact sheet
# of every take; `--intent loop` judges for a cycle. The user's eye outranks
# the sheet.
#
# Review a sweep: `just review out/sweeps/0-1a2b3c4d`
review dir *flags: _build
    {{forge}} gen motion review {{dir}}/*.npz --sheet {{dir}}/sheet.png --metrics {{dir}}/metrics.json {{flags}}
    @echo "sheet: {{dir}}/sheet.png  metrics: {{dir}}/metrics.json"

# Describe the sound, not the game event: material, action, environment,
# tail. `--seconds 3` by default; the record lands beside the wav.
#
# One sound effect from a prompt, to out/audio/sfx/: `just sfx door_slam "heavy oak door slams shut"`
sfx name prompt *flags: _build
    mkdir -p out/audio/sfx
    {{forge}} gen sfx --prompt "{{prompt}}" --out out/audio/sfx/{{name}}.wav \
        --record out/audio/sfx/{{name}}.json {{flags}}

# The ACE-Step server stays resident (~8 GB) until `--stop-server`, which can
# ride on the same call: `just music hub_theme "hopeful synthwave" --duration 60 --stop-server`.
#
# One music track from a prompt, to out/audio/music/.
music name prompt *flags: _build
    mkdir -p out/audio/music
    {{forge}} gen music --prompt "{{prompt}}" --out out/audio/music/{{name}}.ogg \
        --record out/audio/music/{{name}}.json {{flags}}

# A voice is a reference clip (5–15 s of clean speech): `--voice assets-src/voices/<who>.wav`.
#
# One spoken line, to out/audio/voice/: `just speech kessa_hold "Hold the line." --voice assets-src/voices/kessa.wav`
speech name text *flags: _build
    mkdir -p out/audio/voice
    {{forge}} gen speech --text "{{text}}" --out out/audio/voice/{{name}}.wav \
        --record out/audio/voice/{{name}}.json {{flags}}

# ------------------------------------------------------------------- look --

# Front, back, both sides and three head close-ups, to out/views/<stem>.png.
# Back-face culling is off for anything under out/, so a face's inside
# showing through from behind means the surface is missing, not flipped.
# Run it on the raw lift BEFORE rig-mesh. `--no-head` for a prop.
#
# One contact sheet of a glb from seven angles: `just views out/lifts/vex_runner.glb`
views target *flags: (_later "P3" "forge views, rendered by the studio")

# `just studio --model models/barrel.glb`; `--take out/sweeps/x.npz` plays a
# raw take on the real body.
#
# Open the viewer: library browser, stage, transport, metadata, audio.
studio *flags: (_later "P3" "forge studio")

# The same window, opened on the audio library: hear a file, see it, check the mix.
play *flags: (_later "P3" "forge studio --audio")

# Contact sheet for one clip on the stage body: `just sheet walk`
sheet clip *flags: (_later "P3" "forge sheet")

# Contact sheet for every clip. Exits non-zero naming any that render badly.
sheets *flags: (_later "P3" "forge sheet over the catalog")

# Not a gate: renders are not byte-stable across GPUs.
#
# Turntable sheets of every shipped body, for a human to judge.
body-sheets *flags: (_later "P3" "forge turntable over the catalog")

# Which bones a clip actually drives on the stage body: `just bones walk`
bones clip *flags: (_later "P3" "forge bones")

# `--out sheet.png` also renders it playing the reference walk.
#
# Validate a mesh against the rig profile: `just check-mesh out/export/x.glb`
check-mesh glb *flags: (_later "P3" "forge rig check")

# Exits non-zero if the file is silent, clipped or will not decode; the plot
# lands beside the others in out/audio/.
#
# Inspect one audio file: measure it and plot it. `just audio out/audio/x.wav`
audio file *flags: _build
    {{forge}} audio inspect {{file}} --out out/audio/$(basename "{{file}}" | sed 's/\.[^.]*$//').png {{flags}}

# An empty library passes: a project that ships no sound is not a broken one.
#
# Measure every audio asset. Exits non-zero if any is silent or clipped.
audio-list *flags: _build
    {{forge}} audio list {{flags}}

# Plot every audio asset into out/audio/. Exits non-zero naming any defective one.
audio-plots dir="out/audio": _build
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p {{dir}}
    failed=()
    while IFS= read -r f; do
        name=$(basename "$f"); name="${name%.*}"
        if ! {{forge}} audio inspect "$f" --out {{dir}}/$name.png >/dev/null 2>&1; then
            failed+=("$name")
        fi
    done < <(find assets/audio -type f \( -name '*.wav' -o -name '*.ogg' -o -name '*.mp3' -o -name '*.flac' \) | sort)
    echo "wrote $(ls {{dir}}/*.png 2>/dev/null | wc -l) plots to {{dir}}"
    if [ ${#failed[@]} -gt 0 ]; then
        echo "defective: ${failed[*]}" >&2
        exit 1
    fi

# What the library holds: `just catalog --kind sfx --filter door`
catalog *flags: _build
    {{forge}} catalog {{flags}}

# ------------------------------------------------------------------- ship --

# The whole path from a rigged .blend into the library, in the order the
# gates have to run: the Blender export (which refuses a .blend that breaks
# the contract) into out/export/, then the engine-free door — `forge promote
# body` — with every record it was made from: the lift beside the PNG, the
# rig beside the .blend, the export beside the .glb. Refuses an existing name
# unless told `--overwrite`.
# TODO(P3): `forge rig check out/export/<name>.glb` joins between the export
# and the promote, once the engine-side check lands.
#
# Export, validate and file one rigged body: `just promote-mesh vex_runner`
promote-mesh name *flags: _build
    mkdir -p out/export
    {{forge}} gen export assets-src/blender/{{name}}.blend --out out/export/{{name}}.glb \
        --record out/export/{{name}}.export.json
    {{forge}} promote body out/export/{{name}}.glb {{name}} \
        --blend assets-src/blender/{{name}}.blend \
        --lift-record assets-src/refs/characters/{{name}}.lift.json \
        --rig-record assets-src/blender/{{name}}.rig.json \
        --export-record out/export/{{name}}.export.json {{flags}}

# Native bake, no Blender. The shipped recipe is the starting point when the
# name exists; the flags you state land on top; the whole recipe is echoed.
# `just promote-clip walk out/sweeps/walk/take_3.npz --loop --loop-blend 0.2 --trim-start 0.2`
#
# Bake one take into a clip with a recipe and file it.
promote-clip name take *flags: _build
    {{forge}} promote clip {{take}} {{name}} {{flags}}

# The record is the file's stem + .json, which is where `just sfx|music|speech`
# put it; a `--record` among the flags names another, and no record at all
# files the sound as `unknown` provenance — honest, and said on stderr.
# `just promote-audio sfx door_slam out/audio/sfx/door_slam.wav`
#
# File one sound from out/audio/ as sfx, music or voice, with its record.
promote-audio kind name file *flags: _build
    #!/usr/bin/env bash
    set -euo pipefail
    record="${{file}}"; record="${record%.*}.json"
    case " {{flags}} " in
        *" --record "*|*" --record="*) {{forge}} promote audio {{kind}} {{file}} {{name}} {{flags}} ;;
        *) if [ -f "$record" ]; then
               {{forge}} promote audio {{kind}} {{file}} {{name}} --record "$record" {{flags}}
           else
               echo "no record at $record — the sound will say unknown provenance" >&2
               {{forge}} promote audio {{kind}} {{file}} {{name}} {{flags}}
           fi ;;
    esac

# Project the library into assets/library.json. Run it after any hand edit.
manifest: _build
    {{forge}} manifest

# A body has no recipe to re-derive from — it is the file that was rigged
# and approved — and is skipped by name, with the re-ship spelled out.
#
# Re-bake every shipped clip from its own record. Native, no Blender.
rebake *flags: _build
    {{forge}} rebake {{flags}}

# Idempotent, and honest: values that were only ever defaults are nulled,
# never carried forward. `--dry-run` reports without writing.
#
# Bring every sidecar up to the current schema.
migrate *flags: _build
    {{forge}} migrate {{flags}}

# ----------------------------------------------------------------- verify --

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# From P1 a second line builds the engine-free crates with
# `--no-default-features`: a feature nothing builds is a feature that stops
# compiling in a week.
#
# Clippy over the workspace, warnings as errors.
check:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Everything else rests on this, so it gets its own recipe. The display is
# removed from the environment on purpose: if this passes, nothing in the
# capture path needs X11 or Wayland — an adapter (llvmpipe is enough), yes.
#
# Prove offscreen rendering works with no display server at all.
smoke:
    env -u DISPLAY -u WAYLAND_DISPLAY cargo run -q -p forge_capture --example smoke

# Rebuild every shipped clip from its own record and compare against the glb,
# by pose on the rig to under a millimetre; and hold every shipped body to
# the claim its record makes — the bytes that were approved, the source that
# is still there.
#
# Every clip rebuilds, every body is what it claims.
audit *flags: _build
    {{forge}} audit {{flags}}

# Every contract bone at its depth, the rest pose, the weights, stature and
# feet, the reference walk binding 27/27. An empty directory passes: a
# library that ships no body is not a broken one.
#
# Validate every shipped body against the rig profile.
check-bodies *flags: (_later "P3" "forge rig check over assets/bodies")

# Fail if the committed manifest no longer matches a rebuild of the library.
manifest-check: _build
    {{forge}} manifest --check

# Sidecars, hashes, the rig profile's drift, the reference ledger — a PNG
# without a row in assets-src/SOURCES.md fails.
#
# Every engine-free check on the library.
verify *flags: _build
    {{forge}} verify {{flags}}

# The gate to run before committing. Everything in it is a pass/fail
# question with no judgement in it, and the whole recipe is minutes, not
# tens of minutes.
#
# What it covers today: formatting, clippy over the workspace, the test
# suite, offscreen rendering with no display server, every clip rebuilding
# from its own record, the committed manifest against a rebuild, and the
# engine-free verify — sidecars, hashes, profile drift, the reference ledger.
# What still joins it, in the phase that makes it real:
#   check-bodies    P3   the rig profile held against every shipped body
# so that it ends as:
#   ci: fmt-check check test smoke audit check-bodies manifest-check verify
#
# What it deliberately leaves out, and why:
#   publish-check   `cargo package` runs in isolation; only a release can
#                   break it, and only a release cares.
#   views, sheet, sheets, body-sheets, audio-plots, studio, play
#                   renders for a human to look at. Not byte-stable across
#                   GPUs, so there is no pass/fail in them.
#   character, prop, sweep, review, sfx, music, speech
#                   generation: a 16–22 GB checkpoint on the GPU, minutes
#                   each, and nothing about the result is a yes/no question.
#                   `ci-fake` runs the same paths on placeholders instead.
#   rig, rig-mesh, prop-import, promote-mesh
#                   Blender.
#   promote-*, manifest, rebake, migrate, setup
#                   they rewrite assets, sources or the machine.
#   doctor, gpu     they describe this machine, and a runner is not it.
#
# Once smoke joins it is not GPU-free: that needs an adapter (llvmpipe is
# enough) but no display and no Blender. Deliberate — it is the check that
# catches what a component assertion cannot.
#
# The pre-commit gate: fmt-check, clippy, tests, smoke, audit, manifest-check, verify.
ci: fmt-check check test smoke audit manifest-check verify

# The generate paths with no GPU, no backend and no Blender: FORGE_FAKE=1
# makes every `forge gen` write placeholders that pass the same validators
# the real outputs must — a glb that verify_glb accepts, an npz Take::read
# accepts, a WAV, a PNG — and a real record saying `fake: true`. Run in a
# throwaway project made by `forge init`, so nothing under assets/ here is
# touched, and ending in that project's own audit, manifest-check and
# verify. The reference PNG is written here too (a 4×4 flat grey), with its
# ledger row, because a PNG without a row fails verify and should.
#
# The four pipelines end to end on placeholders, then every gate.
ci-fake: _build
    #!/usr/bin/env bash
    set -euo pipefail
    forge="$(pwd)/{{forge}}"
    work=$(mktemp -d -t forge-fake.XXXXXX)
    trap 'rm -rf "$work"' EXIT
    export FORGE_FAKE=1 FORGE_HOME="$(pwd)"
    "$forge" init --project "$work" --name fake >/dev/null
    cd "$work"
    mkdir -p assets-src/refs/props assets-src/refs/characters out/lifts out/props assets-src/blender out/export out/sweeps out/audio/sfx
    python3 - <<'PY'
    import struct, zlib
    def png(path, w, h, rgb):
        raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
        def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
        with open(path, "wb") as f:
            f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    png("assets-src/refs/props/box.png", 4, 4, (200, 200, 200))
    png("assets-src/refs/characters/figure.png", 4, 4, (200, 200, 200))
    PY
    printf '| refs/props/box.png | ci-fake placeholder | box | 2026-08-23 |\n| refs/characters/figure.png | ci-fake placeholder | figure | 2026-08-23 |\n' >> assets-src/SOURCES.md
    echo "== mesh -> prop -> promote model"
    "$forge" gen mesh assets-src/refs/props/box.png --preset prop --out out/lifts/box.glb --record assets-src/refs/props/box.lift.json --seed 1 --verts 2000
    "$forge" gen prop out/lifts/box.glb --out out/props/box.glb --record out/props/box.prop.json --height 1.0
    "$forge" promote model out/props/box.glb box --lift-record assets-src/refs/props/box.lift.json --prop-record out/props/box.prop.json
    echo "== mesh -> rig -> export -> promote body"
    "$forge" gen mesh assets-src/refs/characters/figure.png --preset character --out out/lifts/figure.glb --record assets-src/refs/characters/figure.lift.json --seed 1 --verts 25000
    "$forge" gen rig out/lifts/figure.glb --out assets-src/blender/figure.blend --record assets-src/blender/figure.rig.json --name figure
    "$forge" gen export assets-src/blender/figure.blend --out out/export/figure.glb --record out/export/figure.export.json
    "$forge" promote body out/export/figure.glb figure --blend assets-src/blender/figure.blend --lift-record assets-src/refs/characters/figure.lift.json --rig-record assets-src/blender/figure.rig.json --export-record out/export/figure.export.json
    echo "== motion sweep -> review -> promote clip"
    "$forge" gen motion sweep --out-dir out/sweeps/walk --prompt "a person walks forward" --duration 2 --samples 1 --seeds 0
    "$forge" gen motion review out/sweeps/walk/*.npz --sheet out/sweeps/walk/sheet.png --metrics out/sweeps/walk/metrics.json
    take=$(ls out/sweeps/walk/*.npz | head -n1)
    "$forge" promote clip "$take" walk_fake --record "${take%.npz}.take.json"
    echo "== sfx -> promote audio"
    "$forge" gen sfx --prompt "a door" --seconds 1 --out out/audio/sfx/door.wav --record out/audio/sfx/door.json
    "$forge" promote audio sfx out/audio/sfx/door.wav door --record out/audio/sfx/door.json
    echo "== the gates"
    "$forge" catalog
    "$forge" audit
    "$forge" manifest --check
    "$forge" verify

# forge_studio, forge_mcp and forge stay `publish = false`.
#
# Pre-release gate: every crate meant for the registry must package in isolation.
publish-check: (_later "P6" "cargo package for the seven library crates")

# Every stub above ends here: the name is reserved, the phase is named.
[no-exit-message]
_later phase what:
    @echo "not yet: lands in {{phase}} ({{what}})" >&2
    @exit 1
