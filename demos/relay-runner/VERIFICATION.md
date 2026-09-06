# Relay Run verification

## Current state — 2026-09-06

The native demo passes 40 tests, Clippy with warnings denied and formatting.
The wasm32 WebGPU release builds, and its committed asset/metadata receipts pass
verification. Browser checks also passed against the exact GitHub Actions
artifact: assets ready, rendering, mouse lock, rifle fire, plasma, pause and
resume, with zero final console errors. The audio context entered running state
after the user gesture; this is not a new listening approval.

Successful browser checks used bundled Chromium 145 and the NVIDIA Vulkan ICD.
The tested WebGPU limits disable SSAO. System Chrome 151 and the in-app browser
did not provide a usable adapter in these tests; support is not claimed for
every browser or GPU. Evidence is under `out/relay-browser-20260906/`.

The user made the repository public, and [Pages deployment 34051210780](https://github.com/JedimEmO/asset-forge/actions/runs/34051210780) succeeded.
The [public game](https://jedimemo.github.io/asset-forge/) passed loading, mouse
lock, rifle/plasma input, pause/resume and running audio-context checks with
zero console errors. Public-site screenshots and console evidence are preserved
as `out/relay-browser-20260906/pages-*`.

Toolkit verification passed `just ci` (583 Rust tests, two ignored, 297 Python
tests and all other gates) and `just publish-check` for seven crates.
These checks do not complete release stages 2–6. The current checkout can build
and play using [the committed runtime assets](README.md#build-from-a-checkout);
private generation and review directories are not build prerequisites.

From the repository root, repeat the checks without private asset projects:

```sh
export CARGO_TARGET_DIR="$PWD/target"
python3 demos/relay-runner/web/package.py --check
cargo test --locked --manifest-path demos/relay-runner/Cargo.toml
cargo clippy --locked --manifest-path demos/relay-runner/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path demos/relay-runner/Cargo.toml -- --check
cargo check --locked --target wasm32-unknown-unknown --manifest-path demos/relay-runner/Cargo.toml
```

These commands verify code and asset integrity. Follow [the browser build](web/README.md)
for a rendered play check; successful compilation alone is not visual verification.

## Historical evidence

The following entries retain the observations and test counts from each earlier
package. Local absolute paths identify archived evidence, not current setup
instructions. Native v16 is the accepted asset baseline; previous packages stay
immutable. The former Scrapline showcase is archived at
`archive/scrapline-20260906`.

### v03 prototype — 2026-09-06

Verified locally on 2026-09-06. The playable package is
`/home/mmy/forge-demos/relay-runner-v03`; run its `PLAY.sh` from any directory.
The main desktop launch uses the real adapter. Automated render and input runs
used Xvfb and llvmpipe with isolated test state.

The following passed:

- Twelve deterministic tests: cover occlusion, nearest-target hits, jump clearance,
  shield regeneration, pause, shockwave, reload, dodge immunity, incoming bolt
  occlusion, restart and a
  ten-minute simulation that bounds retained world entities.
- `cargo clippy --locked --offline --manifest-path demos/relay-runner/Cargo.toml -- -D warnings`
  and `cargo fmt --check --manifest-path demos/relay-runner/Cargo.toml`.
- A 900-frame autoplay run through the packaged executable, with shots and hits
  resolved through the actual combat simulation.
- Real X11 keyboard/mouse input: start, free strafe, jump, aim/fire, shockwave,
  dodge, pause and resume. Paused distance stayed exactly unchanged.
- Integrity verification of every payload listed in `game-delivery.json`.

The title, live encounter, pause and defeat captures were reviewed. The camera
keeps the player's back and aiming corridor visible. Ranged enemies are procedural
hovering sentries; the accepted rusher supplies the melee character. A runtime
animation mask combines the scavenger's lower-body run with upper-body aim/fire.
This composition and the weapon attachment do not modify a delivered GLB.

Evidence is indexed in `out/relay-runner-20260906/result.json`.
`test_input.py` reproduces the native input check with Python Xlib and Pillow;
it uses a private Xvfb display and an isolated score directory.

This is a playable prototype, not a release candidate. It has a single procedural
route theme, sampled flat-floor placement and no terrain or support-hand IK.
The previous Forge release evidence remains unchanged; release stages 2–6
are still open. No generators, asset promotions or commits were performed.

The final v03 adds incoming-bolt occlusion to the already reviewed v02 build.
Its twelve tests and strict Clippy check passed. It was launched on desktop
display `:1`, and the native game window was confirmed present. The first
launch attempt without an explicit display failed; that diagnostic is retained.

## Playtest fixes — v04

The v04 package is `/home/mmy/forge-demos/relay-runner-v04`. Dynamic roots now
receive their world transforms in the same deferred command flush that creates
them. A regression test runs the actual presentation system once and checks the
first-frame positions of a sentry, barrier, projectile, pickup and impact.

The weapon HUD keeps the magazine count visible and adds a central round count,
low-ammo prompt, explicit reload countdown and progress bar. Feedback adds muzzle
flashes, reticle kick, impact sparks, enemy hit flinch, elimination text and damage
edge lights, with louder hit/kill sounds. Barriers, sentries and walkway markings
received procedural detail. These are runtime visuals; no accepted asset was
changed and no new TRELLIS generation was performed.

All fourteen tests and strict Clippy passed. New render and package-integrity
evidence lives separately in `out/relay-runner-20260906/v04/`. The reload capture
uses a seeded empty magazine to inspect the lockout message; combat uses autoplay.
Earlier packages, input checks and release evidence are preserved.


## Pixal3D drone integration — v05

New playable package: `/home/mmy/forge-demos/relay-runner-v05/PLAY.sh`.
The reviewed 60k-target Pixal3D drone was normalized through Forge with
`--length 1.5 --held --yaw-deg 180 --budget 60000`. Its final 57,282 triangles,
57,274 exported vertices and bounds were measured by promotion. The four-view
normalized sheet confirms +Z forward and centered origin. The sidecar honestly
uses reconstructed provenance without a fabricated TRELLIS generator block;
experimental receipt, raw selected export, normalization record, adapter and
MIT notice are packaged under `provenance/pixal3d`. nvdiffrast's non-commercial
notice is retained. Manifest check passes; verify checks seven entries with the
expected missing-generator warning for the experimental model.

The drone is integrated into live enemy rendering. Cargo and station assets
remain optional with procedural scenery until reviewed generated replacements
exist. The magazine count and reload progress now live in the lower-right HUD.
The process-private ALSA bridge routes to the desktop's current default sink;
a live relay-runner stream was verified on the default USB audio output.

15 tests pass, including a twenty-drone fixture test and first-frame placement;
strict Clippy and build pass. Evidence lives in
`out/relay-runner-20260906/v05/drone-integration/`, with `checks.json` recording
package hash verification and the underlying reports. Previous evidence is intact.

The bounded crowded scenario holds twenty firing drones, replenishes their
health and the player's health, and measures real frame intervals after 180
warm-up frames. Fixed simulation time is not used as a performance clock.
Screenshot readback is excluded from timing. Benchmark mode requests uncapped
presentation. On desktop X11, Vulkan RTX 4090, i9-13900KF, 1440x900, the optimized
dev build measured mean 3.114 ms, median 3.057 ms, p95 3.840 ms and p99 4.279 ms
across 1,020 samples. These are whole-frame intervals, not GPU timestamps.
The same fixture in Xvfb averaged 19.409 ms, demonstrating that the temporary
display materially changes timing; do not use that result to reject the budget.
Normal VSync gameplay averaged 16.667 ms, retained Playing state after 900
simulation frames, and captured the corner HUD. This qualifies this machine
and synthetic load only; lower-end GPU performance and LODs remain untested.

Reproduce the stress test on the real desktop:
`PLAY.sh --autoplay --quiet --scenario crowded --benchmark --frames 1200 --report /absolute/crowded.json`.
The interactive v05 process was launched separately, with its PID and live
audio routing evidence saved alongside the test artifacts.


## Drone hover orientation — v06

The user reported an odd drone angle. The source model's nose-down pose remained
in the visual mount, while the shared rusher root rotation added pitch on hits.
The consumer now mounts the drone with a visually tuned -25 degree local X
rotation and uses a small Z roll for its hit reaction. Rusher orientation stays
separate. No mesh or Forge record was edited; this is consumer placement.

Strict Clippy and build pass. A bounded 240-frame crowded-scene render confirms
level hovering and forward-facing barrels. The new v06 package's complete hash
manifest passed verification; its model hash is unchanged from v05. Evidence is
in `out/relay-runner-20260906/v06/`. The known v05 process was replaced with v06
for interactive review. Earlier packages and captures remain intact.


## Cargo, station and lighting pass — v08

`/home/mmy/forge-demos/relay-runner-v08/PLAY.sh` is the current playable package.
V07 is the retained pre-fill-light comparison. New reviewed Pixal3D props:

- Cargo: pixal-cargo3, seed42, manual FOV0.2 radians (a trial assumption),
  60k export target, 2048 texture. Earlier seeds42/7 with estimated FOV had
  detached fragments and remain rejected. Recorded normalization: yaw180,
  pitch-20, height0.76m, floor placement, ceiling60k. Final 59,753 triangles.
- Station: pixal-station1, seed42, MoGe camera estimate, 60k target, 2048 texture.
  Recorded normalization: yaw180, pitch-20, height3.2m, floor placement,
  ceiling60k. Final 56,362 triangles; uniformly scaled to2.3m as a tall obstacle.

All raw candidates were judged from four culling-off views before normalization;
selected normalized props were rendered and judged again. No mesh was repaired
by hand. `forge gen prop --pitch-deg` now records a finite world+X rotation,
after yaw and before sizing/placement. Earlier records are untouched.
This option is currently a CLI capability, not a new MCP field.

Cargo replaces low barriers. Station replaces tall barriers, roadside units
and distant industrial scenery. Width and depth of gameplay collision boxes
come from manifest bounds and the same uniform visual scale. Thin orange/red
approach strips replace broad warning panels. A shadowless soft front light
makes panel detail and the player readable against the existing warm rim.
The bounded assets/crowded harness now ignores desktop mouse/keyboard activity;
scripted controls remain fixed and cannot inherit a held fire/focus key.

16 demo tests, strict Clippy/build and all294 Python tests pass. Library
manifest check passes; verify checks11 entries with three expected warnings:
Pixal3D models honestly have reconstructed sidecars without a supported generator
block. Audit verifies integrity and correctly skips model reproduction.
Each selected raw export, experiment receipt, review, adapter and normalization
record is packaged separately. `checks.json` records complete package hashes.

Evidence: `out/relay-runner-20260906/v08/{assets,crowded}.{png,json,log}`.
The assets fixture shows both obstacles at gameplay distance with zero shots.
Desktop Vulkan RTX4090, i9-13900KF,1440x900,20 firing drones plus new scenery:
mean5.644ms,p505.302ms,p958.846ms,p9912.046ms,max25.813ms over1020 measured
frames after180 warm-up frames. Whole-frame wall intervals, not GPU timestamps;
screenshot readback excluded. One synthetic run on this machine, not a general
hardware guarantee. All four generation attempts completed under unchanged
memory guards; their sampled memory figures and exit states are in checks.json.
The game was reopened for user review. Procedural floor, beams, lights and sky
remain deliberate scaffolding for later polish.

## v09 — supplies and final heading (2026-09-06)

Removed small cover from spawning and collision. Cargo now supplies a full
magazine, +30 shield and +25 ability charge, with cyan markers and collection
feedback. Tall stations remain cover. The recorded normalizer applies final
heading +27.5 degrees to cargo and -27 to station after yaw180/pitch-20,
before bounds and floor placement. Four-view and gameplay renders were inspected.
Triangle counts remain 59,753 and 56,362; the accepted drone mount is unchanged.

16 game tests, Clippy with warnings denied, build and 297 Python tests pass.
Library manifest-check, verify and audit pass with the three expected unknown
generator warnings for experimental assets. Package file hashes were checked.
Desktop asset fixture, collection fixture and 900-frame autoplay completed.
The collection fixture reports one collection, full magazine, no reload and
no damage. Shield regeneration continues after the +30 refill.

Evidence: `out/relay-runner-20260906/v09`; package:
`/home/mmy/forge-demos/relay-runner-v09`. Original stage-1 acceptance, rejected
Pixal trials, earlier normalization files and v08 delivery are retained.

## v10 — encounter depth and spectacle (2026-09-06)

Alternating firing-line, pursuit and crossfire formations replace the steady
single-enemy drip. Every third wave has a longer recovery interval and a
supply crate. Four kills chained within six seconds of each other activate
six seconds of faster ammo-free fire; activation refills the magazine and
cancels reload. Kills return 12 shockwave charge. Drone firing has a visible
charge cue; hit reactions delay rusher acceleration.

Added controlled camera kick, spreading debris, thinning shockwave rings,
louder kill/shockwave playback and dynamic music gain. The environment has
a rotating segmented orbital accelerator, emissive energy tracks, a banded
gas giant and layered planetary rings. Wider spacing between overhead arches
opens the skyline. These are procedural scene assets; generated models,
normalization records, source evidence and previous deliveries are unchanged.

18 tests, Clippy with warnings denied and build pass; all 285 package hashes
match. Desktop captures assets-final and burst-final were visually reviewed.
The burst fixture reports four kills and 5.65 seconds of Overdrive remaining.
A 2400-frame autoplay run reached wave 5, 16 kills and 322m, still alive.
Uncapped wall-frame mean 5.509ms, p95 9.405ms, p99 12.485ms (2220 samples,
1440x900, RTX4090). The artificial 20-drone respawn stress fixture averaged
11.812ms, p95 18.529ms, p99 24.088ms; charge refunds repeatedly retrigger
shockwave and produce 6010 kills, so this is not comparable to v08's earlier
crowded timing. No claim of subjective playtest acceptance is made.

Evidence: `out/relay-runner-20260906/v10`; immutable playable package:
`/home/mmy/forge-demos/relay-runner-v10`. Earlier review captures remain beside
the final captures. v09 and stage-1 acceptance evidence are preserved.

## v11 — reactive camera post effects (2026-09-06)

Added cool color grading, a gentle vignette, reactive bloom, radial chromatic
separation for damage/Overdrive and a short lens warp for shockwaves through
Bevy's camera effect stack. Centered radial effects preserve the central ray;
UI is composed afterwards. F6 toggles the pass interactively; --no-post
disables it for deterministic comparison, retaining v10 bloom. No film grain
or motion blur was added. Generated assets and all earlier evidence remain.

The initial linear contrast settings crushed shadows; those captures are
retained as rejected. The revised tonal curve preserves dark model detail.
Reviewed assets-revised.png and burst-revised.png against grade-off.png.
18 gameplay tests, Clippy (-D warnings), build and all 285 package hashes pass.
No shader validation failures appeared in the runtime logs.

Paired 2400-frame uncapped autoplay, 1440x900 on RTX4090: effects on mean
3.201ms, p95 3.957ms, p99 4.615ms; off mean 3.175ms, p95 3.915ms, p99 4.525ms
(2220 wall-frame samples each, excluding warmup). Kill count and wave match.
This single comparison is not a GPU timestamp benchmark or a guaranteed cost.
Evidence: out/relay-runner-20260906/v11. Package:
/home/mmy/forge-demos/relay-runner-v11. Opened for user playtesting.

## v12 — reload clip and Free Fire announcer (2026-09-06)

Generated eight text-only reload takes at seed21; rejected their arm gestures
as lacking a clear magazine action. Authored sparse hand/hip constraints
against the accepted rifle-aim source and generated eight ARDY takes at
seed22. Selected take0, retimed 2.95s to 1.3s, XZ stripped, no loop, all style
knobs identity. Its full-body STATIC advisory is retained. The intended use
is an upper-body overlay; no full-body planted-foot claim, finger animation
or detachable magazine. Reviewed the strip on the real scavenger (27 driven,
0 orphaned) and in-game belt reach, insertion and return with the rifle.
Lower-body locomotion continues; the overlay follows the gameplay timer and
fades out on completion/cancellation. Reload fixture ends with 24 rounds.

Designed an original MOSS announcer, then generated speech through the isolated
speech runtime. User rejected the first call as needing more depth/explosion.
Three heavier sources were rejected for cut tails; a lower-temperature
Titan source has a complete 28ms tail. Two cloned deliveries remain. The
standard 4.72s call is explicitly an audition/provisional playtest selection,
not a user-accepted performance or a claim of verified spoken content. The
user's listening comparison with the drawn-out version is still pending.
All original sources, records and rejected auditions are preserved.

The announcer gets priority, music/SFX ducking for 4.9s and one call per actual
Overdrive activation; extending active Overdrive does not repeat it. UI now
says FREE FIRE / UNLIMITED AMMO. A sound-enabled 330-frame burst fixture
records exactly one activation and one announcer start on the desktop default
output. Quiet autoplay records zero announcer starts. This checks routing
and triggering, not subjective intelligibility or delivery.

19 game tests, Clippy (-D warnings), build, manifest-check, verify (20 checked,
3 expected Pixal warnings) and audit pass. Reload rebuilds byte-for-byte.
All 351 package hashes match. 1200-frame combat smoke survives to wave3,
9 kills; mean wall frame3.127ms, p953.767ms on RTX4090/1440x900.
Evidence: out/relay-runner-20260906/v12. Package:
/home/mmy/forge-demos/relay-runner-v12. Earlier deliveries and stage-1
acceptance remain unchanged.


## 2026-09-06 — v13 plasma, live voice effects and GPU particles

The user accepted v12, requested reload SFX, darker Free Fire, an energy-fed
AOE secondary and Multikill speech, then specifically requested live voice EQ
plus reverb and replacement of the placeholder particle visuals.

v13 ships the original Free Fire WAV byte-identically. A custom streaming
source adds bass emphasis, darker treble and a damped stereo chamber; F7 is a
live dry/effected comparison. New reload/plasma effects and Multikill speech
were generated serially through Forge, inspected and promoted with records.
The first offline EQ experiment and opaque blast screenshots remain in out.

E/MMB launches plasma. The fixture proves four kills, four collected energy
shards, recharge to 100, and one start each for Multikill and Free Fire.
Reload playback starts once and completes at 24 rounds. Q remains separate.
Hanabi GPU particles replace opaque combat rings, spheres, tracers and debris,
including projectile trails, energy pickups and drone charging cues.
The game-window recording and contact sheet show expansion, fade and collection.

25 game tests and Clippy with warnings denied pass. Forge verify checks 26
items with the three retained experimental Pixal warnings; audit rebuilds the
reload clip byte for byte. All 368 packaged payload hashes match. Normal
combat averages 3.94 ms (p95 4.99); twenty firing drones average 3.93 ms
(p95 4.88), at 1440x900 on this RTX4090, uncapped wall-clock frame intervals
after 180 warm-up frames. No particle requests drop in either workload.
The older crowded+autoplay fixture creates an artificial revive/kill/recharge
loop; preserve it as overload evidence, not representative gameplay. Its
240-emitter ceiling holds and drops requests as designed.

Package: `/home/mmy/forge-demos/relay-runner-v13`.
Evidence: `out/relay-runner-20260906/v13/checks.json`, reports, logs,
screenshots and `plasma-particles.mp4`. No previous package was rewritten.


## 2026-09-06 — v14 dramatic lighting and lens pass

The user accepted v13 gameplay and requested stronger lighting, depth of field
and motion blur. v14 separates a warm grazing key from cool rim/fill light,
lowers ambient brightness, narrows shadow cascades and gives rail fixtures,
rifle flashes and detonations actual light sources. Atmospheric fog adds depth.
Bokeh focus smoothly follows targets near the aiming ray in RMB focus; blur
is capped at 2.5 px normally / 5 px focused. Motion-vector shutter is 0.26
running, 0.48 during dodge and 0.06 while focused, zero on menus/pause.
F6 compares post effects; F9 compares the earlier key/fill lighting setup.

Reviewed title, combat, blast, focus and effects-off screenshots, plus a game-
window video and motion sheet. The focus fixture settles at 26.48 m and 0.06
shutter; the comparison has motion samples zero and the earlier lighting.
25 tests and Clippy with warnings denied pass. All 370 payload hashes match;
accepted showcase assets remain byte-identical to v13. Combat averages 5.00 ms,
p95 6.37 / p99 7.55 (one 28.31 ms maximum); twenty firing drones average
4.56 ms, p95 5.75, at 1440x900 on this RTX4090. These are uncapped whole-frame
wall-clock intervals after 180 warm-up frames, not GPU timestamps. No particle
emitter requests dropped. Transparent particles keep their authored trails;
they do not write motion vectors for blur.

Package: `/home/mmy/forge-demos/relay-runner-v14`.
Evidence: `out/relay-runner-20260906/v14/checks.json`, logs, reports,
comparisons and `cinematic-pass.mp4`. Previous packages remain intact.

## Commit checkpoint — 2026-09-06

After user acceptance of v14, `just ci` completed successfully: 583 Rust tests
passed (2 ignored), 297 Python tests passed, and the lint/rustdoc, headless smoke,
audit, body, manifest, verify, MCP and fake-pipeline gates passed. The demo's
25 tests, format check and warnings-as-errors clippy passed; the external consumer
also passed warnings-as-errors clippy. All 370 v14 payload hashes were rechecked.
Logs are in `out/relay-runner-20260906/checkpoint/`. The existing body warnings
about an unmeasured planted foot and absent walk reference remain unchanged.

The staged diff passed whitespace checks and was reviewed for source/provenance
scope and accidental secrets or build outputs. The finish workflow's requested
`/simplify` invocation was unavailable in this environment; it was not run.
See `HANDOFF.md` for the next gameplay pass and the local evidence locations.

## v15 — faster encounters, new models and orbital traffic — 2026-09-06

Package: `/home/mmy/forge-demos/relay-runner-v15`. Evidence:
`out/relay-runner-20260906/v15/`. The user-accepted fallback remains v14.

The director now introduces Trooper, Rusher, Weaver, Sniper and Heavy in the
opening 15 seconds. Pressure increases every 12 seconds, with speed capped at 13 m/s
after one minute, wave gaps of 5.8 to 4 seconds and a 7.5-second recovery every fifth wave.
Flying rifle/plasma hit volumes use mounted model bounds and the same bank as
rendering. Sniper fire commits before its burst and accounts for world scrolling;
a violet line and ground marker expose the dodge window. Heavy 220 HP survives
one 160-damage plasma blast. Existing reload, resource and voice effects remain.

Six new references were imported through Forge and lifted with guarded Pixal3D:
interceptor 56,219 tris, freighter 58,693, cruiser 56,575, heavy 59,554,
radar 58,385 and reactor 58,538. Raw culling-on/off and normalized library sheets
were reviewed. Exact orientations, hashes, recipes, adapter and interrupted
heavy1/reactor1 attempts are preserved in the showcase's `out/v15-provenance`,
which is copied into package provenance. Reactor2 used a stricter 30 GiB hard/
28 GiB soft memory cap; no source GLBs or sidecars were hand repaired.

Sparse asymmetric bays replace repeated station decorations and distant towers.
The machinery stands on attached decks; early radar/reactor bays establish the
new silhouettes. Eight background ships represent three classes, including two
interceptor formations; the freighter crosses center around 9.6 seconds. Ships
receive separate cached material instances with fog disabled and textured fill;
shared combat materials remain unchanged. This fixed the far freighter's red fog
smear and revealed dark hull details. Front fill increased from 650 to 2,200 lux while
retaining the grazing key, rim, restrained ambient and accepted lens effects.

Final `cargo test`: 36 passed. `cargo clippy --all-targets -- -D warnings`, build,
format and diff whitespace checks pass. Showcase manifest and audit pass;
verify checks 44 entries with nine expected experimental generator warnings.
The package's 539 payload hashes and v14's 370 hashes verify; all 16 previous showcase
payload files match v14 (the manifest gains new rows). No baseline file changed.

Rendered threats, opening, minute, crowded and title fixtures exit 0. The minute
run reaches intensity 6 / wave 11, with 27 kills, with no dropped emitters. Final package
opening, crowded and isolated blast runs exit 0. The isolated blast produces one
multikill and starts both MULTIKILL and FREE FIRE once; PLAY.sh selects the desktop
default USB audio sink. Do not add autoplay to this blast fixture when testing its
announcements: the defensive shockwave can clear the cluster first.

Whole-frame measurements varied with desktop presentation: the package opening
averaged 3.60 ms / p95 4.46 ms; longer and focused runs showed a 60 Hz plateau around
16.67 ms / p95 17.1 ms despite requesting uncapped presentation. Keep those reports;
this is not an isolated GPU-cost comparison. The initial preview crowded run was
4.32 ms / p95 5.37 ms. No particle requests were dropped in any of these runs.

## v16 combat feedback — 2026-09-06

38 tests pass, including actual rifle body/critical damage values and bounded
number lifetime, pause, scrolling and restart behavior. Clippy with warnings
as errors, formatting and diff whitespace checks pass. No toolkit or generated
asset changes. Screenshots and logs are in `out/relay-runner-20260906/v16/`.
The `feedback` fixture fires two real shots and visibly shows white 30 and gold
75 CRIT; enemy textures remain readable with warm material fill. The initial
`threats` screenshot exposed an unsupported locator glyph, corrected to the
font's ASCII v and reviewed in `feedback.png`. The earlier image is retained.
The autoplay capture at four seconds has no active numbers because hits have
already expired; the bounded feedback fixture provides the visual evidence.

## Browser build and merge gate — 2026-09-06

The wasm32 WebGPU release compiles. Forty native demo tests and Clippy pass,
including an input regression for start/resume mouse-delta handling. The browser
runtime was exercised with Playwright's Chromium 145 on the NVIDIA Vulkan ICD:
assets ready, mouse lock, rifle fire, plasma, pause and resume passed. The audio
context entered running state after the start gesture; this is an activation
check, not a new listening approval. Final browser console contains zero errors.
Screenshots and test logs are under `out/relay-browser-20260906/`.

The in-app browser exposed no WebGPU adapter. System Chrome 151 also refused an
adapter under the tested headless flags, while software SwiftShader stalled
during startup. The successful test used bundled Chromium with the NVIDIA ICD;
these failures do not imply support for every browser or GPU. Browser WebGPU
limits disable SSAO on the tested configuration; native lighting remains intact.
Automated pointer injection can rotate the view, so the input transition also
has an ECS regression independent of browser automation.

Toolkit `just ci`: 583 Rust tests, two ignored, 297 Python tests, all gates pass.
`just publish-check`: all seven crates package and build in isolation. The first
GitHub packaging run exposed missing Wayland headers; CI prerequisites now
include libwayland-dev and libxkbcommon-dev. The unavailable /simplify command
could not be invoked; diff review, tests, lint and packaging were completed.

## Browser performance investigation — 2026-09-06

The user reports poor frame rate in hardware-accelerated Chrome on an RTX 4090
at 1080p. This is unresolved. Earlier play checks established functionality,
not acceptable frame pacing in the user's browser session.

A controlled Chromium 145 run using Vulkan on the same machine measured
56.4 game updates/s at 1920×1080 during a ten-second playing sample, with
24.7 ms p95 intervals. A separate sustained-fire sample measured 59.3 updates/s
and 21 ms p95. These are CPU-side game update intervals observed through the
existing per-frame DOM state bridge, not GPU timestamps. A fresh installed
Chrome 151 profile exposed no WebGPU adapter; it did not reproduce the user's
working but slow session. Neither result establishes performance in that session.

The F3 panel reports live game update rate, p95 intervals, render dimensions and
browser-reported adapter details to make the affected session diagnosable.
Its local browser check passed display, hide/show and absence of runtime errors.
Raw profiles and the reproduction scripts are preserved in
`out/relay-perf-20260906/`; accepted assets and rendering settings are unchanged.

The user's F3 capture subsequently showed `google / swiftshader`, 1 game FPS
and 2961.6 ms p95. Their Chrome GPU report exposed NVIDIA through OpenGL
Compatibility Mode, with Vulkan disabled and SwiftShader offering Core features.
The launcher now warns about known software adapters before any game download,
expands Linux Vulkan help, and retains an explicit slow-launch option. Hardware,
unknown and missing-adapter cases were also checked with injected adapter results;
all four launch states passed. The software warning was visually reviewed.
This diagnoses the reported software-rendering path; it does not claim that the
user's Chrome configuration has been changed or their hardware retest passed.
