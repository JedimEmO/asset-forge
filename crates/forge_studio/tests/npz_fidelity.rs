//! Does a clip built from a raw take pose the rig the same as the baked `.glb`?
//!
//! This is the test the whole instant-preview design rests on. If an in-memory
//! take and the shipped asset disagree, then what you tune in the studio is
//! not what you get when you keep it — which is worse than having no preview
//! at all, because it looks like it works.
//!
//! Runs with no window, no rendering and no GPU adapter: the model and the
//! clip are loaded by [`forge_studio::binding::headless_app`], so only
//! animation evaluation and transform propagation are needed, and it works
//! anywhere `cargo test` does. The body is the fixture mannequin and the clip
//! is baked fresh by [`forge_motion::bake`] from a fixture take, so the test
//! depends on no sample library and cannot be moved by a re-promote.

use std::path::{Path, PathBuf};

use bevy::{
    animation::{AnimationClip, AnimationTargetId, graph::AnimationGraph},
    asset::LoadState,
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot},
};
use forge_motion::{Edit, InPlace, RigDef, Take, YMode, bake, skeleton};
use forge_studio::binding::{headless_app, install_animation_targets};
use forge_studio::npz_clip::{RigFrames, build};
use forge_studio::stage::find_animation_root;

const MODEL: &str = "mannequin.glb";
const BAKED: &str = "baked.glb";
const TAKE: &str = "../forge_motion/tests/fixtures/blender/gen_roll.npz";

/// Under a millimetre is below what any viewer or metric here can resolve.
const TOLERANCE_MM: f32 = 1.0;

/// A recipe that exercises the knobs a preview has to agree on: trims,
/// root travel removed, root height detrended, a forward lean. Stated here
/// rather than read from a record, because the fixture's record is the old
/// schema and the claim under test is "preview == bake for *this* edit".
fn edit() -> Edit {
    Edit {
        trim_start: 4,
        trim_end: 6,
        in_place: InPlace::Detrend,
        y_mode: YMode::Detrend,
        lean_deg: 12.0,
        ..Edit::default()
    }
}

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// A temporary asset root holding the mannequin and a freshly baked clip.
fn asset_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile =
        forge_rig::RigProfile::load(&manifest_path("../../rigs/humanoid")).expect("profile");
    forge_rig::fixture::write_mannequin(&profile, &dir.path().join(MODEL)).expect("mannequin");
    let rig = RigDef::from_glb(&std::fs::read(profile.glb_path()).expect("rig.glb")).expect("rig");
    let take = Take::read(manifest_path(TAKE)).expect("read take");
    let bytes = bake(&take, &edit(), &rig, "roll").expect("bake");
    std::fs::write(dir.path().join(BAKED), bytes).expect("write clip");
    dir
}

/// Drive one clip to `time` and read every bone's world position, by name.
fn sample(app: &mut App, root: Entity, node: AnimationNodeIndex, time: f32) -> Vec<(String, Vec3)> {
    if let Some(mut player) = app.world_mut().get_mut::<AnimationPlayer>(root) {
        // `play` ADDS an active animation rather than replacing what is there.
        // Without this the previously sampled clip stays active and the two
        // blend — which reads as a near-match and quietly passes the test.
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

fn find(frame: &[(String, Vec3)], bone: &str) -> Vec3 {
    let Some((_, position)) = frame.iter().find(|(n, _)| n == bone) else {
        panic!("no {bone} bone")
    };
    *position
}

/// Spawn the model and wait for its hierarchy, returning the animation root.
fn spawn_rig(app: &mut App) -> Entity {
    let scene = app
        .world()
        .resource::<AssetServer>()
        .load::<WorldAsset>(format!("{MODEL}#Scene0"));
    let spawned = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
    for _ in 0..900 {
        app.update();
        if let Some(LoadState::Failed(err)) =
            app.world().resource::<AssetServer>().get_load_state(&scene)
        {
            panic!("the mannequin failed to load: {err}");
        }
        if app
            .world()
            .get::<Children>(spawned)
            .is_some_and(|c| !c.is_empty())
        {
            break;
        }
    }
    let anim_root = find_animation_root(app.world_mut(), spawned).expect("rig spawned");
    if app.world().get::<AnimationTargetId>(anim_root).is_none() {
        install_animation_targets(app.world_mut(), anim_root);
    }
    anim_root
}

#[test]
fn a_clip_built_from_a_take_matches_the_baked_glb() {
    let root = asset_root();
    let mut app = headless_app(root.path());

    let baked = app
        .world()
        .resource::<AssetServer>()
        .load::<AnimationClip>(format!("{BAKED}#Animation0"));
    let anim_root = spawn_rig(&mut app);
    for _ in 0..900 {
        if matches!(
            app.world().resource::<AssetServer>().get_load_state(&baked),
            Some(LoadState::Loaded)
        ) {
            break;
        }
        app.update();
    }

    // Rest frames MUST be read before a player exists, or they absorb a frame
    // of animation.
    let rig = RigFrames::from_world(app.world(), anim_root);
    assert!(!rig.is_empty(), "no bones found under the animation root");
    assert!(
        rig.missing_joints().is_empty(),
        "rig lacks cskel27 joints: {:?}",
        rig.missing_joints()
    );

    let take = Take::read(manifest_path(TAKE)).expect("read take");
    let built = edit().apply(&take);
    let duration = (built.frames() - 1) as f32 / built.fps;
    let ours = build(&built, &rig);
    assert!(!ours.curves().is_empty(), "built an empty clip");

    let (graph, nodes) = {
        let mut clips = app.world_mut().resource_mut::<Assets<AnimationClip>>();
        let ours = clips.add(ours);
        AnimationGraph::from_clips([baked.clone(), ours])
    };
    let graph = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    app.world_mut()
        .entity_mut(anim_root)
        .insert((AnimationPlayer::default(), AnimationGraphHandle(graph)));
    // The graph must round-trip through an asset event before any pose applies.
    for _ in 0..8 {
        app.update();
    }

    // Guard against the false green. If neither clip applied, every bone would
    // sit at the rest pose and the comparison below would pass at 0.00 mm.
    let moved = sample(&mut app, anim_root, nodes[0], 0.0)
        .iter()
        .zip(sample(&mut app, anim_root, nodes[0], duration * 0.5).iter())
        .map(|((_, a), (_, b))| (*a - *b).length())
        .fold(0.0_f32, f32::max);
    assert!(
        moved > 0.1,
        "the baked clip is not animating: {:.2} mm of motion",
        moved * 1000.0
    );

    let (mut worst, mut worst_at) = (0.0_f32, String::new());
    let mut root_worst = 0.0_f32;
    for step in 0..=24 {
        let t = duration * step as f32 / 24.0;
        let theirs = sample(&mut app, anim_root, nodes[0], t);
        let mine = sample(&mut app, anim_root, nodes[1], t);

        // Pose and placement are checked separately: a shared root offset would
        // otherwise swamp every bone, and a pose error would hide inside it.
        let (hips_a, hips_b) = (find(&theirs, "Hips"), find(&mine, "Hips"));
        root_worst = root_worst.max((hips_a - hips_b).length() * 1000.0);

        for ((name, pa), (_, pb)) in theirs.iter().zip(mine.iter()) {
            if skeleton::index_of(name).is_none() {
                continue; // fingers and the mesh node: driven by neither clip
            }
            let err = ((*pa - hips_a) - (*pb - hips_b)).length() * 1000.0;
            if err > worst {
                worst = err;
                worst_at = format!("{name} @ {t:.2}s");
            }
        }
    }

    println!("pose {worst:.4} mm (worst at {worst_at}), root {root_worst:.4} mm");
    assert!(
        worst < TOLERANCE_MM,
        "pose diverges from the baked glb by {worst:.3} mm at {worst_at}"
    );
    assert!(
        root_worst < TOLERANCE_MM,
        "root travel diverges from the baked glb by {root_worst:.3} mm"
    );
}

/// The take preview adds clips to a graph that is already playing. If a node
/// added after the fact did not bind, a new edit would look like it worked
/// and the character would simply keep playing the old one.
#[test]
fn a_clip_added_to_a_live_graph_drives_the_rig() {
    let root = asset_root();
    let mut app = headless_app(root.path());
    let anim_root = spawn_rig(&mut app);
    let rig = RigFrames::from_world(app.world(), anim_root);

    let take = Take::read(manifest_path(TAKE)).expect("read take");
    let first = app
        .world_mut()
        .resource_mut::<Assets<AnimationClip>>()
        .add(build(&Edit::default().apply(&take), &rig));
    let (graph, nodes) = AnimationGraph::from_clips([first]);
    let graph = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    app.world_mut().entity_mut(anim_root).insert((
        AnimationPlayer::default(),
        AnimationGraphHandle(graph.clone()),
    ));
    for _ in 0..8 {
        app.update();
    }

    let rest = sample(&mut app, anim_root, nodes[0], 0.0);
    let original = sample(&mut app, anim_root, nodes[0], 1.2);

    // Now do what the preview does: build a clip and graft it onto the live graph.
    let styled = Edit {
        arm_bend_deg: 45.0,
        lean_deg: 25.0,
        shoulder_back_deg: 30.0,
        ..Edit::default()
    };
    let added = app
        .world_mut()
        .resource_mut::<Assets<AnimationClip>>()
        .add(build(&styled.apply(&take), &rig));
    let node = {
        let mut graphs = app.world_mut().resource_mut::<Assets<AnimationGraph>>();
        let mut graph = graphs.get_mut(&graph).expect("graph asset");
        let root = graph.root;
        graph.add_clip(added, 1.0, root)
    };
    for _ in 0..8 {
        app.update();
    }

    let grafted = sample(&mut app, anim_root, node, 1.2);
    let moved_from_rest = grafted
        .iter()
        .zip(rest.iter())
        .map(|((_, a), (_, b))| (*a - *b).length())
        .fold(0.0_f32, f32::max);
    let differs_from_original = grafted
        .iter()
        .zip(original.iter())
        .map(|((_, a), (_, b))| (*a - *b).length())
        .fold(0.0_f32, f32::max);

    assert!(
        moved_from_rest > 0.1,
        "the grafted clip left the rig at rest ({moved_from_rest:.4} m of motion)"
    );
    assert!(
        differs_from_original > 0.05,
        "the grafted clip plays identically to the first one \
         ({differs_from_original:.4} m apart), so the new node is not being used"
    );
}
