# Changelog

## 0.1.0 — unreleased

The first cut: a distillation of the asset pipeline one game grew over
three weeks into a toolkit with one job — local generation and judging of
game assets, for any game, driven by Claude Code through committed skills or
by a terminal through one binary. Written clean from the old repository; no
history carried, no legacy reader shipped.

What it does:

- **Three asset classes.** Rigged bodies and static models from a reference
  PNG (TRELLIS.2 lift, headless-Blender auto-rig or prop normalize);
  animation clips from a prompt (ARDY takes, native Rust bake, no Blender in
  the loop); sound effects, music and speech (MOSS-SoundEffect, ACE-Step,
  MOSS-TTS). Every generator runs on the user's GPU under `backends/<name>/`
  with an idempotent installer, an adopt path for existing installs, and a
  `doctor` that says what this machine can run.
- **The judging half.** `forge views` (seven angles, culling off for a raw
  lift), `forge sheet` (a clip posed on the real body), `forge bones`
  (wired or not, no GPU), `forge rig check` (every contract bone at its
  depth, the reference clip binding 27/27), `forge audio inspect` (waveform,
  spectrogram, numbers that fail loudly), and a viewer window. Headless
  renders need a wgpu adapter and no display.
- **Records that say only what is true.** Generator records
  (`forge_record: 1`) beside every output; library sidecars (`schema: 1`)
  beside every shipped file; `null` means unknown; provenance only moves
  down; clips claim reproduction (`forge audit`, bytes then poses to 1 mm),
  bodies, models and audio claim integrity. The reference image is an input:
  sha256 plus a row in `assets-src/SOURCES.md`, never regeneration.
- **The rig profile as data.** `rigs/humanoid/`: a 55-bone contract generated
  from `rig.glb`, sockets, the driven layout, every gate's scalar in
  `profile.toml`. A strict superset of ARDY's skeleton, so no retargeting
  exists anywhere.
- **One manifest.** `assets/library.json` (schema 1), projected from the
  sidecars, byte-deterministic, `sha256` on every entry, refuse-newer, read
  by `forge_manifest` from any engine.
- **For agents.** Six skills (`forge-setup`, `-prop`, `-character`, `-clip`,
  `-audio`, `-review`), an MCP server with eleven tools (lists, renders,
  plots, doctor, two generators, two direct promote doors; no promote for a
  mesh), `CLAUDE.md` with the one-way rule, and a `FORGE_FAKE=1` path that
  exercises every pipeline in CI with no GPU, no backend and no Blender.
- **Crates.** `forge_manifest`, `forge_rig`, `forge_motion`, `forge_library`,
  `forge_audio`, `forge_capture`, `forge_raster` are meant for crates.io;
  `forge_studio`, `forge_mcp` and the `forge` binary are `publish = false`.
  Bevy pinned to `=0.19.0`.

What was deliberately left behind, and where it is recorded
(`designs/decisions.md`): the review queue (promote is direct and refuses
an existing name unless told to overwrite), the studio's generate panel,
the cloud image model and its style board, levels, textures/KTX2, the
parametric body generator, retargeting.

Known limits, stated rather than fought: the TRELLIS.2 texture baker
(nvdiffrast 0.4.0) is **non-commercial** — consent-gated at install, warned
by `doctor`, named in every lift record; a replacement is a follow-up.
Everything is tested on one Linux machine with one 24 GB NVIDIA card.
DINOv3 is gated and a fresh clone cannot lift until `hf auth login`. The
ACE-Step server stays resident until `forge gen music --stop-server`. The
motion review metrics stay in Python behind a frozen `--json` contract.
