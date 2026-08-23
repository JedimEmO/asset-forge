//! Read a real ARDY take and check it against what numpy reports.
//!
//! These assertions are transcribed from `np.load` on the same file, so a
//! silent format change — a renamed array, a dtype switch, a joint-count
//! change — fails here rather than as a subtly wrong pose later.
//!
//! The takes are pinned copies under `tests/fixtures`, not a live source
//! directory: re-promoting a clip replaces its source take in place, and a
//! transcription checked against a file that can change is a test that
//! fails on someone else's schedule.

mod common;

use std::path::PathBuf;

use common::fixtures;
use forge_motion::Take;

fn fixture(name: &str) -> PathBuf {
    fixtures("blender").join(name)
}

#[test]
fn reads_a_shipped_take() {
    let path = fixture("gen_roll.npz");
    let take =
        Take::read(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));

    // numpy: local_rot_mats (80, 27, 3, 3), fps 20, text as below.
    assert_eq!(take.frames(), 80);
    assert_eq!(take.root.len(), 80);
    assert!((take.fps - 20.0).abs() < f32::EPSILON, "fps {}", take.fps);
    assert_eq!(
        take.prompt,
        "A person dives forward into a roll and gets back up."
    );
    assert!((take.duration() - 4.0).abs() < 0.01, "{}", take.duration());
}

#[test]
fn rotations_are_unit_quaternions() {
    let take = Take::read(fixture("gen_roll.npz")).expect("read");
    for (t, frame) in take.rotations.iter().enumerate() {
        for (j, q) in frame.iter().enumerate() {
            assert!(
                (q.length() - 1.0).abs() < 1e-4,
                "joint {j} frame {t} is not unit: {}",
                q.length()
            );
        }
    }
}

#[test]
fn the_root_stands_on_the_ground_at_a_human_height() {
    // Sanity that the axis convention survived the read: ARDY is Y-up, and a
    // 1.8 m human's hips sit near 0.9 m. A Z-up misread would put this near 0.
    let take = Take::read(fixture("gen_roll.npz")).expect("read");
    let y = take.root[0].y;
    assert!((0.6..1.3).contains(&y), "hips at y={y}, expected ~0.9");
}

#[test]
fn a_shipped_take_carries_foot_contacts() {
    // gen_walk is a steady two-beat gait, so beyond shape and presence the
    // labels themselves are checkable: both feet must touch down at some
    // point, and no foot is planted on every frame.
    let take = Take::read(fixture("gen_walk.npz")).expect("read");
    let contacts = take
        .contacts
        .as_ref()
        .expect("shipped takes carry foot_contacts.npy");
    assert_eq!(contacts.len(), take.frames());

    for (column, name) in ["LeftFoot", "LeftToeBase", "RightFoot", "RightToeBase"]
        .iter()
        .enumerate()
    {
        let down = contacts.iter().filter(|c| c[column]).count();
        assert!(down > 0, "{name} never touches in a walk");
        assert!(down < contacts.len(), "{name} never lifts in a walk");
    }
}

/// Every pinned take reads — the Blender-era oracle takes and the one
/// Rust-era clip source — so a reader regression that only bites a take
/// the other tests never open still fails here.
#[test]
fn every_fixture_take_reads() {
    let mut count = 0;
    for dir in ["blender", "rust"] {
        let dir = fixtures(dir);
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten()
        {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "npz") {
                let take = Take::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(take.frames() > 0, "{} is empty", path.display());
                count += 1;
            }
        }
    }
    assert_eq!(count, 5, "expected the five pinned takes, found {count}");
}
