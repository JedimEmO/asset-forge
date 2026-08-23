# forge_rig

A rig profile as data. A profile is a directory — `rigs/humanoid/` in
[asset-forge](https://github.com/JedimEmO/asset-forge) ships one — holding the
bone contract every body must carry (`contract.json`), the attachment points
a prop rides on (`sockets.json`) and the driven layout a motion take writes
(`motion_skeleton.json`), beside the artifacts they were derived from
(`rig.glb`, `rig.blend`). This crate reads that directory, cross-checks the
three files, re-derives the bone table from a `.glb` so file and artifact can
be held to each other, and measures a skinned or unskinned `.glb` without an
engine.

Nothing here is a constant: the crate does not know the number 55 or the
name `Hips`. The contract says which bone is the root and which are driven;
the code knows how to check.

```rust,no_run
use std::path::Path;
use forge_rig::{RigProfile, check_drift, derive_from_glb};

let profile = RigProfile::load(Path::new("rigs/humanoid"))?;
let bytes = std::fs::read(profile.glb_path())?;
let derived = derive_from_glb(&bytes)?;
assert!(check_drift(&profile.contract, &derived.bones).is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What is here

| Module | What it does |
|---|---|
| `lib` | `Contract`, `Sockets`, `MotionSkeleton`, `RigProfile::load` (the cross-checks: every socket on a contract bone, the driven bones exactly the layout's joints, the driven parent chain collapsing to the layout's parents); `derive_from_glb`, `check_drift`, `missing_joints`, `rest_world` |
| `export` | `contract.json` from a `rig.glb` plus a motion skeleton — what `forge rig export-contract` and the `export_contract` example run. Floats print at shortest round-trip precision, so an unchanged rig reproduces the file byte for byte |
| `measure` | `measure_glb` — vertices, triangles, bones skinned, lowest vertex, bounds — read with the container format alone; refuses a file that is not self-contained |
| `fixture` | `write_mannequin` — a rigid-weighted capsule figure skinned to the contract, written in pure Rust and byte-deterministic, so tests never depend on a sample body |

Two facts about a contract's shape: **order is glb node order** (the
`parent` indices point into the array, and drift is checked position for
position), and **the root has no parent** — an engine that binds clips by
hashing each bone's full name path sees every hash change when a parent
appears above the root, and every clip silently binds to nothing.

## Tests

`cargo test -p forge_rig` holds the shipped profile to itself: the bones
re-derived from `rig.glb` match `contract.json` within 1e-5; a re-exported
contract reproduces the committed file byte for byte; the driven set is the
motion layout; every socket sits on a contract bone; the fixture mannequin
passes `measure_glb`. No GPU, no Blender, no sample library.

Part of [asset-forge](https://github.com/JedimEmO/asset-forge); the rules it
enforces are written down in `designs/rig-contract.md` there.
