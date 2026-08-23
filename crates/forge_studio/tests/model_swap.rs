//! Can the model on the stage be changed while the studio is running?
//!
//! The swap is a resequencing of the load that already worked once at startup,
//! and every step of it is invisible: the animation targets, the rest frames
//! read before a player exists, the graph handle carried across. A screenshot
//! shows a mesh; it does not show whether the clip playing on it is bound to
//! anything or whether the rest frames it was built against belong to the model
//! actually standing. Only this does.
//!
//! Runs with no window, no rendering and no GPU adapter, exactly like
//! `tests/npz_fidelity.rs`: the model and the clip are loaded by
//! [`forge_studio::binding::headless_app`], so animation evaluation and
//! transform propagation are all the swap needs, and it works anywhere
//! `cargo test` does. The library is a temporary project built the way a user's
//! would be — `forge init`, the profile installed, a body and a clip promoted —
//! so what is tested is the studio over a real library layout and not over a
//! directory arranged to suit it.

use std::path::{Path, PathBuf};

use bevy::{
    animation::{AnimationClip, AnimationTargetId, graph::AnimationGraph},
    prelude::*,
    world_serialization::WorldAssetRoot,
};
use forge_library::{
    Project,
    promote::{PromoteBody, PromoteClip, promote_body, promote_clip},
    schema::{Actor, ClipRecipe},
};
use forge_studio::{
    binding::headless_app,
    catalog::ClipEntry,
    orbit::OrbitCamera,
    rig_findings::RigFindings,
    stage::{NoFrameBounds, find_animation_root, spawn_stage},
    studio::{
        StudioConfig,
        library::{
            AssetKey, ClipLibrary, ClipOrigin, ModelLibrary, Selection, discover, materialise_take,
        },
        playback::Playback,
        rig::{
            ActiveModel, Rig, StageContract, SwapModel, finish_loading, load_contract, open_model,
            refresh_findings, request_swap,
        },
    },
};

/// The body the studio opens on: the fixture mannequin, promoted.
const FIRST: &str = "bodies/mannequin.glb";
/// A second conforming body — the same mannequin under another name, which
/// is enough: what is under test is the rewiring, not the mesh.
const SECOND: &str = "bodies/mannequin_two.glb";
/// The profile's skeleton with no mesh hung on it at all.
const BARE: &str = "rig.glb";
/// The shipped clip: the walk fixture, promoted with the identity recipe.
const CLIP: &str = "walk";
/// A raw take, for the half of the studio that reads [`Rig::rest_frames`].
const TAKE: &str = "../forge_motion/tests/fixtures/blender/gen_roll.npz";

/// Frames a load is given before the test calls it stuck.
///
/// Generous: the first `.glb` off a cold page cache is the slow one, and a
/// bound that is too tight fails as "the swap did not happen" while saying
/// nothing about why.
const PATIENCE: u32 = 900;

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// A temporary project with the profile, two bodies, a clip and the bare rig.
fn temp_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "swap_test").expect("init");
    project
        .install_profile(&manifest_path("../../rigs/humanoid"))
        .expect("install the profile");
    let profile = project.profile().expect("profile");

    let mannequin = dir.path().join("mannequin.glb");
    forge_rig::fixture::write_mannequin(&profile, &mannequin).expect("mannequin");
    for name in ["mannequin", "mannequin_two"] {
        promote_body(
            &project,
            &PromoteBody {
                name: name.to_owned(),
                glb_path: mannequin.clone(),
                blend_path: None,
                lift_record: None,
                rig_record: None,
                export_record: None,
                prompt: None,
                tags: Vec::new(),
                note: None,
                created_by: Actor::Human,
                overwrite: false,
            },
        )
        .expect("promote the body");
    }
    promote_clip(
        &project,
        &PromoteClip {
            name: CLIP.to_owned(),
            take_path: manifest_path("../forge_motion/tests/fixtures/blender/gen_walk.npz"),
            recipe: ClipRecipe::default(),
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Human,
            take_record: None,
            overwrite: false,
        },
    )
    .expect("promote the clip");
    // The bare rig is not a library asset — nothing promotes a skeleton — so
    // it sits at the asset root, where a swap can still name it.
    std::fs::copy(profile.glb_path(), project.assets.join(BARE)).expect("copy the rig");
    (dir, project)
}

/// The studio's stage, and nothing else: the systems that put a model on it.
///
/// Deliberately not [`forge_studio::studio::StudioPlugin`]. The whole
/// window would drag in a UI tree, fonts and a camera with no render target,
/// none of which the swap depends on — and a test that failed because a text
/// field could not lay out would say nothing about the rig.
fn stage(project: &Project) -> App {
    let mut app = headless_app(&project.assets);
    app.insert_resource(StudioConfig {
        project: project.clone(),
        model: Some(String::from("mannequin")),
        audio: false,
        take: None,
        recipe: None,
        screenshot: None,
        selftest: false,
    })
    .init_resource::<Rig>()
    .init_resource::<ActiveModel>()
    .init_resource::<StageContract>()
    .init_resource::<RigFindings>()
    .init_resource::<ClipLibrary>()
    .init_resource::<ModelLibrary>()
    .init_resource::<forge_studio::studio::audio_view::AudioLibrary>()
    .init_resource::<Selection>()
    .init_resource::<Playback>()
    .add_message::<SwapModel>()
    .add_systems(
        Startup,
        (spawn_stage, camera, load_contract, discover, open_model).chain(),
    )
    .add_systems(
        Update,
        (request_swap, finish_loading, refresh_findings).chain(),
    );
    app
}

/// An orbit camera to be reframed.
///
/// It carries no [`Camera3d`]: the reframe queries `OrbitCamera` and nothing
/// else, and a real camera with no window to draw into would only add a
/// render target that cannot exist.
fn camera(mut commands: Commands) {
    commands.spawn((OrbitCamera::default(), Transform::default()));
}

/// Run frames until `done`, or fail saying what the rig was doing instead.
///
/// Always a frame first: a message written a moment ago has not been read yet,
/// so a condition tested before the first update would answer about the world
/// as it was before the request existed.
fn run_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    for _ in 0..PATIENCE {
        app.update();
        if done(app) {
            return;
        }
    }
    let rig = app.world().resource::<Rig>();
    panic!(
        "{what} did not happen in {PATIENCE} frames (ready {}, swapping {}, status {:?})",
        rig.is_ready(),
        rig.is_swapping(),
        rig.status()
    );
}

/// Drive one clip to `time` and read every bone's world position, by name.
fn sample(app: &mut App, node: AnimationNodeIndex, time: f32) -> Vec<(String, Vec3)> {
    let root = app.world().resource::<Rig>().anim_root().expect("a rig");
    if let Some(mut player) = app.world_mut().get_mut::<AnimationPlayer>(root) {
        // `play` ADDS an active animation rather than replacing what is there,
        // so without this the previously sampled clip stays active and the two
        // blend — which reads as a near-match and quietly passes.
        player.stop_all();
        let active = player.play(node);
        active.set_seek_time(time);
        active.pause();
    }
    app.update();

    let mut out = Vec::new();
    let mut query = app.world_mut().query::<(&Name, &GlobalTransform)>();
    for (name, transform) in query.iter(app.world()) {
        out.push((name.as_str().to_owned(), transform.translation()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// How far the furthest bone moves between two moments of the same clip.
fn travel(a: &[(String, Vec3)], b: &[(String, Vec3)]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|((_, x), (_, y))| (*x - *y).length())
        .fold(0.0_f32, f32::max)
}

/// The graph node the shipped clip plays through.
fn node(app: &App) -> AnimationNodeIndex {
    app.world()
        .resource::<ClipLibrary>()
        .items()
        .iter()
        .find(|item| item.entry.name == CLIP)
        .and_then(|item| item.node)
        .expect("the clip has a graph node")
}

/// How many [`WorldAssetRoot`] entities are on the stage.
///
/// Exactly one, always: the swap spawns the incoming model beside the outgoing
/// one, and a leak here means two rigs interpenetrating for the rest of the
/// session.
fn models_on_stage(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<WorldAssetRoot>>()
        .iter(app.world())
        .count()
}

/// Every named entity under the rig that an animation can address.
fn wired_bones(app: &mut App) -> usize {
    let root = app.world().resource::<Rig>().anim_root().expect("a rig");
    let mut found = 0;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if app.world().get::<AnimationTargetId>(entity).is_some() {
            found += 1;
        }
        if let Some(children) = app.world().get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

/// The whole point: a different mesh, the same rig, the same clip still playing.
#[test]
fn swapping_the_mesh_rewires_the_rig_and_keeps_the_clip_playing() {
    let (_dir, project) = temp_project();
    let mut app = stage(&project);
    run_until(&mut app, "the first model standing up", |app| {
        app.world().resource::<Rig>().is_ready()
    });

    {
        let active = app.world().resource::<ActiveModel>();
        assert_eq!(
            active.path, FIRST,
            "`--model mannequin` resolves to the body"
        );
        assert_eq!(
            active.generation, 1,
            "the model the studio opened on is the first rig to stand up"
        );
        // The browser listed both bodies, bodies first, and the clip.
        let models = app.world().resource::<ModelLibrary>();
        assert_eq!(
            models
                .models()
                .iter()
                .map(|m| m.rel_path.as_str())
                .collect::<Vec<_>>(),
            [FIRST, SECOND]
        );
        assert_eq!(app.world().resource::<ClipLibrary>().items().len(), 1);
    }
    let first_root = app.world().resource::<Rig>().anim_root().expect("a rig");
    let first_bones = wired_bones(&mut app);
    let first_paths = app.world().resource::<Rig>().paths().len();
    let first_frames = app
        .world()
        .resource::<Rig>()
        .rest_frames()
        .expect("rest frames")
        .len();
    println!("mannequin: {first_bones} targets, {first_paths} paths, {first_frames} bones");
    // Every named entity under the root is addressable, and the 27 cskel joints
    // a take drives are among them. The absolute number is the profile's
    // business, so what is pinned is the invariant.
    assert_eq!(
        first_bones, first_paths,
        "some named bones were left without an animation target"
    );
    assert!(
        app.world()
            .resource::<Rig>()
            .rest_frames()
            .is_some_and(|frames| frames.missing_joints().is_empty()),
        "the first model does not conform to the rig contract"
    );
    // The contract was read off the stage in the frame the rig stood up.
    {
        let findings = app.world().resource::<RigFindings>();
        assert_eq!(findings.generation, 1, "the findings follow the stage");
        let lines: Vec<String> = findings
            .lines()
            .map(|(severity, text)| format!("{} {text}", severity.mark()))
            .collect();
        assert!(
            findings.conforms(),
            "the fixture mannequin should satisfy its own contract:\n{}",
            lines.join("\n")
        );
    }

    let clip = node(&app);
    let before = travel(&sample(&mut app, clip, 0.0), &sample(&mut app, clip, 0.6));
    assert!(before > 0.01, "the walk is not animating the first model");

    // A candidate built here, before the swap, so the graph the swap carries
    // across is one that has grown since it was built — which is what
    // `--take` does.
    let candidate = adopt_a_take(&mut app);
    let candidate_node = app
        .world()
        .resource::<ClipLibrary>()
        .get(&candidate)
        .and_then(|item| item.node)
        .expect("the candidate has a node");

    // ------------------------------------------------------------- the swap --
    app.world_mut().write_message(SwapModel::new(SECOND));
    run_until(&mut app, "the swap", |app| {
        app.world().resource::<ActiveModel>().generation == 2
    });

    let rig = app.world().resource::<Rig>();
    assert!(rig.is_ready(), "the rig did not come back up");
    assert!(!rig.is_swapping(), "the swap says it is still going");
    assert_eq!(app.world().resource::<ActiveModel>().path, SECOND);
    let second_root = rig.anim_root().expect("a rig");
    assert_ne!(
        second_root, first_root,
        "the player is still attached to the model that left"
    );
    assert_eq!(
        rig.paths().len(),
        first_paths,
        "both meshes conform to the same rig contract, so they bind the same paths"
    );
    let frames = rig.rest_frames().expect("rest frames were re-sampled");
    assert_eq!(
        frames.len(),
        first_frames,
        "the rest pose is sampled per bone, and the contract is the same"
    );
    assert!(
        frames.missing_joints().is_empty(),
        "the new rig is missing joints: {:?}",
        frames.missing_joints()
    );
    assert_eq!(
        wired_bones(&mut app),
        first_bones,
        "the new mesh's contract bones were not all given animation targets"
    );
    assert_eq!(
        models_on_stage(&mut app),
        1,
        "the model that left is still on the stage"
    );
    // And only the model left. The floor is spawned once and never again.
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<NoFrameBounds>>()
            .iter(app.world())
            .count(),
        1,
        "the swap took the floor with it"
    );
    assert_eq!(
        app.world().resource::<RigFindings>().generation,
        2,
        "the findings were not re-read for the model that stood up"
    );

    // The same node, from the graph built before either mesh was on stage.
    let after = travel(&sample(&mut app, clip, 0.0), &sample(&mut app, clip, 0.6));
    assert!(
        after > 0.01,
        "the shipped clip stopped driving the rig after the swap ({after:.4} m)"
    );
    // And the node that was grafted on afterwards, which is `--take`'s path:
    // a graph carried across is worth nothing if only the nodes that existed
    // at startup still bind.
    let grafted = travel(
        &sample(&mut app, candidate_node, 0.0),
        &sample(&mut app, candidate_node, 0.6),
    );
    assert!(
        grafted > 0.01,
        "a clip added to the live graph stopped playing after the swap ({grafted:.4} m)"
    );

    // The playhead was rewound rather than left where it was.
    assert!(
        app.world().resource::<Playback>().time.abs() < f32::EPSILON,
        "the selected clip should restart from the top on the new rig"
    );

    // The preview path, after the swap: `npz_clip::build` reads the rest
    // frames, so a stale or missing set is a clip that poses nothing.
    let rebuilt = adopt_a_take(&mut app);
    let rebuilt_node = app
        .world()
        .resource::<ClipLibrary>()
        .get(&rebuilt)
        .and_then(|item| item.node)
        .expect("a take materialised after the swap");
    let preview = travel(
        &sample(&mut app, rebuilt_node, 0.0),
        &sample(&mut app, rebuilt_node, 0.6),
    );
    assert!(
        preview > 0.01,
        "a take built from the re-sampled rest frames poses nothing ({preview:.4} m)"
    );

    // ------------------------------------------------ the rig with no mesh --
    app.world_mut().write_message(SwapModel::new(BARE));
    run_until(&mut app, "the swap to the bare rig", |app| {
        app.world().resource::<ActiveModel>().generation == 3
    });

    let rig = app.world().resource::<Rig>();
    assert!(rig.is_ready(), "a skeleton with no mesh is still a rig");
    assert_eq!(app.world().resource::<ActiveModel>().path, BARE);
    assert!(
        rig.rest_frames()
            .is_some_and(|frames| frames.missing_joints().is_empty()),
        "the bare rig should still carry every joint"
    );
    assert!(
        rig.status().contains("no visible mesh"),
        "nothing said the stage looks empty on purpose: {:?}",
        rig.status()
    );
    assert!(!rig.has_visible_mesh());
    let bare = travel(&sample(&mut app, clip, 0.0), &sample(&mut app, clip, 0.6));
    assert!(
        bare > 0.01,
        "the clip stopped playing on the bare rig ({bare:.4} m)"
    );
}

/// A model that will not load must cost the stage nothing.
#[test]
fn a_swap_to_a_model_that_will_not_load_keeps_the_one_on_stage() {
    let (_dir, project) = temp_project();
    let mut app = stage(&project);
    run_until(&mut app, "the first model standing up", |app| {
        app.world().resource::<Rig>().is_ready()
    });
    let standing = app.world().resource::<Rig>().anim_root().expect("a rig");
    let clip = node(&app);

    app.world_mut()
        .write_message(SwapModel::new("models/there_is_no_such_model.glb"));
    run_until(&mut app, "the failed swap giving up", |app| {
        !app.world().resource::<Rig>().is_swapping()
    });

    let rig = app.world().resource::<Rig>();
    assert!(rig.is_ready(), "the stage was emptied by a bad path");
    assert_eq!(
        rig.anim_root(),
        Some(standing),
        "the model that was standing was taken down for one that never arrived"
    );
    assert!(
        rig.status().contains("there_is_no_such_model.glb"),
        "the refusal does not name the model: {:?}",
        rig.status()
    );
    assert!(rig.is_troubled(), "a refusal has to read as one");
    let active = app.world().resource::<ActiveModel>();
    assert_eq!(
        active.path, FIRST,
        "the active model names something nobody can see"
    );
    assert_eq!(
        active.generation, 1,
        "a swap that never happened counted as one that did"
    );
    assert_eq!(
        models_on_stage(&mut app),
        1,
        "the abandoned load left its entity on the stage"
    );

    // And the rig it kept is still playable.
    let moved = travel(&sample(&mut app, clip, 0.0), &sample(&mut app, clip, 0.6));
    assert!(moved > 0.01, "the surviving rig stopped animating");
}

/// Materialise a raw take through the same call `--take` uses.
///
/// The graph must round-trip through an asset event before a node added to it
/// binds to anything, so the frames afterwards are not padding.
fn adopt_a_take(app: &mut App) -> AssetKey {
    let take = forge_motion::Take::read(manifest_path(TAKE)).expect("read take");
    // Always through the edit, even the identity one: that is where a take
    // turns to face the rig's way.
    let take = forge_motion::Edit::default().apply(&take);
    let key = materialise(app, take);
    for _ in 0..8 {
        app.update();
    }
    key
}

fn materialise(app: &mut App, take: forge_motion::Take) -> AssetKey {
    let world = app.world_mut();
    world.resource_scope(|world, mut library: Mut<ClipLibrary>| {
        world.resource_scope(|world, mut clips: Mut<Assets<AnimationClip>>| {
            world.resource_scope(|world, mut graphs: Mut<Assets<AnimationGraph>>| {
                let rig = world.resource::<Rig>();
                materialise_take(
                    &mut library,
                    &mut clips,
                    &mut graphs,
                    rig,
                    &take,
                    ClipEntry::bare("takes/gen_roll.npz", "gen_roll"),
                    ClipOrigin::Candidate,
                )
                .expect("the rig is up, so a take should materialise")
            })
        })
    })
}

/// The scene wrapper the swap despawns is the one it spawned, and nothing else
/// under it survives — a leaked hierarchy would keep posing an invisible body.
#[test]
fn nothing_of_the_old_model_is_left_behind() {
    let (_dir, project) = temp_project();
    let mut app = stage(&project);
    run_until(&mut app, "the first model standing up", |app| {
        app.world().resource::<Rig>().is_ready()
    });
    let old_root = app.world().resource::<Rig>().anim_root().expect("a rig");
    let old_scene = app
        .world_mut()
        .query_filtered::<Entity, With<WorldAssetRoot>>()
        .iter(app.world())
        .next()
        .expect("a model on stage");

    app.world_mut().write_message(SwapModel::new(SECOND));
    run_until(&mut app, "the swap", |app| {
        app.world().resource::<ActiveModel>().generation == 2
    });

    assert!(
        app.world().get_entity(old_scene).is_err(),
        "the old scene entity is still alive"
    );
    assert!(
        app.world().get_entity(old_root).is_err(),
        "the old animation root is still alive, player and all"
    );
    // The new one is the one the rig points at, reachable from the scene root.
    let scene = app
        .world_mut()
        .query_filtered::<Entity, With<WorldAssetRoot>>()
        .iter(app.world())
        .next()
        .expect("a model on stage");
    assert_eq!(
        find_animation_root(app.world_mut(), scene),
        app.world().resource::<Rig>().anim_root()
    );
    // Spawned hidden so the two never overlapped; shown again once it stood.
    // (No visibility propagation runs on this app, so the scene root's own
    // component is what can be asked.)
    assert_eq!(
        app.world().get::<Visibility>(scene),
        Some(&Visibility::Inherited),
        "the model that stood up was left hidden"
    );
    assert!(
        app.world().resource::<Rig>().has_visible_mesh(),
        "the mannequin has a mesh, and the stage should know it"
    );

    // The camera was reframed onto the new subject rather than left where the
    // old one stood.
    let focus = app
        .world_mut()
        .query::<&OrbitCamera>()
        .iter(app.world())
        .next()
        .map(|orbit| orbit.focus)
        .expect("an orbit camera");
    assert!(focus.is_finite(), "the camera was framed on {focus:?}");
}

/// A static model on the stage gets one note, not a wall of contract FAILs.
///
/// A prop has no rig by construction, so "contract bone missing" fifty times
/// over is the definition of a model recited as failures — in a fresh project
/// whose only mesh is a prop, that wall of red was the entire metadata panel.
/// The stage says the one true thing instead, and no clip is auto-selected
/// for a subject that was never meant to move.
#[test]
fn a_static_model_is_not_held_to_the_rig_contract() {
    use forge_library::promote::{PromoteModel, promote_model};
    use forge_studio::studio::library::focus_first;
    use forge_studio::studio::rig::STATIC_MODEL_FINDING;

    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "model_stage").expect("init");
    project
        .install_profile(&manifest_path("../../rigs/humanoid"))
        .expect("install the profile");
    promote_clip(
        &project,
        &PromoteClip {
            name: CLIP.to_owned(),
            take_path: manifest_path("../forge_motion/tests/fixtures/blender/gen_walk.npz"),
            recipe: ClipRecipe::default(),
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Human,
            take_record: None,
            overwrite: false,
        },
    )
    .expect("promote the clip");
    promote_model(
        &project,
        &PromoteModel {
            name: String::from("testbox"),
            glb_path: manifest_path("tests/fixtures/testbox.glb"),
            blend_path: None,
            lift_record: None,
            prop_record: None,
            prompt: Some(String::from("an orange box")),
            tags: Vec::new(),
            note: None,
            created_by: Actor::Human,
            overwrite: false,
        },
    )
    .expect("promote the model");

    let mut app = headless_app(&project.assets);
    app.insert_resource(StudioConfig {
        project: project.clone(),
        model: Some(String::from("testbox")),
        audio: false,
        take: None,
        recipe: None,
        screenshot: None,
        selftest: false,
    })
    .init_resource::<Rig>()
    .init_resource::<ActiveModel>()
    .init_resource::<StageContract>()
    .init_resource::<RigFindings>()
    .init_resource::<ClipLibrary>()
    .init_resource::<ModelLibrary>()
    .init_resource::<forge_studio::studio::audio_view::AudioLibrary>()
    .init_resource::<Selection>()
    .init_resource::<Playback>()
    .add_message::<SwapModel>()
    .add_systems(
        Startup,
        (
            spawn_stage,
            camera,
            load_contract,
            discover,
            open_model,
            focus_first,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (request_swap, finish_loading, refresh_findings).chain(),
    );
    run_until(&mut app, "the model standing up", |app| {
        app.world().resource::<Rig>().is_ready()
    });

    let findings = app.world().resource::<RigFindings>();
    assert_eq!(findings.generation, 1, "the findings follow the stage");
    let lines: Vec<(forge_studio::rig_findings::Severity, String)> = findings
        .lines()
        .map(|(severity, text)| (severity, text.to_owned()))
        .collect();
    assert_eq!(lines.len(), 1, "one line, not a wall of FAILs: {lines:?}");
    assert_eq!(lines[0].1, STATIC_MODEL_FINDING);
    assert!(
        findings.failures().next().is_none(),
        "the note is not a failure"
    );
    assert!(
        !findings.conforms(),
        "a box does not get to claim the contract either"
    );
    assert!(
        app.world().resource::<Selection>().key().is_none(),
        "no clip is auto-selected for a static model"
    );
}
