//! Does a Rust bake reproduce what the Blender pipeline shipped?
//!
//! Four clips spanning the recipe space — strip + loop blend, detrend,
//! retime, plain idle — are baked from take + sidecar recipe and compared
//! channel-for-channel against the frozen Blender-era `.glb` fixtures. Only
//! rotations and the `Hips` translation are compared: the shipped files
//! carry 53 constant translation/scale channels this baker deliberately
//! drops (they imposed cskel27 bone lengths on any bound rig), so byte
//! equality is exactly the wrong bar.
//!
//! One deliberate divergence is factored out before comparing: today's bake
//! lands in **rig space** — `Edit::apply` turns the pose 180° about Y so
//! clips play facing −Z like the rig profile — while the Blender fixtures
//! froze the old take-space (+Z) output. The turn is a rigid whole-pose yaw,
//! so it lives entirely in the root: the fixture's `Hips` rotation channel
//! yawed by 180° and its translation with X/Z negated must equal ours
//! exactly, and every other joint is untouched.
//!
//! The baked bytes are also read back through this crate's own extractor and
//! rig parser, so the writer cannot pass by producing something only the
//! comparison code can read.
//!
//! A fifth clip, `gen_jump` under `tests/fixtures/rust`, was baked by this
//! very writer's previous home and exercises the one knob the Blender set
//! never reached (`y_mode`). Against it the bar IS bytes: the binary chunk
//! must match exactly and the JSON chunk must match apart from the
//! generator string, which names the crate that wrote it.

mod common;

use common::{Case, case, component_gap, humanoid_rig};
use forge_motion::skeleton::JOINT_COUNT;
use forge_motion::{BakeError, ClipChannels, Edit, RigDef, bake};
use glam::{Quat, Vec3};

/// The deliberate rig-space turn: what maps a frozen take-space fixture onto
/// today's output.
fn rig_space_yaw() -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI)
}

/// A fixture root-track sample, carried into rig space.
fn flip_xz(p: Vec3) -> Vec3 {
    Vec3::new(-p.x, p.y, -p.z)
}

const CLIPS: [&str; 4] = ["gen_walk", "gen_roll", "gen_pistol_shoot", "gen_rifle_idle"];

/// See `tests/convention.rs` for how these bounds relate to the measured
/// float32 residue of the Blender chain.
const ROTATION_TOLERANCE: f32 = 1e-4;
const TRANSLATION_TOLERANCE_M: f32 = 5e-4;

#[test]
fn baked_channels_match_the_shipped_clips() {
    let rig = humanoid_rig();
    for name in CLIPS {
        let Case {
            take,
            edit,
            clip_name,
            shipped,
        } = case("blender", name);
        let baked =
            bake(&take, &edit, &rig, &clip_name).unwrap_or_else(|e| panic!("{name} bake: {e}"));

        let ours = ClipChannels::from_glb(&baked)
            .unwrap_or_else(|e| panic!("{name}: baked glb does not reload: {e}"));
        let theirs =
            ClipChannels::from_glb(&shipped).unwrap_or_else(|e| panic!("{name} shipped glb: {e}"));

        assert_eq!(ours.name.as_deref(), Some(clip_name.as_str()), "{name}");
        assert_eq!(
            ours.channel_count, 28,
            "{name}: 27 rotations + 1 Hips translation, nothing else"
        );
        assert_eq!(
            theirs.channel_count, 81,
            "{name}: shipped baseline changed — is this still a Blender bake?"
        );

        let mut worst = 0.0_f32;
        for j in 0..JOINT_COUNT {
            let ours = ours.rotations[j]
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: baked file lacks rotation track {j}"));
            let theirs = theirs.rotations[j]
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: shipped file lacks rotation track {j}"));
            assert_eq!(ours.values.len(), theirs.values.len(), "{name} joint {j}");
            for ((&a, &b), (&ta, &tb)) in ours
                .values
                .iter()
                .zip(&theirs.values)
                .zip(ours.times.iter().zip(&theirs.times))
            {
                // Joint 0 is the Hips node, where the rig-space turn lives.
                let b = if j == 0 { rig_space_yaw() * b } else { b };
                worst = worst.max(component_gap(a, b));
                assert!((ta - tb).abs() < 1e-4, "{name} joint {j}: {ta} vs {tb} s");
            }
        }
        println!("{name}: worst rotation component gap {worst:.2e}");
        assert!(
            worst < ROTATION_TOLERANCE,
            "{name}: baked rotations diverge from shipped by {worst:.2e}"
        );

        let ours_hips = ours.hips_translation.as_ref().expect("baked Hips track");
        let theirs_hips = theirs
            .hips_translation
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: shipped file lacks a Hips translation track"));
        assert_eq!(ours_hips.values.len(), theirs_hips.values.len(), "{name}");
        let worst_m = ours_hips
            .values
            .iter()
            .zip(&theirs_hips.values)
            .map(|(a, b)| (*a - flip_xz(*b)).abs().max_element())
            .fold(0.0_f32, f32::max);
        println!("{name}: worst Hips gap {:.4} mm", worst_m * 1000.0);
        assert!(
            worst_m < TRANSLATION_TOLERANCE_M,
            "{name}: baked Hips translation diverges from shipped by {} mm",
            worst_m * 1000.0
        );
    }
}

/// Split a GLB into its JSON text and its binary chunk, the two things the
/// byte-level parity test compares separately.
fn chunks(glb: &[u8]) -> (String, &[u8]) {
    let u32_at =
        |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().expect("4 bytes")) as usize;
    assert_eq!(&glb[..4], b"glTF");
    let json_len = u32_at(12);
    assert_eq!(&glb[16..20], b"JSON");
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("JSON chunk is utf-8");
    let bin_at = 20 + json_len;
    let bin_len = u32_at(bin_at);
    assert_eq!(&glb[bin_at + 4..bin_at + 8], b"BIN\0");
    (
        json.trim_end().to_owned(),
        &glb[bin_at + 8..bin_at + 8 + bin_len],
    )
}

/// Replace the `asset.generator` string so two bakes of the same clip by
/// differently named crates can be compared as JSON text.
fn without_generator(json: &str) -> String {
    let start = json
        .find("\"generator\":\"")
        .expect("asset.generator present");
    let value_start = start + "\"generator\":\"".len();
    let value_end = value_start + json[value_start..].find('"').expect("generator closes");
    format!(
        "{}<generator>{}",
        &json[..start + "\"generator\":\"".len()],
        &json[value_end..]
    )
}

/// A clip the previous home of this writer shipped, rebuilt here from its
/// take and recipe: the `forge audit` claim (a clip reproduces from its own
/// record) held at the bar of bytes rather than millimetres.
#[test]
fn a_rust_era_clip_rebuilds_byte_for_byte() {
    let rig = humanoid_rig();
    let Case {
        take,
        edit,
        clip_name,
        shipped,
    } = case("rust", "gen_jump");
    assert_eq!(
        edit.y_mode,
        forge_motion::YMode::Detrend,
        "gen_jump is the fixture that exercises y_mode"
    );
    let baked = bake(&take, &edit, &rig, &clip_name).expect("bake gen_jump");

    let (ours_json, ours_bin) = chunks(&baked);
    let (theirs_json, theirs_bin) = chunks(&shipped);
    assert!(
        theirs_json.contains("\"generator\":\"lab_bake "),
        "the fixture is no longer the clip the previous writer shipped"
    );
    assert!(
        ours_json.contains("\"generator\":\"forge_motion "),
        "this writer names itself"
    );
    assert_eq!(
        without_generator(&ours_json),
        without_generator(&theirs_json),
        "gen_jump: glTF JSON differs beyond the generator string"
    );
    assert_eq!(
        ours_bin.len(),
        theirs_bin.len(),
        "gen_jump: binary chunk length differs"
    );
    let first_diff = ours_bin.iter().zip(theirs_bin).position(|(a, b)| a != b);
    assert_eq!(
        first_diff, None,
        "gen_jump: binary chunk first differs at byte {first_diff:?}"
    );
}

/// A baked clip carries the whole rest pose and skin, so it must itself be
/// readable as a rig — the same closure the shipped clips have always had.
#[test]
fn a_baked_clip_round_trips_as_a_rig() {
    let rig = humanoid_rig();
    let Case {
        take,
        edit,
        clip_name,
        ..
    } = case("blender", "gen_walk");
    let baked = bake(&take, &edit, &rig, &clip_name).expect("bake gen_walk");

    let reread = RigDef::from_glb(&baked).expect("baked glb reads back as a rig");
    for (a, b) in rig.bones.iter().zip(&reread.bones) {
        assert_eq!(a.name, b.name);
        assert!(
            component_gap(a.rotation, b.rotation) < 1e-6,
            "{}: rest rotation drifted through the round trip",
            a.name
        );
        assert!(
            (a.translation - b.translation).abs().max_element() < 1e-6,
            "{}: rest translation drifted through the round trip",
            a.name
        );
        for (x, y) in a.inverse_bind.iter().zip(&b.inverse_bind) {
            assert!(
                (x - y).abs() < 1e-6,
                "{}: inverse bind matrix drifted through the round trip",
                a.name
            );
        }
    }
}

/// An edit that trims a take to nothing must refuse, not write a clip with
/// no keys to sample.
#[test]
fn an_over_trimmed_take_is_refused() {
    let rig = humanoid_rig();
    let Case { take, .. } = case("blender", "gen_pistol_shoot");
    let edit = Edit {
        trim_start: take.frames(),
        ..Edit::default()
    };
    // Edit::apply returns the take unchanged when the trim would leave under
    // two frames, so build the degenerate case directly instead.
    let mut short = take.clone();
    short.rotations.truncate(1);
    short.root.truncate(1);
    let error = bake(&short, &edit, &rig, "Broken").expect_err("must refuse one frame");
    assert!(
        matches!(error, BakeError::TooShort { frames: 1 }),
        "{error}"
    );
}
