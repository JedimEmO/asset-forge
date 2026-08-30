---
name: forge-prop
description: Ship a static prop from a reference PNG — judge the image, TRELLIS.2 lift at the prop register, look at the raw mesh from four angles, normalize in headless Blender (metres; floor, ceiling or grip at the origin; matte), promote as a model with its records. Use when the user wants a decor piece, a fixture or a held weapon generated from a reference image rather than modelled, wants one regenerated (another seed, a different size or placement), or reports a prop that is the wrong way up, the wrong size or sits wrong in a hand.
---

# Prop: PNG → lift → look → normalize + promote

Props are the cheap half of the pipeline: no rig, one Blender step, one
promote door. Three `just` recipes with a look between them. Every name
below is checked against `just --list` and `forge --help`, and every log
line quoted was captured on a real run (2026-08-23, a candelabra at the
prop preset, RTX 4090).

From a project made by `forge init` (not the toolkit checkout) the same
recipes run as `just --justfile <toolkit>/justfile --working-directory .
<recipe>` with `FORGE_HOME=<toolkit>` exported so `forge gen` finds
`python/forge_gen`; `forge` itself walks up to the project's `forge.toml`.

## Prerequisites (check, don't assume)

| Check | Command | Healthy |
|---|---|---|
| The PNG is at `assets-src/refs/props/<name>.png`, `<name>` is `[a-z0-9_]+` | `ls assets-src/refs/props/` | present, name legal |
| The name is free | `just catalog --kind model` | `<name>` absent from the `name` column |
| Backends and tools | `just doctor` | `trellis2   ok`, `blender    ok` (doctor exits 1 while any backend is not `ok`; only those two matter here) |
| nvdiffrast | same table | `warn notice: nvdiffrast is non-commercial: …` is expected; the lift record names it. Decide before lifting if the project is commercial. |
| The GPU is free | `just gpu` | `holding   nobody` and `largest   trellis2 needs 22 GB (22528 MiB): fits`. `does NOT fit — stop what holds the card before a generate` (exit 1) names the holder on the `holding   pid N … GB` line: the ComfyUI host → `forge gpu --free` or `systemctl --user stop forge-comfy`; a studio window → close it; else by PID. One generate at a time. |
| The project's style doc, if any | `designs/style-guide-template.md` is the template | the PNG is already in that register; nothing here restyles |

## Step 0 — judge the PNG before any GPU time

`Read` the PNG. TRELLIS lifts what it can see; the gates measure geometry.

| Seen | Consequence downstream | Fix (in the image) |
|---|---|---|
| Flat-on view (one face of a crate, a barrel end-on) | TRELLIS gets one face; the far side comes back thin or absent | three-quarter view, slightly from above, so three faces show |
| Not alone — a second object, a hand, a floor it sits on | the extra lifts with it, or the keyer keeps the floor | the thing alone, centred |
| Gradient background, vignette, ground shadow | the keyer flood-fills from the border: `the image border is not a flat background` refuses, a soft shadow lifts as a skirt | plain flat light background, no shadow |
| Cropped edges, text, a label | missing geometry, or text baked as geometry | whole object in frame, nothing written on it |
| Thin spokes, wires, a lattice, a thin pole | read as cones, fused, or dropped | thick shapes; the register is chunky |
| Subject tiny or filling the frame | `alpha keying kept N% of the image as subject` outside 5–95 % refuses | a third to two thirds of the frame |
| Transparent or glowing parts | ship as flat matte paint | fine, but say so to the user |

Then the ledger: add a row to `assets-src/SOURCES.md` under
`| File | Origin | For | Date |` — the first cell must contain the file
name. Without it `just verify` fails:
`FAIL <name>.png   assets-src/refs/props/<name>.png has no row in SOURCES.md — a reference image claims a ledger row, or it cannot be accounted for`.
Optional `assets-src/refs/props/<name>.txt`: its first paragraph becomes the
record's `prompt`; `--prompt TEXT` overrides, `--source TEXT` says where the
image came from.

## Step 1 — lift: `just prop <name> [flags]`

Runs `forge gen mesh assets-src/refs/props/<name>.png --preset prop --out
out/lifts/<name>.glb --record assets-src/refs/props/<name>.lift.json`. The
prop register is **1024³, 6 000 vertices, 1024² texture, seed 42**: 6 000
vertices comes back as roughly 5–7k triangles, under the 12 000-triangle
cap step 3 enforces. The 512³ / 900-vertex / 512² table it replaced sheared
a crate's rivets into smears. Flags land on top: `--seed N`, `--verts N`,
`--resolution 512`, `--texture 512`, `--source`, `--prompt`,
`--created-by human|agent:<name>` (**pass it**; the default is `unknown`).

| Log line | Healthy | Not |
|---|---|---|
| `mesh: keyed background, subject covers 13%` | 5–95 % (the candelabra, thin on a square canvas, is 13 %; a crate is nearer 30 %) | refused before the GPU: `… not a flat background (median RGB …, 90th-percentile spread N > 24) — cannot key alpha` or `alpha keying kept N% …` → step 0 |
| `mesh: loading microsoft/TRELLIS.2-4B` | seconds from cache | minutes = downloading (`partial` in doctor) |
| `mesh: running 1024_cascade seed 42 (attn flash_attn)` | `1024_cascade` is the preset's pipeline; `--resolution 512` prints `512` | OOM = the card was not free |
| `mesh: baking — decimating to 6000 vertices, 1024² texture, remesh` | the preset's numbers | — |
| `mesh: inner wrote …/out/lifts/candelabra.glb — 4895 verts, 5789 tris` | tris near 2× the *decimation target's* share that survived (a thin prop comes back under 6 000: 5 789 here); the verts figure is **after** the UV unwrap split seams, so it runs close to the tri count, not half of it | tris far under = the surface came back open or in pieces; look, then another seed |
| `candelabra.glb is self-contained — 2292 KiB, 1 node(s), 1 mesh(es), 2 embedded image(s), 0 skin(s); asset.generator = https://github.com/mikedh/trimesh; mesh names = geometry_0` | the file check; `2` images is the base colour plus the PBR map step 3 drops | a failure here is TRELLIS's export, not yours |
| `record   …/assets-src/refs/props/<name>.lift.json` / `output   …/out/lifts/<name>.glb` / `elapsed  101.3 s` | the `forge gen` summary; ~100 s at 1024³ on a 4090 with the weights cached | exit 3 `missing_backend` + hint; 4 `input_rejected`; 5 `backend_failed` + log tail |

Between `loading` and `inner wrote` the backend's own progress bars
(`xatlas: Building output meshes …`, `Gathering results from xatlas`) fill
the log; nothing in them is yours to read. The record carries the image
sha256, model, commit, every knob, the output sha256 and
`texture_baker: "nvdiffrast (NVIDIA Source Code License, non-commercial)"`.
The last run's record is the committed one — finish on the seed you keep.

## Step 2 — look: `just views out/lifts/<name>.glb --no-head`, then `Read out/views/<name>.png`

Prints a stderr `note: under out/, so culling is off — a face's inside
showing means the surface is missing`, then `4 cells, 384x512 each`,
`adapter: NVIDIA GeForce RTX 4090`, `bounds:  0.53 x 0.99 x 0.36 m, lowest y
-0.490`, and `views: …/out/views/<name>.png (1542x538)`, around Bevy's own
`INFO` chatter. The sheet: header `CANDELABRA.GLB  4 VIEWS  CULL OFF  0.53 X
0.99 X 0.36 M` (a raw lift is still in TRELLIS's unit cube, tallest
dimension ≈ 1 and `lowest y` near −0.5 — not metres yet), tiles `FRONT`
`BACK` `LEFT` `RIGHT`.

**The imaged face lands on the `FRONT` tile.** The `FRONT` tile shows the
file's +Z side and a lift comes out with its pictured side toward +Z — so
a terminal's screen, a crate's stencilled face, a counter's front show in
`FRONT`, and `BACK` is the far side TRELLIS had to invent. (A raw lift is
not yet normalized, so its labels name the file's axes, not the content's
facing: read the tiles by what they show, and a lift that came out facing
elsewhere is turned with `--yaw-deg` at step 3.) `BACK` is the tile to
judge:

- **Far side present?** `BACK` is what TRELLIS invented: expect a plausible
  but smeared surface (the back of a chair lifted from its front shows a
  mirrored blur of the seat's paint — that is normal). With culling off, a
  *missing* back shows as the inside of the imaged face instead: unlit,
  dark, the front's texture seen from behind, with the silhouette's edge
  reading as a rim rather than a surface. Thin or absent →
  `just prop <name> --seed 7`, look again.
- **Nothing that is not the prop** — a skirt of keyed shadow at the base, a
  speck floating beside it.
- **For a weapon, read the axes now.** `--long-axis` names the lift's
  hilt→tip axis in Blender's frame (what the importer sees): up in the
  `FRONT` tile is `z`; toward the `LEFT` tile's camera is `x` (toward
  `RIGHT`'s is `-x`); toward the `FRONT` camera is `-y` (toward `BACK`'s is
  `y`). A sword lying tip-to-the-right in the `FRONT` tile is
  `--long-axis x`; a pistol pointing at the `FRONT` camera is
  `--long-axis -y`. Note which way the edge or the sights face too; that
  is the roll. Step 3 needs both.

## Step 3 — normalize and file: `just prop-import <name> --height M | --length M [placement] [flags]`

Runs `forge gen prop out/lifts/<name>.glb --out out/props/<name>.glb
--record out/props/<name>.prop.json <flags>` in headless Blender — joins,
merges doubles, drops loose geometry, yaws, scales, places, mattes, packs,
exports, self-checks — and then, without a pause, `forge promote model
out/props/<name>.glb <name> --lift-record assets-src/refs/props/<name>.lift.json
--prop-record out/props/<name>.prop.json`. Every flag you pass goes to
`gen prop` only; the promote runs with none (see below for `--overwrite`).

Exactly one size: `--height M` (the Blender +Z extent, the usual) or
`--length M` (the longest extent — a sword, a plank). Then placement:

| Prop | Flag | Origin lands at |
|---|---|---|
| floor-standing decor, a fixture, a crate | *(none)* | the lowest vertex, centred in X and Y — a level drops it on the floor |
| hangs from a ceiling (cobweb, roots, a lamp) | `--hang` | the highest vertex |
| carried or socketed, no grip point | `--held` | the bounding-box centre |
| a weapon for a hand socket | `--grip M --long-axis ±x\|±y\|±z [--roll-deg D] [--socket hand_r]` | the point `M` metres up the hilt→tip axis, with that axis turned onto the authoring long axis (glTF **+Y**) and the edge or sights rolled to the authoring front (glTF **−Z**) — the frame `rigs/humanoid/sockets.json` expects (`hand_r`, `hand_l`, `back`, `hip_l`, `head`); `--socket` checks the name and writes it into the record |

Other flags: `--yaw-deg D` (turn about +Z first), `--budget N` (the
triangle ceiling; default the profile's 12 000 — **never raise it by
reflex**), `--created-by`.

Read the log. Blender's own lines (`INFO: glTF import finished in 0.02s`,
`INFO Draco is available …`, `Finished glTF 2.0 export`, `Blender 5.2.0 LTS
(hash …)`) interleave with the `prop:` ones; only the `prop:` lines and the
summary are the tool's. The `grip:` and `input_rejected` lines are from
`python/forge_gen/blender/prop.py` (a floor prop prints neither); the rest
were captured:

| Log line | Healthy | Not |
|---|---|---|
| `prop: merged/dropped 1995 verts in cleanup` | **about a third to half of the lift's vertex count** — the lift's count is after the UV unwrap split every seam, and this merge puts the seams back (4 895 → 2 900 here). A large number is normal. | — (pieces show on the raw sheet, not here; `nothing is left of the mesh after cleanup` is the only refusal in this step) |
| `prop: grip: lift z -> +Z (glTF +Y), rolled 0.0 deg; the front should face +Y (glTF -Z)` | only with `--grip`; says what it turned | — |
| refused: `forge: input_rejected: N tris exceeds the 12000 budget — regenerate with fewer verts (a lower decimation target at the lift); do not raise the budget by reflex` (exit 4) | — | `just prop <name> --verts 4000`, then steps 2–3 again |
| `prop: packed N image(s)` | **absent** on a lift: a glb's images arrive packed and the line prints only when something had to be packed | — |
| `prop: candelabra.glb is self-contained — 1626 KiB, 1 node(s), 1 mesh(es), 1 embedded image(s), 0 skin(s); asset.generator = Khronos glTF Blender I/O v5.2.39; mesh names = Prop` | `1 embedded image(s)` — the texture came through | `0 embedded image(s)` = the lift had no texture; a failure here is a Blender export bug, not yours |
| `prop: 2900 verts, 5782 tris, 1 material(s), 1 embedded image(s)` | one material, one image; the verts are Blender's count (seams merged) | — |
| `prop: bounds x -0.321..+0.321  y -0.219..+0.219  z +0.000..+1.200 m` | Blender axes, metres. Floor: `z` starts at `+0.000` and ends at `--height`. `--hang`: `z` ends at `+0.000`. Grip: `z` straddles zero by the grip height (a 0.16 m grip reads `z -0.160..+0.860`), `x`/`y` centred | a grip that missed is a number here, not a surprise: re-run with the corrected `--grip`/`--long-axis`/`--roll-deg` |
| `record   …/out/props/<name>.prop.json` / `output   …/out/props/<name>.glb` / `elapsed  5.5 s` | the `gen prop` summary; seconds | — |
| `ingested candelabra.glb: 4875 verts, 5782 tris, 0.64 × 1.20 × 0.44 m` then `manifest refreshed` then `-> models/candelabra.glb (recorded, trellis2, created 2026-08-23 by human)` | the promote; `recorded`; the verts are the glTF's seam-split count again, so they differ from the `prop:` line and that is fine. `by human` even when you passed `--created-by` to the recipe — that flag went to `gen prop` (its record says `agent:<name>`); the promote ran with none | `no assets-src/refs/props/<name>.lift.json — the model will say reconstructed, not recorded` = the lift record is missing; step 1 wrote it, so find out why before shipping |

Refused with exit 2 when the name exists: `<name> already exists as
models/<name>.glb; pass overwrite to replace it`. `prop-import` cannot
forward that flag; when replacing is the intent, run the promote by hand
exactly as the recipe does, plus the flag:

```sh
forge promote model out/props/<name>.glb <name> \
    --lift-record assets-src/refs/props/<name>.lift.json \
    --prop-record out/props/<name>.prop.json --overwrite
```

The same by-hand promote takes `--tag grip`, `--note …`, `--created-by
agent:<name>`. A replaced asset's tags survive unless restated. **A value
with spaces (`--note "two words"`, `--prompt …`) does not survive a `just`
recipe's `*flags`** — the shell re-splits it (`Syntax error: Unterminated
quoted string` when it has an apostrophe, silently wrong otherwise); pass
those to `forge promote model` directly.

Material: the TRELLIS base colour only, `metallic 0.0 / roughness 0.9`,
double-sided; the normal and metallic-roughness maps are disconnected on
purpose (the library paints its lighting into the diffuse). Writes
`assets/models/<name>.glb` + `<name>.json` (integrity of the glb, both
records, `bounds_m`) and refreshes `assets/library.json`.

## Step 4 — verify

- `just views <name> --no-head` — the shipped file by library name, culling
  **on**: `bounds:  0.64 x 1.20 x 0.44 m, lowest y 0.000`, header
  `CANDELABRA.GLB  4 VIEWS  CULL ON  0.64 X 1.20 X 0.44 M`. Right way up,
  right size (metres now), origin where the placement said (`lowest y
  0.000` for a floor prop; negative by the grip height for a weapon). A
  shipped prop's `FRONT` tile is the +Z side, `LEFT`/`RIGHT` the subject's
  own left (+X) and right (−X). A weapon stands hilt-down along +Y with
  its edge on the authoring front (glTF −Z, the `BACK` camera's side):
  edge-on in `FRONT`/`BACK`, the flat in `LEFT`/`RIGHT`. The flat in
  `FRONT` means the roll is 90° off.
- `just studio --model <name>` — for the user; orbitable, beside the stage
  body for scale. Their eye outranks the sheet. (For a model the terminal
  prints one note — `static model: no rig, nothing to hold to the
  contract` — instead of rig findings; that is a model, not a defect.)
- `just manifest-check` (`1 checked, ok` / `assets/library.json matches a
  rebuild of the library`), `just verify` (`N checked, ok`; the ledger
  row), `just audit`. Those three are the project's gates; `just ci` is
  the toolkit's own gate, run from the checkout — its dev recipes always
  act on the checkout, never on your project.

## Step 5 — commit set (only when the user asks)

`assets-src/refs/props/<name>.png` + `<name>.lift.json` (+ `<name>.txt`);
the `assets-src/SOURCES.md` row; `assets/models/<name>.glb` + `<name>.json`;
`assets/library.json`. Never `out/` (the lift and the normalized file are
derived and gitignored), never the PNG without its row.

## Placing it (not part of shipping)

A consumer reads `assets/library.json` → `models[]` (name, path, sha256,
`bounds_m`) and, for a held prop, the socket table in
`rigs/humanoid/sockets.json`: a prop authored grip-at-origin, long axis
+Y, front −Z needs no per-prop correction, and the last centimetre of fit
is the caller's offset at attach time. How a level scatters a floor prop
or hangs a ceiling one is the game's business.

## Known limits (say them, don't fight them)

- One matte material, base colour only. Gloss, metal, glow and the normal
  map are stripped; a glowing screen ships as painted light.
- A prop is a shell. An open barrel has no inside unless the reference
  showed one; a lattice becomes a slab.
- The far side is invented from one view. Sweep seeds, do not sculpt.
- 12 000 triangles is the ceiling and 6 000 vertices the register; a prop
  that needs more is two props.
- `forge views` labels `FRONT` as the file's +Z side — the contract's
  front. On a raw lift (not yet normalized) that is where the pictured
  side usually lands, but the labels name the file's axes, not the
  content's facing: read the tiles as described in step 2, and turn a
  wrong-facing lift with `--yaw-deg` at step 3.
- Models claim integrity, not reproduction (Blender's export is not
  byte-stable); `just audit` holds them to the approved hash (`note
  <name>   model skipped: integrity-only, no recipe`) and `just rebake`
  skips them loudly.
- The texture bake is nvdiffrast, non-commercial, named in the lift record.
- `forge gen prop` reads its budget and matte from the project's rig
  profile (`assets-src/rigs/humanoid/profile.toml`, handed over by `forge
  gen`; `--profile DIR` or `$FORGE_RIG_PROFILE` override it).
