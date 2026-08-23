//! The discovery experiment, kept as a test: what exactly did the Blender
//! pipeline put in the shipped `.glb` channels?
//!
//! Each shipped clip is rebuilt in memory — raw take, sidecar recipe,
//! `Edit::apply` — and compared against the channels extracted from the
//! frozen Blender-era fixture. The claim under test is the crate-level
//! convention: rotation channels are the two-sided rest conjugation
//! `Crest(parent)⁻¹ · L · Crest(j)` with rest frames from the humanoid
//! profile's `rig.glb`, and the `Hips` translation channel is the edited
//! root track verbatim. No writer is involved, so if this passes and the
//! proof test fails, the bug is in serialization, not in the maths.
//!
//! `Edit::apply` now lands in rig space (the deliberate 180° turn on the
//! root — see `forge_motion::edit`), while the fixtures froze the old
//! take-space output; the comparison carries the fixture's `Hips`
//! channels through the same turn before matching, and every other joint
//! must match untouched.
//!
//! The three clips cover the recipe space: `gen_walk` strips travel and
//! loop-blends, `gen_roll` detrends a travelling one-shot, and
//! `gen_pistol_shoot` retimes.

mod common;

use common::{case, component_gap, humanoid_rig};
use forge_motion::ClipChannels;
use forge_motion::skeleton::JOINT_COUNT;
use glam::{Quat, Vec3};

/// Worst quaternion-component gap seen empirically is in the 1e-6 range —
/// float32 noise from Blender's euler round-trip. 1e-4 is two orders above
/// that and far below anything a pose could show.
const ROTATION_TOLERANCE: f32 = 1e-4;

/// Half a millimetre on the hips; measured residue is under a micrometre.
const TRANSLATION_TOLERANCE_M: f32 = 5e-4;

#[test]
fn shipped_channels_are_the_rest_conjugated_take() {
    let rig = humanoid_rig();
    for name in ["gen_walk", "gen_roll", "gen_pistol_shoot"] {
        let case = case("blender", name);
        let edited = case.edit.apply(&case.take);
        let shipped = ClipChannels::from_glb(&case.shipped)
            .unwrap_or_else(|e| panic!("{name} shipped glb: {e}"));

        let mut worst = 0.0_f32;
        let mut unconjugated_worst = 0.0_f32;
        for j in 0..JOINT_COUNT {
            let track = shipped.rotations[j]
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: no rotation track for joint {j}"));
            assert_eq!(
                track.values.len(),
                edited.frames(),
                "{name}: shipped keys vs edited frames for joint {j}"
            );
            for (t, (&shipped_q, frame)) in track.values.iter().zip(&edited.rotations).enumerate() {
                let ours = rig.to_node_rotation(j, frame[j]);
                // The fixture's root channel predates the rig-space turn.
                let shipped_now = if j == 0 {
                    Quat::from_rotation_y(std::f32::consts::PI) * shipped_q
                } else {
                    shipped_q
                };
                worst = worst.max(component_gap(shipped_now, ours));
                unconjugated_worst = unconjugated_worst.max(component_gap(shipped_q, frame[j]));
                let expected = t as f32 / edited.fps;
                assert!(
                    (track.times[t] - expected).abs() < 1e-4,
                    "{name}: key {t} at {} s, expected {expected} s",
                    track.times[t]
                );
            }
        }
        println!("{name}: worst rotation component gap {worst:.2e}");
        assert!(
            worst < ROTATION_TOLERANCE,
            "{name}: conjugated take diverges from shipped channels by {worst:.2e}"
        );
        // The conjugation is load-bearing: the raw ARDY-local rotations are
        // nowhere near the shipped channels, so a pass above cannot be a
        // comparison that would accept anything.
        assert!(
            unconjugated_worst > 0.5,
            "{name}: raw take already matches shipped ({unconjugated_worst:.2e}) — \
             the experiment is not discriminating"
        );

        let hips = shipped
            .hips_translation
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: no Hips translation track"));
        assert_eq!(hips.values.len(), edited.frames(), "{name}: Hips keys");
        let worst_m = hips
            .values
            .iter()
            .zip(&edited.root)
            .map(|(a, b)| {
                let a_now = Vec3::new(-a.x, a.y, -a.z);
                (a_now - *b).abs().max_element()
            })
            .fold(0.0_f32, f32::max);
        println!("{name}: worst Hips gap {:.4} mm", worst_m * 1000.0);
        assert!(
            worst_m < TRANSLATION_TOLERANCE_M,
            "{name}: edited root track diverges from shipped Hips channel by {} mm",
            worst_m * 1000.0
        );
    }
}
