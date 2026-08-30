# asset-forge

A local toolkit for **agentic game asset creation**: meshes and rigged
characters, animation clips, and sound — generated on your own GPU from a
reference image or a prompt, judged by a human *and* an AI agent, and
shipped as engine-agnostic files (glTF, WAV/OGG, PNG, JSON) under one
manifest. Nothing is modelled, animated or recorded by hand; the repo is one
path from "describe it" to "it is in the library", with a gate at every step
that fails loudly.

The generating half is the easy half — plenty of models produce meshes,
motion and audio. The judging half is the point. An agent cannot orbit a
mesh, watch an animation or listen to a sound, so this turns all three into
things it *can* inspect: rendered contact sheets, plots, and numbers that
fail loudly. Claude Code drives it through committed skills and an MCP
server; a terminal drives it through one binary, `forge`, and a `justfile`.

Every generator runs locally. The one thing this repo does not make is the
reference image: bring a PNG, and the record claims the file's integrity
rather than pretending it could paint it again.

```
make                                judge                               ship
PNG → TRELLIS.2 → auto-rig          views, rig check, the walk sheet    promote body → sidecar + manifest
PNG → TRELLIS.2 → prop normalize    views, the studio                   promote model → sidecar + manifest
ARDY motion → native bake           review table, strip on the body     promote clip → sidecar + manifest
ACE-Step, MOSS → audio              waveform, spectrogram, loudness     promote audio → sidecar + manifest
```

## Quickstart

Before the first `just`, the build prerequisites:

- **Rust, via [rustup](https://rustup.rs)** — `rust-toolchain.toml` pins
  1.96.1 and rustup fetches it on the first build. That first build links
  Bevy and takes minutes; after that, seconds.
- **`just`** — `cargo install just`, or the distro package.
- **Bevy's system headers** (Debian/Ubuntu):
  `sudo apt install libasound2-dev libudev-dev pkg-config`.
- **python3 ≥ 3.11** on PATH, for the generator launcher (stdlib only).
- **Disk for the backends** — how much depends on what you make, and
  `forge setup` prints the bill for *your* answer before a byte downloads.
  Everything lands under `$FORGE_BACKENDS_HOME` (default
  `~/.cache/asset-forge/backends`) and the Hugging Face cache.
- **A GPU only for generating** — 24 GB is the `full` tier, 16 GB is
  `lean` (the same lifts at 1024³, a quantised reference model, a smaller
  speech model), and no card at all is `fake`, which is a **first-class
  answer**: every generator writes a branded placeholder through the same
  doors and validators, so the whole path, the viewer, the sheets and the
  shipped sample library work on CPU (llvmpipe).

Every recipe builds and runs `./target/debug/forge` itself; `just install`
puts a release `forge` on PATH (`~/.cargo/bin`) for shells outside the
checkout.

### The first hour

`forge init` asks three questions once, and everything after reads the
answers out of `forge.toml`. They are phrased as **what you make** and
**what card you have**, never as model names:

```sh
git clone https://github.com/JedimEmO/asset-forge && cd asset-forge
just ci-fake                  # before installing anything: the whole pipeline on placeholders

cd ~/my-game && forge init    # 1. what will you make here?  props, characters, clips,
                              #    sfx, music, voice — default props+characters+clips
                              # 2. what card is this?  detected from nvidia-smi and OFFERED:
                              #    >=22 GB full, >=14 lean, none fake
                              # 3. where is ComfyUI?  asked only if a chosen kind runs in it
```

The same three as flags, which is also what an agent's `init_project`
passes — `--make props,characters,clips` (or `all`, or `none`),
`--tier full|lean|fake`, `--comfy-url URL`, `--yes`. With no terminal and
no flags it takes the defaults and prints one line naming each assumption;
it never hangs on a prompt.

```sh
forge setup                   # ONE SCREEN BEFORE A BYTE DOWNLOADS: per chosen kind the
                              # backends, their disk, the total, and every licence fact in
                              # full — nvdiffrast's non-commercial clause, the DINOv3 gated
                              # login, Llama 3's attribution, the SkinTokens encoder
                              # question, ComfyUI's GPL. Then it asks once.
                              # `--yes nvdiffrast --yes llama3` accepts by NAME and is
                              # repeatable; a bare `--yes` is refused, because a blanket yes
                              # to a list nobody read is what the gate exists to prevent.
                              # Resumable: a backend doctor already calls `ok` is skipped.
just doctor                   # ok | partial | missing | broken per chosen backend, and `off`
                              # for a kind you did not choose — never probed, never a reason
                              # to exit 1. Exit 1 only while a CHOSEN backend is not ok, so a
                              # props-only project is not red about music
```

What you accepted is recorded in `$FORGE_BACKENDS_HOME/licences.json`,
beside the installs — because the install is what is licensed, and
`forge.toml` is hand-edited, which would let an acceptance be *typed*
rather than *given*.

Tiers change registers and variants, never features: `lean` runs MOSS-TTS at
1.7B rather than 8B, and
**lifts at 1024³ exactly like `full`** — a 1024³ lift measures 4.7 GB, and
512³ costs the face rather than saving memory.

Already have TRELLIS.2, ARDY, ACE-Step or MOSS installed? Adopt them instead
of rebuilding: `bash backends/<name>/install.sh --adopt-env DIR
--adopt-checkout DIR` ([backends/README.md](backends/README.md)).

A prop, end to end. Drop a PNG, account for it, lift it, look, normalize, look
again:

```sh
cp ~/crate.png assets-src/refs/props/crate.png
# then one row in assets-src/SOURCES.md: | refs/props/crate.png | where it came from | crate | 2026-08-23 |
just prop crate                                 # TRELLIS.2 → out/lifts/crate.glb + refs/props/crate.lift.json
just views out/lifts/crate.glb --no-head        # read out/views/crate.png: four sides, culling off
just prop-import crate --height 0.9             # metres, floor at the origin, matte → assets/models/crate.glb
just studio --model models/crate.glb            # orbit it yourself
```

A character is the same shape with a rig in the middle; a clip is a sweep, a
look and a bake. Each is a skill that checks its prerequisites and quotes the
log lines to read:

```sh
just ref-import ~/drawn/hero.png hero character "xAI Grok, image_edit"
just character hero && just views out/lifts/hero.glb          # then: just body hero; just promote-body hero
just sweep "a person walks forward" --duration 2 --samples 4  # then: just promote-clip walk <take.npz> --loop
just sfx door_slam "heavy oak door slams shut"                # then: just audio …; just promote-audio sfx …
just voice warden "Deep, slow, weathered male voice, grave and calm"   # then: just speech greet "…" --voice warden
```

`.claude/skills/forge-character`, `forge-clip`, `forge-audio` and
`forge-voice` are the full paths; `just` on its own lists every recipe.
No GPU yet? `FORGE_FAKE=1` makes every `forge gen` write branded
placeholders through the same doors and validators — `just ci-fake` is
exactly that, end to end in a throwaway project.

## What is in the box

Three asset classes, one door each into `assets/`, and a refusal at every
door that names what would have passed.

| Class | Make | Judge | Ship | Refuses |
|---|---|---|---|---|
| References | `just ref-import` — the one way a PNG gets under `assets-src/refs/`; the door writes the PNG (original bytes), its `.ref.json` and its `SOURCES.md` row | the door's own measurements, printed | — (a reference is a source, not an asset) | a drawn floor, a contact shadow, a flood-through hole, a key that kept under a tenth or over four fifths of the frame, a span outside 0.7–1.3, under three heads, more than one subject — all of it **before** a GPU minute, because a lift is four minutes and a redraw is a sentence |
| Bodies and models | `just character` / `just prop` (TRELLIS.2), then `just prepare` (normalise + skeleton, no weights) and `just skin` (SkinTokens' weights, and the skeleton **fitted to this body's own bone lengths**) — `just body` runs both; `just prop-import` normalizes a prop | `just views`, `just check-mesh`, the studio | `just promote-body`, `just prop-import` (→ `forge promote body` / `model`) | arms not level with the body's own shoulder line; a left/right gap over 0.35 on an arm run or 0.20 elsewhere; a rest translation more than 1° off the contract's direction; an existing name without `--overwrite` |
| Clips | `just sweep` (ARDY, many takes in one load) | `just review` table + sheet, `just sheet` on the real body, `just bones` | `just promote-clip` (native bake, no Blender) | a clip that drives no bone or never moves (`sheet` exits 1); an unstated recipe knob (every knob is echoed) |
| Audio | `just sfx`, `just music`, `just speech` (MOSS, ACE-Step) — always to `out/audio/`; `just voice` designs a character's voice from a description into `assets-src/voices/<name>/` (MOSS-VoiceGenerator), so a project never has to bring a reference clip, and every line is cloned from it by name | `just audio` plot + numbers, `just audio-list` | `just promote-audio` | a silent or clipped file; a sound with no record ships as `unknown` provenance and says so; a voice clip with neither its record nor a ledger row fails `verify` |

Every promote writes a `<name>.json` sidecar beside the file and `just
manifest` projects the sidecars into `assets/library.json`, the one file a
game reads.

## Hardware and platforms

| Need | What |
|---|---|
| OS | Linux. Everything is tested on one machine; nothing is tested elsewhere yet |
| GPU | NVIDIA, ≥ 16 GB for clips and audio; 24 GB for 1024³ lifts (the body and prop registers) |
| CUDA | 12.4 (the TRELLIS.2 env pins its own toolkit; see `designs/hosting.md`) |
| Blender | ≥ 4.2, headless, only for the rig, export and prop-normalize steps |
| Judging and the viewer | CPU is enough: headless sheets and views need a wgpu adapter and llvmpipe qualifies; no display server |
| Python | 3.11+ system interpreter for the launcher (stdlib only); each backend brings its own env |
| Rust and `just` | rustup (the repo pins 1.96.1), `just`, and Bevy's headers — the prerequisites block above the Quickstart |
| Disk | what the kinds you chose need, and no more; `forge setup` prints the bill for your answer before fetching |

The generators do not share the card. ARDY's sweep is the hungriest at
15.4 GB (measured 2026-08-30); the 1024³ lift is the cheapest at
4.7 GB, and the ComfyUI host holds ~0.4 GB of
CUDA context for as long as its unit is up. `just gpu` says who holds the
card and whether the largest chosen backend would fit; a `backend.toml`'s
`vram_gb` is a budget and is never a measurement.

## The reference image

The reference PNG is an **input**. No image model ships here — one was
measured on 2026-08-30 and set aside, because a picture drawn by a person in
the tool they already have beats two minutes of the whole card and a gate
that cannot see what matters (`designs/decisions.md`, "The reference image
stays brought"). It comes in through one door, `import_reference` /
`forge ref import`, which holds it to a stated format, keys it, pre-checks it
and records its stated source. The sample library's references were made in
Grok, and `SOURCES.md` says so.

The lift record (`<name>.lift.json`, beside the PNG) claims the file's sha256
and a row in [`assets-src/SOURCES.md`](assets-src/SOURCES.md) — where it came
from, on what terms — never its regeneration. A PNG without a row fails
`just verify`.

What the lift needs from the picture, judged by eye before any GPU minute:

| Rule | Why |
|---|---|
| Strict T-pose: arms straight out, horizontal, one arm per side | the rig's rest pose is frozen; the auto-rig refuses reach outside 0.80–1.45 of wrist span or arm tips more than 0.15 m off wrist height, and names the image as the fix |
| Arm span ≈ height | same gate, the other axis |
| Flat, light, uniform background; no cast shadow | the launcher keys the alpha itself (no background-removal model is installed) |
| Uncropped, the whole figure, front view | one view underdetermines the back; cropping loses the feet the stature is measured from |
| Thick, simple shapes; no thin straps or floating parts | anything under 2.5 cm becomes dust the rig step drops; detached shells are re-weighted to what they sit on |
| Props: alone, three-quarter from slightly above | the far side and the top exist only if the picture implies them |

Judge the register before blaming the image: a 1 500-vertex lift melts hands
into cones, and the same PNG at the 25 000-vertex body register brings them
back. The seed is a knob too — sweep it for a hollow skull before touching
the drawing. [`designs/style-guide-template.md`](designs/style-guide-template.md)
is the art-direction doc a project fills in for what the picture should show.

## The rig profile

A rig is a directory of data, not a table in source: `rigs/humanoid/` holds
`contract.json` (every bone, its parent, whether clips drive it, rest
transform — generated from `rig.glb`, never edited), `sockets.json`,
`motion_skeleton.json`, `profile.toml` (every scalar the gates use) and the
artifacts `rig.glb` / `rig.blend`. A project names its profile in
`forge.toml`.

**The one rule: never add a parent above the root bone.** An engine binds a
curve by hashing the bone's full name path and reports nothing when the hash
finds no target — the character holds its T-pose forever. Leaf bones are
allowed and reported; a renamed bone, an inserted bone, a `Root` above `Hips`
are refused.

The shipped profile is a strict superset of ARDY's 27-joint skeleton — same
names, same hierarchy, rest pose preserved — so a raw take binds 27 of 27
curves with zero orphaned and **no retargeting exists anywhere** in the
toolkit. `just rig` re-derives the contract from `rig.glb` and writes the
fixture mannequin every test stands on; `just check-mesh <glb>` holds one
body to it; `just check-bodies` holds the library.
[`designs/rig-contract.md`](designs/rig-contract.md) is the contract;
[`rigs/humanoid/README.md`](rigs/humanoid/README.md) the shipped profile.

## What a record is allowed to claim

Two record kinds, one rule. Generator records (`forge_record: 1`, written by
Python beside every output) and library sidecars (`schema: 1`, written by Rust
beside every shipped file) both hold to **`null` means unknown, and a default
is never written as a measurement**. Provenance is
`recorded | reconstructed | unknown` and only moves down. Clips claim reproduction — `just audit`
rebuilds every one from its record, by bytes and then by pose to under a
millimetre. Bodies, models and audio claim integrity only: TRELLIS.2,
Blender's exporter, MOSS and ACE-Step are not bit-reproducible, so the record
says "this is the file that was checked", plus the seed and the `.blend`
hash where they exist. Every lift record names its texture baker, because
that is a licence fact. [`designs/records.md`](designs/records.md) has both
schemas field by field and what `forge verify` checks.

## Why the judging half has to exist

Bevy — and it is not alone — binds animation curves to bones **purely by
hashed name path**, has no retargeting, and when a clip's names do not match
the skeleton it reports **nothing at all**: no warning, no error, a character
standing in its rest pose. That is indistinguishable from a bad export, a
paused player, or a clip with no motion.

So the tools answer two different questions, and the difference matters:

- **Is it wired up?** `just bones walk` — driven / at rest / orphaned counts,
  no GPU. `just check-mesh` ends with `reference clip: 27 bone(s) driven`.
- **Does it read as the action?** `just sheet walk` — a picture, on the real
  body. This is what catches a "pistol shoot" clip that is really walking
  forward at 0.94 m/s while folding at the waist; every numeric gate passed it.

Audio has the same split. `just audio-list` measures a library — a bark 20 LU
below its neighbours is obvious in a column and invisible per file — and
`just audio <file>` draws one, because clipping reads as flat-topping, dead air
as a gap, a truncated tail as a cliff. Two notes learned the hard way:
**clipping is a run, not a count** (peak-normalising to 0 dBFS puts a sample
at the rail by construction; only sustained flat-topping warns), and
**loudness is approximate and says so** (K-weighting is a high-pass stand-in,
enough to compare one library, not to certify a master).

Meshes, likewise: every gate passed a body whose skull was hollow from behind,
because a gate measures what is there. `just views` with culling off is the
orbit a reviewer would do, run before the rig minute is spent.

## The studio

`just studio` opens one window with five parts: the **library browser**
(bodies, models, clips, audio; filter and tags), the **stage** (orbit, swap
the model, the same lights and lens every headless sheet uses), the
**transport** (scrub and play a clip on the body), the **audio view**
(waveform, spectrogram, playback — `just play` opens straight there) and the
**metadata panel** (the record, the rig findings, the audio numbers).
`--take x.npz` puts a raw ARDY take on the real body before it is baked;
`--screenshot out/studio.png` captures and quits.

The sheets, views and turntables need no window at all: a wgpu adapter
(llvmpipe qualifies) and no display server. `just smoke` proves it;
`just ci` runs on that.

## For agents

Seven skills under `.claude/skills/`, each with prerequisites checked, the
commands in order, the log lines to read, and a seen → consequence → fix
table:

| Skill | Ships |
|---|---|
| `forge-setup` | the three questions answered, the licences read, the chosen backends installed or adopted, `just doctor` green |
| `forge-prop` | a static model from a PNG |
| `forge-character` | a rigged body from a PNG, starting with the reference checklist |
| `forge-clip` | a clip from a prompt: sweep, review, promote with a recipe, strip on the body |
| `forge-audio` | a sound, a track or a line, plotted and promoted |
| `forge-voice` | a character's voice designed from a description — the source every line of that character is cloned from |
| `forge-review` | how to read a sheet: wiring → mechanics → picture; gates versus hints |

The CLI is one binary:

```
forge init [--make …] [--tier …] [--comfy-url …] [--yes]   (the three questions)
      setup [kind…] [--yes <licence>…] [--dry-run]         (one screen, then the installers)
      catalog | manifest [--check] | verify | audit [--fit] | rebake | migrate
      promote clip|body|model|audio        (direct; refuse an existing name unless --overwrite)
      audio inspect|list                   rig export-contract|fixture|check
      gen <cmd…>                           (mesh, prop, rig, export, rig-build, motion sweep|keys|review, sfx, music, speech, voice, doctor)
      serve | stop | jobs                  (the daemon: the queue, the card lock, the job table)
      doctor | gpu | sheet | views | turntable | bones | bundle | studio | mcp
```

`forge mcp` serves twenty-six tools over stdio, registered in
[`.mcp.json`](.mcp.json). That file launches `./target/debug/forge`, which
a fresh clone does not have — run any `just` recipe once (`just doctor` is
the usual first) to build it before the MCP server can start. Images come
back inline under the size vision
models downscale past; a refusal is a **successful frame** naming what does
exist, so a wrong name costs one turn, not a guess.

| Tool | What it does |
|---|---|
| `list_models` | bodies and models with prompt, tags, provenance, measured size |
| `list_clips` | clips with measured length, record and the recipe's non-identity knobs |
| `list_audio` | sounds with cached measurements; a mark on anything defective |
| `render_model` | `forge views` — a mesh from every angle; a path under `out/` renders culling off |
| `render_clip_strip` | `forge sheet` — poses across a clip on a body; 0 bones driven comes back as an error with the picture |
| `inspect_audio` | numbers, the record, a waveform-over-spectrogram plot |
| `import_reference` | the one door under `assets-src/refs/`: format, `mesh.py`'s own keyer, the four keyer pre-checks and the geometry pre-checks, then the PNG's original bytes, its record and its ledger row |
| `generate_mesh` | `forge gen mesh` — TRELLIS.2 lifts a reference into `out/lifts/`, character or prop register |
| `prepare_body` | normalise, drop dust, matte, insert the profile's skeleton, no weights; a refusal returns the arm-height numbers and names the reference PNG, and every limb's cross-section is printed and recorded as a note |
| `skin_body` | the whole skin → fit → re-prepare → re-skin → re-attach loop; the done frame carries the fit table and `motion_scale` |
| `export_body` | `forge gen export` — the rigged `.blend` to the `.glb` a body is filed as, through the export gate. The step between `skin_body` and `promote_body`, and there is no way round it |
| `promote_body` | the export gate + `forge rig check` + the taken-name refusal, then `promote body` |
| `promote_model` | the doors `just prop-import`'s promote runs |
| `generate_clips` | `forge gen motion sweep` + `review`; refuses with the doctor line when ARDY is absent |
| `generate_audio` | sfx, music or speech to `out/audio/`, never the library; `voice` names a designed voice or a brought clip |
| `promote_clip` | bake one take with a recipe stated in full; refuses a taken name unless `overwrite`, then echoes what it replaced |
| `promote_audio` | file an auditioned sound as sfx, music or voice |
| `export_bundle` | one glb: a body's skin and any number of clips as named animations, for handing outside the toolkit; nothing is filed |
| `doctor` | what this machine can run: ok, partial, missing, broken — and `off` for a kind the project did not choose |
| `init_project` | make a project: what you make, what card this is, where ComfyUI is. Refuses an existing project unless `adopt` |
| `licences` | every licence the chosen kinds carry, **each notice in full** — you cannot accept what you were not shown |
| `setup` | install what a kind needs. **Refused** until `accept` names every gated id, and the refusal lists exactly which |
| `wait` / `cancel` / `status` / `list_runs` | a generate returns a job; these are how you follow it, stop it, and see what the card is doing |

**A body and a model have doors now.** The first shape of this surface had
none, on the ground that a mesh needs a human looking at it. The human is in
the loop through the harness that issues every command, and what protects the
library is the export gate, the rig check and the refused taken name — all of
which `promote_body` runs. The thing that must not be automatable is
accepting a licence, which is why `accept` is an explicit argument.
`just mcp-check` handshakes the server and holds the tool list to exactly
these twenty-six, and `just mcp-session` runs the whole path — the audio leg
(`init_project → licences → setup → doctor → generate_audio → wait →
inspect_audio → promote_audio → verify`) and the character leg
(`import_reference → generate_mesh → wait → prepare_body → wait → skin_body →
wait → export_body → wait → promote_body → render_model → verify`), plus the
refusals — over **both** transports, stdio and the daemon's streamable HTTP
at `/mcp`. Every step of that leg is a tool call, which is the point of it:
`export_body` was missing until 2026-08-31 and the gate was shelling the
missing verb, so it proved the terminal rather than the surface.

## Backends and licences

**The texture baker is non-commercial.** TRELLIS.2 bakes its textures
through nvdiffrast 0.4.0, which ships under the NVIDIA Source Code License —
research and evaluation only, no commercial use. Everything else in the lift
(TRELLIS.2 code and weights, CuMesh, FlexGEMM, utils3d) is MIT, but a mesh
textured through this pipeline passed through nvdiffrast. The installer
requires explicit consent with the licence printed, `forge doctor` warns
while it is installed, every lift record carries
`texture_baker: "nvdiffrast (NVIDIA Source Code License, non-commercial)"`,
and a replacement baker is an open follow-up. Decide whether that fits your project before you lift
anything you mean to sell.

Nothing below is vendored; the installers fetch it under
`~/.cache/asset-forge/backends/` and the tree holds only links. Verified from
the files on disk, 2026-08-23:

| Component | Licence | Note |
|---|---|---|
| **nvdiffrast 0.4.0** | **NVIDIA Source Code License — NON-COMMERCIAL** | consent-gated install; doctor warns; the lift record names it |
| nvdiffrec `renderutils` | NVIDIA, non-commercial | **never installed** — nothing here needs it |
| `briaai/RMBG-2.0` | commercially restrictive | **never downloaded** — stubbed; references are flat-background by contract |
| TRELLIS.2 code, `microsoft/TRELLIS.2-4B` weights | MIT | |
| `facebook/dinov3-vitl16-pretrain-lvd1689m` | DINOv3 License (Meta) — gated | accept on the model page, then `hf auth login --token <tok>`; doctor prints both |
| CuMesh, FlexGEMM, utils3d | MIT | |
| flash-attn | BSD-3 | optional; `ATTN_BACKEND=sdpa` fallback |
| ARDY code / `ARDY-Core-RP-20FPS-Horizon40` weights | Apache-2.0 / NVIDIA Open Model License | |
| Meta-Llama-3-8B-Instruct (ARDY's text encoder base) | Llama 3 Community License | attribution: "Built with Meta Llama 3" |
| LLM2Vec | MIT | |
| ACE-Step 1.5 code + weights | MIT | |
| MOSS-TTS family, MOSS-SoundEffect-v2 | Apache-2.0 | MOSS-SoundEffect weights are ~11 GB |
| `OpenMOSS-Team/MOSS-VoiceGenerator` (1.7B) | Apache-2.0 | the voice designer behind `just voice`; ~4 GB, the same env as MOSS-TTS |
| Blender | GPL | a tool; nothing of it ships in an asset |

[backends/README.md](backends/README.md) has the install order, the adopt
flags, the VRAM matrix and the launcher rules;
[designs/hosting.md](designs/hosting.md) the traps, dated per backend.

## Crates

| Crate | One line |
|---|---|
| [`forge_manifest`](crates/forge_manifest) | the consumer contract: the typed, versioned `library.json` a game reads |
| [`forge_rig`](crates/forge_rig) | a rig profile as data: contract, sockets, driven layout; derive from a `.glb`, check drift, measure a file |
| [`forge_motion`](crates/forge_motion) | ARDY takes as data: read, edit, derive footsteps, bake a `.glb` clip on a rig |
| [`forge_library`](crates/forge_library) | sidecars, catalog, project, the four promote doors, manifest projection, verify, audit, rebake |
| [`forge_audio`](crates/forge_audio) | decode, measure and plot a sound; no output backend |
| [`forge_capture`](crates/forge_capture) | windowless Bevy frame capture and contact-sheet composition |
| [`forge_raster`](crates/forge_raster) | a CPU canvas with a 5×7 font, for labelled review images |
| [`forge_studio`](crates/forge_studio) | headless sheets and views, the viewer window, `rig check`, `bones`, `audit`'s posed half — `publish = false` |
| [`forge_mcp`](crates/forge_mcp) | the MCP tools as a library, served by `forge mcp` — `publish = false` |
| [`forge`](crates/forge) | the binary — `publish = false` |

None of the crates is published to crates.io, and none will be — the
names collide with existing registry crates, and a toolkit that ships a
binary, a Python layer and a rig profile together is honestly depended on
as one thing: use a git or path dependency into the checkout
(`designs/decisions.md` has the entry). `just publish-check` stays as the
packaging-hygiene gate: each of the seven library crates packages and
builds in isolation. Six of those seven link no engine at all — only
`forge_capture` pulls in Bevy — so `cargo test -p forge_library` never
pays for it. Bevy is pinned to `=0.19.0`: its `AnimationTargetId` hashing
changed in 0.19 and nothing here persists those ids.

## Layout

```
forge.toml  justfile  CLAUDE.md  .mcp.json      the project root marker, the recipes, the rules, the server
crates/        forge_{raster,audio,capture,rig,motion,manifest,library,studio,mcp}, forge
python/        forge_gen: the generator launcher, one module per command, the Blender steps
backends/      one directory per generator: backend.toml, install.sh, probe.py (envs live outside the tree)
rigs/humanoid/ the rig profile: contract.json, sockets.json, motion_skeleton.json, profile.toml, rig.glb, rig.blend
assets/        the library: bodies/ models/ clips/ audio/{sfx,music,voice}/, one .json beside each file, library.json
assets-src/    what assets are made from: refs/{characters,props}/<name>.png + .lift.json, SOURCES.md, takes/, blender/, voices/<name>/{ref.wav,voice.json}
designs/       decisions.md (the lessons ledger; it wins), records.md, rig-contract.md, style-guide-template.md, hosting.md
.claude/skills/ forge-{setup,prop,character,clip,audio,voice,review}
out/           gitignored: lifts/ props/ export/ sweeps/ sheets/ views/ audio/
```

## Using it from your game

```sh
just install                            # or: cargo install --path crates/forge --locked
export FORGE_TOOLKIT=~/src/asset-forge  # where the clone lives; forge init copies the rig profile from it
cd ~/my-game && forge init --name my-game
```

`just install` puts a release `forge` on PATH (`~/.cargo/bin`). Outside
the toolkit checkout, `forge init` needs `FORGE_TOOLKIT` (or `FORGE_HOME`)
pointing at the clone so it can install the rig profile — it exits 2 and
says so when it cannot. `forge init` writes `forge.toml`, the `assets/`
and `assets-src/` directories, the reference ledger's header, an empty
manifest, and a copy of the rig profile under `assets-src/rigs/`. Every
`forge` verb walks up from the working directory to the nearest
`forge.toml`, so the `just` recipes run from your project against its
library:

```sh
just --justfile ~/src/asset-forge/justfile --working-directory . sheet walk
```

(The dev recipes — `fmt`, `check`, `test`, `pytest`, `ci`, … — are the
exception: run that way they still act on the toolkit checkout, never on
your game.)

Your game reads one file, `assets/library.json`, through
[`forge_manifest`](crates/forge_manifest) (serde only, no engine; a git or
path dependency — the crates are not on crates.io): the rig
(profile, bone table, sockets, the `.glb` hash), bodies, models with their
bounds in metres, clips with duration, loop flag, root-motion mode and
events, and audio — every entry with its `sha256`. A newer manifest than the
crate understands is refused by number, so an old build says "behind" rather
than "corrupt".

Props attach at the profile's sockets — offsets from contract bones, in the
bone's own space, carrying a prop authored grip-at-origin, long axis +Y,
front −Z:

| Socket | Bone | For |
|---|---|---|
| `hand_r`, `hand_l` | `RightHand`, `LeftHand` | the grip, blade along the bone |
| `back` | `Spine3` | stowed, hilt over the right shoulder |
| `hip_l` | `Hips` | a scabbard, drawn across the body |
| `head` | `Head` | hats and helmets, front turned to face forward |

No runtime crate ships: spawning a body, playing a clip by name and attaching
at a socket are a few dozen lines in any engine that loads glTF, and the
manifest has everything they need.

Handing one asset to somebody who has none of this — an artist, another
project, a jam team — is `forge bundle`: one self-contained `.glb` carrying a
body's skin and any number of clips as named animations, plus a
`<stem>.bundle.json` record naming and hashing everything that went into it.

```sh
just bundle out/fit_warlock/drow_warlock_fitted.glb walk,roll \
    out/bundles/warlock.glb --motion-scale 1.0156
```

It is a merge, not a bake. Every clip's channels target bones by name and
every body carries the contract's names, so re-pointing the channels at the
body's nodes is the whole operation and the curve values travel byte for
byte; a clip that drives a bone the body lacks is refused, naming the bone.
`--motion-scale` multiplies the root travel — the body's leg length against
the profile's reference legs, so a fitted skeleton travels its own stride —
and touches no rotation. The body may be a library name or any rigged `.glb`,
the clips library names or paths, and the output goes wherever you say:
nothing is filed in the library, because the library already holds every
input. A bundle is derived, so it is regenerated rather than repaired.

## Licence

MIT OR Apache-2.0, at your option — **for the code**. The sample assets
under `assets/` and `assets-src/` are not covered by the code licence:
they are governed by [`assets-src/SOURCES.md`](assets-src/SOURCES.md),
summarised per kind in [`assets/LICENSE.md`](assets/LICENSE.md). In
particular, the three lifted sample meshes' textures passed through
nvdiffrast (non-commercial — see above) and are **not for commercial
use**.

The sample library under `assets/` and `assets-src/` is shipped so the tools
have something to show on a fresh clone. Its meshes were lifted with
TRELLIS.2 (MIT, code and weights) out of reference images drawn in Grok; its clips come from
[ARDY](https://github.com/nv-tlabs/ardy) (code Apache-2.0, checkpoints under
the NVIDIA Open Model License; outputs are usable); its sounds from MOSS and
ACE-Step (Apache-2.0 and MIT). The per-image record, the licence answer for
the reference images, and what each record is and is not allowed to claim
are in [`assets-src/SOURCES.md`](assets-src/SOURCES.md). Every lifted texture
in the sample passed through nvdiffrast; see above.
