# Forge 2 — one queue, one rig, one door

The plan for the second shape of the toolkit, written 2026-08-30 before any
code moved, revised the same day after review. What was asked for, in the
order the user cares about: **rigging through SkinTokens**, because it
skins to the rig it is handed and that is what makes a user's own skeleton
possible; **ComfyUI** as a host for the models it hosts well; **one process
that is the agent's door and the human's monitor**, with the toolkit fully
usable over MCP; and, from the review, **the reference image made here**
instead of brought, with a bring-your-own door kept. ARDY stays: the
profile is its skeleton, it has keyframes, and its licence has no
territory clause. `decisions.md` still wins over this file; every
principle in `CLAUDE.md` survives.

Two stances the user set, recorded so nobody re-litigates them: the
projects this serves are **open-source games**, so non-commercial
components (nvdiffrast) are acceptable and stay named in every record —
the fact travels, the stance can change later; and **an agent makes one
asset at a time** — there is no batch door, a set is a loop the agent runs.

## What does not change

- The one-way rule. Derived artefacts are never repaired by hand.
- `null` means unknown; provenance only moves down; a recipe states every
  knob; nothing is inherited.
- Sidecars are truth, the manifest is a projection.
- Gates at the API surface. Refusals are successful frames. No review
  queue.
- One native bake. Clips claim reproduction (≤ 1 mm); bodies, models and
  audio claim integrity.
- The humanoid profile, its 55 bones and 27 driven, ARDY's `cskel27`,
  the frozen rest pose, every shipped clip and the sidecar schema. **Not
  one clip is rebaked by this plan.**
- Look before you spend. The strip on the real body, the seven-view sheet,
  the plot, the user's eye in the studio.

## The facts the plan bends around

Researched 2026-08-30 from repositories, papers, licence texts and issue
trackers.

**SkinTokens (VAST-AI).** Successor to UniRig; one autoregressive model
emits skeleton and skin tokens from a point-cloud encoding of the mesh.
`--use_skeleton` reads the first armature in the input file and predicts
weights *for that skeleton*; `--use_transfer` binds them onto the untouched
original mesh (textures, scale, shells preserved), ≤ 4 influences. Names
outside the Mixamo/VRoid templates pass through untyped; a 55-bone
humanoid is inside the training distribution (up to 102 joints).
Disconnected shells are concatenated and weighted from local geometry.
No seed: a rig is `recorded` with its output hashed, never reproduced.
Code and weights MIT; the Michelangelo encoder files carry an open GPL
question (issue #9); a bone-tail export bug (issue #8, one line). Env:
torch 2.7/cu128, pip `bpy ≥ 4.2`, flash-attn (patchable to SDPA), and
"at least 14 GB" by upstream's own claim — measured here at 3.3–4.4 GB
(`hosting.md`, 2026-08-30), which is the number the tier table uses.
The authors call skin-only-on-a-given-skeleton a demo feature with no
numbers, so the spike is the evidence.

**ComfyUI.** One worker thread, sequential. `POST /prompt`, `GET
/history/{id}`, `GET /view`, `POST /free`, `GET /system_stats`, `GET
/object_info`, the Manager's snapshot. Models stay resident; wrapper packs
mostly load *outside* its memory manager, so `/free` alone does not clear
the card. GPL-3.0 core; driving it over HTTP from a separate process is
clean. Coverage that is mature: ACE-Step 1.5 native; MOSS-TTS,
VoiceGenerator and SoundEffect v2 through TTS-Audio-Suite (MIT, 1.2k★);
Qwen-Image and FLUX.1-schnell native for images. Coverage that is not:
TRELLIS.2 wrappers carry the same CUDA build fight with Windows-first
wheels; SkinTokens packs are weeks old; ARDY has no node at all; one of
three HY-Motion packs already 404s. **Lesson:** ComfyUI is worth having
for what it hosts well and is not worth being the scheduler.

**Image models for references.** Qwen-Image (Apache-2.0, ComfyUI-native,
20B — fp8 fits 24 GB, GGUF for 16 GB) and FLUX.1-schnell (Apache-2.0,
~12 GB fp8). Pose conditioning exists for both (ControlNet-Union
variants). Which one lands a strict T-pose most reliably is a spike, not a
guess. Neither is bit-reproducible across GPUs, so the reference keeps
claiming integrity; what changes is that its provenance becomes
`recorded` (model hash, seed, workflow hash, prompt) instead of a stranger
with a `SOURCES.md` row.

## The decisions made

1. **ARDY stays.** Motion is not this plan's problem. Skeleton-as-data
   and a second motion source (HY-Motion, video-to-motion) are a later
   plan that starts with an SMPL-family profile; noted, not scheduled.
2. **`forge serve` owns the queue and the card.** Not ComfyUI. Two
   executors behind one queue: `comfy` for ACE-Step, MOSS and the image
   model; `env` for TRELLIS.2, ARDY, SkinTokens and Blender — the
   per-backend launcher that exists today, driven by the daemon instead of
   by a shell. Every UX property (jobs, status, remote card, never two
   generates) belongs to the daemon and does not depend on a node pack
   existing. The CLI is a client of the daemon when one is up and runs
   in-process otherwise, so `just character` and an MCP `generate_mesh`
   share one queue.
3. **SkinTokens runs in the `env` executor**, first-party checkout, not
   through a community pack: it is the piece we care most about, we want
   to patch issue #8 and the flash-attn import ourselves, and the packs
   need Blender on PATH anyway.
4. **Blender shrinks.** The weights ladder goes. Geometry prep before
   skinning (normalise to stature, the T-pose fit gate, the dust filter,
   the budget, inserting the profile armature) stays in headless Blender
   as `forge gen prepare`; prop normalise and `rig-build` stay; the export
   gate moves to Rust where `forge_rig` already derives bones from a glb.
5. **A user's rig is a profile.** `forge rig import <glb|fbx>` derives the
   five profile entries from its armature; SkinTokens skins our meshes to
   it; their clips play because the skeleton is theirs. Our ARDY clips do
   not play on it — a retarget is its own later door, never a silent
   layer.
6. **The reference image is made here, and may still be brought.**
   `generate_reference` draws it in the project's style with pose
   conditioning; `import_reference` takes a PNG the user made and holds it
   to the same format. Both write `assets-src/refs/<name>.png` with a
   `<name>.ref.json` beside it and the `SOURCES.md` row. The fit gate's
   "re-proportion the reference" becomes a re-roll.
7. **The monitor is a TUI.** `forge top`, a ratatui client of the daemon:
   the card, the queue, jobs with a log tail, the library, a record's
   chain, doctor. Text is what a monitor shows; images already have two
   homes, the Bevy studio for the eye and the MCP render results for the
   agent. No web stack.
8. **Licence acceptance is explicit at both doors.** The CLI asks on a
   TTY or takes `--yes <licence>`; the MCP `setup` refuses a gated kind
   unless `accept` names its licence after `licences` returned the text.
   The receipt records who accepted and when.

## The shape after

```
   Claude Code ── /mcp (streamable HTTP) or stdio ─┐
   forge CLI ──── client when the daemon is up ────┤
   forge top ──── TUI ──────────────────────────────┤
                                       ┌───────────┴───────────┐
                                       │      forge serve       │
                                       │  queue · card lock     │─── forge_library (Rust)
                                       │  jobs · records        │    catalog · promote · verify · bake
                                       └──────┬─────────┬───────┘
                          executor "env"      │         │      executor "comfy"  (HTTP)
                 ┌────────────────────────────┴──┐   ┌──┴─────────────────────────────┐
                 │ TRELLIS.2 · ARDY · SkinTokens  │   │ ComfyUI (systemd --user, :8188)│
                 │ Blender: prepare · prop · build│   │ ACE-Step · MOSS ×3 · Qwen-Image│
                 └───────────────────────────────┘   └────────────────────────────────┘
   Bevy (in-process): headless sheets, strips, rig check, the studio window
```

The queue is one; the card lock is one; a job is one row in the daemon's
state with its record path, its executor, its exit and its log. A job
through `comfy` is a workflow template under `backends/<name>/workflows/`
with inputs patched; a job through `env` is today's `forge gen` launcher
call. Both write the same generator record shape.

### `backend.toml`, second form

`executor = "env" | "comfy" | "tool"` replaces `env_kind` at the top
level; `env` backends keep `[env]`, `python`, `cuda`, `install.sh` and
`probe.py` exactly as today. A `comfy` backend has no interpreter:

```toml
name = "qwen_image"
role = "reference"
executor = "comfy"
license = "Apache-2.0 (Qwen-Image); ControlNet-Union: Apache-2.0"
vram_gb = 24                           # a budget: fp8 measured at 23.3 GB, Q4 GGUF at 16.2 on lean
[comfy]
packs = []                             # native; a pack entry is { repo, commit }
workflows = ["reference.api.json"]     # inputs patched: prompt, style_prefix, seed, pose_image
unload_node = null                     # native models honour /free
[[models]]
id = "Comfy-Org/Qwen-Image_ComfyUI"
store = "comfy:models/diffusion_models"
license = "Apache-2.0"
```

Doctor renders the same words per backend, `ok | partial | missing |
broken`, plus `off` for a kind the project did not choose — from the
probe for `env`, from `/object_info` and the snapshot for `comfy`. Exit 1
only while a *chosen* kind is not `ok`.

### The records, second form

`forge_record: 2` — `backend` gains `executor`, and for `comfy`:
`comfyui_commit`, `workflow_sha256`, `packs {repo: commit}`; the six
existing keys keep their meaning. A new record `kind: "ref"`, tool
`qwen_image | imported`, written beside the PNG: prompt, style prefix,
seed, pose conditioning, model hash; or for an import, `source` as the
user stated it. `forge verify` holds every reference to either its
`.ref.json` (output hash = the PNG) or a `SOURCES.md` row — the same rule
a designed voice already lives under. A rig record names its `skinner`
and the encoder licence note, the way a lift record names its
`texture_baker`. **The library sidecar stays at `schema: 1`**: no kind, no
generator and no claim changes; a body's `post` block gains
`skinner {tool, model_sha256, commit}`, which is an addition and bumps
nothing.

### The stranger's first hour

`forge init` asks three questions once and writes them to `forge.toml`;
everything after reads them. Choices are phrased as what you make and
what card you have, never as model names.

```toml
[make]
props = true          # trellis2 + qwen_image
characters = true     # trellis2 + skintokens + qwen_image
clips = true          # ardy
sfx = false           # moss_sfx
music = false         # acestep
voice = false         # moss_tts

[hardware]
tier = "full"         # full (24 GB) | lean (16 GB) | fake (no card): detected, overridable
comfy_url = "http://127.0.0.1:8188"   # or another machine's; env backends and Blender stay local
```

**The kind → backend map, one fact in one place** (Phase 1 publishes it as
`MakeKind::backends` in `forge_library::project`, and every `backend.toml`
says the same thing with `executor =`):

| you choose | it needs | executor |
|---|---|---|
| `props` | `trellis2`, `qwen_image` | env, comfy |
| `characters` | `trellis2`, `skintokens`, `qwen_image` | env, env, comfy |
| `clips` | `ardy` | env |
| `sfx` | `moss_sfx` | comfy |
| `music` | `acestep` | comfy |
| `voice` | `moss_tts` | comfy |

Anything `comfy` adds `comfy` itself, the host. `--make none` and tier
`fake` choose nothing, so every doctor row reads `off` and doctor exits 0 —
which is how the gate runs green on a runner with no card.

**The licence ids**, the words `--yes <id>` and the MCP `setup`'s `accept`
name a licence by. `licences` returns each one's whole notice; `setup`
refuses until every id whose "yes" column says yes is in `accept`.

| id | what | backend | needs your yes |
|---|---|---|---|
| `nvdiffrast` | NVIDIA Source Code License (1-Way Commercial), non-commercial | `trellis2` | yes |
| `dinov3` | DINOv3 License (Meta), gated — a token only a human holds | `trellis2` | yes |
| `llama3` | Llama 3 Community License, attribution required | `ardy` | yes |
| `skintokens_encoder` | the Michelangelo encoder question (issue #9) | `skintokens` | no, a warning |
| `comfyui_gpl` | GPL-3.0-or-later, driven over HTTP | `comfy` | no, a fact |

The receipt is `$FORGE_BACKENDS_HOME/licences.json` — beside the installs,
because the install is what is licensed, and never in `forge.toml`, which is
hand-edited and would let an acceptance be *typed* rather than *given*.

Tiers change registers and variants, not features: lean runs the image
model quantised (Qwen-Image Q4_K_M GGUF) and MOSS-TTS 1.7B. Fake is
`FORGE_FAKE=1` made a first-class answer. **Lean lifts at 1024³ like the
full tier** — the Phase 0 spike measured a 1024³ lift at 4.7 GB, so a
16 GB card has three quarters of itself spare during one, and 512³ turned
out to be a speed knob (16 s saved, the face and the fingers lost), not a
memory one. The lean tier's real constraint is motion and the reference
image, and it always was.

`forge setup` prints, before a byte downloads, every licence fact the
chosen kinds carry — nvdiffrast (non-commercial), the DINOv3 gated login,
the SkinTokens encoder note — and the disk each kind costs, then asks
once; resumable; never asks twice.

Both VRAM columns are **approximate peaks measured on one 24 GB card on
2026-08-30** (`nvidia-smi` at 10 Hz; the lean column under a 16 GB cap —
`designs/hosting.md` § Lean tier says exactly what the cap bounds and what
it does not). Rows still marked *budget* are estimates nobody has sampled;
they are conservative and they are not measurements. No number here is a
certificate that a real 16 GB part is enough — such a part has roughly
15.0–15.5 GB usable once its own context and a desktop are resident.

| you make | backends | full (24 GB) | lean (16 GB) | weights on disk | needs your yes |
|---|---|---|---|---|---|
| props | trellis2, qwen_image | 1024³ **4.7 GB**; image fp8 **23.3 GB** — alone | 1024³ **4.7 GB**; image Q4 GGUF **16.2 GB** — alone | TRELLIS.2-4B + DINOv3 (measure); Qwen-Image 20.4 GB fp8 or 13.1 GB Q4 GGUF | nvdiffrast NC; DINOv3 login |
| characters | + skintokens | + **3.3–4.4 GB** | + **3.3–4.4 GB** | + 1.6 GB | as props; skinner note is a warning |
| clips | ardy | **15.4 GB** | **15.4 GB** — marginal on a real 16 GB part | core + Llama-3/LLM2Vec encoder | Llama 3 notice |
| sfx | moss_sfx | ~6–8 GB (budget) | same | ~11 GB | none |
| music | acestep | ~8 GB (budget) | same | ~7.5 GB | none |
| voice | moss_tts | 4B ~12 GB (budget) | 1.7B ~5 GB (budget, not run) | ~8 GB + 4 GB | none |

The image model is the only thing here that wants the whole card, on
either tier; the lift, which every table in this repo had at ~22 GB for a
week, is the cheapest GPU step of the three.

### The reference door

`generate_reference` and `import_reference` are the two ways a PNG gets
under `assets-src/refs/`. The tool descriptions carry the format, because
the fit gate downstream measures against it and a wrong image costs a
lift:

> A reference is one PNG, 1024 px or more on its long side, of one
> subject on a flat, uniform background: no floor, no shadow, no gradient,
> nothing behind it. The subject fills about nine tenths of the height.
> **Character:** the front view, facing the camera, in a strict T-pose —
> arms straight out and horizontal, palms down, legs slightly apart, feet
> flat — holding nothing, with no hair, cloth or gear crossing the
> silhouette of the arms or legs; proportions of seven heads or more,
> because the fit gate measures reach against wrist span and arm height
> against the wrists. **Prop:** a three-quarter view that shows the top
> and one side, the whole object inside the frame, resting the way it
> will rest in the game. The importer keys the background to alpha,
> hashes the file, writes the `SOURCES.md` row from the `source` you
> state, and runs the silhouette pre-check the fit gate would otherwise
> fail after a lift.

`generate_reference` prefixes the prompt with the project's style guide
line, conditions the pose on the profile's T-pose skeleton rendered as a
pose image, and returns a job whose result is the PNG and a contact of
the four seeds it drew, so the agent picks one and the others are
discarded. The record names the seed kept.

### The MCP surface, whole

Fully usable from a client with no shell. Every step is a tool; `just`
recipes and skills are the same doors with a terminal in front, and each
skill names which door a step uses. Thirty-two tools in five groups;
`mcp-check` pins the list.

| group | tools | notes |
|---|---|---|
| looking | `list_models` `list_clips` `list_audio` `list_runs` `list_backends` `inspect_record` `render_model` `render_clip_strip` `inspect_audio` `status` | `list_runs` walks `out/` and `assets-src/refs` reading the record beside each output: prompt, seed, when, promoted or not. `status`: free VRAM and who holds it, the queue, jobs in flight with a log tail |
| setting up | `init_project` `licences` `setup` `doctor` | `setup` refuses a gated kind unless `accept` names its licence; the receipt records who accepted. The DINOv3 login is a token only a human holds; the tool says so and stops |
| making | `generate_reference` `import_reference` `generate_mesh` `prepare_body` `skin_body` `generate_clips` `generate_audio` `design_voice` `import_rig` | every one writes under `out/` or `assets-src/` and **returns a job**. `generate_clips` takes ARDY's keyframes and presets as arguments (what `motion keys` does today) |
| jobs | `wait` `cancel` | `wait(job, max_s)` returns the result, or `{running, position, elapsed, eta}`; results served from ComfyUI's node cache say `cached: true, same_as` |
| shipping | `promote_body` `promote_model` `promote_clip` `promote_audio` `verify` `audit` `manifest_check` | the same gates as the CLI; a taken name refused unless `overwrite`; what was replaced echoed; the manifest rewritten |

**The door for a mesh opens.** The first form had no MCP promote for a
body or a model. The human is in the loop through the harness that issues
every command — the ground "No review queue" already stands on — and what
protects the library is the export gate, the rig check and the refused
taken name, all of which `promote_body` runs. The thing that must not be
automatable is accepting a licence, which is why `accept` is an explicit
argument.

## Phases

Each ends green — `just ci` and a commit — with its reason in
`decisions.md`. Rigging is what the user cares about, so it is first
after the daemon it needs.

**Phase 0 — spike. Done 2026-08-30, all three answers yes.** Three
time-boxed spikes ran on the real card, each a dated entry in `hosting.md`
and a lesson in `decisions.md`. **The skinner is go:** SkinTokens skinned
`vex_runner`'s raw lift with the profile's armature inserted, and while the
skeleton it hands back is ours in count, order and parents but not in name
or position, the per-vertex indices read as skin-order and re-attached to the
untouched `rig.blend` pass `forge rig check` 10 of 10 — the walk driving 27
of 27, 0 unweighted vertices — and the detached pauldrons bind rigidly to one
bone each where the bone-heat ladder smeared them across three. **The
reference model is Qwen-Image:** given the style guide's line and pose
conditioning on the profile's rest pose, it held the arms within 1.6° of
horizontal in 4 of 4 and drew the project's flat, posterized look in 4 of 4,
while FLUX.1-schnell drooped to 6.4° and drew a photograph every time; the
fit gate passed both, so the eye chose, not the gate. **The lean column is
measured:** props, characters and clips carry sampled peaks, 1024³ lifts on
both tiers, and Q4_K_M GGUF is lean's one substitution. Two things the spikes
also bought: the keyer, not the prompt, decides whether a drawn reference is
liftable, and half the repository's `vram_gb` figures were budgets reading as
facts. Phases 1–4 stand as written.

**Phase 1 — the daemon (2 weeks). Landed, audited and closed
2026-08-30.** `forge.toml` carries `[make]` and `[hardware]`; `forge init`
asks the three questions on a TTY and takes the defaults with one line of assumptions
where there is none; `forge setup` prints one screen — backends, disk, total,
every licence in full — before a byte downloads, refuses a bare `--yes`, and
appends acceptances to `$FORGE_BACKENDS_HOME/licences.json`; doctor has its
fifth word (`off`), its `executor`/`chosen` columns, the comfy ladder against
a shared `/object_info`, and exits 1 only for a chosen backend. **`forge
serve` is up**: the FIFO, the `flock(2)` card lease, the job table, both
executors, the HTTP API with MCP nested at `/mcp`, and `forge jobs` /
`forge job show|log|cancel` / `forge stop` as its terminal doors. **The
three audio venvs are gone**: ACE-Step is native to the host, the three MOSS
models come through TTS-Audio-Suite, and `forge_record: 2` carries
`executor`, `comfyui_commit`, `workflow_sha256` and `packs`.
`init_project`, `licences` and `setup` are MCP tools; `mcp-session` runs the
agent's whole path over both transports and, since the audit, from a
directory that is not a project yet.

**What the audit found, and what it means for the phase** (the fixes are in
`decisions.md` and `designs/serve.md`, all dated 2026-08-30). Most of it was
one shape — a fact that existed in one place and was defaulted in another:
the terminal door submitted every generate with no backend while the MCP
door named one, tier `fake` never reached the queue, the card was called
back against a number an earlier job's leftovers were inside of, and the
setup screen billed 9.5 GB in front of a 73.7 GB download. Two things are
**still open**, and neither is a defect of the daemon: `forge gen speech`
cannot make a line, because MOSS-TTS 1.7B does not run under the host's
transformers 5 and the pack answers with silence (`backends/moss_tts`'s
notice names the pin that would lift it); and `forge gen music` renders but
does not promote, because ACE-Step 1.5 turbo comes off the host at 0.0 dBFS
and the clipping gate is right to refuse it. The MCP `setup` tool also
**plans and gates rather than installing** — `serve.md` §7 has it returning
a job, and the queue schedules `forge gen` command lines, so an install
executor is a decision of its own; the tool's description says what it does
instead of what the plan said it would. One fix came after the audit and
belongs to the same shape: the comfy release ladder now stops at step 1 when
the host does not answer, and `forge gpu --free` clears a withheld lease only
where it measured one back — nothing measured is nothing claimed, in both
directions.

**What did not ship, said plainly.** The two audio verbs above: speech makes
no line at this pin, music renders and cannot promote. The MCP `setup` gates
and plans rather than installing, so there is no install executor. The tool
surface is **eighteen**, not the thirty-two the table above lists:
`inspect_record`, `list_backends`, `design_voice`, `import_rig`, `audit`
and `manifest_check` are Phases 2 to 4, and `mcp-check` is pinned at
eighteen until they land. No mesh moved: `generate_mesh`, `prepare_body`,
`skin_body`, `promote_body` and `promote_model` are Phase 2, and SkinTokens is still only the Phase 0 spike
with its checkout, patches and probe committed. No reference is drawn here
yet — `backends/qwen_image` is Phase 3, and every reference in the sample
library still lives on a `SOURCES.md` row. `forge top` does not exist. And
nothing under `assets/` was rebaked, which is the promise this plan opened
with.

**Phase 1, as planned —** `forge serve`: the queue, the card
lock, the job table, two executors. `env` is today's launcher driven
in-process; `comfy` is a client cut from `audio/music.py`'s server client
(`submit`, `wait_for`, `fetch`, `free`, `stats`, `object_info`). ComfyUI
as a systemd user unit under `$FORGE_BACKENDS_HOME/comfy` with a committed
snapshot and `extra_model_paths.yaml` over the weight caches on disk;
audio first — ACE-Step native (the resident server, its pidfile and the
soundfile patch deleted), MOSS ×3 through TTS-Audio-Suite. The CLI
becomes a daemon client when one is up. `forge_record: 2`. MCP over
streamable HTTP at `/mcp` and stdio; `generate_audio` returns a job;
`wait`, `cancel`, `status`, `list_runs`; `init_project`, `licences`,
`setup`, `doctor` as tools; the three questions in `forge init`; doctor's
`off`. **New gate `mcp-session`**: a scripted fake-tier session through
the MCP (`init_project → setup → doctor → generate_audio → wait →
inspect_audio → promote_audio → verify`), the agent's path held green the
way `ci-fake` holds the shell's. Retire `acestep`, `moss_sfx`, `moss_tts`
venvs.

**Phase 2 — rigging (2 weeks).** `backends/skintokens/` in the `env`
executor with the issue-#8 and SDPA patches under `patches/`. `forge gen
prepare` (Blender: normalise, fit gate, dust, budget, armature in) →
`forge gen skin` (skin-only, transfer on, postprocess on) → the export
gate in Rust (`forge_rig`: armature node, depth, rest pose, influences,
self-contained container) → `forge rig check`. The rig record carries
`skinner`; doctor warns on the encoder note. `forge rig import` for a
user's armature. Over MCP: `generate_mesh`, `prepare_body`, `skin_body`,
`import_rig` as jobs; `promote_body`, `promote_model`; `mcp-session`
grows the character loop. Delete the rescue ladder, `_auto_weights` and
`rig.py`'s bind half; keep the shell-abort gate on SkinTokens' output.
Gate: `vex_runner` and one plated body re-skinned, `check-bodies` green,
the walk binding 27 of 27.

**Phase 3 — references (1–2 weeks).** `backends/qwen_image/` (or
schnell) in the `comfy` executor; the pose image rendered from the
profile's rest pose; the style prefix read from the project's style
guide; `generate_reference` and `import_reference` with the format text
above; the `ref` record; `forge verify`'s rule extended; the sample
library's references given `.ref.json` records where they can be honestly
`reconstructed` and rows where they cannot. Gate: describe → reference →
lift → prepare → skin → promote for one new character, no tool outside
the toolkit touched.

**Phase 4 — the monitor and the rest (1–2 weeks).** `forge top`; the
remaining tools (`design_voice`, `list_backends`, `inspect_record`,
`audit`, `manifest_check`, `generate_clips` as a job with keyframes);
`mcp-check` re-pinned to thirty-two; the six skills rewritten with log
lines captured from real runs, each step naming its door. `just studio`
unchanged. Gate: a stranger's session from the README to a promoted
character and a promoted clip, from Claude Code alone.

**Later, each its own decision.** TRELLIS.2 into the `comfy` executor if
a Linux wrapper ever builds cleanly (nothing waits on it). Skeleton-as-data
in `forge_motion` and an SMPL-family profile, which is what HY-Motion or
video-to-motion would need. A retarget door for user profiles, judged on
the strip, its map hashed. A texture baker without nvdiffrast, if the
commercial stance ever changes. A second, non-humanoid profile.

## Risks, named

| Risk | Where it bites | What retires it |
|---|---|---|
| SkinTokens skin-only is a demo mode with no numbers | the whole point | the Phase 0 spike on a real lift, judged on the walk strip; if it loses to bone heat on plated bodies, the ladder stays and this plan shrinks to the daemon and the references |
| SkinTokens has no seed | a rig is unrepeatable | it claims integrity, like a body; the record names the output hash and the commit |
| The encoder licence question | shipping | `skinner` in the record; doctor warns; it runs in its own process already |
| Wrapper packs bypass ComfyUI's memory manager | the card stays held | every template ends in the pack's unload node; the daemon's card lock reads `/system_stats` and restarts the unit when free VRAM does not return |
| Node-result caching returns a stale output | a "re-roll" that never ran | `cached: true, same_as` on the job; the seed is always in the template |
| A generate blocks an MCP call for minutes | every agent session | jobs; `wait` has a ceiling; `mcp-session` in CI |
| An agent accepts a licence nobody read | every project | `setup` refuses without `accept`; `licences` returns the text; the receipt names who |
| The image model will not hold a T-pose | the reference door | pose conditioning plus the fit gate as the judge; four seeds per draw; `import_reference` is always there |
| Two doors race for the card | terminal + agent | the daemon owns the lock; the CLI is its client when it is up |
| The TUI becomes a second story about the library | staleness | it renders the daemon's job table and the sidecars; it holds nothing of its own |

## What gets deleted

`backends/acestep/` and the `moss_*` venvs and probes; `audio/music.py`'s
server half; `blender/rig.py`'s weights ladder and both rescue functions;
the "no promote for a mesh" rule in `forge_mcp` and its `mcp-check` line;
the studio-only assumption in `forge gpu`. Nothing under `assets/` moves.
Each deletion is its own commit with its reason in `decisions.md`.
