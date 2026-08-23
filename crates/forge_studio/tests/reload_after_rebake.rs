//! Does reloading a `.glb` swap the clip under the handle already in hand?
//!
//! The studio leans on exactly this after a re-promote replaces a shipped
//! clip: the library keeps its handle and graph node, calls
//! `AssetServer::reload` on the labelled path, and trusts the `#Animation0`
//! beneath it to take on the new bake. If reload ever stops reaching labelled
//! sub-assets, the studio goes back to showing the old motion until a restart
//! — the failure that looks, from the reviewer's chair, like a promote that
//! did nothing.
//!
//! Two claims, two tests: that Bevy's reload does what the studio assumes,
//! and that the studio's own rescan notices a re-promote, a new clip and a
//! deleted one without being told. Both run with no window, no rendering and
//! no GPU adapter, in a temporary project built through `forge_library`'s own
//! doors, so the bytes that change are bytes a real promote wrote.

use std::path::{Path, PathBuf};

use bevy::{animation::AnimationClip, asset::LoadState, prelude::*};
use forge_library::{
    Project,
    promote::{PromoteClip, promote_clip},
    schema::{Actor, ClipRecipe},
};
use forge_studio::{
    binding::headless_app,
    studio::{
        StudioConfig,
        audio_view::AudioLibrary,
        library::{ClipLibrary, LibraryView, ModelLibrary, Selection, discover, rescan_models},
        rig::Rig,
    },
};

/// The take every clip here is baked from.
const TAKE: &str = "../forge_motion/tests/fixtures/blender/gen_roll.npz";
/// A second take, for the clip that arrives while the window is open.
const OTHER_TAKE: &str = "../forge_motion/tests/fixtures/rust/gen_jump.npz";

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// An empty project with the profile installed.
fn temp_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "reload_test").expect("init");
    project
        .install_profile(&manifest_path("../../rigs/humanoid"))
        .expect("install the profile");
    (dir, project)
}

/// Promote `take` as `name` with `recipe`, replacing what is there.
fn promote(project: &Project, name: &str, take: &str, recipe: ClipRecipe) {
    promote_clip(
        project,
        &PromoteClip {
            name: name.to_owned(),
            take_path: manifest_path(take),
            recipe,
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Human,
            take_record: None,
            overwrite: true,
        },
    )
    .expect("promote");
}

/// A recipe that leaves a visibly shorter clip than the identity one.
fn trimmed() -> ClipRecipe {
    ClipRecipe {
        trim_start_s: 0.25,
        trim_end_s: 1.3,
        ..ClipRecipe::default()
    }
}

/// Pump updates until `check` answers, or fail with `what`.
fn wait_for<T>(app: &mut App, what: &str, check: impl Fn(&mut App) -> Option<T>) -> T {
    for _ in 0..900 {
        app.update();
        if let Some(found) = check(app) {
            return found;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

/// Let the studio's poll fire once.
///
/// The timer runs on the app's clock, which is real time on `MinimalPlugins`
/// — but `Time<Virtual>` clamps any one frame's delta to 250 ms, so one long
/// sleep before one update would advance the poll a quarter second. Several
/// short frames add up to past the two-second poll.
fn poll_tick(app: &mut App) {
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.update();
    }
}

fn duration_of(app: &mut App, handle: &Handle<AnimationClip>) -> Option<f32> {
    let server = app.world().resource::<AssetServer>();
    if let Some(LoadState::Failed(err)) = server.get_load_state(handle.id()) {
        panic!("clip failed to load: {err}");
    }
    app.world()
        .resource::<Assets<AnimationClip>>()
        .get(handle)
        .map(AnimationClip::duration)
}

#[test]
fn reload_swaps_the_clip_under_an_existing_handle() {
    let (_dir, project) = temp_project();
    promote(&project, "roll", TAKE, ClipRecipe::default());

    let mut app = headless_app(&project.assets);
    let handle: Handle<AnimationClip> = app
        .world()
        .resource::<AssetServer>()
        .load("clips/roll.glb#Animation0");
    let original = wait_for(&mut app, "the original clip", |app| {
        duration_of(app, &handle)
    });

    // What the replacement should measure, read through its own root so the
    // expectation cannot drift from the fixture.
    let expected = {
        promote(&project, "other", TAKE, trimmed());
        let handle: Handle<AnimationClip> = app
            .world()
            .resource::<AssetServer>()
            .load("clips/other.glb#Animation0");
        wait_for(&mut app, "the replacement clip", |app| {
            duration_of(app, &handle)
        })
    };
    assert!(
        (original - expected).abs() > f32::EPSILON,
        "the two bakes must differ in length for this test to see anything"
    );

    // The bake: same path, new bytes. Nothing watches the file system here —
    // the studio ships without `file_watcher` — so nothing may change yet.
    promote(&project, "roll", TAKE, trimmed());

    // The reload must name the *labelled* path. The server tracks handles by
    // the exact path they were loaded under, and nothing ever loaded the bare
    // source — so reloading it finds no handle and quietly does nothing.
    app.world()
        .resource::<AssetServer>()
        .reload("clips/roll.glb#Animation0");

    let reloaded = wait_for(&mut app, "the reload to land", |app| {
        duration_of(app, &handle).filter(|d| (d - original).abs() > f32::EPSILON)
    });
    assert!(
        (reloaded - expected).abs() < f32::EPSILON,
        "the handle should now see the replacement ({expected}s), got {reloaded}s"
    );
}

/// The studio's own poll: a re-promote is reloaded under the handle the
/// library already holds, a clip that arrives is listed, and one that is
/// deleted leaves the list and the selection with it.
#[test]
fn the_rescan_notices_a_rebake_an_arrival_and_a_deletion() {
    let (_dir, project) = temp_project();
    promote(&project, "roll", TAKE, ClipRecipe::default());

    let mut app = headless_app(&project.assets);
    app.insert_resource(StudioConfig {
        project: project.clone(),
        model: None,
        audio: false,
        take: None,
        recipe: None,
        screenshot: None,
        selftest: false,
    })
    .init_resource::<Rig>()
    .init_resource::<ClipLibrary>()
    .init_resource::<ModelLibrary>()
    .init_resource::<AudioLibrary>()
    .init_resource::<Selection>()
    .init_resource::<LibraryView>()
    .add_systems(Startup, discover)
    .add_systems(Update, rescan_models);

    let handle = |app: &App| {
        app.world()
            .resource::<ClipLibrary>()
            .items()
            .iter()
            .find(|item| item.entry.name == "roll")
            .map(|item| item.handle.clone())
    };
    let original_handle = wait_for(&mut app, "the clip to be listed", |app| handle(app));
    let original = wait_for(&mut app, "the original clip", |app| {
        duration_of(app, &original_handle)
    });
    app.world_mut()
        .resource_mut::<Selection>()
        .select(forge_studio::studio::library::AssetKey::clip("roll"));

    // The re-promote, a new clip, and nothing watching the file system.
    promote(&project, "roll", TAKE, trimmed());
    promote(&project, "jump", OTHER_TAKE, ClipRecipe::default());

    poll_tick(&mut app);

    {
        let library = app.world().resource::<ClipLibrary>();
        let names: Vec<&str> = library
            .items()
            .iter()
            .map(|item| item.entry.name.as_str())
            .collect();
        assert_eq!(names, ["roll", "jump"], "the arrival was not listed");
        let roll = library
            .items()
            .iter()
            .find(|item| item.entry.name == "roll")
            .expect("roll");
        assert_eq!(
            roll.handle, original_handle,
            "a rebake must keep the handle: the graph node points at it"
        );
        assert!(
            roll.entry
                .recipe()
                .is_some_and(|recipe| (recipe.trim_start_s - 0.25).abs() < f32::EPSILON),
            "the sidecar was not refreshed with the new recipe"
        );
    }
    let reloaded = wait_for(&mut app, "the rescan's reload to land", |app| {
        duration_of(app, &original_handle).filter(|d| (d - original).abs() > f32::EPSILON)
    });
    assert!(reloaded < original, "the trimmed bake is the shorter one");

    // The deletion: the row goes, and so does a selection naming it.
    std::fs::remove_file(project.assets.join("clips/roll.glb")).expect("delete the clip");
    std::fs::remove_file(project.assets.join("clips/roll.json")).expect("delete the sidecar");
    poll_tick(&mut app);
    let names: Vec<String> = app
        .world()
        .resource::<ClipLibrary>()
        .items()
        .iter()
        .map(|item| item.entry.name.clone())
        .collect();
    assert_eq!(names, ["jump"], "the deleted clip is still listed");
    assert!(
        app.world().resource::<Selection>().key().is_none(),
        "the selection still names a clip that is gone"
    );
}
