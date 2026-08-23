# asset-forge — local generation and previews of game assets, for any game.
#
# Every generator runs on your GPU; every output is an engine-agnostic file
# (glTF, WAV/OGG, PNG, JSON) plus one manifest. Paths come from forge.toml at
# the root, so nothing here needs plumbing. `just` on its own lists everything.
#
# From another project — one made by `forge init` — the same recipes run as
# `just --justfile <this file> --working-directory . sheet walk`: the binary
# is found beside this file, the library is the one under the working
# directory, because `forge` walks up from there to its forge.toml.
#
# A recipe that answers "not yet: lands in P<n>" is a promise, not a bug: the
# phases are in the plan, and the name is reserved here so the skills can be
# written against it before the code exists. One is left: publish-check.

forge := justfile_directory() / "target/debug/forge"

default:
    @just --list

# The binary is wiring and printing; every recipe below that starts with
# {{forge}} builds it first. It links Bevy through forge_studio — minutes
# the first time, seconds after — which is the price of one binary that
# renders, checks and ships; the logic stays in the library crates so
# `cargo test -p forge_library` never pays it. forge_capture rides along so
# `smoke` finds its example already compiled.
_build:
    cargo build -q --manifest-path {{justfile_directory()}}/Cargo.toml -p forge -p forge_capture

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
# `forge gen rig-build` (Blender) rebuilds the artifact; this is the data
# half: the contract re-derived from the committed rig.glb — a diff here
# means the rig changed — and the fixture mannequin every test stands on,
# at out/fixture/mannequin.glb, where `sheet` and `studio` also reach for
# it when the library has no body.
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
# Run it on the raw lift BEFORE rig-mesh. `--no-head` for a prop; a library
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
        if ! {{forge}} sheet "$name" {{flags}} >/dev/null 2>&1; then
            failed+=("$name")
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
# Export, validate and file one rigged body: `just promote-mesh vex_runner`
promote-mesh name *flags: _build
    mkdir -p out/export
    {{forge}} gen export assets-src/blender/{{name}}.blend --out out/export/{{name}}.glb \
        --record out/export/{{name}}.export.json
    {{forge}} rig check out/export/{{name}}.glb
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
# name the skills are written against — no more, no fewer, and never a
# promote for a mesh. No GPU: nothing is rendered, the list is the test.
# The server's own banner goes to stderr, which is the rule this also
# proves: anything on stdout that is not a frame would break the parse.
#
# Handshake `forge mcp` over stdio and check the tool surface.
mcp-check: _build
    #!/usr/bin/env bash
    set -euo pipefail
    expected="doctor generate_audio generate_clips inspect_audio list_audio list_clips list_models promote_audio promote_clip render_clip_strip render_model"
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
# What it covers: formatting, clippy over the workspace, the test suite,
# offscreen rendering with no display server, every clip rebuilding from
# its own record by bytes and by pose, the rig profile held against every
# shipped body, the committed manifest against a rebuild, and the
# engine-free verify — sidecars, hashes, profile drift, the reference ledger.
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
# It is not GPU-free: smoke needs an adapter (llvmpipe is enough) but no
# display and no Blender. Deliberate — it is the check that catches what a
# component assertion cannot. check-bodies and audit's posed half need no
# adapter at all: a headless app with an animation player and no renderer.
#
# The pre-commit gate: fmt-check, clippy, tests, smoke, audit, check-bodies, manifest-check, verify.
ci: fmt-check check test smoke audit check-bodies manifest-check verify

# The generate paths with no GPU, no backend and no Blender: FORGE_FAKE=1
# makes every `forge gen` write placeholders that pass the same validators
# the real outputs must — a glb that verify_glb accepts, an npz Take::read
# accepts, a WAV, a PNG — and a real record saying `fake: true`. Run in a
# throwaway project made by `forge init`, so nothing under assets/ here is
# touched, and ending in that project's own audit, manifest-check and
# verify. The reference PNG is written here too (a 4×4 flat grey), with its
# ledger row, because a PNG without a row fails verify and should.
#
# The four pipelines end to end on placeholders, then every gate — after
# the MCP server has handshaken and listed its tools (mcp-check).
ci-fake: _build mcp-check
    #!/usr/bin/env bash
    set -euo pipefail
    forge="{{forge}}"
    work=$(mktemp -d -t forge-fake.XXXXXX)
    trap 'rm -rf "$work"' EXIT
    export FORGE_FAKE=1 FORGE_HOME="{{justfile_directory()}}"
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
