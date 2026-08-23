# forge_motion

Motion takes as data: read an ARDY `.npz`, apply an edit recipe, derive
footsteps, and bake the result into a `.glb` animation clip on a rig — no
engine, no Blender, no generator installed.

```rust,no_run
use forge_motion::{Edit, InPlace, RigDef, Take, bake};

let take = Take::read("out/sweeps/walk/walk__d2_c2.0_s0_3.npz")?;
let rig = RigDef::from_glb(&std::fs::read("rigs/humanoid/rig.glb")?)?;
let edit = Edit { in_place: InPlace::Strip, loop_blend_s: 0.2, ..Edit::default() };
let glb = bake(&take, &edit, &rig, "walk-loop")?;
std::fs::write("assets/clips/walk.glb", glb)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What is here

| Module | What it does |
|---|---|
| `take`, `npy` | `Take::read` — the `.npz` container and the `.npy` members inside it (uncompressed, so `zip` only locates them): joint rotations, root track, foot contacts, fps |
| `skeleton` | the driven layout (`cskel27`: joint names, parents, feet, hands) — held equal to the profile's `motion_skeleton.json` by a test |
| `edit` | `Edit::apply` — trims, retime, in-place (`off`, `strip`, `detrend`) and height modes, loop blend, and the four style knobs (exaggerate, arm bend, lean, shoulder back). Every recipe ends by turning the take into rig space |
| `events` | `footsteps` — foot-plant events derived from the take's contact labels |
| `rig` | `RigDef::from_glb` — the rest transforms of the driven bones read from a rig's `.glb`, refusing a duplicate, a missing joint, a non-identity armature |
| `bake` | `bake(take, edit, rig, name)` — one `Armature` root, the driven bones with the rig's rest transforms, rotation channels conjugated into the rig's bone frames, the root translation verbatim; a three-vertex anchor triangle so every loader keeps the skeleton |
| `channels` | `ClipChannels::from_glb` — read a baked clip's channels back, which is how the audit compares a rebuild to a shipped file |

## Why it is exact

A take's rotations are joint-local in ARDY's own bone frames; a rig whose
bones rest in other frames needs them re-expressed as
`Crest(parent)⁻¹ · L(j, t) · Crest(j)`. The correction is two-sided and
reaches into the parent's rest frame — a single-bone conjugation is wrong by
up to 169° in a way that looks like a plausible pose. The convention was
measured against clips the old Blender pipeline shipped, not assumed:
`tests/convention.rs` and `tests/bake_matches_shipped.rs` hold the native
bake to those frozen fixtures to float noise (1e-6 per quaternion component,
under a thousandth of a millimetre on the root track).

The bake is deterministic for a fixed input, which is what lets a library
claim every clip reproduces from its own record (`forge audit`).

Part of [asset-forge](https://github.com/JedimEmO/asset-forge).
