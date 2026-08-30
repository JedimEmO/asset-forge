# Changelog

## Unreleased

**Forge 2, Phase 1 — the daemon.** One queue owns the card, both doors are
its clients, and the audio models move off their own environments onto a
ComfyUI host the toolkit drives but does not schedule. The plan is
`designs/forge2.md`, the daemon's contract `designs/serve.md`, the reasons
`designs/decisions.md`, the install traps `designs/hosting.md` — all dated.

### Added

- **`forge serve`** — the daemon: one FIFO at concurrency 1, a job table
  written atomically under `out/serve/`, and a card lease that is an
  exclusive `flock(2)` rather than a pidfile, so the kernel drops it when
  the holder dies. Two executors behind it: `env` (today's per-backend
  launcher, driven in-process) and `comfy` (HTTP to the host). A loopback
  HTTP API with a bearer token and a 0600 endpoint file, with the MCP
  router nested at `/mcp`.
- **Jobs at both doors.** `forge gen` submits and follows the log with the
  exit codes it always had (`^C` still cancels and gives the card back),
  and runs in-process — taking the same lock, writing the same row — when
  no daemon is up. Terminal doors: `forge jobs`, `forge job show|log|cancel`,
  `forge stop`; `just serve`, `just stop`, `just jobs`, `just job-log`.
- **The three questions.** `forge.toml` grows `[make]` (six kinds chosen by
  what you make, never by model name) and `[hardware]` (`tier` =
  `full | lean | fake`, detected and overridable; `comfy_url`). `forge init`
  asks them once on a TTY and states its assumptions where there is no
  terminal. The kind → backend map lives once, in `MakeKind::backends`.
- **One screen before a byte downloads.** `forge setup` prints the
  backends, their disk, the total and every licence in full, asks once,
  takes `--yes <id>` by name, refuses a bare `--yes`, and appends what was
  accepted to `$FORGE_BACKENDS_HOME/licences.json` beside the installs.
- **Doctor's fifth word.** `off` for a kind `[make]` did not choose —
  never probed, never a reason to exit 1 — plus `executor` and `chosen`
  columns, one shared `GET /object_info` for every comfy row, and exit 1
  only while a *chosen* backend is not `ok`.
- **MCP: the tool surface over two transports.** `init_project`, `licences`,
  `setup`, `doctor`, `status`, `list_runs`, `wait`, `cancel` join the
  existing surface; `generate_audio` returns a job. `forge mcp` in a
  directory with no `forge.toml` now serves a session — `init_project`,
  `licences` and a doctor that says "no project here" — instead of exiting
  before the handshake.
- **New gate `mcp-session`** — one scripted fake-tier session
  (`init_project → setup → doctor → generate_audio → wait → inspect_audio →
  promote_audio → verify`) run twice, over stdio and over the daemon's
  streamable HTTP. `just ci` runs it; so does GitHub.
- `forge_record: 2` — `backend` gains `executor`, and a comfy job records
  `comfyui_commit`, `workflow_sha256` and `packs`. `backend.toml` gains
  `executor = "env" | "comfy"`.
- `backends/comfy` — ComfyUI as a systemd `--user` unit with a committed
  snapshot, `extra_model_paths.yaml` over the weight caches, pinned packs
  and tracked API-format workflow templates.
- **`forge bundle`** — one self-contained `.glb` carrying a body's skin and
  any number of clips as named animations, for handing an asset outside the
  toolkit, with a `<stem>.bundle.json` record hashing every input. A merge,
  not a bake: channels are re-pointed at the body's bones by name and their
  values copied byte for byte, a clip driving a bone the body lacks is
  refused by name, and `--motion-scale` multiplies the root travel and
  nothing else. `just bundle`, and `export_bundle` as the MCP surface's
  nineteenth tool.

**Forge 2, Phases 2 and 3 — the fitted skeleton, the mesh doors, the
reference door.** The design is `designs/skin.md`; the decision it
implements is `decisions.md`'s of 2026-08-30: bone lengths belong to the
body, and the skinner's weights say what they are.

- **`forge ref import` / MCP `import_reference`** — the one way a PNG gets
  under `assets-src/refs/`. Format (one PNG, 1024 px or more on the long
  side), then `mesh.py`'s **own** keyer at its own tolerance, then four
  refusals the 2026-08-30 spike proved ride all the way to a lift — a drawn
  floor band, a contact shadow, a flood-through hole, a key that kept
  outside 10–85 % of the frame — then the geometry pre-checks (span over
  height 0.7–1.3, heads, one subject above the dust fraction, a prop whole
  inside its frame). All of it before a GPU minute. It writes three files
  and they are the door's to write, never a hand's: the PNG **as it was
  drawn, byte for byte**, a `<name>.ref.json` record, and the `SOURCES.md`
  row. `just ref-import`, and `just ref-format` prints the format text —
  which has one home, `FORMAT` in `python/forge_gen/reference.py`, with
  every other copy generated from it.
- **The gain is a knob in the graph.** `backends/acestep/workflows/music.api.json`
  gains node `14`, `AudioAdjustVolume`, wired between `VAEDecodeAudio` and
  `SaveAudio` and filled from `forge gen music --gain-db`; the value lands
  in `params.gain_db`. The clipping gate does not move — it is right, and a
  gain that merely dodged it would be the hand-repair of an audio file. The
  node's `volume` is an integer, so a fractional `--gain-db` is refused by
  name rather than rounded into a record claiming a gain nothing was
  rendered at.

### Changed

- **Audio runs on the host.** ACE-Step 1.5 native; MOSS-TTS,
  MOSS-VoiceGenerator and MOSS-SoundEffect through TTS-Audio-Suite at a
  pinned commit. Every audio verb now transcodes to WAV (the host writes
  FLAC) and holds its own output to `forge audio inspect`'s three checks —
  silence, full-scale runs, a truncated tail — *before* a record is
  written, refusing with exit 5 and the measurement in the message.
- Both doors resolve a generate's backend from one map, build their queue
  options from the project (so `tier = "fake"` reaches the queue instead of
  relying on an environment variable), and call the card back against the
  card's own idle floor rather than a number an earlier job's leftovers are
  inside of.
- Read verbs (`forge jobs`, `forge job show|log`, `forge serve --status`)
  open the store with no worker and reconcile nothing; only the daemon
  adopts rows a previous process left. `forge stop` cancels its child by
  recorded pid and waits for the terminal row.
- The comfy release ladder stops at step 1 when the host does not answer:
  no restart, no withheld lease, `vram_after_gb: null`. `forge gpu --free`
  clears a withholding only where it measured a return.
- An installer is handed `--yes` only for the licence ids this machine's
  receipt covers, and the comfy host is told which model group the chosen
  kinds need — a music project no longer pulls 73.7 GB of image weights.

- **`backends/moss_tts`'s speech notice is rewritten.** Its "WHAT LIFTS
  THIS: a pin built against transformers >= 5" was wrong: `fab00263` **is**
  TTS-Audio-Suite v5.8.7 (2026-08-28), already past the release that moved
  the pack to transformers 5, so a pack bump is a no-op. Neither is the
  pack's isolated secondary runtime the lever — at this pin it has a MOSS
  *profile* with no packages in it, no worker, no proxy, and an explicit
  `Isolated runtime is not implemented for engine 'moss_tts'`. All of that
  is readable in the checkout with no card, and it is now what the notice
  says, along with what would actually lift it.
- **`just` recipes:** `just ref-import` and `just ref-format` are new;
  `just rig-mesh` became `just prepare` + `just skin` (`just body` runs
  both) and `just promote-mesh` became `just promote-body`. `mcp-check`
  pins twenty-five tool names and `mcp-session` grows the character leg.
- The reference format text no longer asks for "seven heads or more" as a
  rule: the fitted skeleton made proportion a preference. The door refuses
  below three heads and notes anything under seven.

### Removed

- The `acestep`, `moss_sfx` and `moss_tts` virtualenvs, their probes, and
  ACE-Step's resident server with its pidfile, `--stop-server` and
  soundfile patch. `forge gpu --free` is the door for the card now.
- **`just rig-mesh`**, and the reach gate it ran: it measured a body's span
  against wrists the fit now moves to that body. `just promote-mesh`
  survives one release as a recipe that **dies by name**, the courtesy
  `backends/comfy/install.sh --models` got.

### Known limitations

- `forge gen speech` cannot make a line: MOSS-TTS 1.7B does not run under
  the host's transformers 5 at this pin and the pack answers with silence,
  which the gate refuses. A pack bump is **not** the fix and neither is the
  pack's isolated runtime; `backends/moss_tts`'s notice says what was
  measured and what would lift it.
- `forge gen music`'s `--gain-db` default of **−3** is a **budget**, not a
  measurement. Pinning it is three renders of the busiest arrangement at
  −2, −3 and −4 with `peak_dbfs` read off each, keeping the one that lands
  at −2.0 ± 0.5 dBFS; that needs the card and has not been done. It says
  "budget" in the door, in the template's saved value and in
  `designs/hosting.md`.
- The reference door's floor-band and flood-through thresholds are budgets
  for the opposite reason: the *good* side is measured across every
  reference in this repository, but no picture on disk exercises the bad
  side. `designs/decisions.md` names the three numbers that did move when
  the pictures were measured against them.
- The MCP `setup` plans, gates and records; it does not install. Its
  description says so.

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
