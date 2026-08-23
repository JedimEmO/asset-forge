---
name: forge-clip
description: Ship an animation clip from a prompt — ARDY sweep, the review table and sheet, one take promoted with a stated recipe through the native bake, the strip judged on the real body, the audit. Use when the user wants a new motion, wants one regenerated or retimed, or says a clip reads wrong.
---

# Clip: prompt → sweep → review → promote → strip

Four `just` recipes in a fixed order, with a look between each. A shipped
clip is a pure function of its take, its recipe and the rig profile; `just
audit` rebuilds every one and holds it to the bytes. Nothing here touches
Blender and nothing after the sweep needs the GPU. Every log line quoted
was captured on a real run (2026-08-23, "a person swings a sword overhead
with both hands", 4 s × 4 samples, on a freshly lifted body).

From a project made by `forge init` the recipes run as `just --justfile
<toolkit>/justfile --working-directory . <recipe>` with
`FORGE_HOME=<toolkit>` exported — with one catch in step 1.

## Prerequisites (check, don't assume)

- `just doctor` — the `ardy` row reads `ok`. `partial` names the weight or
  text encoder that is missing; `missing` is not installed → `forge-setup`.
  The other rows do not matter for a clip.
- **The GPU is free.** `just gpu`. ARDY wants ~16 GB; the usual holder is
  the ACE-Step server (`holding pid N 10.4 GB …/backends/acestep/.env/bin/python`),
  which stays resident until `target/debug/forge gen music --stop-server`
  (there is no bare `just music` form — the recipe needs a name and a
  prompt). A studio window with a model loaded on the real adapter holds
  the card too; close it first.
- **A body on the stage.** `just catalog --kind body` lists what there is;
  `forge.toml [studio] stage_body` names the one `sheet`, `bones` and the
  studio pose on, else the first body, else the fixture mannequin
  (`just rig` writes it to `out/fixture/mannequin.glb`). On a real body,
  `just check-mesh assets/bodies/<body>.glb` must say
  `reference clip: 27 bone(s) driven` — with no `walk` in the library yet
  it says `reference: none in the library` and `WARN: no reference clip
  'walk' in the library; walk binding not checked — promote it, or bind
  this mesh in the studio to see what moves` instead (exit 0), and the
  binding is proven by the first `just sheet`. In a project without a
  `[studio] stage_body`, `sheet` and `bones` say `warning: no [studio]
  stage_body in forge.toml — posing on the first body, <name>` and carry
  on.

## Steps

### 1. Sweep — `just sweep "<prompt>" --duration <s> --samples <n> --seeds <n>`

Flags land on `forge gen motion sweep` verbatim. The ones that matter:
`--duration S [S …]` (default 4), `--samples N` per cell (default 8),
`--seeds N [N …]` (default 0; each seed reruns the grid), `--cfg W [W …]`
(ARDY's 2.0), `--label <slug>`, `--created-by agent:<name>` (the record
says `unknown` otherwise — `forge gen` never assumes who asked),
`--no-postprocess` only to see ARDY's raw output without its foot-skate
correction.

One model load covers the whole grid, so **eight takes cost barely more
than one**; ask for `--samples 8` and pick. Ask for **2–4× the length you
need**: the usable window is cut out at promote with trims, the lead-in is
where the model settles, and a 4 s take holds a 1.5 s loop with room to
choose the seam.

Writes `out/sweeps/<seed>-<8 hex of sha256(prompt)>/` (the first `--seeds`
number; a multi-seed sweep shares one directory and the file names carry
the seed). Inside: `<label>__d<dur>_c<cfg>_s<seed>_<k>.npz` one per take
(the label is the first 28 characters of the prompt's slug:
`a_person_swings_a_sword_over__d4_c2_s0_2.npz`), `<take>.take.json` its
record (every knob, the prompt as an input, the file hashed), `sweep.json`
the list in generation order. The hash is why two prompts with the same
slug do not overwrite each other. Nothing under `out/` is a source;
promote copies what ships.

| Log line | Healthy | Not |
|---|---|---|
| `Loaded ARDY-Core-RP-20FPS-Horizon40 @20fps in 43s` | the load, once, ~45 s, after the weight-loading bars and `Setting up text encoder (mode=local)` | exits 3 in ~100 ms with `ardy is not installed` → `forge-setup`; a CUDA OOM → `just gpu`, stop what holds the card |
| `4 takes: 1 prompts x 1 seeds x 1 cfg x 1 durations x 4 samples` | the grid you asked for | a count you did not expect: a `--duration` or `--seeds` list was read as two values |
| `  a_person_swings_a_sword_over... x4  d=4 cfg=2 seed=0  (1s)` | one line per cell; the generation itself is a second | — |
| `wrote 4 takes to …/out/sweeps/0-971fc342` then `record   …/sweep.json`, one `output   …npz` per take, `out_dir  …`, `elapsed  49.1 s` | N = the product above; the whole call is the load plus seconds | — |

**Running from another project** works through the same form for every
recipe here: `just --justfile <toolkit>/justfile --working-directory . sweep …`.
The sweep runs the review itself and ends with `sheet: <dir>/sheet.png
metrics: <dir>/metrics.json`.

**Prompt rules** (the lessons in `designs/decisions.md`, each earned):

- **A grounded activity, not a geometry.** Say what the character is doing
  and to what; never describe the pose. A pose description gives a still.
- **Say how many hands.** "swings a two-handed axe", "fires a pistol in the
  right hand"; an unstated grip comes back as whichever the prior prefers.
- **Weak priors give arm-waves.** A verb the model has not seen much of
  ("parries", "casts") turns into generic arm motion; describe the
  everyday action it resembles.
- **Standing life is not in the model.** An idle must be an activity too:
  "stands guard, shifting weight from foot to foot, looking left and right",
  not "stands still".
- **Wrists barely move.** 27 driven joints, fingers never; do not prompt
  for hand detail, it cannot arrive.

### 2. Review — runs after the sweep; `just review <dir> [flags]` to rerun

`--intent oneshot` for a strike, a death, a jump (drops the loop-only flags
SEAM, SEAMVEL, STOPS, DRIFT and nothing else); `--suggest` prints the trims
that cut the usable clip out of each take, in `promote clip` units;
`--sort foot_skate_mps` (any metric); `--min-speed 0.8` so a locomotion
loop window cannot be won by a standing-still stretch; `--detail` with one
`.npz` for three camera rows, an onion skin and the plots;
`--trim-start/--trim-end` to review a take as if trimmed.

The table, one row per take, header:

```
name duration_s avg_speed_mps start_ratio stop_ratio foot_skate_mps jitter_pct activity_m loop_gap_m drift_deg net_turn_deg left_steps flags
```

`duration_s` is `3.95` for `--duration 4`: 79 frames at 20 fps, the
model's own length, not a trim. With `--suggest` each take gets two
lines after the table, in promote units:

```
  a_person_swings_a_sword_over__d4_c2_s0_2
    oneshot: --trim-start 0.0 --trim-end 2.0   (1.95s kept)
    loop:    --trim-start 0.0 --trim-end 2.05 --loop   (1.9s cycle, seam 0.036m, 0.4m/s, cv 0.88)
```

then `Contact sheet: <dir>/sheet.png`, the `output`/`metrics_json`/`sheet`/
`elapsed` summary, and the recipe's own `sheet: <dir>/sheet.png  metrics:
<dir>/metrics.json`. `flags` names every gate the take trips (`-` is
clean). The gates are
`rigs/humanoid/profile.toml [review]`, nothing is a constant in the code:

| Flag | Fires when | Means |
|---|---|---|
| `SKATE` | `foot_skate_mps` > 0.25 | a planted foot slides; `nan` when no foot ever plants |
| `JITTER` | `jitter_pct` > 32 | high-frequency noise as a share of real per-frame travel |
| `SINKS` | `penetration_m` > 0.05 | the lowest foot goes through the floor |
| `FLOATS` | `lowest_foot_m` > 0.10 | never plants at all |
| `STATIC` | `activity_m` < 0.035 **and** `avg_speed_mps` < 0.35 | nothing happens — a strafe travels and is not static |
| `SEAM` | `loop_gap_m` > 0.12 | pose mismatch across the loop seam (loop intent only) |
| `SEAMVEL` | `loop_vel_gap` > 0.9 m/s | velocity mismatch across the seam (loop only) |
| `DRIFT` | \|`drift_deg`\| > 35 | heading wanders over the clip (loop only) |
| `TURNS` | \|`net_turn_deg`\| > 25 | ends facing somewhere else — select on **net** turn, not cumulative: cumulative rewards a wobble |
| `STOPS` | `peak_speed_mps` > 0.8 and `stop_ratio` < 0.45 | arcs to a halt: an episode, not a loop (loop only) |

A clean row is mechanically sound — necessary, not sufficient. **`Read
<dir>/sheet.png`**: eight keyframes per take as a stick figure under a
bold header (`<take>   3.95s  0.2m/s   travel 0.58m  steps 1   [action
0.00-1.95s]`), a numbers line ending in `clean` (green) or the flags
(red), the prompt in italics. Metrics catch skating; only the sheet
catches "that is a shove, not a strike". The stick figure has no
shoulders, so the final judgement waits for the real body (step 4) — or
put a take on it now for the user: `just studio --take <dir>/<take>.npz`.
**Discard here, keep on the strip**: a take that looks wrong as a stick
figure is not promoted to find out; a take that looks right is promoted
and judged again. (All four sword takes were clean and the sticks raised
both arms; on the body the "overhead" swing reads as a two-handed sweep
at chest height with a lunge — the strip is where that was seen.)

### 3. Promote — `just promote-clip <name> <dir>/<take>.npz --record <dir>/<take>.take.json [knobs]`

The native bake: `forge promote clip`, no Blender. `--record` is the take's
`.take.json` and makes the clip `recorded`; without it the clip is
`reconstructed`, which is less. The take is copied to
`assets-src/takes/<name>.npz` untrimmed so the clip stays re-bakeable after
`out/sweeps/` is swept. The name is `[a-z0-9_]+`; a loop gets `-loop` on the
animation's name *inside* the glb (what an engine binds by), not on the
asset name.

| Knob | What it does (`forge_motion::edit`) |
|---|---|
| `--trim-start S`, `--trim-end S` | seconds cut from the raw take; `--suggest` above prints them |
| `--in-place off\|strip\|detrend` | root X/Z travel: `off` keeps it; `strip` pins the hips — steady locomotion loops the game moves itself; `detrend` removes only the linear drift, keeping the lunge-and-settle — rolls and travelling one-shots. On a roll the difference between the last two is 417 mm |
| `--y-mode off\|strip\|detrend` | root height, same three words; `strip` flattens dips too, so only where every departure from standing height is the game's business (a capsule that jumps itself) |
| `--loop` / `--no-loop` | bake as a loop (the name gets `-loop`, the tail blends to frame 0) / turn an inherited loop off |
| `--loop-blend S` | seconds of tail blended toward frame 0; above zero makes it a loop, zero makes it not one; runs last |
| `--exaggerate X` | scale on each arm joint's swing about its clip-mean pose; 1.0 unchanged; measured after the style knobs |
| `--arm-bend DEG` | constant forearm bend — runner arms |
| `--lean DEG` | forward lean split down Spine/Spine1/Spine2 (0.4/0.35/0.25) so it reads as a lean, not a kink |
| `--shoulder-back DEG` | upper-arm pull-back so the hands ride beside the hips |
| `--retime src:dst,src:dst,…` | piecewise time remap in seconds — compress a slow windup, keep the strike 1:1; `--retime ""` removes an inherited one |
| `--event T:NAME[:sfx:SOUND]` | an authored event at T seconds **on the raw take** (the bake maps it through trim and retime); repeat for more. **Footsteps are derived from the take's contacts and must not be stated** |
| `--clip NAME` | the animation's name inside the glb when it must differ from the asset name |
| `--prompt`, `--tag`, `--note` | catalog text; unstated, the record's own prompt stands and a replaced clip's tags survive |
| `--created-by human\|agent:<name>` | who is promoting; defaults to `human` — say `agent:<name>` when it is you |
| `--overwrite` | replace an existing name; refused otherwise |

Read, in order:

| Log line | Healthy | Not |
|---|---|---|
| `recipe (every knob stated, nothing inherited from the bake):` then `  trim          0.000s off the start, 2.000s off the end` / `  in_place      detrend` / `  y_mode        off` / `  loop          no` (or `yes, 0.100s blend`) / `  exaggerate    1.000` / `  arm_bend      0.00 deg` / `  lean          0.00 deg` / `  shoulder_back 0.00 deg` | every knob you meant, and the ones you did not state at identity | a knob you did not state with a value: the name exists and its shipped recipe was the base (below) |
| `baked sword_overhead.glb: 40 frames, 1.95s, 3 event(s)` | frames = kept seconds × 20; events = contacts found + yours (here two derived footsteps and the one `--event 0.55:swing`) | the take's full length when you trimmed: a trim that leaves < 2 frames is ignored and the take comes through whole |
| `manifest refreshed` then `-> clips/sword_overhead.glb (recorded, ardy, created 2026-08-23 by agent:e2e)` | `recorded` | `reconstructed` = no `--record` was passed |

A `--note` or `--prompt` with spaces does not survive the recipe's
`*flags` — the shell re-splits it (`Syntax error: Unterminated quoted
string` when it holds an apostrophe, three stray arguments otherwise); run
`forge promote clip <take> <name> …` by hand for those.

**Name exists:** stderr `sword_overhead already exists as
clips/sword_overhead.glb; the knobs not stated here were read from its
recipe, not defaulted`, then `forge: sword_overhead already exists as
clips/sword_overhead.glb — pass --overwrite if replacing it is the
intent.` and `the recipe it would have baked with:` with the knob lines
(plus `clip name     sword_overhead`), exit 2, nothing written. With
`--overwrite` the shipped recipe is the starting point and your
flags land on top — an `--overwrite --lean 6` keeps the old trims — and the
bake prints `the recipe it replaced, beside the one that shipped:` as
`was | now` columns. So on an overwrite **state every knob** you care
about, `--no-loop` and `--retime ""` to clear what you do not want, and read
both columns. Authored events are not carried (`note: the replaced clip
carried N authored event(s) … restate them with --event`); footsteps are
re-derived. Promote refreshes `assets/library.json` itself.

### 4. Judge the strip — `just sheet <name>`

Eight poses, three-quarter view, on the stage body, to
`out/sheets/<name>.png`. `--views all` (front, back, left, right, top, one
band each), `--head-row`, `--body <other>`, `--t0 0.2 --t1 0.6` to zoom a
window, `--frames 12`. **`Read` the PNG.** Judge silhouette, foot contact,
which hand holds what, limbs through the body, the seam (last cell against
the first on a loop). The summary on stdout:

| Line | Healthy | Not |
|---|---|---|
| `8 cells, 384x512 each` / `adapter: …` / `bounds:  2.35 x 2.20 x 1.84 m, lowest y -0.189` / `clip:    1.950s, 8 sampled` / `times:   0.00 0.28 0.56 0.84 1.11 1.39 1.67 1.95` | the bounds are the union of every sampled pose, so a lunge is wider than the body | — |
| `bones:   27 driven, 31 at rest, 0 orphaned` | 27 driven, 0 orphaned is the whole check; at rest is everything else in the hierarchy, fingers first | `0 driven` → exit 1 `sheet: the clip drives none of this skeleton's bones` — the body, not the clip: `just check-mesh` it |
| `sheet: …/out/sheets/<name>.png (1542x1052)` | the header reads `TORV_WARDEN.GLB / SWORD_OVERHEAD.GLB  1.95S  27 BONES DRIVEN`, cells `#N T.TTS THREE_QUARTER` | exit 1 `sheet: every sampled pose is identical` — a frozen clip; the take was STATIC or the window is one frame |

`just bones <name>` when in doubt: no GPU, the names hashed the way Bevy
binds them — `installed animation targets on 58 entities (model had
none)`, `model:`/`clip:`/`root:    Armature  (58 named entities)`, then
`27 of 58 skeleton bones driven, 0 orphaned curve(s)` (58 is every named
entity under the root: 55 bones plus `Armature`, `Body` and its material
node, which head the undriven list) and the full paths of what holds its
rest pose — the finger leaves always do. `NOTHING BOUND` is spelled out
when the names do not match. For the user's own eyes: `just studio`
(browse to the clip, transport, orbit) or `just studio --take <npz>
--recipe assets/clips/<name>.json` to watch the raw take under the shipped
recipe. **The user's eye outranks every sheet.** "Reads as a shove" means
go back to the sweep, not to the knobs.

### 5. Verify

- `just audit` — six sections, each `N checked, ok`: `library integrity`
  (every recorded hash), `derived footsteps` (`2/2 clips with contacts
  re-derive their footsteps`), `root tracks`, `clip rebuilds` (`2/2 clips
  rebuild byte for byte`, with bodies and models listed as `note <name>
  body skipped: integrity-only, no recipe`), `clip poses` (`2/2 clips pose
  the mannequin exactly as their shipped file`), `bodies` (`1/1 bodies
  conform to the contract`). A clip that does not reproduce is a record
  that lies; `just audit --fit` names the recipe that would, and the fix
  is a re-promote with that recipe, never a hand edit of either file.
- `just manifest-check`, `just verify`, then `just ci`.

## Seen → consequence → fix

| Seen | Consequence | Fix |
|---|---|---|
| `ardy is not installed — generation through it is off`, exit 3 in ~100 ms | no GPU work was attempted | `forge-setup` |
| CUDA out of memory during the load | the card was not free | `just gpu`; stop the ACE-Step server or the studio; never two generates at once |
| every row `STATIC` | the prompt described a pose or a stillness | re-prompt as an activity (rules in step 1) |
| `SKATE` on a take you like | ARDY's own foot-skate pass already ran | another sample or seed; `--no-postprocess` is for seeing, not shipping |
| the character slides or moonwalks on the stage | root travel left in a clip the game moves itself | `--in-place strip` (locomotion loop) or `detrend` (roll, lunge) and re-promote |
| a jump lifts twice, or lands in the floor | root Y and the game's capsule both moving | `--y-mode detrend` keeps the crouch and removes the net rise; `strip` only when the game owns all of the height |
| a loop pops at the seam | `SEAM`/`SEAMVEL`, or a window cut off-cycle | `just review <dir> --suggest`, take its loop trims, `--loop --loop-blend 0.2` |
| the strike lands sideways / ends facing away | `TURNS`: large `net_turn_deg` | pick the take with the smallest **net** turn; `--intent oneshot` keeps this flag on purpose — a death may end facing anywhere, a strike should not, so judge the number against the motion |
| `0 driven` on the sheet | the body is not on the profile; the clip is baked against `rigs/humanoid/rig.glb` | `just check-mesh assets/bodies/<body>.glb`; the fix is the body's export, never the clip |
| `baked … 0 event(s)` on a walk | the take carries no contact labels, so no footsteps were derived | the model labels contacts on every take, so none means no foot ever planted (`foot_skate_mps` is `nan` on that row): a `FLOATS` take, not a walk — pick another. Any other beat — a swing, a shot, a landing — is `--event T:<name>` by hand, T read off the review sheet's frame labels |
| `already exists … pass --overwrite` | refused, nothing written | re-run with `--overwrite` and every knob stated; read `was \| now` |
| `sh: 1: Syntax error: Unterminated quoted string` from `just promote-clip` | a `--note`/`--prompt` with spaces or an apostrophe went through `*flags` | `forge promote clip …` by hand for that flag |
| `just audit` names the clip | its record does not rebuild its bytes | `just audit --fit`, re-promote with the named recipe and `--overwrite` |
| the user says it still reads wrong | the sheet passed and the picture did not | back to step 1 with a better prompt or another seed; knobs cannot make a different motion |

## Commit set

`assets-src/takes/<name>.npz`; `assets/clips/<name>.glb` + `<name>.json`;
`assets/library.json`. Never `out/`. Commit only when the user asks.

## Known limits (say them, don't fight them)

- 20 fps takes, 27 driven joints; fingers and wrists are not animated and
  no prompt changes that.
- Standing life is not in the model; an idle is an activity or it is a
  statue with `STATIC` on it.
- The review sheet is a stick figure; the body is judged at step 4 or in
  the studio.
- ARDY generates facing +Z; the bake turns every clip to −Z, the engine's
  forward, as its last stage. Raw `.npz` in the studio is turned the same way; nothing
  downstream carries a compensation.
- Records of takes promoted out of a sweep with `--record` are `recorded`;
  the sample clips that shipped with the toolkit are `reconstructed` with
  nulled seeds, and that is the honest example, not a defect.
