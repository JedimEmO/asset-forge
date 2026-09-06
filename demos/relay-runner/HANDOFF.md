# Relay Run gameplay handoff — 2026-09-06

The user accepted v14: “its wonderful.” This is the visual and game-feel
checkpoint before the next gameplay additions. No specific addition has been
selected yet. Start from this source; preserve the accepted assets and evidence.

## Accepted build and evidence

- Playable package: `/home/mmy/forge-demos/relay-runner-v14/PLAY.sh`.
- Visual captures, reports, checks and desktop log:
  `out/relay-runner-20260906/v14/` in the toolkit checkout.
- Checkpoint verification: `out/relay-runner-20260906/checkpoint/`.
- v14 delivery: 370 payload hashes verified; showcase assets unchanged from v13.
- RTX 4090, 1440×900, 1,200 frames (180 warm-up): combat mean 5.00 ms,
  p95 6.37 ms; passive twenty-drone fixture mean 4.56 ms, p95 5.75 ms.
  These are whole-frame intervals, not GPU timestamps. No dropped emitters.

Keep v14 and earlier version directories immutable. Package future changes into
a new version directory. Evidence under `out/` and external asset projects is
local and intentionally not committed; this Git checkpoint alone is not a backup
of those files.

## What to preserve

Free lateral movement and mouse aiming, rifle/reload feedback, generated reload
animation and sound, energy-funded plasma AOE, multikill and Free Fire calls,
live dark voice EQ/reverb, GPU particles, warm grazing light and cool rim,
reactive combat lights, restrained depth of field and motion blur are accepted.
F6 compares post-processing, F7 voice DSP and F9 lighting. Launch through PLAY.sh
for the default desktop audio interface. The reload is upper-body composition;
fingers and a detached magazine are not animated.

## Code map

- `src/sim.rs`: simulation, encounters, damage, resources and combat events.
- `src/main.rs`: application wiring, controls, fixtures and runtime reports.
- `src/art.rs`: world, characters and animation composition.
- `src/vfx.rs`: Hanabi particles, lifetime management and scrolling.
- `src/lighting.rs`, `src/post.rs`: lights and cinematic lens/grade.
- `src/audio.rs`, `src/voice_fx.rs`: playback, announcer queue and streaming DSP.
- `src/ui.rs`: menus and HUD.

Choose the next gameplay slice with the user. Useful candidates are a distinct
enemy attack pattern, an encounter objective or a run upgrade choice. These are
backlog suggestions, not agreed scope. Implement one coherent slice and compare
it against v14 before expanding further.

## Asset inputs and provenance

- Frozen stage-1 consumer: `/home/mmy/forge-stage1-20260906/consumer-final`.
- Showcase Forge project: `/home/mmy/forge-demos/relay-assets-v01`.
- Pixal3D trial: `/home/mmy/forge-demos/pixal3d-trial-20260906`.

The showcase supplies reviewed drone/cargo/station meshes, reload and audio.
Keep its records, source references, rejected auditions and selected exports.
Pixal3D provenance remains reconstructed where recorded as such; retain its
warnings and the texture baker's non-commercial notice. The toolkit's
`assets-src/voices/relay_announcer` is the first, rejected voice audition, kept
with its original record. The selected deeper voice is `relay_announcer_titan`
in the external showcase project. Do not mistake the former for the shipped voice.

Follow the upstream source/record/promote workflow for any asset correction;
never patch a shipped GLB or sidecar. Do not run generators while the game or
another GPU backend holds the card. Stage 1 is complete; the remaining release
stages in `designs/release-handoff.md` are a separate workstream.

## Build, package and verify

From the toolkit root:

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo test --locked --offline --manifest-path demos/relay-runner/Cargo.toml
cargo clippy --locked --offline --manifest-path demos/relay-runner/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path demos/relay-runner/Cargo.toml -- --check
cargo build --locked --offline --manifest-path demos/relay-runner/Cargo.toml
python3 demos/relay-runner/package.py \
  --delivery /home/mmy/forge-stage1-20260906/consumer-final \
  --binary target/debug/relay-runner \
  --showcase /home/mmy/forge-demos/relay-assets-v01 \
  --pixal-trial /home/mmy/forge-demos/pixal3d-trial-20260906 \
  --output /home/mmy/forge-demos/relay-runner-v15
```

Use v15 only if it does not exist; the packager refuses an existing destination.
Run `just ci` for toolkit changes. Asset changes also require the showcase's
verify, audit and manifest-check gates through the toolkit justfile.

Capture bounded combat, focus, blast and menu fixtures and inspect the images.
For the sustained twenty-drone benchmark use `--scenario crowded --benchmark
--quiet --frames 1200 --report /absolute/crowded.json` **without `--autoplay`**.
The fixture revives enemies: combining it with autoplay can create an artificial
plasma kill/recharge loop. Its overload evidence is retained under v13; it is not
a representative frame-time benchmark. Use `--autoplay` for normal combat.
Verify the new package's payload hashes and play it with sound before acceptance.
