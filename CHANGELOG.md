# Changelog

## 0.1.0 — 2026-08-23

The first cut: the asset pipeline one game grew over three weeks, distilled
into a toolkit with one job — local generation and judging of game assets,
for any game, driven by Claude Code through committed skills or by a
terminal through one binary. Written clean; no history carried, no legacy
reader shipped.

### Ships

**Crates** (ten; not published to crates.io — depend on them by path or
git, see `designs/decisions.md`; Bevy pinned to `=0.19.0`; toolchain
1.96.1; `just publish-check` packages each of the seven library crates in
isolation as a hygiene gate):

- `forge_raster` — a CPU canvas with a 5×7 bitmap font, for labelled review
  images. No dependencies beyond `image`.
- `forge_manifest` — the consumer contract: `assets/library.json` at schema
  1, typed and versioned, refuse-newer, `sha256` on every entry. The one
  crate a game's build pulls in; `serde` and `serde_json` and nothing else.
- `forge_rig` — a rig profile as data: the 55-bone contract, sockets and
  driven layout read from a directory, derived from a `.glb`, held to drift;
  `measure` reads a body without an engine; the fixture mannequin every test
  stands on.
- `forge_motion` — ARDY takes as data: read a `.npz`, apply the edit recipe
  (trim, retime, in-place and detrend, posture offsets, loop blend), derive
  footsteps, bake a `.glb` clip on a rig. Native Rust; no Blender in the
  loop. Ships its Blender-era oracle fixtures so the bake is held to them.
- `forge_audio` — decode (WAV, OGG, MP3, FLAC), measure, plot. No output
  backend, so it runs where there is no sound card.
- `forge_capture` — windowless Bevy frame capture and contact-sheet
  composition; the one library crate that links Bevy.
- `forge_library` — sidecar schema 1, the scan-derived catalog, the project
  file (`forge.toml`), the four direct promote doors (body, model, clip,
  audio — each refuses an existing name unless told `--overwrite`), the
  manifest projection, `verify`, `audit`, `rebake`, `migrate`.
- `forge_studio` (unpublished) — headless clip sheets and seven-angle
  views with culling off, turntables, the viewer window, `rig check`,
  `bones`, and `audit`'s posed half.
- `forge_mcp` (unpublished) — eleven MCP tools as a library: lists,
  renders, audio plots, `doctor`, `generate_clips`, `generate_audio`,
  `promote_clip`, `promote_audio`. No promote for a mesh.
- `forge` (unpublished) — the binary: `init`, `catalog`, `promote`,
  `manifest`, `verify`, `audit`, `rebake`, `migrate`, `doctor`, `gpu`,
  `gen`, `views`, `sheet`, `turntable`, `bones`, `rig`, `audio`, `studio`,
  `mcp`.

**Backends** (`backends/<name>/`: pinned commit, idempotent `install.sh`,
`--adopt-env`/`--adopt-checkout` for an install that already exists, an
in-env `probe.py`; envs and weights live under `~/.cache/asset-forge/`,
never in the tree):

- `trellis2` — reference PNG → textured mesh at 1024³ (character and prop
  presets; the seed is a knob).
- `ardy` — prompt → motion takes (sweeps over seeds and samples), and the
  Python review metrics behind a frozen `--json` contract.
- `acestep` — music, through a resident server (`--stop-server`).
- `moss_sfx`, `moss_tts` — sound effects and speech; `moss_tts` also hosts
  MOSS-VoiceGenerator, so `forge gen voice <name> --describe "…"` designs a
  character's voice from a description into `assets-src/voices/<name>/`
  (`ref.wav` + a `voice` record with the description, the line and the
  seed) and `forge gen speech --voice <name>` clones every line from it —
  a project never has to bring a reference clip it does not own.
- `blender` — headless auto-rig to the profile, prop normalize, the
  contract-checked export, and `rig-build`.
- `forge doctor` (ok | partial | missing | broken per backend, Blender,
  ffmpeg, the GPU, the rig profile) and `forge gpu` (who holds the card;
  exits 1 when the largest backend would not fit).
- `FORGE_FAKE=1` — every `forge gen` writes branded placeholders that pass
  the same validators as real output (a fake never overwrites a real file);
  `just ci-fake` runs the five pipelines — prop, character, clip, sfx and
  voice→speech — end to end on them with no GPU, no backend and no Blender.

**The rig profile as data** — `rigs/humanoid/`: `contract.json` (55 bones,
generated from `rig.glb`), `sockets.json`, `motion_skeleton.json`,
`profile.toml` with every gate's scalar, the fixture clip. A strict superset
of ARDY's skeleton, so no retargeting exists anywhere.

**Records that say only what is true** — generator records (`forge_record:
1`) beside every output, sidecars (`schema: 1`) beside every shipped file;
`null` means unknown; provenance (`recorded | reconstructed | unknown`) only
moves down; clips claim reproduction (`forge audit`: bytes, then poses on
the fixture mannequin to 1 mm), bodies, models and audio claim integrity.
A reference PNG is an input: sha256 plus a row in `assets-src/SOURCES.md`.
Every lift record names its texture baker. A designed voice is a source
with its record beside it, and `forge verify` holds every
`assets-src/voices/<name>/ref.*` to that record (or a ledger row for a
brought clip); a shipped line carries the clip as its `source` and the
voice record in its generator block.

**Skills** — seven under `.claude/skills/`, every command checked against
the justfile and every log line captured from a real run: `forge-setup`,
`forge-prop`, `forge-character`, `forge-clip`, `forge-audio`,
`forge-voice`, `forge-review`. Plus `CLAUDE.md` with the one-way rule and
`.mcp.json` for the server.

**The sample library** — one body (`vex_runner`: `.glb`, `.blend`, the
reference PNG and its lift record), two models (`sword` at the grip,
`barrel` on the floor), six clips (`walk`, `roll`, `pistol_shoot`, `idle`,
`jump`, `death`) baked from the takes committed under `assets-src/takes/`
with honest `reconstructed` provenance, two sound effects and one music
track rendered here with seeds (`recorded`), one designed voice
(`assets-src/voices/crypt_warden/` — MOSS-VoiceGenerator from a description
at seed 7, with its record) and one line cloned from it
(`audio/voice/warden_greeting.wav`, `recorded`). `assets/library.json`
projected from it and checked in CI.

**CI** — `just ci`, one gate matching what GitHub Actions runs: fmt,
clippy `-D warnings` + rustdoc `-D warnings`, Rust tests, pytest, the
offscreen smoke, audit, check-bodies, manifest-check, verify, `mcp-check`
and `ci-fake`. The workflow adds a headless job on lavapipe (smoke,
sheets, check-bodies, views on the committed fixture, audit,
manifest-check, verify) and publish-check.

### Deliberately not in 0.1.0

Each is an open follow-up, not a gap nobody noticed:

- **A replacement for nvdiffrast.** TRELLIS.2's texture baker (nvdiffrast
  0.4.0) is **non-commercial**: consent-gated at install, warned by
  `doctor`, named in every lift record. A pure-torch or Blender UV bake is
  the follow-up; until then, decide whether the licence fits your project
  before lifting anything you mean to sell.
- **OmniVoice.** Speech is MOSS-TTS only.
- **Motion review metrics in Rust.** Foot contact, drift and frozen joints
  stay in Python behind the frozen `--json` contract `forge gen motion
  review` speaks.
- **Retargeting.** By construction: the profile is a superset of the
  generator's skeleton, and a second rig profile is a follow-up.
- **KTX2 and texture tooling.** Textures ship as the PNGs inside the glb.
- **A runtime crate.** Nothing here loads assets into an engine; a game
  reads `library.json` through `forge_manifest` and loads the files itself.

### Left behind, and where it is recorded

`designs/decisions.md` has each: the review queue (promote is direct and
refuses an existing name unless told to overwrite), the studio's generate
panel, the cloud image model and its style board, levels, the parametric
body generator.

### Known limits

Everything is tested on one Linux machine with one 24 GB NVIDIA card; the
headless CI job is proven locally on llvmpipe and not yet seen green on a
GitHub runner. DINOv3 is gated, so a fresh clone cannot lift until
`hf auth login`. The ACE-Step server stays resident (~8 GB) until
`forge gen music --stop-server`. The library crates' tests read
`rigs/humanoid/` from the repository and are not meant to run from a
packaged `.crate` (which is fine: the crates are not published — use them
by path or git).
