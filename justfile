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

# `--adopt-env` / `--adopt-checkout` onboard an install that already exists.
# nvdiffrast is non-commercial and asks before it is fetched.
#
# Install one backend under backends/<name>/: `just setup trellis2`
setup name *flags: (_later "P2" "backends/<name>/install.sh behind forge setup")

# ok | partial | missing | broken per backend; exits non-zero if any is not ok.
#
# Every backend, Blender, ffmpeg, the GPU and the rig profile in one table.
doctor *flags: (_later "P2" "forge doctor aggregating forge gen doctor --json")

# Look before you spend: the generators do not share 24 GB, and a second one
# started blind ends in an OOM, not a queue.
#
# Who holds the GPU right now.
gpu: (_later "P2" "forge gpu over nvidia-smi")

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

# The seed is a real knob: one front view underdetermines the back of a
# shape, and a seed can leave the rear of a skull absent. Look with `views`
# before rigging anything. `just character vex_runner --seed 7`
#
# Reference PNG -> textured character mesh via TRELLIS.2, to out/lifts/.
character name *flags: (_later "P2" "forge gen mesh on assets-src/refs/characters/<name>.png")

# Reference PNG -> textured prop mesh via TRELLIS.2, to out/lifts/: `just prop barrel`
prop name *flags: (_later "P2" "forge gen mesh on assets-src/refs/props/<name>.png")

# Refuses a mesh that is not near the T-pose; the fix is always the reference
# image, never the weights. Then `just promote-mesh <name>`.
#
# Lifted glb -> rigged .blend + rig record in headless Blender.
rig-mesh name *flags: (_later "P2" "forge gen rig")

# Metres; floor, ceiling or grip at the origin; matte. Then `just promote-mesh`.
#
# Normalize a lifted glb into a prop, to out/props/: `just prop-import barrel --height 0.9`
prop-import name *flags: (_later "P2" "forge gen prop")

# Needs the GPU: ARDY is ~16 GB, so nothing else large may be resident.
#
# Audition animation prompts: `just sweep prompts.txt`
sweep prompts *flags: (_later "P2" "forge gen motion sweep")

# The metrics table (foot contact, drift, frozen joints) and a contact sheet
# of every take. The user's eye outranks the sheet.
#
# Review a sweep: `just review out/sweeps/walk`
review dir *flags: (_later "P2" "forge gen motion review")

# One sound effect from a prompt, to out/audio/: `just sfx "door slam"`
sfx prompt *flags: (_later "P2" "forge gen sfx (MOSS)")

# The ACE-Step server stays resident (~8 GB) until `--stop-server`.
#
# One music track from a prompt, to out/audio/.
music prompt *flags: (_later "P2" "forge gen music (ACE-Step)")

# One spoken line, to out/audio/: `just speech "Stand down." --voice calm`
speech text *flags: (_later "P2" "forge gen speech (MOSS-TTS)")

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

# The rig gates run first, then the write, then the manifest. Refuses an
# existing name unless told `--overwrite`, and echoes both records when it does.
# What this recipe ends as: the Blender export (P2, `forge gen export`) and the
# engine-side rig check (P3, `forge rig check`) in front of the engine-free
# door that already exists — `forge promote body <glb> <name> --blend
# --lift-record --rig-record --export-record`, or `forge promote model` for a
# prop. Until then, call that door directly on a .glb you have checked.
#
# File a rigged body or a normalized prop into the library with its record.
promote-mesh name *flags: (_later "P2/P3" "forge gen export + forge rig check, then forge promote body | forge promote model")

# Native bake, no Blender. The shipped recipe is the starting point when the
# name exists; the flags you state land on top; the whole recipe is echoed.
# `just promote-clip walk out/sweeps/walk/take_3.npz --loop --loop-blend 0.2 --trim-start 0.2`
#
# Bake one take into a clip with a recipe and file it.
promote-clip name take *flags: _build
    {{forge}} promote clip {{take}} {{name}} {{flags}}

# `just promote-audio sfx door_slam out/audio/door_slam.wav --record out/audio/door_slam.json`
#
# File one sound from out/audio/ as sfx, music or voice, with its record.
promote-audio kind name file *flags: _build
    {{forge}} promote audio {{kind}} {{file}} {{name}} {{flags}}

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
#   rig, rig-mesh, prop-import
#                   Blender.
#   promote-*, manifest, rebake, migrate, setup
#                   they rewrite assets, sources or the machine.
#
# Once smoke joins it is not GPU-free: that needs an adapter (llvmpipe is
# enough) but no display and no Blender. Deliberate — it is the check that
# catches what a component assertion cannot.
#
# The pre-commit gate: fmt-check, clippy, tests, smoke, audit, manifest-check, verify.
ci: fmt-check check test smoke audit manifest-check verify

# forge_studio, forge_mcp and forge stay `publish = false`.
#
# Pre-release gate: every crate meant for the registry must package in isolation.
publish-check: (_later "P6" "cargo package for the seven library crates")

# Every stub above ends here: the name is reserved, the phase is named.
[no-exit-message]
_later phase what:
    @echo "not yet: lands in {{phase}} ({{what}})" >&2
    @exit 1
