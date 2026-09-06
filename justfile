# asset-forge — local generation and previews of game assets, for any game.
#
# Every generator runs on your GPU; every output is an engine-agnostic file
# (glTF, WAV/OGG, PNG, JSON) plus one manifest. Paths come from forge.toml at
# the root, so nothing here needs plumbing. `just` on its own lists everything.
#
# From another project — one made by `forge init` — the same recipes run as
# `just --justfile <this file> --working-directory . sheet walk`: the binary
# is found beside this file, the library is the one under the working
# directory, because `forge` walks up from there to its forge.toml. The dev
# recipes under "verify" (fmt, check, test, pytest, smoke, ci, …) are the
# exception the other way: they always act on this checkout, never on the
# working directory, so `… --working-directory <your-game> ci` gates the
# toolkit and does not lint, test or rewrite your game.
#
# .mcp.json launches ./target/debug/forge, which a fresh clone does not
# have: any recipe that touches the binary builds it first (`just doctor`
# is the usual first one), and `just install` puts a global `forge` on PATH.

forge := justfile_directory() / "target/debug/forge"

# Rustup discovers toolchains from cwd, not --manifest-path. Keep external-game
# recipes on this checkout's compiler instead of rebuilding with the user's default.
export RUSTUP_TOOLCHAIN := shell("sed -n 's/^channel *= *\"\\(.*\\)\"/\\1/p' \"$1/rust-toolchain.toml\"", justfile_directory())

default:
    @just --justfile {{justfile()}} --list

# The binary is wiring and printing; every recipe below that starts with
# {{forge}} builds it first. It links Bevy through forge_studio — minutes
# the first time, seconds after — which is the price of one binary that
# renders, checks and ships; the logic stays in the library crates so
# `cargo test -p forge_library` never pays it. forge_capture rides along so
# `smoke` finds its example already compiled.
_build:
    cargo build -q --manifest-path {{justfile_directory()}}/Cargo.toml -p forge -p forge_capture

# ------------------------------------------------------------------ setup --

# The kind-shaped front door is `forge setup [kind…]`: it prints one screen —
# per chosen kind the backends, their disk, the total and every licence fact
# in full — before a byte downloads, asks once, records what you accepted in
# $FORGE_BACKENDS_HOME/licences.json, and skips every backend doctor already
# calls ok. This recipe is the backend-shaped door under it, for installing
# or adopting one at a time.
#
# `--adopt-env DIR --adopt-checkout DIR` onboard an install that already
# exists; `--no-models` leaves the weights to the first run; `--yes` accepts
# every licence prompt without a TTY (nvdiffrast's non-commercial one is
# printed either way). `all` — the default — runs every backends/*/install.sh
# in turn with the same flags; that is roughly 80 GB of disk, so it prints
# the bill first and refuses to start without `--yes`.
#
# Install one backend under backends/<name>/: `just setup trellis2 --yes`
setup backend="all" *flags: _build
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    if [ "{{backend}}" = all ]; then
        # The bill, before anything is fetched — printed by the door that
        # knows it. A heredoc here said `acestep ~10 GB, moss_sfx venv +
        # ~11 GB, moss_tts 4B ~8 GB` long after none of those was true, and
        # a bill nobody can re-derive is exactly the drift `forge setup`
        # exists to stop: every weights figure it prints is the sum of that
        # backend's own `[[models]] gb`.
        "{{forge}}" setup props characters clips sfx music voice --dry-run || true
        echo
        echo "That screen is 'forge setup''s own, for all six kinds. This recipe is the" >&2
        echo "backend-shaped door under it: it runs each backends/*/install.sh in turn with" >&2
        echo "the flags you passed, which is NOT what 'forge setup' does — that one installs" >&2
        echo "only what your project's [make] chose, tells the comfy host which model group" >&2
        echo "to fetch, and hands an installer --yes only for licences you named." >&2
        case " {{flags}} " in
            *" --yes "*|*" -y "*) ;;
            *)
                echo >&2
                echo "Re-run as 'just setup all --yes' after reading the bill (add --no-models to" >&2
                echo "make the envs now and download weights on first use), or take one backend at" >&2
                echo "a time: 'just setup trellis2 --yes'. 'forge setup' is the kind-shaped door," >&2
                echo "and the one that asks about each licence by name." >&2
                exit 2 ;;
        esac
        for script in backends/*/install.sh; do
            echo "== $(basename "$(dirname "$script")")"
            bash "$script" {{flags}}
        done
    else
        [ -f backends/{{backend}}/install.sh ] || { echo "no backends/{{backend}}/install.sh — known: $(ls backends/*/install.sh | xargs -n1 dirname | xargs -n1 basename | tr '\n' ' ')" >&2; exit 2; }
        bash backends/{{backend}}/install.sh {{flags}}
    fi

# A release build of the CLI onto PATH (~/.cargo/bin), for shells that are
# not sitting in this checkout — `forge init` in your game is the usual
# reason. The recipes themselves never need it: they build and run
# ./target/debug/forge, which is also the binary .mcp.json launches.
#
# Put a global `forge` on PATH: `just install`
install:
    cargo install --path {{justfile_directory()}}/crates/forge --locked

# Five words per backend: ok | partial | missing | broken | off. `partial`
# means the env runs but a weight is not cached, and the first generate
# through it would download for minutes. `off` is not a probe result — it is
# `[make]` in forge.toml not having chosen the kind, so the row is never
# probed (which is what makes this fast on a props-only project), is printed
# with the line that turned it off, and never votes on the exit code.
# **Exits 1 only while a CHOSEN backend is not ok**; a project at tier
# `fake`, or with nothing chosen, reads all-off and exits 0.
# `--json` for a machine, `--quick` to skip the in-env probes (seconds each).
#
# Every backend, Blender, ffmpeg, the GPU and the rig profile in one table.
[no-exit-message]
doctor *flags: _build
    {{forge}} doctor {{flags}}

# Look before you spend: the generators do not share 24 GB, and a second one
# started blind ends in an OOM, not a queue. Exits 1 when the largest chosen
# backend would not fit in what is free, naming who holds the rest.
# `forge gpu --free` is the door — and for anything TTS-Audio-Suite loaded
# it is not enough: the pack has no unload node at this pin and POST /free
# does not touch its models, so `systemctl --user restart forge-comfy` (4.4 s,
# measured) is the lever. Native ACE-Step gives the card back by itself.
#
# Who holds the GPU right now.
[no-exit-message]
gpu *flags: _build
    {{forge}} gpu {{flags}}

# Not an asset, a law — the one mesh-shaped thing in the library nobody lifts.
# `forge gen rig-build` (Blender) rebuilds the artifact; this is the data
# half: the contract re-derived from the committed rig.glb — a diff here
# means the rig changed — and the fixture mannequin every test stands on,
# at out/fixture/mannequin.glb, where `sheet` and `studio` also reach for
# it when the library has no body. The contract half always acts on this
# checkout's rigs/humanoid/; the mannequin lands under the working
# directory's out/, which is where the recipes that need it look.
#
# Re-export the profile's contract and write its mannequin (rigs/humanoid/).
rig: _build
    cargo run -q --manifest-path {{justfile_directory()}}/Cargo.toml -p forge_rig --example export_contract -- {{justfile_directory()}}/rigs/humanoid
    {{forge}} rig fixture out/fixture/mannequin.glb

# ------------------------------------------------------------------ serve --

# One process that is the agent's door and the human's monitor: the queue,
# the card lock, the job table and both executors. The CLI is a client of it
# when one is up and runs in-process otherwise, so `just sfx` at a terminal
# and an agent's `generate_audio` go through one queue and cannot race for
# the card. It writes out/serve/daemon.json — the port and the token a
# client needs — and serves MCP over streamable HTTP at /mcp.
#
# `just serve` starts it in its own process group, waits for it to write
# out/serve/daemon.json, prints the port and comes back to the shell;
# `just serve --foreground` is the only mode that stays in this terminal.
#
# Start the daemon: `just serve` (add --foreground to keep it in this shell).
serve *flags: _build
    {{forge}} serve {{flags}}

# A job in flight is CANCELLED, not drained: the daemon may not exit with a
# generator still on the card, because the card lock goes with it and the
# next door would take the lease against a running generate. The row says
# `cancelled` with the note, and partial outputs under out/ are left.
#
# Stop the daemon.
stop *flags: _build
    {{forge}} stop {{flags}}

# Queued, running, done, failed, with what each one made and how long it took.
#
# Every job the daemon knows.
jobs *flags: _build
    {{forge}} jobs {{flags}}

# The last thing a generate said before it stopped saying anything is
# usually the answer.
#
# One job's log, tailed: `just job-log j-20260830-141207-3f9a`
job-log job *flags: _build
    {{forge}} job log {{job}} {{flags}}

# --------------------------------------------------------------- generate --

# Every recipe here is `forge gen <cmd>`: the Python launcher resolves the
# backend before any GPU work (a missing one exits 3 in ~100 ms), writes
# where --out says plus a forge_record beside it, and never touches assets/.
# `FORGE_FAKE=1` in front of any of them writes placeholders that pass the
# same validators, with no backend and no Blender — what `ci-fake` runs.

# The one way a PNG gets under assets-src/refs/. Format, then mesh.py's own
# keyer, then the four keyer pre-checks the 2026-08-30 spike proved ride all
# the way to a lift (a drawn floor, a contact shadow, a flood-through hole, a
# key that kept the backdrop), then the geometry pre-checks — all of it before
# a GPU minute, because a lift is four minutes and a redraw is a sentence. The
# PNG stored is the file you drew, byte for byte; the record and the
# SOURCES.md row are written by the door and never by hand.
# `just ref-import out/refs_grok/ember_knight_v3.png ember_knight_v3 character "xAI Grok, image_edit"`
#
# A drawn PNG -> a checked, recorded reference under assets-src/refs/.
ref-import image name kind source *flags: _build
    {{forge}} gen ref-import {{image}} --name {{name}} --kind {{kind}} --source "{{source}}" {{flags}}

# The reference format text has ONE home — FORMAT and FORMAT_AMENDMENT in
# python/forge_gen/reference.py — and every other copy is generated from it:
# `markdown` writes the block the forge-character skill includes, and the MCP
# tool's description is generated at build time by crates/forge_mcp/build.rs,
# which reads the same two constants. Three hand-maintained copies held to
# byte equality is a test that fails on a rewrap and teaches people to edit
# the fixture.
#
# Print the reference format: `just ref-format`, `just ref-format markdown`
ref-format kind="text": _build
    {{forge}} gen ref-import --print-format {{kind}}

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

# Metres, matte, dust dropped, the profile's skeleton inserted and NO weights
# — the skinner wants a bare mesh. Two gates, both about the picture and not
# about the weights: the arm tips level with THIS BODY'S OWN shoulder line
# ([fit] arm_height_tolerance_m 0.15, a budget), and every arm run's median
# cross-section at or above [fit] limb_radius_min_fraction 0.22 of its own
# length (measured: the sliver that walked with a 2.8 m arm read 0.20-0.21,
# and vex_runner's thinnest arm reads 0.245). The leg ratios are measured,
# printed and never refused — a T-pose isolates an arm and does not isolate a
# leg. A refusal names the reference PNG because that is where the fix is.
#
# Lifted glb -> normalised mesh + a skeleton, no weights: `just prepare vex_runner`
prepare name *flags: _build
    mkdir -p out/prepare
    {{forge}} gen prepare out/lifts/{{name}}.glb --out out/prepare/{{name}}.glb \
        --record out/prepare/{{name}}.prepare.json {{flags}}

# SkinTokens' weights, then the skeleton fitted to what those weights say this
# body's bones are, then a second prepare and skin against the fitted skeleton,
# then the re-attach — five steps, one door, no options about the number of
# passes (a second fit walks the torso downhill by 74 mm a time). Names,
# hierarchy and rest ROTATIONS stay frozen, so every clip still binds by name
# with nothing rebaked; lengths become a fact of this body that the sidecar
# records. Refuses a raw L/R gap over [fit] asymmetry_arms 0.35 on the arms or
# [fit] asymmetry_other 0.20 elsewhere, and a run fitted outside 0.4-2.5.
#
# Prepared glb -> weights on a skeleton fitted to this body: `just skin vex_runner`
skin name *flags: _build
    mkdir -p out/skin assets-src/blender
    {{forge}} gen skin out/prepare/{{name}}.glb --blend assets-src/blender/{{name}}.blend \
        --record assets-src/blender/{{name}}.rig.json --name {{name}} {{flags}}

# Dies by name, for one release, the same courtesy promote-mesh gets: the
# rename is not a deletion and an old command line deserves to be told so.
[private]
rig-mesh name="" *flags="":
    @echo "rig-mesh became prepare + skin when the skinner changed: bone heat is gone, SkinTokens makes the weights, and the skeleton is fitted to the body (just prepare <name> && just skin <name>, or just body <name>). See designs/skin.md." >&2
    @exit 1

# The whole loop on one lift, in the order the gates run. Needs the card:
# SkinTokens is 3.3-4.4 GB and runs twice. `just gpu` first.
#
# Lifted glb -> rigged .blend: prepare then skin. `just body vex_runner`
body name *flags: _build
    just --justfile {{justfile()}} --working-directory {{invocation_directory()}} prepare {{name}} {{flags}}
    just --justfile {{justfile()}} --working-directory {{invocation_directory()}} skin {{name}}

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
    # Not `just review`: from a user project this justfile is run as
    # `just --justfile <toolkit>/justfile --working-directory .`, and a bare
    # `just` inside a recipe would look for a justfile in the project and
    # find none. Call the tool directly.
    {{forge}} gen motion review "$dir"/*.npz --sheet "$dir"/sheet.png --metrics "$dir"/metrics.json
    echo "sheet: $dir/sheet.png  metrics: $dir/metrics.json"

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

# ACE-Step runs inside the ComfyUI host now, so there is no resident server
# of its own to stop and no `--stop-server`: it is native to the host, and
# measured 2026-08-30 it gives the card back by itself when the graph ends.
# At this pin a track renders and does NOT promote — the host normalises it
# to 0.0 dBFS and the clipping gate refuses it (designs/hosting.md).
#
# One music track from a prompt, to out/audio/music/.
music name prompt *flags: _build
    mkdir -p out/audio/music
    {{forge}} gen music --prompt "{{prompt}}" --out out/audio/music/{{name}}.ogg \
        --record out/audio/music/{{name}}.json {{flags}}

# Describe who speaks — gender, age, pitch, pace, accent, texture, mood —
# and the model speaks one audition line in that voice. The seed is the
# voice: reroll `--seed N` until it is the character, never edit the wav.
# Lands as a source, assets-src/voices/<name>/{ref.wav,voice.json}, and
# refuses an existing one without --overwrite — every line cloned from it
# afterwards would change. `--line` replaces the default audition sentence.
#
# Design a voice from a description: `just voice warden "Deep, slow, weathered male voice, grave and calm"`
voice name describe *flags: _build
    {{forge}} gen voice {{name}} --describe "{{describe}}" {{flags}}

# A voice is a reference clip (5–15 s of clean speech). `--voice <name>` is
# one designed by `just voice` (assets-src/voices/<name>/ref.wav, its record
# carried into the line's); `--voice path/to/clip.wav` is one you brought,
# which then needs a row in assets-src/SOURCES.md.
#
# One spoken line, to out/audio/voice/: `just speech kessa_hold "Hold the line." --voice kessa`
speech name text *flags: _build
    mkdir -p out/audio/voice
    {{forge}} gen speech --text "{{text}}" --out out/audio/voice/{{name}}.wav \
        --record out/audio/voice/{{name}}.json {{flags}}

# ------------------------------------------------------------------- look --

# Front, back, both sides and three head close-ups, to out/views/<stem>.png.
# Back-face culling is off for anything under out/, so a face's inside
# showing through from behind means the surface is missing, not flipped.
# Run it on the raw lift BEFORE prepare. `--no-head` for a prop; a library
# name (`just views barrel`) renders the shipped file, culling on.
#
# One contact sheet of a glb from seven angles: `just views out/lifts/vex_runner.glb`
views target *flags: _build
    {{forge}} views {{target}} {{flags}}

# Opens on forge.toml's stage_body, else the first body, else the fixture
# mannequin — never an empty stage. `just studio --model models/barrel.glb`;
# `--take out/sweeps/x.npz` plays a raw take on the real body.
#
# Open the viewer: library browser, stage, transport, metadata, audio.
studio *flags: _build
    {{forge}} studio {{flags}}

# The same window, opened on the audio library: hear a file, see it, check the mix.
play *flags: _build
    {{forge}} studio --audio {{flags}}

# Eight poses, three-quarter view, to out/sheets/<clip>.png; `--views all`,
# `--head-row`, `--body <name>` for another body. Exits 1 when the clip
# drives no bone or never moves — a picture of a T-pose is not a sheet.
#
# Contact sheet for one clip on the stage body: `just sheet walk`
sheet clip *flags: _build
    {{forge}} sheet {{clip}} {{flags}}

# Every clip the catalog holds, to out/sheets/. A clip that binds to nothing
# or never moves is named at the end and fails the recipe; an empty library
# passes, because a project that ships no clip is not a broken one.
#
# Contact sheet for every clip. Exits non-zero naming any that render badly.
sheets *flags: _build
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p out/sheets
    failed=()
    count=0
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        count=$((count + 1))
        log="out/sheets/$name.log"
        if ! {{forge}} sheet "$name" {{flags}} >"$log" 2>&1; then
            failed+=("$name")
            echo "sheet failed: $name (full log: $log)" >&2
            cat "$log" >&2
        fi
    done < <({{forge}} catalog --kind clip | awk 'NR > 1 && $1 == "clip" { print $2 }')
    echo "rendered $count clip(s) to out/sheets"
    if [ ${#failed[@]} -gt 0 ]; then
        echo "did not render cleanly: ${failed[*]}" >&2
        exit 1
    fi

# Every view and the head row, posed on the reference walk, to
# out/sheets/bodies/<name>.png. Not a gate: renders are not byte-stable
# across GPUs, and what a turntable shows is for a person to judge.
#
# Turntable sheets of every shipped body, for a human to judge.
body-sheets *flags: _build
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p out/sheets/bodies
    count=0
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        count=$((count + 1))
        {{forge}} turntable "$name" --out out/sheets/bodies/$name.png {{flags}} >/dev/null 2>&1 \
            || echo "did not render: $name" >&2
    done < <({{forge}} catalog --kind body | awk 'NR > 1 && $1 == "body" { print $2 }')
    echo "wrote $(ls out/sheets/bodies/*.png 2>/dev/null | wc -l) sheets for $count body(s) to out/sheets/bodies"

# No GPU: the names are hashed the way Bevy binds them and compared. Exits 1
# when nothing binds — the silent failure the whole check exists for.
#
# Which bones a clip actually drives on the stage body: `just bones walk`
bones clip *flags: _build
    {{forge}} bones {{clip}} {{flags}}

# Every contract bone at its depth, the rest pose, the weights, stature and
# feet, the reference walk binding 27/27. `--out sheet.png` also renders it
# playing the reference walk. Exits 1 on any FAIL line.
#
# Validate a mesh against the rig profile: `just check-mesh out/export/x.glb`
check-mesh glb *flags: _build
    {{forge}} rig check {{glb}} {{flags}}

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
# the contract) into out/export/, then the engine-side check — `forge rig
# check`, the hierarchy as Bevy will actually bind it, every contract bone
# at its depth, the reference walk 27/27 — and only then the door, `forge
# promote body`, with every record it was made from: the lift beside the
# PNG, the rig beside the .blend, the export beside the .glb. Refuses an
# existing name unless told `--overwrite`.
#
# Export, validate and file one rigged body: `just promote-body vex_runner`
promote-body name *flags: _build
    mkdir -p out/export
    {{forge}} gen export assets-src/blender/{{name}}.blend --out out/export/{{name}}.glb \
        --record out/export/{{name}}.export.json
    {{forge}} rig check out/export/{{name}}.glb
    {{forge}} promote body out/export/{{name}}.glb {{name}} \
        --blend assets-src/blender/{{name}}.blend \
        --lift-record assets-src/refs/characters/{{name}}.lift.json \
        --rig-record assets-src/blender/{{name}}.rig.json \
        --export-record out/export/{{name}}.export.json {{flags}}

# Dies by name, for one release, the courtesy `install.sh --models` got: an
# old command line deserves to be told what happened to it rather than
# "unknown recipe".
[private]
promote-mesh name="" *flags="":
    @echo "promote-mesh became promote-body when the skinner changed; the rig step is now prepare + skin (just prepare <name> && just skin <name>, or just body <name>). See designs/skin.md." >&2
    @exit 1

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
    record="{{file}}"; record="${record%.*}.json"
    case " {{flags}} " in
        *" --record "*|*" --record="*) {{forge}} promote audio {{kind}} {{file}} {{name}} {{flags}} ;;
        *) if [ -f "$record" ]; then
               {{forge}} promote audio {{kind}} {{file}} {{name}} --record "$record" {{flags}}
           else
               echo "no record at $record — the sound will say unknown provenance" >&2
               {{forge}} promote audio {{kind}} {{file}} {{name}} {{flags}}
           fi ;;
    esac

# A merge, not a bake: every clip's channels are re-pointed at the body's
# bones by name and their values copied, so a clip driving a bone the body
# lacks is refused by name. The record lands beside the file as
# <stem>.bundle.json. Nothing is filed in the library — a bundle is an
# export, regenerated rather than repaired.
# `just bundle out/fit_warlock/drow_warlock_fitted.glb walk,roll out/bundles/warlock.glb --motion-scale 1.0156`
#
# One glb carrying a body's skin and any number of clips as named animations.
bundle body clips out *flags: _build
    {{forge}} bundle {{body}} --clips {{clips}} --out {{out}} {{flags}}

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

# Every dev recipe here is pinned to this checkout the way `_build` is
# (`--manifest-path`, or a `cd` into {{justfile_directory()}}): run as
# `just --justfile <toolkit>/justfile --working-directory <your-game> ci`
# they gate the toolkit and never format, lint, test or rewrite your game.
# The library-facing gates (audit, check-bodies, manifest-check, verify)
# stay on the working directory on purpose — they judge *your* library.

fmt:
    cargo fmt --all --manifest-path {{justfile_directory()}}/Cargo.toml

fmt-check:
    cargo fmt --all --manifest-path {{justfile_directory()}}/Cargo.toml -- --check

# Two lines: clippy over the workspace, then the rustdoc build, both with
# warnings as errors — nothing else builds the docs, and an unbracketed
# `<placeholder>` in a doc comment is invisible until rustdoc reads it as
# HTML. (No crate in this workspace declares a cargo feature, so there is
# no feature matrix to hold green; the day one grows a feature, its build
# line goes here.)
#
# Clippy and rustdoc over the workspace, warnings as errors.
check:
    cargo clippy --workspace --all-targets --manifest-path {{justfile_directory()}}/Cargo.toml -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --manifest-path {{justfile_directory()}}/Cargo.toml

test:
    cargo test --workspace --manifest-path {{justfile_directory()}}/Cargo.toml

# The launcher's own suite — install python[dev]; no backend or GPU required.
# The Rust side's `python_records` test re-runs the record capture, but
# only this runs test_cli, test_launcher, test_backends, test_doctor,
# test_glb and test_npz.
#
# The Python layer's tests: pytest over python/tests.
pytest:
    cd {{justfile_directory()}}/python && python3 -m pytest -q

# Everything else rests on this, so it gets its own recipe. The display is
# removed from the environment on purpose: if this passes, nothing in the
# capture path needs X11 or Wayland — an adapter (llvmpipe is enough), yes.
#
# Prove offscreen rendering works with no display server at all.
smoke:
    env -u DISPLAY -u WAYLAND_DISPLAY cargo run -q --manifest-path {{justfile_directory()}}/Cargo.toml -p forge_capture --example smoke

# Rebuild every shipped clip from its own record and compare against the glb
# — by bytes, then by pose on the fixture mannequin to under a millimetre —
# and hold every shipped body to the claim its record makes (the bytes that
# were approved, the source that is still there) and to the rig contract.
# The posed half needs an animation player, not a renderer: no adapter.
# `--fit` names the recipe that would reproduce a clip whose recorded one
# does not.
#
# Every clip rebuilds, by bytes and by pose; every body is what it claims.
audit *flags: _build
    {{forge}} audit {{flags}}

# Every contract bone at its depth, the rest pose, the weights, stature and
# feet, the reference walk binding 27/27 — `forge rig check` on every glb
# under assets/bodies. An empty directory passes: a library that ships no
# body is not a broken one. No renderer: the mesh is spawned in a headless
# app and its hierarchy read as Bevy will bind it.
#
# Validate every shipped body against the rig profile.
check-bodies *flags: _build
    #!/usr/bin/env bash
    set -uo pipefail
    failed=()
    bodies=0
    while IFS= read -r glb; do
        [ -n "$glb" ] || continue
        bodies=$((bodies + 1))
        echo "== $glb"
        if ! {{forge}} rig check "$glb" {{flags}}; then
            failed+=("$(basename "$glb")")
        fi
    done < <(find assets/bodies -type f -name '*.glb' 2>/dev/null | sort)
    if [ "$bodies" -eq 0 ]; then
        echo "no bodies under assets/bodies — nothing to hold to the contract, and that passes"
    fi
    if [ ${#failed[@]} -gt 0 ]; then
        echo "failed the rig contract: ${failed[*]}" >&2
        exit 1
    fi
    echo "$bodies body(ies) conform to the rig profile"

# Fail if the committed manifest no longer matches a rebuild of the library.
manifest-check: _build
    {{forge}} manifest --check

# The server .mcp.json launches, driven the way a client drives it: a
# scripted initialize, the initialized notification and tools/list over
# stdin, newline-delimited JSON-RPC, and the reply checked for every tool
# name the skills are written against — no more, no fewer. No GPU: nothing is
# rendered, the list is the test.
# The server's own banner goes to stderr, which is the rule this also
# proves: anything on stdout that is not a frame would break the parse.
#
# Handshake `forge mcp` over stdio and check the tool surface.
mcp-check: _build
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    expected="audit cancel doctor export_body export_bundle generate_audio generate_clips generate_mesh import_reference init_project inspect_audio licences list_audio list_clips list_models list_runs manifest_check prepare_body prepare_prop promote_audio promote_body promote_clip promote_model render_clip_strip render_model setup skin_body status verify wait"
    reply=$(printf '%s\n' \
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mcp-check","version":"0"}}}' \
        '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
        '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
        | timeout 20 {{forge}} mcp 2>/dev/null)
    listed=$(printf '%s\n' "$reply" | python3 -c '
    import json, sys
    names = []
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        frame = json.loads(line)
        if frame.get("id") == 2:
            names = sorted(t["name"] for t in frame["result"]["tools"])
    print(" ".join(names))
    ')
    if [ "$listed" != "$expected" ]; then
        echo "mcp-check: tools/list said: ${listed:-(nothing)}" >&2
        echo "mcp-check: expected:        $expected" >&2
        exit 1
    fi
    echo "forge mcp serves $(echo "$expected" | wc -w | tr -d ' ') tools: $expected"

# The agent's whole path, scripted, over BOTH transports — stdio and the
# daemon's streamable HTTP at /mcp — because "one tool surface, two
# transports, one queue" is the claim this phase makes and a transport
# nothing exercises ships ungated. The script: initialize, tools/list
# against the pinned twenty-five, init_project, licences, the setup gate (a
# gated kind with an empty accept must refuse and name the id), doctor (an
# `off` row, exit 0), generate_audio (a job id comes back, not the file),
# wait, inspect_audio, promote_audio, verify — plus the two negative legs
# that rot silently: wait on an unknown job, and a second promote onto a
# taken name. Then the character loop, on the fake tier: import_reference,
# generate_mesh, wait, prepare_body, wait, skin_body, wait, export_body,
# wait, promote_body, render_model, verify — plus a second promote_body on
# the same name refused, then accepted with overwrite. Every step of it is a
# tool call: a gate that shells a missing verb in the middle of the loop it
# is holding green proves the shell, not the surface.
#
# It runs against `env!("CARGO_BIN_EXE_forge")`, so the binary under test is
# this build with no `just` step in front of it. No GPU, no display, no
# backend, no secret, no network: the project is a tempdir at tier `fake`.
#
# The agent's path through the MCP, end to end, on both transports.
mcp-session:
    cargo test -p forge --test mcp_session --manifest-path {{justfile_directory()}}/Cargo.toml -- --nocapture

# Sidecars, hashes, the rig profile's drift, the reference ledger — a PNG
# without a row in assets-src/SOURCES.md fails.
#
# Every engine-free check on the library.
verify *flags: _build
    {{forge}} verify {{flags}}

# The gate to run before committing — the one gate, the same set GitHub
# Actions runs. Everything in it is a pass/fail question with no judgement
# in it, and the whole recipe is minutes, not tens of minutes.
#
# What it covers: formatting, clippy and the rustdoc build over the
# workspace, the Rust test suite, the Python launcher's pytest suite,
# offscreen rendering with no display server, every clip rebuilding from
# its own record by bytes and by pose, the rig profile held against every
# shipped body, the committed manifest against a rebuild, the engine-free
# verify — sidecars, hashes, profile drift, the reference ledger — the MCP
# handshake and tool surface, the agent's whole scripted session over both
# transports (mcp-session), and the five generate pipelines end to end on
# FORGE_FAKE placeholders in a throwaway project, driven through the
# documented `just --justfile … --working-directory …` form.
#
# What it deliberately leaves out, and why:
#   publish-check   `cargo package` runs in isolation; only a release can
#                   break it, and only a release cares.
#   views, sheet, sheets, body-sheets, audio-plots, studio, play
#                   renders for a human to look at. Not byte-stable across
#                   GPUs, so there is no pass/fail in them — though `sheets`
#                   does exit non-zero on a clip that binds to nothing or
#                   never moves, and GitHub Actions runs it for that.
#   bones, check-mesh
#                   one asset at a time; `check-bodies` and `audit` run the
#                   same checks over the whole library.
#   character, prop, sweep, review, sfx, music, speech, voice
#                   generation: a 16–22 GB checkpoint on the GPU, minutes
#                   each, and nothing about the result is a yes/no question.
#                   `ci-fake` runs the same paths on placeholders.
#   prepare, skin, body, prop-import, promote-body
#                   Blender, and skin also wants the card.
#   promote-*, manifest, rebake, migrate, setup, install
#                   they rewrite assets, sources or the machine.
#   doctor, gpu     they describe this machine, and a runner is not it.
#
# It is not GPU-free: smoke needs an adapter (llvmpipe is enough) but no
# display and no Blender. Deliberate — it is the check that catches what a
# component assertion cannot. check-bodies and audit's posed half need no
# adapter at all: a headless app with an animation player and no renderer.
#
# The pre-commit gate: fmt, clippy+doc, tests, pytest, smoke, audit, check-bodies, manifest-check, verify, mcp-check, mcp-session, ci-fake.
ci: fmt-check check test pytest smoke audit check-bodies manifest-check verify mcp-check mcp-session ci-fake

# The generate paths with no GPU, no backend and no Blender: FORGE_FAKE=1
# makes every `forge gen` write placeholders that pass the same validators
# the real outputs must — a glb that verify_glb accepts, an npz Take::read
# accepts, a WAV, a PNG — and a real record saying `fake: true`. Run in a
# throwaway project made by `forge init`, so nothing under assets/ here is
# touched, and every step goes through the recipes above exactly the way a
# game project drives them: `just --justfile <this file> --working-directory
# <project> <recipe>` — so the documented integration form is what this
# gate tests, and a recipe that quietly assumes the toolkit checkout fails
# here first. (The ledger bans a *bare* recursive `just`, the kind that
# hunts for a justfile in the project; these calls name their justfile,
# because the recursion is the thing under test.) The reference PNGs go in
# through `ref-import`, which is what writes their ledger rows — a PNG
# without a row fails verify and should, and the row is the door's to write.
# The voice path designs a
# placeholder voice, clones a line from it by name and files the line, so
# verify's voice check runs on a record it has to read. It ends in the
# throwaway project's own gates: catalog, audit, check-bodies,
# manifest-check, verify.
#
# The five generate pipelines end to end on placeholders — no GPU, no backend, no Blender.
ci-fake: _build mcp-check
    #!/usr/bin/env bash
    set -euo pipefail
    forge="{{forge}}"
    work=$(mktemp -d -t forge-fake.XXXXXX)
    trap 'rm -rf "$work"' EXIT
    export FORGE_FAKE=1 FORGE_HOME="{{justfile_directory()}}"
    jf() { just --justfile "{{justfile()}}" --working-directory "$work" "$@"; }
    "$forge" init --project "$work" --name fake >/dev/null
    cd "$work"
    mkdir -p out/drawn
    # Two pictures a person could have drawn, at the size the door demands:
    # a T-posed figure and a prop clear of the frame. They are not
    # placeholders — `ref import` needs no card, so on tier `fake` it keys
    # and measures for real wherever Pillow, numpy and OpenCV are importable,
    # and a 4x4 grey square would be refused for its long side (or, on a bare
    # runner, filed with every measurement null and a note saying so). This
    # is the caller drawing a reference, which is the only way one is ever
    # made — the toolkit ships no image model.
    FORGE_TOOLKIT="{{justfile_directory()}}" python3 - <<'PY'
    import os, sys
    sys.path.insert(0, os.path.join(os.environ["FORGE_TOOLKIT"], "python"))
    from forge_gen.png import write_png

    SIZE = 1024

    def canvas():
        return bytearray(SIZE * SIZE)

    def box(flags, x0, y0, x1, y1):
        for y in range(y0, y1):
            flags[y * SIZE + x0:y * SIZE + x1] = b"\x01" * (x1 - x0)

    def write(path, flags):
        pixels = bytearray()
        for value in flags:
            pixels += b"\x80\x80\x80\xff" if value else b"\x00\x00\x00\x00"
        write_png(path, SIZE, SIZE, bytes(pixels))

    top, bottom, centre, heads = 100, 900, SIZE // 2, 6.0
    height = bottom - top
    half = height // 2
    arm = top + int(round(height / heads))
    figure = canvas()
    box(figure, centre - 60, top, centre + 60, arm)                 # head and neck
    box(figure, centre - half, arm, centre + half, arm + 70)        # the arms, straight out
    box(figure, centre - 90, arm, centre + 90, top + int(height * 0.62))
    box(figure, centre - 80, top + int(height * 0.62), centre - 10, bottom)
    box(figure, centre + 10, top + int(height * 0.62), centre + 80, bottom)
    write("out/drawn/figure.png", figure)

    prop = canvas()
    box(prop, 200, 300, 800, 700)                                   # clear of every edge
    write("out/drawn/box.png", prop)
    PY
    echo "== ref-import: the door that writes the ledger row"
    jf ref-import out/drawn/box.png box prop "ci-fake placeholder"
    jf ref-import out/drawn/figure.png figure character "ci-fake placeholder"
    echo "== mesh -> prop -> promote model"
    jf prop box --seed 1 --verts 2000
    jf prop-import box --height 1.0
    echo "== mesh -> prepare -> skin -> export -> rig check -> promote body"
    jf character figure --seed 1 --verts 25000
    jf prepare figure
    jf skin figure
    jf promote-body figure
    echo "== motion sweep -> review -> promote clip"
    jf sweep "a person walks forward" --duration 2 --samples 1 --seeds 0
    take=$(ls out/sweeps/0-*/*.npz | head -n1)
    jf promote-clip walk_fake "$take" --record "${take%.npz}.take.json"
    echo "== sfx -> promote audio"
    jf sfx door "a door" --seconds 1
    jf promote-audio sfx door out/audio/sfx/door.wav
    echo "== voice -> speech --voice <name> -> promote audio voice"
    jf voice warden "deep, slow, grave" --seed 1
    jf speech warden_greeting "Few come this deep." --voice warden
    jf promote-audio voice warden_greeting out/audio/voice/warden_greeting.wav
    echo "== the queue's own doors, on the rows those runs wrote"
    job=$("$forge" jobs --json --limit 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
    jf job-log "$job" > /dev/null
    jf jobs > /dev/null
    echo "== the gates, on the throwaway project"
    jf catalog
    jf audit
    jf check-bodies
    jf manifest-check
    jf verify

# forge_studio, forge_mcp and forge stay `publish = false`; the seven named
# below are the registry surface. One `cargo package` call with every crate
# on it is the honest form, not a shortcut: cargo packages them in
# dependency order and verifies each dependent against the siblings it just
# packaged, through a temporary registry under target/package/tmp-registry,
# so forge_library builds against the forge_rig .crate and not the path. A
# dependent packaged on its own fails at verify with "no matching package
# named forge_raster found", because its path dependency becomes a registry
# dependency in the .crate and nothing is on the registry yet — which is
# also why `cargo publish` has to go one crate at a time in the order
# printed, waiting for the index between them.
#
# What is not in a .crate, and why that is fine: the root LICENSE-* files
# (cargo packages nothing above the crate directory; `license` in
# [workspace.package] is the claim, and crates.io shows it), and rigs/ (the
# tests that read it run here, not from a downloaded crate). forge_motion
# carries its oracle fixtures (tests/fixtures/, ~1.3 MiB) on purpose: the
# bake tests are that crate's proof, and it stays far under the 10 MiB
# registry cap the loop below holds every .crate to. `--allow-dirty` so
# the gate runs on a working tree; a release runs it on a clean one.
#
# Pre-release gate: every crate meant for the registry packages and builds in isolation.
publish-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cd "{{justfile_directory()}}"
    order=(forge_raster forge_manifest forge_rig forge_motion forge_audio forge_capture forge_library)
    args=()
    for crate in "${order[@]}"; do args+=(-p "$crate"); done
    cargo package "${args[@]}" --allow-dirty
    version=$(cargo metadata --no-deps --format-version 1 \
        | python3 -c 'import json, sys; print({p["name"]: p["version"] for p in json.load(sys.stdin)["packages"]}["forge_raster"])')
    cap=$((10 * 1024 * 1024))
    echo
    echo "publish order — cargo publish -p <crate>, one at a time, in this order:"
    n=0
    for crate in "${order[@]}"; do
        n=$((n + 1))
        file="target/package/$crate-$version.crate"
        [ -f "$file" ] || { echo "  $n. $crate — no $file" >&2; exit 1; }
        bytes=$(stat -c %s "$file")
        printf '  %d. %-15s %8d KiB  %s\n' "$n" "$crate" "$((bytes / 1024))" "$file"
        if [ "$bytes" -gt "$cap" ]; then
            echo "$crate: $((bytes / 1024)) KiB is over the 10 MiB crates.io cap" >&2
            exit 1
        fi
    done
    echo "publish-check: $n crates package and build in isolation at $version"
