---
name: forge-review
description: Read the pipeline's renders and plots the way they are meant to be read — a seven-view mesh sheet, a rig-check or clip sheet, a sweep review table and stick sheet, an audio plot — in the order wiring → mechanics → picture, knowing which lines are gates and which are hints, and when the user's eye outranks the sheet. Use before promoting anything, when the user asks whether an asset looks right, or when a number and a picture disagree.
---

# Reviewing: wiring → mechanics → picture

Generating is the easy half. Every artefact below exists because a number
cannot see and an eye cannot count: the sheet catches the clip that was
"pistol shoot" on paper and a walk at 0.94 m/s in the picture; the line
`0 bone(s) driven` catches the body that looks perfect at rest and will
stand in T-pose through every clip. Read in this order, every time:

1. **Wiring** — did the thing bind, decode, parse? Exit code, the `bones:`
   line, the header's `N BONES DRIVEN`, `verdict:`. A wiring failure makes
   the picture meaningless: the pose you are looking at is the rest pose.
2. **Mechanics** — the gates: `ok:`/`FAIL:` findings, review flags, audio
   warnings. Pass/fail, no judgement in them.
3. **Picture** — the eye. Is it the thing? Does it read as the action? Is
   the back of the head there? This is the step no gate replaces.

A render is for eyes, not CI: its exit status is its only pass/fail, and
frames are not byte-stable across GPUs. Never compare two sheets by bytes.

## The artefacts and the commands that make them

| Artefact | Command | Lands at |
|---|---|---|
| Seven-view (or four-view) mesh sheet | `just views <name\|path.glb> [--no-head] [--cull-off] [--views LIST] [--out PNG]` | `out/views/<stem>.png` |
| Rig-check findings + walk sheet | `just check-mesh <glb> [--out out/sheets/<name>.png]`; the whole library: `just check-bodies` (findings only) | terminal + PNG |
| Turntable of a shipped body | `just body-sheets` (every body) — `forge turntable <body> --out PNG` for one | `out/sheets/bodies/<name>.png` |
| Clip strip on a body | `just sheet <clip> [--body <name>] [--frames 8] [--views three_quarter\|front\|back\|left\|right\|top\|all] [--columns 4] [--cell 384x512] [--t0 0 --t1 1] [--head-row] [--out PNG]`; every clip: `just sheets` | `out/sheets/<clip>.png` |
| Bone binding, no GPU | `just bones <clip> [--body <name>]` | terminal |
| Sweep review table + stick sheet | `just sweep "<prompt>" [--duration 2 --samples 4 --seeds 0 1]` (generates, then reviews) or `just review out/sweeps/<dir> [--intent loop\|oneshot] [--window action\|loop\|full]` | `<dir>/sheet.png`, `<dir>/metrics.json`, the table on stdout |
| A raw take on the real body | `just studio --take out/sweeps/<dir>/<take>.npz [--recipe r.json]` | a window |
| Audio plot + numbers | `just audio <file>`; every shipped sound: `just audio-list` (table) and `just audio-plots` (PNGs) | `out/audio/<stem>.png` |
| The user's own look | `just studio [--model <name>]`, `just play` for audio | a window |

`Read` the PNG (the Read tool shows images). The MCP tools `render_model`,
`render_clip_strip` and `inspect_audio` are the same renderers; what
follows applies to their images too.

## 1. The mesh sheet (`just views`)

Header: `<NAME>.GLB  7 VIEWS  CULL OFF  W X H X D M` (`4 VIEWS` with
`--no-head`; `CULL ON` for a library name or a path outside `out/`).
Tiles: `FRONT` `BACK` `LEFT` `RIGHT`, then `HEAD FRONT` `HEAD BACK`
`HEAD BACK TOP`. Terminal: `7 cells, 384x512 each`, `adapter: …`,
`bounds:  W x H x D m, lowest y …`, `views: out/views/<stem>.png (1542x1052)`.

- **Wiring.** The bounds. A raw lift is in TRELLIS's unit cube (`1.00`
  tall, `lowest y -0.4…`): not metres yet. A shipped body is `~1.80` tall
  with `lowest y` near `0`; a shipped floor prop has `lowest y 0.000`; a
  grip prop is negative by its grip height (`-0.160` on the sample sword).
  Wrong here is wrong everywhere.
- **Orientation, as this build renders it.** The `FRONT` camera is on the
  −Z side; the rig profile's rest pose faces **+Z**, and lifts of a pictured
  side come out facing +Z. So a correctly facing body shows its back in
  `FRONT` and its face in `BACK`/`HEAD BACK`; `HEAD FRONT` is the rear of
  the skull; `HEAD BACK TOP` is the face and crown from above. A prop's
  pictured side is the `BACK` tile and `FRONT` is the side TRELLIS
  invented. The sample `vex_runner` renders this way. Read tiles by what
  they show, not by their labels.
- **Culling off** (default under `out/`): a missing surface shows as the
  *inside* of the surface behind it — dark, the texture seen from behind,
  the outline reading as a rim rather than a dome. That is what a hollow
  skull looks like in `HEAD FRONT`. Culling **on** hides it: the same hole
  reads as a see-through gap, easy to miss at 384 px. Judge raw lifts with
  culling off; judge shipped files both ways.
- **Picture**: the rear skull closed; T-pose intact, one hand per side;
  nothing that is not the subject (a puddle under the feet is a lifted
  shadow, a speck is dust); the register held (fingers or a mitt, not a
  cone; pauldrons separate from the torso); a prop's invented far side
  plausible (a smeared mirror of the front is normal, a gap is not).
- **Not a test.** `just views` exits 0 whenever it rendered; there is no
  gate in it. Hollow, flipped or the wrong size are all exit 0.

## 2. The rig check (`just check-mesh`, `just check-bodies`)

Terminal: `subject:`, `profile:   humanoid v1`, `reference: clips/walk.glb`,
then one finding per line padded as `ok:   ` / `note: ` / `WARN: ` /
`FAIL: `, the sheet summary when `--out` was given, and the tally
`N finding(s) passed, N failed, N note(s), N warning(s)`. Exit 1 on any
`FAIL`.

- **Wiring** is the last `ok:` line: `reference clip: 27 bone(s) driven,
  31 at rest, 0 orphaned curve(s)`. `0 bone(s) driven` is the silent
  failure the whole check exists for — the engine reports nothing, the
  character holds its rest pose forever. Fewer than 27 is a bone at the
  wrong depth or name. `reference: none in the library` with `WARN: no
  reference clip 'walk' in the library; walk binding not checked — promote
  it, or bind this mesh in the studio to see what moves` (tally `9
  finding(s) passed, 0 failed, 0 note(s), 1 warning(s)`, exit 0) means
  binding was **not** checked at all; do not read the rest as a pass, and
  with `--out` the PNG is then a seven-view rest sheet (`rendered at rest:
  no reference clip in the library to play`), not the walk.
- **Mechanics**: `animation root is named Armature`, `armature transform is
  identity`, `Hips found at depth 2, directly under the animation root`,
  `all 55 contract bones present at contract depth`, `rest rotations match
  the contract`, `no unknown bones between Hips and the leaves`, `skin
  present, N weighted vertices`, `height 1.80 m, within 1.4-2.2 m`, `feet
  at y=0.000 m`. Every `FAIL` names what it measured; `note: extra leaf bone
  X` is allowed and listed so a reviewer sees it.
- **Picture** (`--out`): the body playing the reference walk — header
  `SUBJECT.GLB / REFERENCE.GLB  2.60S  27 BONES DRIVEN`, eight
  `THREE_QUARTER` cells `#N T.TTS`. Look for candy-wrapper elbows and knees
  (weights bleeding across a joint), a shoulder plate or lamp that drifts
  off the arm as it swings (a detached shell weighted to the wrong surface),
  feet sinking or hovering, a cape or skirt that stays rigid, a head that
  does not turn with the neck. The fix for all of them is upstream — seed,
  reference, stature — never a weight edit.

## 3. The clip strip (`just sheet`, `just bones`)

Header: `<BODY>.GLB / <CLIP>.GLB  2.60S  27 BONES DRIVEN`. One band per
view, cells `#N T.TTS VIEW`; `--head-row` adds `FACE FRONT`,
`FACE THREE_QUARTER`, `FACE LEFT`. Terminal: `8 cells, 384x512 each`,
`bounds: …`, `clip:    2.600s, 8 sampled`, `times:   0.00 0.37 …`,
`bones:   27 driven, 31 at rest, 0 orphaned`, `sheet: out/sheets/<clip>.png`.

- **Wiring.** `bones: 27 driven` and `0 orphaned`. `WARNING: the clip
  drives none of this skeleton's bones — the pose you are looking at is
  the rest pose.` exits 1; so does a frozen clip (`every sampled pose is
  identical`). `just bones <clip>` gives the same counts with no GPU and
  lists the full name paths (`Armature/Hips/Spine/…`) of what is undriven —
  the finger leaves always are, by contract.
- **Mechanics** live in the sweep review (below) and in `just audit`, six
  sections each ending `N checked, ok`: `library integrity`, `derived
  footsteps`, `root tracks`, `clip rebuilds` (`N/N clips rebuild byte for
  byte`; bodies and models appear as `note <name>   body skipped:
  integrity-only, no recipe`), `clip poses` (`N/N clips pose the mannequin
  exactly as their shipped file`), `bodies` (`N/N bodies conform to the
  contract`).
- **Picture.** Read the `THREE_QUARTER` band first for silhouette, then
  `front`/`left` (`--views all`) for limbs through the body, feet sliding,
  the hips' travel. Does it read as the action the prompt named? A cut
  judged as a strike on a stick figure was a shove on the body — the stick
  figure had no shoulders to show it, which is why the strip is rendered on
  the real body. Loop seams: compare `#0` and the last cell.

## 4. The sweep review (`just sweep`, `just review`)

Terminal: one row per take under the header
`name duration_s avg_speed_mps start_ratio stop_ratio foot_skate_mps jitter_pct activity_m loop_gap_m drift_deg net_turn_deg left_steps flags`,
plus `sheet: <dir>/sheet.png  metrics: <dir>/metrics.json`. Files:
`<label>__d<dur>_c<cfg>_s<seed>_<k>.npz` + `.take.json` + `sweep.json`. The
sweep itself logs `Loaded ARDY-Core-RP-20FPS-Horizon40 @20fps in 43s` and
`1 takes: 1 prompts x 1 seeds x 1 cfg x 1 durations x 1 samples`.

- **Wiring.** The row exists and `duration_s` is what was asked, less the
  model's rounding to whole frames (`3.95` for `--duration 4`: 79 frames
  at 20 fps). A take that did not load is absent, not zero.
- **Mechanics** — the `flags` column, thresholds from
  `rigs/humanoid/profile.toml [review]`:

  | Flag | Means | Threshold |
  |---|---|---|
  | `SKATE` | a planted foot slides | `foot_skate_mps > 0.25` |
  | `JITTER` | high-frequency noise vs real travel | `jitter_pct > 32` |
  | `SINKS` / `FLOATS` | lowest foot below the floor / never reaches it | `> 0.05 m` below / `> 0.10 m` above |
  | `STATIC` | nothing happens *and* nothing travels | `activity_m < 0.035` and `avg_speed_mps < 0.35` |
  | `SEAM` / `SEAMVEL` | pose / velocity mismatch across the loop seam | `loop_gap_m > 0.12` / `loop_vel_gap > 0.9` |
  | `DRIFT` | heading wanders | `abs(drift_deg) > 35` |
  | `TURNS` | ends facing elsewhere (net turn, not cumulative — cumulative rewards a wobble) | `abs(net_turn_deg) > 25` |
  | `STOPS` | arcs to a halt: an episode, not a loop | `stop_ratio < 0.45` with peak speed > 0.8 |

  `--intent oneshot` drops the loop-only flags. A clean row is necessary,
  not sufficient.
- **Picture** — `<dir>/sheet.png`: one row per take, eight stick-figure
  keyframes across the active window, a bold header per row (`<name>
  2.0s  1.2m/s   travel 2.4m  steps 3   [action 0.00-2.00s]`), a second
  line with the numbers that ends in the flags or `clean` (red when
  flagged, green when not), the prompt in italics. Stick figures show
  rhythm, travel and contact; they do **not** show shoulders, hands or
  what a limb passes through. Pick on the stick sheet, then confirm on the
  body: `just studio --take <take>.npz`, or promote and `just sheet`.

## 5. The audio plot (`just audio`, `just audio-list`)

Terminal: `file:`, `format:   1.000s  48000 Hz  1 ch`, `level:    peak
-1.0 dBFS   rms -20.1 dBFS   lufs -22.4   crest 19.1 dB`, `shape:    lead
silence N ms   tail N ms   dc +0.0000   full-scale N (run N)`, then
`verdict:  clean` or one `warning:  …` per problem, then `plot:
out/audio/<stem>.png (1400x690)`. `just audio-list` prints a row per
asset (`? sfx/<name>.wav   1.50s  48000 Hz 1ch  peak   -0.0  lufs  -18.8`)
with `! = defect, ? = worth a look`. Exit non-zero for silent or clipped.

The PNG: title `<FILE>.WAV`, facts line `1.00S  48000HZ  1CH  PEAK -37.2DB
RMS -65.3DB  LUFS -62.9  CREST 28.1DB`, then `CLEAN` or up to two warnings
in red; a waveform panel (hot samples in red) over a log-frequency
spectrogram with a time axis.

- **Wiring.** It decoded, the duration is what was asked, the sample rate
  is the library's.
- **Mechanics** — the warnings, most serious first: `file is silent`;
  `clipped: N consecutive samples pinned at full scale (N total) - audible
  distortion` (a *run*, not a count: peak-normalising puts one sample at the
  rail by construction); `DC offset ±0.0xx`; `N ms of silence before the
  first sound - a one-shot will feel late`; `ends N dB quieter than it
  starts - will jump at the loop point` (only over 10 s); `crest factor N
  dB - very compressed`; `peaks at only N dBFS - very quiet next to a
  normalised library`. LUFS is a K-weighted stand-in: enough to compare
  within one library, not to certify a master.
- **Picture.** Clipping reads as flat tops; dead air as a gap; a truncated
  tail as a cliff; over-compression as a solid block with no dynamics; a
  tonal hum as a horizontal line in the spectrogram; a click as a vertical
  one. Nobody here can hear the file — the plot and the numbers are the
  whole review, and the user's ear is the verdict.

## Gates versus hints

| Gate (pass/fail; a failure is a stop) | Hint (read it, then look) |
|---|---|
| exit codes; `forge gen` 3/4/5/6 | `mesh: keyed background, subject covers N%` inside 5–95 % |
| `FAIL:` lines from `forge rig check`; `check-bodies` | `note:` lines; `WARN: no reference clip` (unchecked, not failed) |
| `just sheet` exit 1: nothing bound / frozen | the strip itself |
| the fit gate (`not a T-pose`), the 20 % weightless abort, the 12 000-tri prop cap | dust counts, shell counts, decimation lines |
| `just audit` byte and pose parity; `just manifest-check`; `just verify` | the review flags (a clean row is not a good clip; a flagged row can be the take the user wants, e.g. `TURNS` on a turn) |
| `audio`: silent, clipped | the other audio warnings |

A gate is never argued with at the threshold. A hint is never promoted into
a gate by a reviewer: it is a reason to look.

## When the eye outranks the sheet — and when it does not

- The user in `just studio` outranks every sheet. "Still hollow" means still
  hollow; "it walks like it is wading" means the take is wrong however
  clean its row. Go back to the seed, the reference, the prompt or the
  recipe — never to the file.
- A sheet outranks an eye on **wiring**: a body that looks perfect at rest
  with `0 bone(s) driven` is broken, and no amount of looking at it
  standing still changes that. A clip with `0 orphaned` curves that looks
  wrong is a bad clip, not a binding bug.
- A clean table outranks nothing. Every gate passed the mask-faced lift
  and the walking "pistol shoot".
- One render is one sample. Culling on and off, all views, the head row:
  when something seems off, render more, not the same thing again.

## What not to do

- Do not repair the artefact. Not the `.blend`, not the `.glb`, not the
  sidecar, not the recipe's knobs to make a gate pass. The fix is upstream
  of the file, and a hand-edited file is the one link nobody can re-derive.
- Do not raise a threshold or a budget by reflex: `--budget` exists for a
  lift that came back heavier than its register, not for a gate in the way.
- Do not pass `--overwrite` to get past `already exists … pass overwrite to
  replace it` unless replacing is the user's stated intent. The refusal
  echoes the old recipe beside the new one so the decision is visible.
- Do not judge a clip on the stick sheet or the table alone; the strip on
  the real body is where the clip is judged.
- Do not treat a render as a test or compare sheets byte for byte across
  machines. `just ci` leaves every render out on purpose.
- Do not re-render hoping for a different answer; the same file renders the
  same. Change the input, then render.
- Do not promote a mesh through the MCP server — there is no tool for it,
  by design: characters and props go through `forge-character` and
  `forge-prop` with a human looking.
