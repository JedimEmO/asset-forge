# The rig contract

What a skinned mesh must be for every clip in a library to play on it,
unchanged — and where that requirement lives. The contract is **data in a
profile directory**, not a table in source: nothing in the Rust knows the
number 55 or the name `Hips`; the profile says which bone is the root, which
bones a clip drives, and what the rest pose is, and the code only knows how
to hold a file to it. The shipped profile is `rigs/humanoid/`
([its README](../rigs/humanoid/README.md) has the skeleton drawn out and the
numbers explained); this document is the contract any profile has to meet.


**Lengths are the body's; names, hierarchy and rest rotations are not
(2026-08-30).** `forge gen skin` fits the skeleton to what the skinner's own
weights say this body's bones are, so a shipped body's local rest
*translations* are its own and its sidecar records all 55 of them plus a
`motion_scale`. What stays frozen is everything a clip binds through: the
names, the hierarchy, and the rest **rotations** — so every clip in the
library still binds by name path with no retarget and nothing under
`assets/` was rebaked. The exporter therefore checks a rest translation's
**direction** (within `[export] rest_direction_tolerance_deg = 1.0°`) and
its length **ratio** (0.4–2.5), never its length. `designs/skin.md` is the
design; `designs/decisions.md` carries the reasons.

## What a profile is

A directory, named in `forge.toml` by `rig = "<name>"` under `[paths] rigs`
(the toolkit's own `rigs/`; a project made by `forge init` gets a copy under
`assets-src/rigs/`). Five things must be in it:

| Entry | What it is | Who reads it |
|---|---|---|
| `contract.json` | every bone in `rig.glb` node order: `name`, `parent` (index or `null`), `driven`, `rest_translation`, `rest_rotation`; plus `root`, `front`, `stature_m {reference, min, max}`, `foot_tolerance_m`, `rest_rotation_tolerance`, `driven_layout`, `reference_clip`, and `sources {glb sha256, blend sha256}`. **Generated** by `forge rig export-contract`, never typed | `forge_rig`, `forge rig check`, `forge promote body`, a game through the manifest |
| `sockets.json` | named offsets from contract bones (translation in the bone's space, a rotation carrying the prop's authoring frame in) and the `authoring_frame` a prop is built in | `forge_rig`, `forge gen prop --grip/--socket`, the manifest |
| `motion_skeleton.json` | the driven layout: the joints a motion take writes, in take order, with parents, feet, hands and the contact-column order the review reads | `forge_motion` (asserted equal to its own constants by a test), `forge gen motion review` |
| `profile.toml` | every scalar a gate uses — `[bones]` stature band, foot tolerance, rest tolerance, reference clip; `[fit]` the T-pose gate; `[rig]` budget, dust, shell and abort fractions; `[prop]` budget and frame; `[material]` matte; `[export]` tolerances and influences; `[fingers]` layout; `[review]` thresholds — each with what it gates | `forge_gen.profile` (Python), `forge doctor` |
| `rig.glb`, `rig.blend`, `fixture/<clip>.glb` | the skeleton as an artifact (the contract is derived from the `.glb` and carries its hash), the same skeleton as the Blender file `forge gen prepare` opens to insert into every body, and a driven-only clip carrying the rest pose so `forge gen rig-build` can rebuild the `.blend` from nothing | `forge_rig::derive_from_glb`, `forge gen prepare`, `forge gen rig-build` |

`RigProfile::load` holds the files to each other: every socket sits on a
contract bone, the driven bones are exactly the layout's joints, and the
driven parent chain collapses to the layout's parents. `forge verify`
re-hashes `rig.glb` against the contract (a mismatch fails) and `rig.blend`
(a mismatch warns: the blend is the authoring source, not what the contract
was read from), and re-derives the bones from the `.glb` to check them
position for position.

## The one rule: never add a parent above the root

An engine binds an animation curve to a bone by hashing the bone's **full
name path** from the animation root — `Armature/Hips/Spine/…` — into an id.
Every clip in the library carries ids computed against this exact
hierarchy. Insert a `Root` above `Hips` and every path becomes
`Armature/Root/Hips/…`: every hash changes, every curve finds nothing, and
the engine reports **no error at any log level** — the character holds its
rest pose forever. Renaming a bone or inserting one mid-chain does the same
to everything below it. Root motion does not need a root bone: it ships as
the root bone's translation curve and as per-clip metadata in the manifest.

This is why `contract.json` carries `parent` as an index and the validator
checks **depth**, not only names: a bone with the right name at the wrong
depth hashes differently and binds to nothing.

## The driven set

A profile's skeleton is a **strict superset of its motion generator's**:
the same names, the same hierarchy, the rest pose preserved, so a raw take
binds every one of its curves with zero orphaned and no retargeting layer
anywhere. The driven bones are the ones `motion_skeleton.json` lists and
`contract.json` marks `driven: true`; the rest exist so a mesh can be
skinned (fingers, in the shipped profile) and **no clip ever animates
them**. The humanoid profile has 55 bones of which 27 are driven — ARDY's
`cskel27` exactly, plus finger leaves. `forge rig check` ends by binding the
profile's `reference_clip` (the walk: it drives all 27) and expects

```
ok:   reference clip: 27 bone(s) driven, 0 at rest, 0 orphaned curve(s)
```

A finger leaf carrying little or no weight on a mitt-resolution mesh is the
register, not a defect: the contract wants every bone present and every
vertex weighted to *some* contract bone, not every bone deforming.

## Profile fields the gates read

| Field | Where | What it gates |
|---|---|---|
| rest pose | `contract.json` rest transforms; `[export] rest_tolerance_m`, `[bones] rest_rotation_tolerance` | frozen. Clips are baked against it; the exporter refuses a bone moved more than 0.1 mm, rig check a rest rotation off by more than 1e-3 per component. The mesh is moved to meet the rig, never the reverse — hence the strict T-pose in the reference image and the `[fit]` gate (reach 0.80–1.45 of wrist span, arm tips within 0.15 m of wrist height), whose message names the image as the fix |
| stature | `contract.json stature_m`; `[bones] reference_stature_m, min_stature_m, max_stature_m` | the auto-rig scales a body to the reference (1.80 m shipped); rig check refuses outside the band (1.4–2.2 m) — a prop mistaken for a body, or a mesh still in raw units |
| feet | `foot_tolerance_m` | the lowest skinned vertex within it of y = 0 (0.05 m shipped), or every clip hovers or sinks the character |
| facing | `contract.json front`; `[bones] front` | which way the **rest** pose faces in glTF axes (`+Z` shipped: the toes sit in front of the ankles). Clips may carry a half turn so the animated character faces an engine's forward; anything measured against the rest pose — a socket offset — uses the rest front or comes out mirrored |
| influences | `[export] max_influences` | weights per vertex (4), normalised to 1.0; every vertex weighted, zero-weighted vertices refused; the auto-rig aborts when more than `[rig] unweighted_abort_fraction` of the mesh comes back weightless rather than ship a statue |
| armature node | `[bones] armature_node` | the scene node above the root, identity transform, not a bone — and the first segment of every bone path |

## Extra bones

A bone the contract does not name is allowed below a contract bone as long
as no contract bone hangs beneath it: a lone leaf (a holster, a muzzle
point, a socket marker) or, when `[export] extra_bones` allows `run`, an
unbranched chain (hair, a tail). A leaf adds a new path without rewriting
any existing one, so it is safe by the same logic that makes a root bone
fatal. Nothing in the library animates them; a lifted body carries none.
`forge rig check` reports each as a note (`extra leaf bone Holster`), and a
note is not a failure. A contract bone *under* an extra is the inserted-bone
failure from the one rule, and no naming changes it.

## Export settings

`forge gen export` applies these headless and refuses a `.blend` that
breaks the contract before a byte is written, every check gathered first
so the fix is one round trip: one armature named as `armature_node`; every
contract bone with its contract parent; the rest pose the profile's to
`rest_tolerance_m`; extras only where allowed; meshes skinned,
untransformed (`transform_tolerance`), textured and packed. Then glTF
Binary, `+Y` up, object transforms applied so the armature is identity, no
glTF leaf bones (they would appear as `_end` strangers), ≤ `max_influences`
weights, modifiers applied, animation off, materials exported with every
image packed. The post-export self-check proves the file is self-contained
— one JSON chunk, one BIN chunk, no `uri` anywhere — because a glb that
references a texture on one disk renders pink on every other.

## Validating

```sh
forge rig check out/export/<name>.glb              # one file; exit 1 on any FAIL
forge rig check out/export/<name>.glb --out x.png  # and render it playing the reference clip
just check-mesh out/export/<name>.glb              # the same
just check-bodies                                  # every glb under assets/bodies
just rig                                           # re-export the contract, write the fixture mannequin
```

Findings print one per line, each starting with one of four marks:

| Mark | Means |
|---|---|
| `ok:` | the check passed — `all 55 contract bones present at contract depth`, `rest rotations match the contract`, `height 1.80 m, within 1.4-2.2 m`, `feet at y=0.000 m` |
| `note:` | worth saying, breaks nothing — an extra leaf bone |
| `WARN:` | a check that could not be made — `no reference clip 'walk' in the library; walk binding not checked — promote it, or bind this mesh in the studio to see what moves` |
| `FAIL:` | the contract is broken — `Hips found at depth 3, expected 2 — a parent above Hips rewrites every AnimationTargetId and every clip binds to nothing`; `reference clip: 0 bone(s) driven …` |

`forge promote body` runs the same findings and refuses on any `FAIL`;
`forge audit` runs them over every shipped body and reports `N/N bodies
conform to the contract`. The engine-side check spawns the mesh in a
headless app and reads its hierarchy as the engine will bind it — no
renderer, no adapter — so it is a CI gate and not an eye-render.

## Regenerating a profile

```sh
forge gen rig-build                                                 # rig.blend (+ rig.glb) from the fixture clip + [fingers]
cargo run -q -p forge_rig --example export_contract -- rigs/humanoid   # contract.json from rig.glb (what `just rig` runs)
cargo test -p forge_rig                                             # drift, layout, sockets, byte-for-byte
```

A rebuilt rig must land on the contract to `[fingers]
rebuild_tolerance_m` / `rebuild_rotation_tolerance`, and a re-exported
contract must reproduce the committed file byte for byte. If either moves,
something about the rig changed: bump `[profile] version` and expect every
clip in the library to need rebaking.

## Adding a second profile

Another profile is another directory beside `humanoid/` with the same five
entries, named in `forge.toml`. What has to exist before it can work:

- **A motion generator whose skeleton the profile supersets.** Clips bind by
  name path with nothing in between; this repository ships **no retarget**
  and will not grow one (every retargeting layer is a place for a clip to
  be subtly wrong in a way no number catches). A quadruped profile needs a
  quadruped motion source whose joints are that profile's driven set, named
  identically, and a `motion_skeleton.json` describing its take layout.
- `motion_skeleton.json` that `forge_motion` can read as the driven layout,
  and a reference clip that drives every joint of it, so `forge rig check`
  exercises binding fully.
- A `rig.blend` the auto-rig can open, a `fixture/` clip in the rest pose
  for `rig-build`, and `profile.toml` with every section filled: the fit
  gate is the T-pose gate and is humanoid-shaped — another body plan
  states its own rest pose and its own fit rule there.
- `sockets.json` may be empty; `authoring_frame` still has to be stated.

Until those exist, the shipped humanoid profile is the only one the toolkit
has been run on, and "a second profile" is a listed follow-up rather than a
path to try.
