//! The two headless renders, end to end, on a real adapter.
//!
//! A sheet of a freshly baked clip on the fixture mannequin, and the seven
//! views of a raw lift with culling off. Neither depends on a sample
//! library: the body is written from the rig contract and the clip is baked
//! from a fixture take into a temporary asset root, and the lift is a unit
//! cube TRELLIS produced from a test image, pinned under `tests/fixtures`.
//!
//! Needs a wgpu adapter — llvmpipe is enough — and no display. Without one
//! Bevy panics while the app is built, so the tests catch that one panic,
//! say why they are skipping, and pass; a CI box without Mesa is not a
//! rendering bug. The PNGs land under `out/p3/` for a person to look at, and
//! the assertions are about what a person cannot be asked to check every
//! run: that something was drawn, that the playhead moved, that the clip
//! drives every driven bone.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::math::UVec2;
use forge_capture::ink_fraction;
use forge_motion::{Edit, InPlace, RigDef, Take, YMode, bake};
use forge_studio::{
    SheetRequest, Shot, Stage, View, ViewsRequest, render_clip_sheet, render_views,
};

const TAKE: &str = "../forge_motion/tests/fixtures/blender/gen_walk.npz";

/// Bevy's message when `request_adapter` comes back empty.
const NO_ADAPTER: &str = "Unable to find a GPU";

/// Two render apps building in parallel threads would race for the same
/// task pools; the tests run one at a time.
static GPU: Mutex<()> = Mutex::new(());

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Where the PNGs go — gitignored, and where a person looks.
fn out_dir() -> PathBuf {
    let dir = manifest_path("../../out/p3");
    std::fs::create_dir_all(&dir).expect("out/p3");
    dir
}

/// Run a render, or explain why it could not run here.
///
/// Returns `None` only for the missing-adapter panic; any other panic is
/// re-raised so a real failure stays a failure.
fn on_gpu<T>(what: &str, render: impl FnOnce() -> T) -> Option<T> {
    let _serial = GPU
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(render)) {
        Ok(value) => Some(value),
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            if message.contains(NO_ADAPTER) {
                println!("skipping {what}: no GPU adapter reachable from this process");
                None
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// A temporary asset root holding the mannequin and a freshly baked clip.
fn mannequin_and_clip() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile =
        forge_rig::RigProfile::load(&manifest_path("../../rigs/humanoid")).expect("profile");
    forge_rig::fixture::write_mannequin(&profile, &dir.path().join("mannequin.glb"))
        .expect("mannequin");
    let rig = RigDef::from_glb(&std::fs::read(profile.glb_path()).expect("rig.glb")).expect("rig");
    let take = Take::read(manifest_path(TAKE)).expect("read take");
    let edit = Edit {
        in_place: InPlace::Strip,
        y_mode: YMode::Strip,
        loop_blend_s: 0.2,
        ..Edit::default()
    };
    let bytes = bake(&take, &edit, &rig, "walk").expect("bake");
    std::fs::create_dir_all(dir.path().join("clips")).expect("clips dir");
    std::fs::write(dir.path().join("clips/walk.glb"), bytes).expect("write clip");
    dir
}

#[test]
fn a_sheet_of_a_fresh_clip_on_the_mannequin_moves_and_binds() {
    let root = mannequin_and_clip();
    let stage = Stage::new(root.path(), "mannequin.glb");
    let request = SheetRequest {
        frames: 4,
        views: vec![View::ThreeQuarter, View::Front],
        head_row: true,
        ..SheetRequest::new("clips/walk.glb")
    };
    let Some(shot) = on_gpu("the clip sheet", || {
        render_clip_sheet(&stage, &request).expect("render")
    }) else {
        return;
    };
    let png = out_dir().join("test_sheet.png");
    shot.save_png(&png).expect("save");
    println!("{}\n-> {}", shot.summary(), png.display());

    assert_eq!(
        shot.cells,
        4 * 2 + 3,
        "four poses from two views plus the head row"
    );
    assert_eq!(shot.times.len(), 4);
    assert!(
        shot.duration > 0.5,
        "the walk is {:.2}s long",
        shot.duration
    );
    let diff = shot.diff.as_ref().expect("a sheet always diffs the clip");
    assert_eq!(
        diff.bound.len(),
        27,
        "every driven bone is bound:\n{}",
        diff.report()
    );
    assert_eq!(diff.orphaned, 0, "{}", diff.report());
    assert!(!shot.is_frozen(), "the playhead did not move between cells");
    assert!(shot.failure().is_none(), "{:?}", shot.failure());
    let ink = ink_fraction(&shot.sheet);
    assert!(ink > 0.0, "the sheet is blank");
    assert!(!shot.adapter.is_empty());
}

#[test]
fn the_seven_views_of_a_raw_lift_show_it_inside_and_out() {
    let stage = Stage::from_path(&manifest_path("tests/fixtures/testbox.glb")).expect("fixture");
    let request = ViewsRequest {
        cull_off: true,
        cell: UVec2::new(384, 384),
        ..ViewsRequest::default()
    };
    let Some(shot) = on_gpu("the views render", || {
        render_views(&stage, &request).expect("render")
    }) else {
        return;
    };
    let png = out_dir().join("test_views.png");
    shot.save_png(&png).expect("save");
    println!("{}\n-> {}", shot.summary(), png.display());

    assert_eq!(
        shot.cells, 7,
        "front, back, left, right and three head close-ups"
    );
    assert!(shot.diff.is_none(), "no clip, no diff");
    assert!(shot.failure().is_none());
    let (lo, hi) = shot.bounds;
    let size = hi - lo;
    // A raw TRELLIS lift is a unit cube about the origin.
    for axis in [size.x, size.y, size.z] {
        assert!((axis - 1.0).abs() < 0.02, "bounds {size}");
    }
    assert!(
        lo.y < -0.45 && hi.y > 0.45,
        "centred on the origin, not standing on it: {lo} .. {hi}"
    );
    assert!(ink_fraction(&shot.sheet) > 0.0, "the views sheet is blank");
    assert_eq!(
        shot.sheet.width(),
        4 * shot.cell.x + 3 * 2,
        "four cells to a row with the default gutter"
    );
}

/// The rest-pose views of a rigged body frame it standing on the floor, and
/// a pose held from a clip reports the clip's binding beside the views.
#[test]
fn views_of_a_body_can_hold_a_pose_from_a_clip() {
    let root = mannequin_and_clip();
    let stage = Stage::new(root.path(), "mannequin.glb");
    let request = ViewsRequest {
        views: vec![View::Front, View::Left],
        head_row: false,
        pose: Some((String::from("clips/walk.glb"), 0.3)),
        ..ViewsRequest::default()
    };
    let Some(shot) = on_gpu("the posed views", || {
        render_views(&stage, &request).expect("render")
    }) else {
        return;
    };
    let summary: String = Shot::summary(&shot);
    println!("{summary}");
    assert_eq!(shot.cells, 2);
    assert_eq!(shot.times, [0.3]);
    assert!(shot.diff.as_ref().is_some_and(|d| d.bound.len() == 27));
    assert!(
        shot.bounds.0.y.abs() < 0.1,
        "a body stands on y = 0: {}",
        shot.bounds.0
    );
    assert!(ink_fraction(&shot.sheet) > 0.0);
}
