---
name: forge-character
description: Ship a rigged character from a reference PNG — judge the image, TRELLIS.2 lift, look at the raw mesh from seven angles, headless-Blender auto-rig to the humanoid profile, promote through the export gate and rig check, verify in the studio. Use when the user wants a playable or enemy character generated from a reference image, wants one regenerated (another seed, a different stature, fixed facing), or reports a mesh defect such as a hollow head or an orbiting shoulder plate.
---

# Character: PNG → lift → look → rig → promote

Five `just` recipes in a fixed order with one look between each. Everything
after the PNG is reproducible from the records beside it; the PNG itself is
an input — brought, not made — and claims only its sha256 and a ledger row.
Every name below is checked against `just --list` and `forge --help`; if a
recipe here is missing there, this file is wrong, not the justfile. Every
log line quoted was captured on a real run (2026-08-23, `torv_warden`, a
T-posed warden at the character preset, RTX 4090, Blender 5.2).

From a project made by `forge init` (not the toolkit checkout) the same
recipes run as `just --justfile <toolkit>/justfile --working-directory .
<recipe>` with `FORGE_HOME=<toolkit>` exported so `forge gen` finds
`python/forge_gen`; `forge` itself walks up to the project's `forge.toml`.

## Prerequisites (check, don't assume)

| Check | Command | Healthy |
|---|---|---|
| The PNG is at `assets-src/refs/characters/<name>.png`, `<name>` is `[a-z0-9_]+` | `ls assets-src/refs/characters/` | file present, name legal |
| The name is free | `just catalog --kind body` | `<name>` absent from the `name` column (taken → `--overwrite` at step 4, only if replacing is the intent) |
| Backends and tools | `just doctor` | `trellis2   ok`, `blender    ok`, `rig       humanoid v1: 55 bones (27 driven by cskel27), 5 socket(s), rigs/humanoid — glb sha ok, blend sha ok, no drift`. Doctor exits 1 while *any* backend is not `ok` (on this machine `moss_tts   partial`) — that does not block a lift; `trellis2` and `blender` do. |
| nvdiffrast | same table | `warn notice: nvdiffrast is non-commercial: …` is expected and stays. A commercial project must decide before lifting; the lift record will name it. |
| The GPU is free | `just gpu` | `holding   nobody` and `largest   trellis2 needs 22 GB (22528 MiB): fits`. Seen `does NOT fit — stop what holds the card before a generate` (exit 1): the `holding   pid N … GB  <process>` line names the holder. The ComfyUI host holding the last model: `forge gpu --free`, or `systemctl --user stop forge-comfy`. A studio window: close it. Anything else: by PID, never `pkill -f`. Never start a lift while another generate runs. |
| The project's style doc, if it has one | `designs/style-guide-template.md` is the template | it decides proportions and the look; the PNG has to already be in that register — the pipeline does not restyle |

Blender is `$BLENDER_BIN` or `blender` on PATH (doctor prints which);
TRELLIS.2 runs in `backends/trellis2/.env` and needs the DINOv3 gate
accepted (`doctor` says `hf auth login --token` when it is not).

## Step 0 — judge the PNG before any GPU time

**The reference is brought, not made here.** No image model ships in this
toolkit — one was measured on 2026-08-30 and set aside, because a picture a
person draws in the tool they already have beats two minutes of the whole
card and a fit gate that cannot see limb volume (`designs/decisions.md`,
"The reference image stays brought"). Draw it, or have the user draw it, and
bring it in through `import_reference` / `forge ref import` (Phase 3; until
that door lands, copy the PNG in and write its `SOURCES.md` row by hand as
below). The sample library's references were made in **Grok**, and
`SOURCES.md` says so.

`Read` the PNG (the Read tool shows images). The gates downstream measure
geometry, not intent: the image has to *be* a T-pose, not describe one. A
lift is minutes of a 22 GB card; a look is free.

| Seen | Consequence downstream | Fix (in the image — the pipeline never compensates) |
|---|---|---|
| Arms more than ~10° off horizontal, or bent | `rig: fit — … arm tips at z A vs wrist z W` fails on height → `not a T-pose` | both arms straight out at shoulder height, palms down |
| Two forearms, a hanging gauntlet, a second limb on one side | fit fails on span, or the rig binds a third limb | one arm per side, shoulder to fist in one line |
| Squat or wide: arm span visibly longer than height | reach > 1.45 (the first squat take measured 1.52) | taller, about six head heights, longer legs and torso, shoulders higher, so span ≈ height |
| A crown, tall hood or headdress adding ~10 % height | the body scales down under it; shoulders land below the skeleton's; fit fails on arm *height* | not an image fix — `--stature 2.0` at step 3, then the span check still has to pass |
| Gradient background, vignette, ground shadow | the keyer flood-fills from the border: a gradient keys as body (`the image border is not a flat background`) and a shadow lifts as a puddle under the feet | plain flat light background, no shadow |
| Cropped at the feet or hands, a weapon, a cape, text | missing geometry, or an un-riggable shell stuck to the body | whole body in frame; weapons are props on sockets, not part of the body |
| Side or three-quarter view | the rest pose faces the rig's front; a turned body fails fit or rigs twisted | front view |
| Thin wedges, spikes, thin straps | TRELLIS reads thin shapes as cones or drops them | thick shapes; tube limbs; oversized hands and boots read best |
| Subject tiny or filling the frame | `alpha keying kept N% of the image as subject` outside 5–95 % is refused | subject roughly a third to two thirds of the frame |

Legal but loud: oversized fists past the wrist joint are a register (the
reach ceiling of 1.45 makes room); a mohawk or pauldron taller than the head
is fine; asymmetry (one big cybernetic arm) is fine as long as it is still
one arm.

Two files ride with the PNG:

- `assets-src/SOURCES.md` — add the row now, in the table under
  `| File | Origin | For | Date |`. The first cell must contain the file
  name. A PNG without a row fails `just verify`:
  `FAIL <name>.png   assets-src/refs/characters/<name>.png has no row in SOURCES.md — a reference image claims a ledger row, or it cannot be accounted for`.
  Origin is where it came from and on what terms — the licence answer lives
  here and nowhere else.
- `assets-src/refs/characters/<name>.txt` (optional) — its first paragraph
  becomes the lift record's `prompt` (what the image was made with); `--prompt TEXT`
  on the lift overrides, `--source TEXT` says where the image came from.

## Step 1 — lift: `just character <name> [flags]`

Runs `forge gen mesh assets-src/refs/characters/<name>.png --preset character
--out out/lifts/<name>.glb --record assets-src/refs/characters/<name>.lift.json`.
The preset is the body register — **1024³ voxels, 25 000 vertices, 1024²
texture, seed 42** — and it is the register, not a starting point: a
1 500-vertex body melted gauntlets into cones and fused pauldrons into the
torso, and the same reference at 25 000 brought the face back. Judge the
register before blaming the image.

Flags land after the preset: `--seed N`, `--verts N`, `--resolution 512`,
`--texture 512`, `--source TEXT`, `--prompt TEXT`, `--created-by human|agent:<name>`.
**Pass `--created-by`**: the record's default is `unknown`. 1536³ is refused
(does not fit 24 GB).

Read the log in order:

| Log line | Healthy | Not |
|---|---|---|
| `mesh: keyed background, subject covers 24%` | 5–95 % (a T-pose on a square canvas is 20–30 %) | refused before the GPU: `the image border is not a flat background (median RGB …, 90th-percentile spread N > 24) — cannot key alpha` or `alpha keying kept N% of the image as subject …` → step 0, background |
| `mesh: loading microsoft/TRELLIS.2-4B` | once, ~seconds from cache | minutes = weights downloading (doctor said `partial`); a DINOv3 401 = gate not accepted |
| `mesh: running 1024_cascade seed 42 (attn flash_attn)` | `1024_cascade` is the preset's pipeline (`--resolution 512` prints `512`); `flash_attn` or `sdpa` both fine | an OOM here = the card was not free (`just gpu` first) |
| `mesh: baking — decimating to 25000 vertices, 1024² texture, remesh` | the preset's numbers; `remesh` always | — |
| `mesh: inner wrote …/out/lifts/torv_warden.glb — 20194 verts, 23818 tris` | tris near 2× the *mesh* vertices that survived decimation (a body at 25 000 comes back ~24k tris); the verts printed are **after** the UV unwrap split the seams, so they sit close to the tri count, not half of it | tris far under ~20k = the surface came back open or in pieces; look at it, then another seed |
| `torv_warden.glb is self-contained — 2804 KiB, 1 node(s), 1 mesh(es), 2 embedded image(s), 0 skin(s); asset.generator = https://github.com/mikedh/trimesh; mesh names = geometry_0` | the file check; `2` images is the base colour plus the PBR map the rig step drops | a failure here is TRELLIS's export, not yours |
| `record   …/assets-src/refs/characters/<name>.lift.json` / `output   …/out/lifts/<name>.glb` / `elapsed  86.9 s` | the summary `forge gen` prints; **about 90 s** at 1024³ / 25 000 on a 4090 with the weights cached | exit 3 `missing_backend` with the install line as hint; exit 4 `input_rejected`; exit 5 `backend_failed` with a log tail |

Between `loading` and `inner wrote` the backend's own progress bars
(`xatlas: Building output meshes …`) fill the log; nothing in them is
yours to read.

The record beside the PNG carries the image sha256, the model and commit,
every knob, the output sha256 and `texture_baker: "nvdiffrast (NVIDIA
Source Code License, non-commercial)"`. Keep that field; it is a licence
fact. **The last run's record is the committed one** — when sweeping
seeds, finish on the seed you keep.

Seed sweep: `just character <name> --seed 7`, then `--seed 1234`. Seeds
differ in whether the rear of a skull closes, whether a strut survives,
whether a back panel is clean. Seed 42 once lifted a front view into a
mask that every downstream gate accepted; 7 and 1234 closed it.

## Step 2 — look: `just views out/lifts/<name>.glb`, then `Read out/views/<name>.png`

Prints a stderr `note: under out/, so culling is off — a face's inside
showing means the surface is missing`, then `7 cells, 384x512 each`,
`adapter: NVIDIA GeForce RTX 4090`, `bounds:  1.00 x 0.93 x 0.25 m, lowest y
-0.465`, and `views: …/out/views/<name>.png (1542x1052)`, around Bevy's own
`INFO` chatter. A raw lift is still in TRELLIS's unit cube, so the bounds
and the header's `1.00 X 0.93 X 0.25 M` are not metres yet (the span is the
1.00: a T-pose is wider than it is tall).

The sheet: header `<NAME>.GLB  7 VIEWS  CULL OFF  W X H X D M`; top row
`FRONT` `BACK` `LEFT` `RIGHT`; bottom row `HEAD FRONT` `HEAD BACK`
`HEAD BACK TOP` (the last cell is empty).

**Orientation, as this build renders it.** `FRONT` is the file's +Z side
— the contract's front. The profile's rest pose faces **+Z** (toes at
z = +0.16), and a lift of a front-view reference comes out facing +Z — so
a correctly facing lift shows its **face in `FRONT` and `HEAD FRONT`**
and its back in `BACK`; `HEAD BACK` is the rear of the skull, `HEAD BACK
TOP` the rear of the skull and the crown from above; `LEFT`/`RIGHT` are
the subject's own left (+X) and right (−X). The shipped sample
`vex_runner` renders exactly this way. If the face is in `BACK`, the lift
faces the wrong way: rig it with `--yaw-deg 180`.

Judge, in this order:

- **Rear skull closed?** In `HEAD BACK` and `HEAD BACK TOP` you must see
  scalp, hair or a helmet — not the inside of the face. With culling off a
  missing rear surface shows as the face's inside: dark, the features
  inverted like a mask seen from behind, the skull's outline reading as a
  rim rather than a dome.
  `LEFT`/`RIGHT` confirm the head has depth. Hollow → step 1 with another
  seed, look again. The fix is never a patch in Blender.
- **T-pose intact after the lift** — arms straight, nothing fused to the
  torso, one hand per side.
- **Nothing that is not the character** — a puddle under the feet (a ground
  shadow lifted), a floating speck the size of a hand (dust the rig step
  drops if it is under 2.5 cm; bigger than that, it ships).
- **The register held** — gauntlets have fingers or a mitt, not a cone;
  pauldrons are separate from the torso.

Read every tile; one front view underdetermines a shape, which is why
there are seven.

## Step 3 — rig: `just rig-mesh <name> [--stature 1.80] [--yaw-deg 180] [--budget 60000]`

Runs `forge gen rig out/lifts/<name>.glb --out assets-src/blender/<name>.blend
--record assets-src/blender/<name>.rig.json --name <name>` in headless
Blender: opens the profile's `rig.blend` (the armature *is* the contract
source), imports the lift as one `Body`, normalizes (yaw, stature, feet at
Z = 0, centred on the hips), gates the pose, cleans, binds with bone heat,
rescues what bone heat left weightless, mattes, packs the texture, saves.
Add `--created-by` here too; `--prompt TEXT` records the reference's prompt
as an input.

- `--stature` is the height the top of the mesh is fitted to (profile
  default 1.80 m; rig check accepts 1.4–2.2). A crown or hood that is a
  tenth of the figure scales the body down under it and drops the shoulders
  below the skeleton's — fit then fails on arm *height*. A taller stature
  buys that back only while the span check still passes. **When the two
  checks pull against each other** (2.0 fails on span, 1.85 on height) the
  reference's proportions are wrong — the "taller, six heads, shoulders
  higher" edit — and the stature then comes down.
- `--budget` is the triangle ceiling before a decimate kicks in; 60 000
  keeps a 25k-vertex lift intact. Lower it only to cap a lift that came
  back heavier; never raise it by reflex.

The whole step is seconds (`elapsed  3.2 s` on a 23k-tri body), not
minutes. The first line is Blender's `Read blend: "…"` naming the
**project's** profile directory, `assets-src/rigs/humanoid/rig.blend` —
at the toolkit root the project's profile *is* `rigs/humanoid`, so the
two paths are the same directory there (see known limits); then glTF's
`INFO: glTF import finished`, then the `rig:` lines. Captured on `torv_warden`; the
dust, decimate and bone-heat lines did not fire on that body and are
quoted from `python/forge_gen/blender/rig.py`, the refusals likewise:

| Log line | Healthy | Not |
|---|---|---|
| `rig: fit — reach 1.35 of wrist span (wrist x 0.719 m, mesh half-span 0.969 m), arm tips at z 1.407 vs wrist z 1.480 m` | reach `0.80 ≤ R ≤ 1.45` and `abs(tips − wrist) ≤ 0.15`; 1.35 is a body with oversized fists past the wrist joint, and the gate makes room for exactly that | exit 4: `forge: input_rejected: not a T-pose: the mesh spans 1.52x the skeleton's wrist reach (the gate is 0.80–1.45) — arms are not straight out to the sides …` or `… the widest geometry sits at z 1.20 m but the wrists rest at 1.40 m (more than 0.15 m apart) — arms are not horizontal. The rest pose is frozen, so fix the reference image (arms straight out, horizontal) and regenerate the mesh — do not bend weights around it.` → step 0, then 1–3 again. Never the weights, never the thresholds. |
| `rig: dropped N dust island(s) of [sizes] face(s), each under 2.5 cm across` | **absent** when nothing was dust (the record then says `dust_islands_dropped: 0`); a few islands, each tiny (single digits to low tens of faces) | hundreds of faces in the list = a real part was dropped; it was under 2.5 cm across *after* scaling to stature, so the lift is in pieces — look at the raw sheet, then another seed or more verts |
| `rig: decimated T -> A tris for the 60000 budget` | absent for a 25k lift | present = the lift came back heavier than the register; fine if the sheet still reads |
| `rig: bone heat lost N of T verts — weighting a voxel-remeshed proxy` then `rig: proxy has P verts, Q weightless after bone heat` | **absent**: the pair prints only when the first bind left more than 20 % of the mesh weightless and the proxy had to be tried | the proxy is the last rung: exit 5 `forge: backend_failed: even the voxel-remeshed proxy left N of T vertices weightless (more than 20%) — the surface is broken; regenerate the mesh (another seed, or a higher decimation target at the lift)` → step 1 |
| `rig: 793 vert(s) in detached shells re-weighted from the surface they sit on, overriding bone heat` | any count: a plated body has dozens of shells and ~2k verts in them, a cloth body a handful, this warden 793 in 11. Each island under 2 % of the mesh — a lamp, a buckle, a plate — rides the surface it sits on as one rigid piece, nearest-first, so a lamp on a pad borrows from the pad and not the thumb. | — (a large count on a plated body is the asset's shape, not a defect) |
| `rig: rescued 793 weightless vert(s) from the nearest weighted vertex — 0 sliver(s) smoothed, 11 detached shell(s) kept rigid` | the same count as the line above; well under a fifth of the mesh | — |
| `Info: 16105 vertex weights limited` / `Info: Saved as "torv_warden.blend"` | Blender's own: the four-influence cap, then the save | — |
| `rig: packed N image(s)` | **absent** on a lift: a glb's images arrive packed and the line prints only when something had to be packed; the export gate is what refuses a missing or unpacked image | — |
| `rig: 11669 verts, 23597 tris, 793 vert(s) rescued from the nearest weighted vertex, saved …/assets-src/blender/torv_warden.blend` then `rig: next — forge-gen export …/assets-src/blender/torv_warden.blend --out <glb> --record <json>` | present; tris under the budget; the verts are Blender's (seams merged: 20 194 in the lift → 11 669 here) | — |
| `record   …/assets-src/blender/<name>.rig.json` / `output   …/assets-src/blender/<name>.blend` / `elapsed  3.2 s` | the summary | — |

Finger leaves collect little or no weight on a mitt-resolution mesh. That
is the register, not a defect: the contract wants every bone present and
every vertex weighted to *some* bone, not every bone deforming.

## Step 4 — promote: `just promote-mesh <name> [--overwrite] [--created-by …] [--tag …] [--note …]`

Three commands in the order the gates have to run, about two seconds in
all; any `FAIL` or non-zero exit is a stop (the one `WARN` below is not —
the recipe carries on, and you read the result as partial).

1. `forge gen export assets-src/blender/<name>.blend --out out/export/<name>.glb --record out/export/<name>.export.json`
   — the export gate. Every check runs before a byte is written and all
   failures are listed together, in Blender's vocabulary: one armature named
   `Armature`, every contract bone under its contract parent, rest pose
   within 0.1 mm of the profile's `rig.blend`, extra bones only as leaves,
   meshes skinned, untransformed, textured, every image packed. Healthy:
   ```
   export: 1 mesh object(s), 1 material(s)
   export: torv_warden.blend passed every pre-export check
   export: wrote …/out/export/torv_warden.glb
   export: torv_warden.glb is self-contained — 2555 KiB, 57 node(s), 1 mesh(es), 1 embedded image(s), 1 skin(s); asset.generator = Khronos glTF Blender I/O v5.2.39; mesh names = Body
   record   …/out/export/torv_warden.export.json
   output   …/out/export/torv_warden.glb
   elapsed  1.7 s
   ```
   (57 nodes = 55 bones + the armature + the body; `1 skin(s)`.) A refusal
   leaves no half-right `.glb` behind. Flags you pass to `promote-mesh` do
   **not** reach this step, so the export record says `created_by: unknown`
   — known, and harmless: the body's sidecar carries who promoted.
2. `forge rig check out/export/<name>.glb` — the hierarchy as the engine
   will bind it. Expected, every line `ok:`:
   ```
   subject:   out/export/torv_warden.glb
   profile:   humanoid v1
   reference: clips/walk.glb
   ok:   animation root is named Armature
   ok:   armature transform is identity
   ok:   Hips found at depth 2, directly under the animation root
   ok:   all 55 contract bones present at contract depth
   ok:   rest rotations match the contract
   ok:   no unknown bones between Hips and the leaves
   ok:   skin present, 19717 weighted vertices
   ok:   height 1.80 m, within 1.4-2.2 m
   ok:   feet at y=0.000 m
   ok:   reference clip: 27 bone(s) driven, 31 at rest, 0 orphaned curve(s)
   10 finding(s) passed, 0 failed, 0 note(s), 0 warning(s)
   ```
   `note: extra leaf bone X` is allowed and reported. **In a library with
   no `walk` clip yet** (every fresh `forge init` project) the third line
   is `reference: none in the library`, the last finding is `WARN: no
   reference clip 'walk' in the library; walk binding not checked — promote
   it, or bind this mesh in the studio to see what moves`, the tally is `9
   finding(s) passed, 0 failed, 0 note(s), 1 warning(s)` and the exit is 0:
   binding was *not* checked, the promote still runs, and step 5 proves
   the binding once a clip exists (`forge-clip`, or promote the toolkit's
   sample take as `walk`). A `FAIL:` here after a clean export gate is a
   new bug, not a flag to add.
3. `forge promote body out/export/<name>.glb <name> --blend … --lift-record
   assets-src/refs/characters/<name>.lift.json --rig-record
   assets-src/blender/<name>.rig.json --export-record
   out/export/<name>.export.json` plus your flags. Prints
   `ingested torv_warden.glb: 19717 verts, 23597 tris, 55 bones, 1.80 m tall; source
   assets-src/blender/torv_warden.blend`, `manifest refreshed`, then
   `-> bodies/torv_warden.glb (recorded, trellis2, created 2026-08-23 by agent:e2e)`
   (the verts are the glTF's seam-split count, so they differ from the
   `rig:` line's 11 669; `by human` unless you passed `--created-by`).
   Refused with exit 2 when the name exists: `<name> already exists as
   bodies/<name>.glb; pass overwrite to replace it` → `--overwrite`, only
   when replacing is the intent; the replaced asset's tags survive unless
   restated. Every record is checked for kind — a lift record handed to
   `--rig-record` is refused, not filed. A `--note` or `--prompt` with
   spaces does not survive the recipe's `*flags` (the shell re-splits it);
   run this promote by hand for those.

Writes `assets/bodies/<name>.glb` + `<name>.json` (schema 1 sidecar:
integrity of the glb and the blend, the three records, `rig: humanoid`)
and refreshes `assets/library.json`.

## Step 5 — verify

- `just views <name>` — the shipped file by library name, culling **on**
  (how an engine draws it): `bounds:  1.94 x 1.80 x 0.48 m, lowest y
  -0.000` — metres now, 1.80 tall, feet on the floor. The skull check
  again, post-cleanup; add `--cull-off` to see through again.
- `just check-mesh assets/bodies/<name>.glb --out out/sheets/<name>.png` —
  the same findings, plus the body playing the reference walk. The report
  must say `reference clip: 27 bone(s) driven`; `0 bone(s) driven` is a
  bone-name mismatch and the body would stand in T-pose in the game. The
  terminal also prints the sheet's summary (`8 cells, 384x512 each`,
  `clip:    2.600s, 8 sampled`, `times:   0.00 0.37 …`, `bones:   27
  driven, 31 at rest, 0 orphaned`, `sheet: out/sheets/<name>.png`).
  `Read` the sheet: header `SUBJECT.GLB / REFERENCE.GLB  2.60S  27 BONES
  DRIVEN` (the check stages copies under those names), eight
  `THREE_QUARTER` cells labelled `#N T.TTS THREE_QUARTER` across the walk
  — look for candy-wrapper elbows and knees, a shoulder plate that drifts
  off the arm (a detached shell weighted to the wrong surface — step 1
  with another seed, not a weight edit), feet through the floor, a rigid
  cape. **With no `walk` in the library** the terminal says `rendered at
  rest: no reference clip in the library to play` and the PNG is a
  seven-view rest sheet (`SUBJECT.GLB  7 VIEWS  CULL ON …`) — the views
  again, not the binding check; ship a `walk` first. `just sheet walk
  --body <name> --views all --head-row` gives every band and the face row
  (in a project whose `forge.toml` names no `[studio] stage_body` it says
  `warning: no [studio] stage_body in forge.toml — posing on the first
  body, <name>`). `forge-review` has the full reading order.
- `just studio --model <name>` — for the user's eyes, orbitable, clips on
  the transport. Their verdict outranks every sheet: "still hollow" means
  still hollow; go back to the seed or the reference.
- `just check-bodies` (`== assets/bodies/<name>.glb`, the findings, then
  `1 body(ies) conform to the rig profile`), `just manifest-check`
  (`1 checked, ok` / `assets/library.json matches a rebuild of the
  library`), `just verify` (`N checked, ok`: the ledger row, hashes,
  drift), `just audit`. Those are the project's gates; `just ci` is the
  toolkit's own gate, run from the checkout — its dev recipes always act
  on the checkout, never on your project.

## Step 6 — commit set (only when the user asks)

`assets-src/refs/characters/<name>.png` + `<name>.lift.json` (+ `<name>.txt`
if one exists); the `assets-src/SOURCES.md` row;
`assets-src/blender/<name>.blend` + `<name>.rig.json`;
`assets/bodies/<name>.glb` + `<name>.json`; `assets/library.json`.
**Never** `*.blend1`, never `out/` (the lift, the export and the sheets are
derived and gitignored), never the PNG without its row.

## Known limits (say them, don't fight them)

- Face texel density: a full-body reference at 1024² gives a soft face.
  A closer reference is a different character, not a fix.
- Emissive, gloss, metal: the register strips everything but the base
  colour and forces `metallic 0.0 / roughness 0.9`, double-sided. A visor
  glow ships flat matte; an emissive channel is an offered follow-up.
- One mesh wearing its clothes as paint. No wardrobe, no parts: an outfit
  change is a new reference and a new lift. Equipment (a sword, a pistol)
  is a prop on a socket (`forge-prop`) and needs nothing from the body.
- Finger bones carry ~no weight on a mitt mesh — per contract.
- `forge views` labels `FRONT` as the file's +Z side — the profile's
  front, so on a promoted body `FRONT` is the face and `LEFT`/`RIGHT` are
  the subject's own. A raw lift is not yet normalized: its labels name
  the file's axes, so read those tiles as described in step 2 and turn a
  wrong-facing lift with `--yaw-deg 180` at step 3.
- The texture bake is nvdiffrast, non-commercial, named in every lift
  record; a replacement baker is a follow-up, not a flag.
- Bodies claim integrity, not reproduction: Blender's glTF export is not
  byte-stable, so `just rebake` skips them loudly and `just audit` holds
  them to the hash that was approved (`note <name>   body skipped:
  integrity-only, no recipe`, then `1/1 bodies conform to the contract`).
- `forge gen rig` and `forge gen export` open the project's profile
  (`Read blend: "…/assets-src/rigs/humanoid/rig.blend"` is the first log
  line; at the toolkit root that is `rigs/humanoid`) — the same profile
  `forge rig check` holds the body to. `--profile DIR` or
  `$FORGE_RIG_PROFILE` override it.
