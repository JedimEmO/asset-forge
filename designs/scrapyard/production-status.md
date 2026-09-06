# Scrapyard first production batch

Generated 2026-09-04. Angled top-down 3D arena, scavenger versus rogue industrial machines.
This batch contains three accepted library assets.

| Asset | Status | File |
| --- | --- | --- |
| Scrap rifle | Accepted static prop, 5,808 triangles, 0.85 m long | ../../assets/models/scrapyard_rifle.glb |
| Rusher | Accepted rigged body, 23,658 triangles, 55 bones, exported height 1.80 m | ../../assets/bodies/scrapyard_rusher.glb |
| Scavenger | Accepted rigged body, 23,237 triangles, 55 bones, 1.80 m | ../../assets/bodies/scrapyard_scavenger.glb |

## Validation

- Rusher: all 12 rig findings pass, 27 animated bones, zero orphan curves. Worst planted-foot contact +0.049 m within the 0.050 m tolerance.
- Both characters rendered and visually reviewed with all seven existing clips: idle, walk, death, jump, pistol_shoot, roll, sword_chop.
- These clips demonstrate compatibility; they are not the final run-and-gun animation set.
- Scavenger: all 12 rig findings pass; worst planted-foot contact -0.048 m within the unchanged 0.050 m tolerance.
- Library verification: 52 checks passed. Manifest matches its sidecars.
- Audit: all seven clips rebuild byte-for-byte and reproduce their poses; all four library bodies conform. Meshes and bodies claim integrity only.
- No commits made.

## Scavenger alignment resolved

The seed-7 mesh with skinning temperature 0.7 now passes the original walk check.
Its backpack skewed bounding-box centering, leaving the legs forward of the
skeleton's fixed depth plane. Preparation now supports a recorded depth offset;
this body uses -0.12 m along glTF +Z. Skinning's second preparation pass now
preserves stature, yaw, depth offset and triangle budget from the first record.

After preparation, skinning and export, worst planted-foot penetration fell
from 0.097 m to 0.048 m. The tolerance and shared walk clip were unchanged.
All seven existing animation strips were reviewed again before promotion.
The samples demonstrate rig compatibility; rifle holding and combat-specific
animation still need dedicated work.

Eight fresh walk takes were explored separately; candidate comparisons did not
resolve the old body's contact failure. They were not substituted into the library.
Historical rejected exports remain under drafts/; use the accepted library body.
The old failure report is previews/scavenger-rig-check-before.txt; the passing
report is previews/scavenger-rig-check.txt. Preview sheets show the accepted body.

Pipeline regression tests: 248 passed, 1 skipped. A transient SkinTokens
voxelization failure succeeded on retry without changing the backend.

## Design and pipeline findings

- Rusher reference v1 produced a hollow sensor head with seeds 42 and 7. Reference v2 replaces it with a solid block and covered joints; seed 42 closes the head.
- Rusher preparation requested 1.65 m, but the skin/reprepare/export path produced 1.80 m with motion_scale 0.9167. This discrepancy is recorded, not silently corrected. The second-pass setting loss is now fixed for future generation; the accepted rusher has not been regenerated.
- The rusher sensor is dim in the baked texture. The game supplies a red team ring and explicit attack/impact feedback for runtime readability.
- The rifle has an axial grip 0.28 m from the stock. Batch02 reviewed its actual left-handed holding pose and records the corrected hand_l consumer attachment.
- Arena image is concept art only. Its ground needs less surface noise for production.
- The first batch did not measure crowd rendering. See the Bevy game verification report for subsequent runtime checks.

## Provenance and remaining work

Images were made with OpenAI built-in image_gen; prompts and original references are retained here.
Meshes use TRELLIS.2; characters use SkinTokens and the project's humanoid rig.
Every accepted asset was promoted through Forge's record and manifest workflow.
The current nvdiffrast texture baker is marked non-commercial in the generation records.
These outputs do not establish commercial-release readiness.

The [second production batch](batch-02/README.md) now adds the reviewed rifle
attachment, player aim/run/fire and rusher rush/attack clips, three combat-effect
atlases, an accepted magnet pickup, seven sound effects and a 32-second combat
music loop. Repair/overdrive meshes remain rejected drafts; their reference
images are retained for future generation and runtime geometric pickups can
represent them. The current library passes 83 integrity checks. Gameplay and
performance validation are tracked with the Bevy game.

The playable result is [SCRAPLINE // LAST SHIFT](../../crates/scrapyard_arena/README.md). Its runtime combines these accepted assets with procedural arena and machine meshes, meaningful upgrade choices, ten waves and a Foreman finale.
