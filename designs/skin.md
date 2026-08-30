# The fitted skeleton, the mesh doors, the reference door

Phases 2 and 3 of `designs/forge2.md`, written after the four spikes that
proved them (`python/forge_gen/spike_fit.py`,
`blender/{prepare_spike,spike_fit_rig,spike_reattach}.py`,
`spike_skin.py`; evidence under `out/spike_fit/`, `out/fit_warlock/`,
`out/spike/`). The decision this document implements is the one
`decisions.md` records on 2026-08-30: **bone lengths belong to the body, and
the skinner's weights say what they are.** Names, hierarchy and rest
*rotations* stay frozen, so every clip in the library still binds by name
path with no retarget and nothing under `assets/` is rebaked; bone *lengths*
become a per-body fact the sidecar records and the manifest carries.

The angle throughout is **the smallest honest door**. Every proven line moves
to its door by `git mv` and keeps its function names; nothing is rewritten to
look like a door. Phase 2 and 3 together are net negative in Python:
`blender/rig.py` goes away, four spike files stop being spikes, one gate and
one recipe die by name.

Two rules govern every number below. A figure is either **measured**, and
this document says on which body and from which file, or it is marked
**budget**, and the implementer's first task is to measure it and pin it —
the `vram_gb` lesson of 2026-08-30 in a new place. And a gate nobody can
calibrate does not ship as a refusal: it ships as a printed note, with the
lesson dated in `decisions.md`.

---

## 0. Every gate, its door, its number, and where the number came from

| Gate | Door | Refuses when | Number | Where it comes from |
|---|---|---|---|---|
| T-pose, arm height | `gen prepare` | arm-tip geometry not level with **the body's own shoulder line** | `[fit] arm_height_tolerance_m = 0.15` | shipped; **budget** by the ledger's own words ("the tolerance is a budget, not a measurement of where bodies break"). The witch measures 3.4 cm against her fitted wrists and is inside it against her own arm tube before any fit runs |
| reach / wrist span | — | **deleted** | — | `forge2.md`: "the fit gate stops measuring span". It measured a body's span against wrists the fit had just moved to it |
| sliver, arms | `gen prepare` | an arm run's median cross-section radius below `[fit] limb_radius_min_fraction` of the run's reference length | **0.22**, measured — see §1 | the courier sliver 0.200–0.214 against `vex_runner` 0.245–0.610 and `courier_v2` 0.505–0.674 |
| sliver, legs | `gen prepare` | never — measured and printed | — | measured: `vex_runner` legs read 0.149–0.183 and the sliver's legs 0.265–0.310. The number does not separate, so it is not a gate |
| dust | `gen prepare` | islands under 0.025 m across are dropped, not refused | shipped | ledger 2026-08-20 |
| no weights arrive | `gen prepare` | a vertex group on the input | shipped (`_refuse_any_skin`) | prepare hands SkinTokens a bare mesh |
| ratio band | `gen skin` (fit) | a run fitted outside `[0.4, 2.5]` | shipped spike default | |
| support | `gen skin` (fit) | a run resting on fewer than 8 effective vertices | shipped spike default | the thinnest real support measured is 25.0 (`vex_runner` left thigh) |
| symmetry, arms | `gen skin` (fit) | raw L/R gap over `[fit] asymmetry_arms = 0.35` on `Arm→ForeArm`, `ForeArm→Hand` | measured worst **28.7 %** | `out/fit_warlock/fit.json` (forearm), `out/spike_fit/control_vex_runner.fit.json` 24.2 %, `pass1.fit.json` 18.9 % — a body that walks at 28.7 % is why 0.25 is too tight |
| symmetry, elsewhere | `gen skin` (fit) | raw L/R gap over `[fit] asymmetry_other = 0.20` | measured worst **16.2 %** | the hip run on both the witch and the warlock; `vex_runner`'s clavicle 8.4 %, the warlock's shin 10.0 % |
| grounding factor | `gen skin` (fit) | warns outside `[0.7, 1.4]` | measured 1.138 (witch), 1.1675 (`vex_runner`), 1.2175 (warlock) | the warning band holds all three |
| off-axis landmark | `gen skin` (fit) | warns above 0.5 × the run's length | shipped spike default | "no length fixes a direction" |
| `motion_scale` | `gen skin` (fit) | outside `[0.75, 1.35]` | **budget** | the three measured are 0.978, 1.016, 1.06; the band is wide on purpose and refuses only a collapse |
| rest **direction** | `gen export` | a bone's local rest translation more than `[export] rest_direction_tolerance_deg = 1.0°` off the contract's | measured **0.0000°** of drift over the spike's whole fitted skeleton | `hosting.md` § Fitted skeleton spike. A degree is a generous ceiling on a quantity that moved by nothing |
| rest length | `gen export` | a fitted bone outside `[export] length_ratio_min/max` = `0.4 / 2.5` × the contract's | the fit's own band | a bone at 0.02 of reference is a collapsed skeleton, not a short body |
| zero-length bone | `gen export` | a segment under `[export] rest_zero_length_m = 1e-4` compared by position at `rest_tolerance_m` | shipped 0.1 mm | a zero-length segment has no direction to check |
| rest rotations | `gen export`, `rig check` | per-component gap over `[bones] rest_rotation_tolerance` | shipped 1e-3; measured drift **3.1e-6** | unchanged |
| feet on contact frames | `rig check` | the planted foot's own lowest skinned vertex outside `[bones] contact_foot_tolerance_m = 0.05` of y = 0 | fitted witch −1.5…+3.0 cm, baseline −3.3…+0.8 cm | both pass; a body whose legs the fit got wrong does not |
| stature, feet at rest, binding | `rig check` | unchanged | shipped | |
| format | `ref import` | under 1024 px on the long side, not a PNG, not one file | the format text | |
| keyer: floor band | `ref import` | opaque pixels across more than 25 % of the bottom 2 % of rows after the border flood | **budget** | the drawn ground plane that lifted as a slab |
| keyer: contact shadow | `ref import` | a component under ankle height, vertical extent below 8 % of subject height, horizontal extent over 1.4 × foot width | **budget** | "a faint contact shadow still passes the keyer as a detached island above the 0.025 m dust threshold and rides a foot bone" |
| keyer: flood-through | `ref import` | interior holes over 0.5 % of subject area | **budget** | the FLUX cream jacket the border flood punched through |
| keyer: retained alpha | `ref import` | subject fraction outside `[0.15, 0.85]` | shipped (`mesh.py` `SUBJECT_BAND` is 0.05–0.95) | tightened at the import door, where a redraw is cheap |
| geometry: span | `ref import` | span/height outside `[0.7, 1.3]` | **budget** | the fit scales lengths; it cannot rotate an arm |
| geometry: heads | `ref import` | below **4.0** heads refuses; 4.0–7.0 is a **note** | measured: the four-head witch now ships | the format text's "seven heads or more" predates Phase 2's answer, and the door says so |
| silence, clipping | `gen speech`, `gen music` | unchanged: peak ≤ −60 dBFS, or 0.0 dBFS with a run of pinned samples | shipped | the gate that caught 1.000 s of digital zeros |

**Three of the reference door's numbers moved when the pictures were measured
(2026-08-30, after this table was written).** The implementer ran the shipped
gates over all 21 reference PNGs on disk and `designs/decisions.md` — which
wins over any spec, this one included — carries the reasons:

| row above | what ships | why |
|---|---|---|
| heads: below **4.0** refuses, 4.0–7.0 notes | below **3.0** refuses, 3.0–7.0 notes | the four-head witch measures **3.37** from a silhouette that counts her hat; 4.0 refuses the body Phase 2 exists to ship |
| retained alpha `[0.15, 0.85]` | `[0.10, 0.85]` | measured 0.123 (`courier_v2_42`) to 0.278 (`barrel`); 0.15 refuses two references that lifted |
| subject fill, a refusal | a printed **note**, never a refusal | measured 0.634–0.95 with a character that lifted at 0.69 — it does not separate, and this document's own rule says such a gate ships as a note |

Everything else in the table is what shipped. Three refusals, written out,
because they are the design:

```
prepare: <name>'s arm tips sit 0.29 m below its own shoulder line (tips at
  y=1.02 m, shoulders at y=1.31 m, measured from the arm tube's centroid),
  past [fit] arm_height_tolerance_m 0.15. The arms are not horizontal in
  assets-src/refs/characters/<name>.png. Redraw with the arms straight out,
  palms down; a weight cannot fix a pose. (The four-head witch is NOT this
  refusal: against her own shoulder line she is inside the tolerance, and
  under the old gate she read 16-23 cm under the skeleton's wrists, which
  was a statement about the skeleton and not about her pose.)

prepare: courier_qwen's left upper arm measures 0.20 of its run across
  (5.9 cm through a 29.5 cm bone), under [fit] limb_radius_min_fraction
  0.22; the right reads 0.21. A limb thinner than the bone it hangs on
  animates as a sliver. This is the reference, not the lift: a posterized
  picture with 25-pixel shins gives TRELLIS.2 no shading to lift volume
  from. Redraw with the guide's volume sentences — a large head, big hands
  and boots, limbs as wide as the neck, a baked key with occlusion painted
  into the pits — or re-lift at another seed and look at the seven views.

export: Head's rest translation points 2.3 deg off the contract's
  (+0.000, +0.094, +0.011 against +0.000, +0.094, -0.009). Clips are baked
  against rest ROTATIONS, and a rotated bone binds perfectly and animates
  wrongly. Lengths are yours; directions are not.
```

---

## 1. The doors

### `forge gen prepare` — `python/forge_gen/blender/prepare.py`

`git mv python/forge_gen/blender/prepare_spike.py
python/forge_gen/blender/prepare.py`, registered in `cli.py`'s `COMMANDS` as

```python
("prepare", "forge_gen.blender.prepare", "Lifted glb -> normalised mesh + a skeleton, no weights"),
```

`TAG` is already `"prepare"`. The outer/inner split, `_from_lift`,
`_from_rigged_blend`, `_refuse_any_skin`, `_check_armature_survived` and
`_out_path` are kept verbatim. `blender/rig.py` is deleted and the four
functions prepare actually calls — `_normalize`, `_cleanup_and_budget`,
`_load_profile`, and the fit gate that was `_skeleton_fit_or_die` — move
into `prepare.py` with their bodies unchanged except where this section says
otherwise. Said once, so there is no second account: **`rig.py` goes away,
its bind half with it, and prepare owns the survivors.**

What the door gains:

- **A record.** `records.py` learns `kind: "prepare"`, tool `blender`; the
  door writes `out/prepare/<name>.prepare.json` beside
  `out/prepare/<name>.glb`, hashing the lift as its `mesh` input, with
  `params {profile, stature_m, yaw_deg, tri_budget, dust_diagonal_m,
  skeleton}` and `measured {vertices, triangles, dust_islands_dropped,
  shoulder_line_y_m, arm_tip_y_m, limb_radius: {<run>: {ratio, stations,
  off_axis_m}}}`. `forge gen skin` then hashes *this* glb, so the chain is
  `lift → prepare → skin` by hash.
- **A fake.** `run_fake(args)` writes a placeholder through
  `placeholders.placeholder_glb` — the profile's own bone nodes plus a boxy
  200-triangle body, no weights — writes the same record with `fake: true`,
  and calls `placeholders.refuse_real` on the output.
- **`--skeleton <blend|glb>`**, default the profile's `rig.blend`. This is
  the one new flag and the whole of what changes between the fit loop's two
  prepares.

**The fit gate, re-anchored.** The reach check is deleted: `reach`,
`reach_min`, `reach_max`, their two `InputRejected` arms, `_wrist_bone`'s
use for span, and `[fit] reach_min`/`reach_max` in `profile.toml`.
`forge_gen.profile` refuses a profile that still names either, so a dead
gate cannot be carried forward in a hand-written profile. What remains is
the arm-height check measured against **the body's own shoulder line**: the
median height of vertices with `|x| > 0.55 × half_span`, which is the
measurement the spike already used to cross-check the witch at 1.311 m. The
gate now refuses a *pose*, which is all it was ever able to see, and says
nothing about stature.

**One geometry module, two readers.** `python/forge_gen/fitgeom.py` takes an
`(N, 3)` array and returns `{shoulder_y, crotch_y, lowest_y, half_span,
arm_tip_y, heads}` plus `limb_sections()`. `prepare.py` calls it inside
Blender (arrays via `foreach_get`), `fit.py` calls it on the arrays it
already reads out of the glb, and the gate and the fit therefore cannot
disagree about where a shoulder is. It is the only genuinely new Python
file in Phase 2.

**The sliver check, measured.** `limb_sections()` walks each limb run's
frozen axis and samples three stations at 0.25, 0.5 and 0.75 of it. At a
station it takes the vertices within ±2 cm along the axis, then keeps only
the cluster reachable by 3.5 cm links from the point nearest the axis — the
clustering is not optional: without it the slab at mid-thigh contains the
far leg, the crotch and the skirt, and the number measures the body, not the
limb. The station's radius is the median distance of that cluster to the
axis; the run's radius is the median of its three stations; the ratio is
that over the run's reference length. Measured 2026-08-30 on the prepared
glbs that are still on disk:

| body | prepared glb | upper arm L/R | forearm L/R | judged |
|---|---|---|---|---|
| courier_qwen | `out/spike/prepare/courier_qwen.glb` | **0.200 / 0.206** | **0.214 / 0.212** | walked as a sliver, arm stretched to 2.8 m |
| courier_flux | `out/spike/prepare/courier_flux.glb` | 0.244 / 0.253 | 0.164 / 0.166 | never judged on a strip |
| courier_v2 | `out/spike/v2/courier_v2_prepared.glb` | 0.505 / 0.545 | 0.669 / 0.674 | walked and shot correctly |
| vex_runner | `out/prepare/vex_runner.glb` | 0.245 / 0.311 | 0.608 / 0.610 | ships |

So `[fit] limb_radius_min_fraction = 0.22` **on the arm runs**: it refuses
every arm of the body that walked as a sliver and leaves 11 % of headroom
under the thinnest arm of the body that ships. The leg runs are measured and
printed in the record and **never refused** — `vex_runner`'s thighs read
0.149 and 0.152, below the sliver's own 0.296 and 0.310, because a T-pose
isolates an arm and does not isolate a leg. The honest thing is to say that
in the record rather than to invent a leg threshold, and the acceptance run
re-measures all four numbers on three more bodies; if any good arm lands
under 0.22 the gate becomes a note in the same commit, with the lesson dated
in `decisions.md`. The off-axis distance is reported beside the radius: the
sliver's right arm sits a median 1.47 m off the contract axis, which is a
direction failure and not a thinness one, and the fit's own off-axis warning
already names that family.

Dust filter, triangle budget, matte register, armature insertion: unchanged.

### `forge gen skin` — `python/forge_gen/skin.py`

`git mv python/forge_gen/spike_skin.py python/forge_gen/skin.py`, registered
as

```python
("skin", "forge_gen.skin", "Prepared glb -> SkinTokens weights on a skeleton fitted to this body"),
```

Everything that made the spike safe survives untouched: `_refuse_a_stale_server`
(port 59876 by PID, never `pkill -f`), `_refuse_a_busy_card`, `_demo_argv`,
`_run_demo`, `analyse`, `_alignment`, the `unweighted_abort_fraction`
shell-abort gate, `SKINNER` / `SKINNER_NOTE`, the record writer, and the
`env` executor call exactly as the spike made it (`cwd = checkout`,
`SKINTOKENS_CHECKOUT`, `--use_skeleton --use_transfer --use_postprocess`).

What it gains is the rest of the chain — **five steps, one door, no options
about the number of passes**:

1. **skin** the prepared glb → `out/skin/<name>.skinned.glb`;
2. **fit** — `python/forge_gen/fit.py` (`git mv` of `spike_fit.py`),
   imported as a library. `fit()`, `_centroid`, `_split_length`,
   `_symmetrise`, `_ground`, `_inherit`, `gate()`, `run_table()` keep their
   names and their arithmetic. Three changes, all from the ledger: the limb
   runs come from the weights, **the root and the shoulder line come from
   `fitgeom`** (the weights put `vex_runner`'s root 5.8 cm high — they place
   a limb's end well and a body's centre badly), and `LANDMARKS` moves out of
   source into `profile.toml`'s `[fit] landmarks`, which is body-plan
   knowledge and belongs in the profile. `fit()` runs **once**, from the
   unfitted skin;
3. **build the per-body armature** — `python/forge_gen/blender/fit_rig.py`
   (`git mv` of `blender/spike_fit_rig.py`). **No scratch profile:**
   `_loosen`, `--reach-max`, `--arm-height-tolerance`, the profile copy and
   the `contract.json` regeneration are deleted, because those gates have
   changed shape upstream. `run_in_blender` keeps `_heads`, `_to_blender`
   and `_tail_child`, opens `rigs/<profile>/rig.blend`, scales the bone
   heads by the report's per-run ratios, and writes the armature into the
   body's working `.blend` and into `out/skin/<name>/skeleton.glb` for the
   second prepare. Rest rotations are **copied, never recomputed**; anything
   above `ROTATION_TOLERANCE` aborts. That abort is the safety argument of
   the whole design;
4. **prepare again** against that skeleton (`--skeleton
   out/skin/<name>/skeleton.glb`) and **skin again** — the proven chain is
   two SkinTokens runs and two prepares (`out/fit_warlock/` holds
   `drow_warlock_p1.*` and `drow_warlock_fit.*`; `hosting.md` measured the
   re-skin at 26.5 s with 55 of 55 bones carrying weight). Re-attaching
   pass-1 weights to joints that moved up to 14.7 cm would bind the body to
   the skeleton the fit just corrected;
5. **re-attach** — `python/forge_gen/blender/reattach.py` (`git mv` of
   `blender/spike_reattach.py`), by joint **order**, not name, onto the
   fitted armature; writes `assets-src/blender/<name>.blend`,
   `<name>.rig.json` and `<name>.fit.json`.

Two SkinTokens runs at ~27 s, two prepares at ~1.7 s, one armature build at
~5 s, one re-attach at ~5 s: the spike's own minute.

There is **no `--passes`, no `--diagnose` and no `--skip-refit`.** The second
fit walks the torso downhill 73.5 mm a time, so a knob whose only correct
value is off is surface, not a diagnostic; `convergence()` survives as a
library function with no caller in the door, exercised by `test_fit.py`
against the frozen `pass1.fit.json` / `pass2.fit.json`, which is how the
evidence is kept without adding a door.

### `forge gen export` — the direction rule

`python/forge_gen/blender/export.py::_check_bones` is the only door in the
toolkit with a rest-translation rule, so it is the only door that changes.
On the fitted witch it printed 55 problems, worst 252.33 mm. Per bone with a
contract parent, it now checks:

- the **direction** of the local rest translation, within
  `[export] rest_direction_tolerance_deg` (1.0°);
- the **length ratio**, inside `[export] length_ratio_min` /
  `length_ratio_max` (0.4 / 2.5);
- a segment shorter than `[export] rest_zero_length_m` (1e-4) on either
  side, compared by position at `rest_tolerance_m` as today;
- rest **rotations** unchanged at `[bones] rest_rotation_tolerance`.

The parent check, the depth check, the extras policy and the
self-contained-container check are untouched. `rest_tolerance_m` stays in
`profile.toml` because `forge gen rig-build` still holds a rebuilt profile to
its own contract; the exporter stops reading it for anything but zero-length
segments.

### Where each spike goes

| spike | becomes | registered as |
|---|---|---|
| `blender/prepare_spike.py` | `blender/prepare.py` | `prepare` (`run`, `run_fake`) |
| `spike_skin.py` | `skin.py` | `skin` (`run`, `run_fake`) |
| `spike_fit.py` | `fit.py` | library only, no subcommand |
| `blender/spike_fit_rig.py` | `blender/fit_rig.py` | library, called by `skin.py` |
| `blender/spike_reattach.py` | `blender/reattach.py` | library, called by `skin.py` |

`python/tests/test_spike.py` is deleted; its cases move to `test_prepare.py`,
`test_fit.py` and `test_skin.py`.

---

## 2. The profile, the contract, the records

**The humanoid profile keeps its reference skeleton.** `rigs/humanoid/rig.glb`,
`rig.blend` and `fixture/` do not move a millimetre: they are the neutral
prior every fit starts from, what `forge audit` binds clips to for the ≤ 1 mm
claim, and what the manifest publishes. `[profile] version` does **not** bump
— no bone was renamed, no rest rotation moved, no clip is rebaked.

**`profile.toml` (the source) and `contract.json` (its projection).** Every
scalar a gate reads is written once in `profile.toml` and mirrored into
`contract.json` by `forge rig export-contract`, never typed. New:

```toml
[bones]
contact_foot_tolerance_m = 0.05    # the planted foot's own lowest skinned vertex, on the reference clip's contact frames

[fit]
landmarks = ["Neck", "LeftArm", "RightArm", "LeftForeArm", "RightForeArm",
             "LeftHand", "RightHand", "LeftUpLeg", "RightUpLeg", "LeftLeg",
             "RightLeg", "LeftFoot", "RightFoot", "LeftToeBase", "RightToeBase"]
limb_radius_min_fraction = 0.22    # an arm run's median cross-section over its reference length; measured, see designs/skin.md
asymmetry_arms = 0.35              # raw L/R gap on upper arm and forearm; worst measured 28.7% (drow_warlock)
asymmetry_other = 0.20             # everywhere else; worst measured 16.2% (the hip, on two bodies)

[export]
rest_direction_tolerance_deg = 1.0
length_ratio_min = 0.4
length_ratio_max = 2.5
rest_zero_length_m = 0.0001
```

Removed: `[fit] reach_min`, `[fit] reach_max`, `[rig] shell_fraction` and
the proxy knobs the bind half read. `contract.json` is regenerated **once**,
by its own door, carrying `rest_direction_tolerance_deg`, `length_ratio_min`,
`length_ratio_max` and `contact_foot_tolerance_m`; the byte-for-byte
reproduction test is re-frozen in the same commit. A new pytest holds every
scalar the two files share to equality, because today nothing does.

**Sidecar schema 2.** `crates/forge_library/src/schema/mod.rs`: `SCHEMA = 2`,
and `Sidecar` gains one optional field, typed in a new
`crates/forge_library/src/schema/body.rs`:

```json
"body": {
  "motion_scale": 0.9780,
  "bones": [
    {"name": "LeftToeBase", "rest_translation": [0.000000, 0.140000, 0.000000]},
    "… 55 entries, contract node order, local translations, metres, six decimals"
  ]
}
```

`motion_scale` is the fitted `Hips` rest height over the contract's, four
decimals — the number a consumer multiplies the root translation track by,
and nothing else. `body` is `Some` only on `Kind::Body` and refused as
`Some` on every other kind at promote. A bump because the record can now
**say more**, exactly the rule in `records.md`.

**Migration is a measurement, not a guess.** `migrate.rs` 1 → 2 re-derives
`bones` from the shipped `.glb` through `forge_rig::derive_from_glb` and
writes `motion_scale: 1.0`. It does not copy the profile contract's
translations: that would be a default written where a measurement belongs,
and a body legitimately 0.09 mm off the contract — inside the old exporter's
0.1 mm rule, so shippable — would then fail verify's own 0.1 mm re-derivation
on the first run, with no door to fix it because a shipped sidecar is never
hand-edited. Nothing is nulled; there is nothing here the file does not know.
`provenance` is untouched.

**Manifest schema 2.** `crates/forge_manifest/src/lib.rs` (`SCHEMA` and
`BodyEntry` live there; `forge_library/src/manifest.rs` only builds it):
`BodyEntry` gains `motion_scale: f32`. Old readers say "you are behind"
instead of "corrupt". `forge bundle` stops defaulting `--motion-scale` to
1.0, reads the body sidecar's value when the flag is unstated, applies it to
the root track alone, and states in the `.bundle.json` which value it used
and where it came from.

**The rig record** (`<name>.rig.json`, `kind: "rig"`, tool `skintokens`)
gains, in `params`:

```json
"skinner": {"tool": "skintokens (VAST-AI/SkinTokens, MIT)",
            "commit": "273b691d35989d71cd17ff2895fdc735097b92d1",
            "note": "the Michelangelo encoder carries an open licence question (upstream issue #9)"},
"fit": {"passes": 1,
        "motion_scale": 0.9780,
        "asymmetry_arms": 0.35, "asymmetry_other": 0.20,
        "sources": {"limbs": "weights", "root": "geometry",
                    "shoulder_line": "geometry", "ground": "geometry"},
        "ratios": {"Spine": 0.7311, "LeftArm": 0.8497, "…": 0.0},
        "runs": [{"run": "Hips->Neck", "reference_length_m": 0.60272,
                  "ratio": 0.7311, "ratio_measured": 0.7311, "support": 338.0,
                  "off_axis_m": 0.0769, "mirrored": false}],
        "raw_asymmetry": {"upper_arm": 0.189, "forearm": 0.176, "hip": 0.162},
        "grounding": {"left": 1.1383, "right": 1.1383},
        "symmetry_worst": {"run": "LeftArm->LeftForeArm", "gap": 0.189}}
```

`seed` stays `null` — SkinTokens samples with no seed, two runs of the same
input give different hashes, and a rig claims **integrity, never
reproduction**. `null` means unknown, not zero.

**The `ref` record** — §5.

---

## 3. `forge rig check` and `forge verify`

`forge rig check` **gains two findings and loses none.** It passed the
fitted witch 10 of 10 with joints 252 mm from the contract, which is the
hole this fills. In `crates/forge_studio/src/rig_findings.rs`, in its
existing table-driven style:

- `check_rest_directions(contract, bones, out)` — every bone's local rest
  translation direction within `rest_direction_tolerance_deg`, length
  unchecked. `ok: rest translation directions match the contract (worst
  0.03 deg, LeftHandIndex3)` / `FAIL: LeftFoot's rest translation points
  1.9 deg off the contract — lengths are per body, directions are not`.
- `check_contact_feet(contract, clip, skin, out)` — bind the profile's
  `reference_clip`, CPU-skin at its contact frames (the contact columns of
  `motion_skeleton.json`; absent those, the frames where a foot's horizontal
  speed is minimal), and refuse when **the planted foot's own lowest
  vertex** is outside `contact_foot_tolerance_m` of y = 0. The measurement
  is the planted foot's, not the whole mesh minimum, and the finding says
  so: the spike measured −1.5…+3.0 cm this way against a whole-clip sheet
  minimum of −0.103 m, and the two are different questions. The whole-clip
  `lowest_y` line stays a note.

`forge verify` gains rule 3b: for every body, `derive_from_glb` the shipped
`.glb` and compare each bone's rest translation to the sidecar's `bones[]`
within 0.1 mm and each direction to the contract within tolerance, and
recompute `motion_scale` from the derived skeleton, failing on a gap over
0.005. A sidecar that claims a skeleton the file does not carry is exactly
the drift this design can otherwise produce silently — and it is the same
shape of claim `content_hash` already makes.

---

## 4. MCP — nineteen becomes twenty-five

| tool | shape | notes |
|---|---|---|
| `generate_mesh` | job (`env`, trellis2) | `kind: "generate_mesh.character\|prop"`; claims `out/lifts/<name>.glb` |
| `prepare_body` | job (`env`, blender) | claims `out/prepare/<name>.glb`; a refusal returns the fit-gate and sliver numbers |
| `skin_body` | job (`env`, skintokens) | the whole skin → fit → re-prepare → re-skin → re-attach loop; returns the fit table and `motion_scale` |
| `promote_body` | direct write | export gate + `rig check` + taken-name refusal, then `promote body` |
| `promote_model` | direct write | the doors `just prop-import`'s promote runs |
| `import_reference` | job (no card) | queued like every other write under `assets-src/`, so one door owns the source tree |

`promote_body` and `promote_model` are direct writes for the reason
`forge2.md` settled: the human is in the loop through the harness that issues
every command, and what protects the library is the export gate, the rig
check and the refused taken name — all of which the tool runs. Every refusal
is a **successful frame** naming what does exist: a missing backend prints
doctor's line, a taken name prints the record it would replace and says
`pass overwrite: true`, a fit refusal prints the measured numbers and names
the reference PNG.

The surface, sorted, as `mcp-check` compares it:

```
cancel doctor export_bundle generate_audio generate_clips generate_mesh
import_reference init_project inspect_audio licences list_audio list_clips
list_models list_runs prepare_body promote_audio promote_body promote_clip
promote_model render_clip_strip render_model setup skin_body status wait
```

Four pinned places move in one commit: `justfile`'s `mcp-check`
`expected=` string; `crates/forge/tests/mcp_session.rs`'s `const TOOLS: [&str;
25]`; `crates/forge/tests/cli.rs`'s `mine` array and its "Sixteen of the
nineteen" comment; and `crates/forge_mcp/src/lib.rs`'s
`the_tool_surface_is_the_nineteen_names_mcp_check_pins` plus the module doc
and the server instructions, whose "no promote for a body or a model"
sentence is deleted (`lib.rs:31`, the instructions text, and the assertion at
`lib.rs:339` that pins it).

`mcp-session` grows the character loop on the fake tier, appended to the
scripted session: `import_reference → generate_mesh → wait → prepare_body →
wait → skin_body → wait → promote_body → render_model → verify`, plus one
negative leg — a second `promote_body` on the same name refused, then
accepted with `overwrite`.

---

## 5. The reference door

`forge ref import <png> --name <name> --kind character|prop --source
"<what the user states>"`, and MCP `import_reference` with the same
arguments. One implementation: `python/forge_gen/reference.py`, registered as
`("ref-import", "forge_gen.reference", …)`, because the keyer is `mesh.py`'s
and belongs beside it; the Rust `forge ref import` submits that same command
line to the queue.

**The format text has one home.** It lives in
`python/forge_gen/reference.py` as `FORMAT`, verbatim from `forge2.md`
§ The reference door — "A reference is one PNG, 1024 px or more on its long
side…" through "…runs the silhouette pre-check the fit gate would otherwise
fail after a lift" — and every other copy is generated from it: the MCP
tool's `#[tool(description = …)]` reads a `const REFERENCE_FORMAT: &str`
that a build step or a checked-in generated file takes from the Python
source, and the skill quotes it by including the same generated block. Three
hand-maintained copies held to byte equality across a `.rs`, a `--help`
reflow and a markdown file is a test that fails on a rewrap and teaches
people to edit the fixture; one source and generated copies is the same
intent that survives contact.

The text gets one honest amendment, dated in the door and in `decisions.md`:
the "seven heads or more" sentence now reads as what the fitted skeleton
made it — **the door refuses below four heads and notes 4.0–7.0**, because
the four-head witch is the body Phase 2 exists to ship.

Order of operations, all of it before any GPU minute:

1. **format** — one PNG, ≥ 1024 px on the long side;
2. **key** — `mesh.py`'s own border flood at its own tolerance, the actual
   keyer and not a re-implementation;
3. **keyer pre-checks** — floor band, contact shadow, flood-through holes,
   retained alpha. Each is a refusal, not a warning: the spike proved all
   three ride through to the lift;
4. **geometry pre-checks** on the keyed alpha — span/height, heads, subject
   fill, exactly one island above the dust fraction; for `--kind prop`, the
   whole object inside the frame with ≥ 2 % margin;
5. **write** — `assets-src/refs/<kind>s/<name>.png` holding **the original
   bytes**, never the keyed image. `mesh.py` keys again at lift time, so a
   keyed PNG in the source tree would be a derived artefact whose hash and
   `SOURCES.md` row describe something the user never drew. Then
   `<name>.ref.json` beside it and the `SOURCES.md` row, written by the
   door.

```json
{"forge_record": 2, "kind": "ref", "tool": "imported", "created": "2026-08-30",
 "created_by": "agent:claude",
 "backend": {"name": null, "commit": null, "python": null, "torch": null,
             "model": null, "model_revision": null, "executor": null,
             "comfyui_commit": null, "workflow_sha256": null, "packs": null},
 "inputs": [{"role": "image", "path": "out/refs_grok/ember_knight_v3.png",
             "sha256": "sha256:…", "source": "xAI Grok, image_edit from a style board",
             "prompt": null}],
 "params": {"kind": "character", "long_side_px": 1536, "stated_source": "xAI Grok, …"},
 "measured": {"width": 1536, "height": 1536, "alpha_fraction": 0.41,
              "span_over_height": 1.02, "heads": 7.4, "subject_fill": 0.89,
              "islands": 1, "floor_band": 0.0, "contact_shadow_px": 0,
              "interior_hole_fraction": 0.0, "backdrop_rgb": [214, 214, 216],
              "keyer_tolerance": 28},
 "outputs": [{"path": "assets-src/refs/characters/ember_knight_v3.png",
              "sha256": "sha256:…", "bytes": 1048576}],
 "fake": false, "note": null}
```

`forge verify`'s rule 5 (`verify.rs::refs`) extends: a PNG under
`assets-src/refs/` passes on **either** a `.ref.json` whose output hash is
the PNG **or** a `SOURCES.md` row — the same either/or a designed voice
already lives under. The sample library's references get `.ref.json` records
written **through the door** where the origin is honestly known
(`vex_runner`, the Grok refs, `--source` restating the existing row) and keep
their rows alone where it is not.

The **sliver check is not here.** A picture cannot be measured for volume;
only a mesh can. That is the whole of the 2026-08-30 "a reference that passes
every gate can still lift to junk" lesson, and the check lives at `prepare`.

---

## 6. The two audio fixes

**Music: the gain becomes a stated knob in the graph.**
`backends/acestep/workflows/music.api.json` gains node `14`, class
`AudioAdjustVolume` — native, `comfy_extras/nodes_audio.py`, **verified
present in the installed host at pin `169fcf35`** — wired
`12 (VAEDecodeAudio) → 14 → 13 (SaveAudio)`, `_meta.title`:
`"PATCH:gain_db — ACE-Step 1.5 turbo normalises to peak, so the record states
the gain it was rendered at"`. `python/forge_gen/audio/music.py::template_inputs`
fills it from `--gain-db`; the value lands in `params.gain_db` and in the
record. **A trap for `hosting.md` § ComfyUI, dated 2026-08-30:** the node's
`volume` input is `IO.Int` (`default=1, min=-100, max=100`, gain
`10 ** (volume / 20)` applied to the waveform), so a fractional gain is
coerced or refused by the host — the door refuses a non-integer `--gain-db`
by name rather than rounding one. The default ships as **budget −3** and the
implementer's first task is three renders at −2, −3 and −4, reading
`peak_dbfs` off each, pinning the value that puts the busiest arrangement at
−2.0 ± 0.5 dBFS, and writing the measurement into `hosting.md`. The clipping
gate does not move: it is right, and a gain that merely dodges it would be
the hand-repair of an audio file. This is the fix *inside* the one-way rule —
the gain is a knob in the graph, not a normalise applied to a shipped file.

**Speech: the diagnosis is narrower than the notice reads.**
`backends/moss_tts/backend.toml` pins TTS-Audio-Suite at `fab00263` =
**v5.8.7, 2026-08-28** — already past v5.0.0 (transformers 5 in the main
environment, isolated secondary runtimes for legacy engines) and past v5.5.0
(the MOSS-TTS v1.5 family). There is no newer tag to move to, so **a pack
bump is a no-op**, and the notice's "WHAT LIFTS THIS: a pin built against
transformers >= 5" sentence is known wrong and is rewritten. The failure is
the 1.7B `MOSS-TTS-Local-Transformer` delay model running the pack's
vendored transformers-4 `_sample` in the host's main environment. One thing
to attempt, as a measurement: run that engine through the pack's **isolated
secondary runtime** for legacy engines and measure a line, its peak and its
VRAM. The same `backend.toml` records that the pack does not offer a
MOSS-TTS-Local-Transformer v1.5 variant, so pointing the engine node at one
is not an option to spend a card-hour on.

If the isolated runtime makes a real line, the door opens and the notice says
what was measured. If it does not, the door **stays exactly as it is** —
refusing, with the silence check (`peak −120.0 dBFS` is a refusal, not a
record) and a `[[notices]]` naming the pin, the runtime that was tried and
why it did not lift. Either way `hosting.md` gains a dated entry. **Do not
ship babble:** a shimmed model on wrong arithmetic is the one outcome that
would look like success.

---

## 7. What is deleted

- `blender/rig.py` **entirely**: the bind half (`_auto_weights`, `_bind`,
  `_detached_shell_vertices`, `_clear_weights`, `_unweighted`,
  `_proxy_weights`, `_nearest_vertex_rescue`, `_nearest_bone_rescue`,
  `_seen_add` — both rescue ladders and the proxy) with the survivors moved
  into `blender/prepare.py`; the `rig` subcommand in `cli.py`; `[rig]`'s
  shell and proxy knobs. `unweighted_abort_fraction` stays: it is the
  shell-abort gate on SkinTokens' output.
- **The reach check** — `reach`, `reach_min`, `reach_max`, and `_wrist_bone`'s
  use for span.
- **The spikes as spikes**: five files renamed, `python/tests/test_spike.py`
  deleted, and the "not registered in `cli.py`, not run by `just ci`"
  paragraphs replaced by door documentation.
- **`just rig-mesh`**, replaced by `just prepare <name>` + `just skin <name>`,
  with `just body <name>` running the whole loop. **`just promote-mesh`**
  becomes **`just promote-body`** (export → rig check → `promote body` with
  the lift, rig and export records); `promote-mesh` survives one release as a
  recipe that **dies by name** — "promote-mesh became promote-body when the
  skinner changed; the rig step is now prepare + skin" — the courtesy
  `install.sh --models` got.
- The **"no promote for a mesh"** rule in `crates/forge_mcp/src/lib.rs`, the
  server instructions and its `mcp-check` line.

Each deletion is its own commit with its reason in `decisions.md`.

---

## 8. Tests, fixtures, acceptance

**cargo.** `forge_rig`: the direction/length comparison as a unit function
with the zero-length case; `derive_from_glb` on a fitted fixture.
`forge_library`: schema-2 round trip and `deny_unknown_fields`; `body`
refused on a clip; migrate 1 → 2 re-deriving 55 bones from a fixture glb with
`motion_scale 1.0` and provenance untouched; verify's re-derivation green and
drifted (2 mm, and 1°); bundle defaulting to the sidecar's scale.
`forge_manifest`: schema-2 round trip with `motion_scale` on `BodyEntry`.
`forge_studio`: `check_rest_directions` and `check_contact_feet`, both marks.
`forge_mcp`: the twenty-five-name pin; each new tool's refusal frame asserted
on its text. `forge`: `cli.rs` and `mcp_session.rs` at 25, and the character
loop over both transports.

**pytest.** `test_prepare.py` — the arm-height gate against a synthetic
shoulder line; the sliver check on a synthetic sliver and on a synthetic good
limb; "a skinned file arrives" refused. `test_fit.py` — the frozen reports
`python/tests/fixtures/fit/{vex_runner,moss_witch_v4}.fit.json`, copied from
`out/spike_fit/`, must reproduce the same ratios; the symmetry gate refuses
0.40 on an arm and passes 0.287; the root comes from geometry, not weights;
`convergence()` on `pass1`/`pass2` still measures the downhill walk.
`test_skin.py` — argv building, the stale-server refusal, the record shape,
the `fit` block. `test_reference.py` — a floor band, a contact shadow and a
flood-through hole each refused by name from generated PNGs; a clean PNG
passing and writing all three files; the stored PNG is byte-identical to the
input. `test_profile.py` — every scalar `profile.toml` and `contract.json`
share is equal, and a profile naming `reach_min` is refused. `test_cli.py`,
`test_records.py` — the new `COMMANDS`, and byte equality of the `prepare`
and `ref` records between the Python and Rust writers.

**ci-fake** grows fakes for `prepare`, `skin` and `ref-import`; the loop
becomes: write a placeholder PNG → `ref-import` → `character` → `prepare` →
`skin` → `promote-body` → the gates. The fake skin writes a fitted-looking
skeleton (bones scaled 0.95) so schema 2's `bones[]` and `motion_scale` are
exercised end to end with no card. `just ci`'s list is unchanged.

**Frozen fixtures that must not move:** `assets/clips/*.glb` and their
sidecars, `crates/forge_motion/tests/fixtures/`, `rigs/humanoid/rig.glb`,
`rig.blend` and `fixture/`, the records byte-equality fixtures.
`rigs/humanoid/contract.json` moves exactly once, regenerated by its own
door, in the commit that adds the four scalars.

**Real-run acceptance**, one card, one job at a time, `just gpu` between:

1. `vex_runner` from its existing lift → prepare → skin → export → rig check
   → `promote body --overwrite`. Report `motion_scale` (the weights-only root
   read 1.0605; from geometry it must land inside 1 % of 1.00 or the geometry
   root is wrong), 55 of 55 bones weighted, 0 unweighted, the walk driving 27
   of 27, and the four arm sliver ratios.
2. `out/refs_grok/ember_knight_v3.png` → `ref import` → the whole loop → a new
   body promoted, no `--overwrite`.
3. `out/grok/moss_witch_v4` → the same loop → promoted. The body the old fit
   gate refused five times.
4. One deliberately bad picture — a contact shadow — refused by `ref import`
   before any GPU minute.
5. `just check-bodies`, `forge audit`, `just verify`, `just manifest` +
   `just manifest-check` green.
6. **Every clip played on every fitted body as a strip, judged by eye** — the
   walk, `pistol_shoot`, `roll`. Numbers say it is wired; the picture says
   what it is, and the spike's own evidence is pictures (`out/spike_fit/gifs/`,
   `out/fit_warlock/`).

---

## 9. Three implementers, disjoint files

**The four shared contracts**, frozen in one pre-branch commit that also puts
§0's gate table into `designs/rig-contract.md`, so all three read one copy:

1. **Sidecar schema 2** — `"body": {"motion_scale": f64, "bones": [{"name":
   String, "rest_translation": [f64; 3]}]}`, 55 entries in contract node
   order, local translations, metres, six decimals, absent on every non-body
   kind. B's `skin.py` writes the same numbers into the rig record; A's
   `promote body` reads them from the **glb**, never from the record, so the
   sidecar's claim is re-derivable.
2. **The `fit` block** — the JSON in §2, written by B into `<name>.rig.json`
   `params`, read by A's `promote body` for `provenance: recorded` and by C's
   skills for what to print.
3. **The `ref` record** — the JSON in §5, written by C, read by A's verify.
4. **The tolerances**, named identically in `profile.toml` (B),
   `contract.json` and `export::Options` (A, generated) and every message
   that quotes them (C): `rest_direction_tolerance_deg = 1.0`,
   `length_ratio_min = 0.4`, `length_ratio_max = 2.5`,
   `rest_zero_length_m = 0.0001`, `contact_foot_tolerance_m = 0.05`,
   `limb_radius_min_fraction = 0.22`, `asymmetry_arms = 0.35`,
   `asymmetry_other = 0.20`.

| | owns |
|---|---|
| **A — Rust** | `crates/forge_rig/src/{lib.rs,export.rs,measure.rs}` and `examples/export_contract.rs`; `crates/forge_library/src/{schema/mod.rs,schema/body.rs,sidecar.rs,verify.rs,migrate.rs,manifest.rs,promote.rs,audit.rs,bundle.rs,catalog.rs}`; `crates/forge_manifest/src/lib.rs`; `crates/forge_studio/src/{rig_check.rs,rig_findings.rs}`; `crates/forge_mcp/src/{lib.rs,server.rs,tools/*}`; `crates/forge/src/{cli.rs,commands/*}`; `crates/forge/tests/{cli.rs,mcp_session.rs}`; every `Cargo.toml`; `rigs/humanoid/contract.json` **as regenerated output only** |
| **B — Python** | `python/forge_gen/cli.py`; `python/forge_gen/{skin.py,fit.py,fitgeom.py,records.py,placeholders.py,profile.py}`; `python/forge_gen/blender/{prepare.py,fit_rig.py,reattach.py,export.py}` and the deletion of `blender/rig.py`; `python/tests/test_{cli,prepare,fit,skin,records,placeholders,profile}.py`; `rigs/humanoid/profile.toml` |
| **C — reference, audio, docs** | `python/forge_gen/reference.py`, `python/tests/test_reference.py`; `python/forge_gen/audio/**`; `backends/**`; `.claude/skills/*`; `README.md`, `CLAUDE.md`, `CHANGELOG.md`, `designs/*.md`; the `justfile`, including `mcp-check`'s `expected=` and `mcp-session`'s character loop |

**Contested files, one owner each.** `cli.py` is **B's** — C sends the one
`COMMANDS` row for `ref-import` as a patch note. The `justfile` is **C's** —
A sends `promote-body` and the twenty-five-name `expected=` string, B sends
`prepare`, `skin` and `body`; A's Rust test is what fails if C mistypes the
list. Every `Cargo.toml` is **A's**. `profile.toml` is **B's**; A reads those
numbers only through `contract.json` and `export::Options`, and B's new
pytest holds the two equal. `designs/hosting.md` and `designs/decisions.md`
are **C's file** — A and B each hand C a dated paragraph rather than editing
the ledger, because a ledger with three writers is three ledgers. The
reference format text is **C's** source of record in `reference.py`; A's
`tools/reference.rs` takes a generated copy.

**Order of landing.** A's schema 2, migrate and the direction tolerance in
the contract first (B's exporter reads the tolerance from `contract.json`);
B's doors second; C's reference door and audio third; the four pinned lists
and `mcp-session` last, in one commit — whoever pushes second rebases. Each
of the three ends on `just ci` green and a commit whose message says what
changed and why, in one breath.
