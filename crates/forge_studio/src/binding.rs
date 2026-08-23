//! Diagnose and repair animation/skeleton bone-name mismatches in Bevy.
//!
//! # The problem this exists for
//!
//! Bevy binds an [`AnimationClip`]'s curves to entities purely by name. Each
//! curve is keyed by an [`AnimationTargetId`], which is a hash of the bone's
//! **full name path from the animation root**. If a clip was authored against a
//! skeleton whose bones are named even slightly differently, the hashes differ
//! and the clip drives nothing.
//!
//! Crucially, that failure is **completely silent**. Bevy's `animate_targets`
//! does the moral equivalent of:
//!
//! ```ignore
//! let Some(curves) = clip.curves_for_target(target_id) else { continue };
//! ```
//!
//! There is no warning at any log level. The character simply holds its rest
//! pose, which looks identical to a broken export, a bad import, a paused
//! player, or a clip that genuinely has no motion. [`ClipDiff`] turns that into
//! a one-line answer.
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use forge_studio::binding::{ClipDiff, SkeletonPaths};
//! # fn f(world: &World, root: Entity, clip: &bevy::animation::AnimationClip) {
//! let paths = SkeletonPaths::from_world(world, root);
//! let diff = ClipDiff::new(clip, &paths);
//! println!("{}", diff.report());
//! # }
//! ```
//!
//! # Two constraints the API cannot paper over
//!
//! * `AnimationTargetId` is a blake3 hash and is **not invertible**. Given a
//!   curve you cannot recover the bone name it wanted. Anything that needs
//!   names must carry its own path table — which is what [`SkeletonPaths`] is.
//! * The hashing algorithm **changed in Bevy 0.19** (name lengths are now
//!   prefixed, fixing a collision between different hierarchies). Never persist
//!   these ids; always recompute them at runtime.
//!
//! # Without a GPU
//!
//! [`headless_app`] builds the smallest Bevy app that can load a glTF, spawn
//! its hierarchy and evaluate a clip on it: no renderer, no window, no
//! adapter. [`bones_report`] runs the diff on top of it, which is what makes
//! `forge bones` a CI gate rather than an eye-render.

use std::fmt::Write as _;
use std::path::Path;

use bevy::{
    animation::{AnimationClip, AnimationTargetId, graph::AnimationGraph},
    asset::{AssetPlugin, LoadState},
    platform::collections::{HashMap, HashSet},
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot},
};

use crate::render::{RenderError, Stage};
use crate::stage::find_animation_root;

/// Every bone path under an animation root, and the id each one hashes to.
///
/// This is the lookup table Bevy itself does not keep: it maps in the direction
/// the hash cannot go, from [`AnimationTargetId`] back to a readable path.
#[derive(Debug, Clone, Default)]
pub struct SkeletonPaths {
    by_id: HashMap<AnimationTargetId, Vec<Name>>,
}

impl SkeletonPaths {
    /// Walk a spawned hierarchy and record the path of every named entity.
    ///
    /// `root` must be the **animation root** — the entity carrying
    /// [`AnimationPlayer`], whose own name is the first element of every path,
    /// matching how `bevy_gltf` builds ids at import time.
    #[must_use]
    pub fn from_world(world: &World, root: Entity) -> Self {
        let mut this = Self::default();
        this.walk(world, root, &mut Vec::new());
        this
    }

    fn walk(&mut self, world: &World, entity: Entity, path: &mut Vec<Name>) {
        // An unnamed node breaks the chain: bevy_gltf refuses to build ids for
        // anything below it, so stopping here mirrors the importer exactly.
        let Some(name) = world.get::<Name>(entity) else {
            return;
        };
        path.push(name.clone());
        self.by_id
            .insert(AnimationTargetId::from_names(path.iter()), path.clone());
        if let Some(children) = world.get::<Children>(entity) {
            for &child in children {
                self.walk(world, child, path);
            }
        }
        path.pop();
    }

    /// Does the skeleton contain a bone hashing to `id`?
    #[must_use]
    pub fn contains(&self, id: &AnimationTargetId) -> bool {
        self.by_id.contains_key(id)
    }

    /// The bone path that hashes to `id`, if any.
    #[must_use]
    pub fn path(&self, id: &AnimationTargetId) -> Option<&[Name]> {
        self.by_id.get(id).map(Vec::as_slice)
    }

    /// Number of named entities recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no named entities were found — usually the wrong root entity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Every (id, path) pair, in unspecified order.
    pub fn iter(&self) -> impl Iterator<Item = (&AnimationTargetId, &Vec<Name>)> {
        self.by_id.iter()
    }
}

/// Which bones of a skeleton a clip actually drives.
#[derive(Debug, Clone, Default)]
pub struct ClipDiff {
    /// Skeleton bones the clip drives, as slash-joined paths.
    pub bound: Vec<String>,
    /// Skeleton bones the clip leaves at their rest pose.
    pub unbound: Vec<String>,
    /// Curve targets with no matching bone.
    ///
    /// These can only be counted, never named: the id is a one-way hash, so a
    /// curve aimed at a bone this skeleton lacks carries no recoverable name.
    pub orphaned: usize,
}

impl ClipDiff {
    /// Compare a clip's curve targets against a skeleton's bone paths.
    #[must_use]
    pub fn new(clip: &AnimationClip, paths: &SkeletonPaths) -> Self {
        let targets: HashSet<AnimationTargetId> = clip.curves().keys().copied().collect();

        let mut bound = Vec::new();
        let mut unbound = Vec::new();
        for (id, path) in paths.iter() {
            let joined = join(path);
            if targets.contains(id) {
                bound.push(joined);
            } else {
                unbound.push(joined);
            }
        }
        bound.sort();
        unbound.sort();

        let orphaned = targets.iter().filter(|id| !paths.contains(id)).count();
        Self {
            bound,
            unbound,
            orphaned,
        }
    }

    /// True when the clip drives at least one bone and no curve is orphaned.
    ///
    /// Unbound bones are not a failure on their own — fingers and prop bones
    /// are routinely left at rest by a clip that is otherwise perfect.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        !self.bound.is_empty() && self.orphaned == 0
    }

    /// True when nothing at all connected — the silent-failure case.
    #[must_use]
    pub fn is_total_mismatch(&self) -> bool {
        self.bound.is_empty()
    }

    /// A human-readable summary suitable for a CLI or a test failure message.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        let total = self.bound.len() + self.unbound.len();
        let _ = writeln!(
            out,
            "{} of {total} skeleton bones driven, {} orphaned curve(s)",
            self.bound.len(),
            self.orphaned
        );
        if self.is_total_mismatch() {
            out.push_str(
                "  NOTHING BOUND — the clip's bone names do not match this skeleton.\n  \
                 Bevy reports no error for this; the character just holds its rest pose.\n",
            );
        }
        if !self.unbound.is_empty() {
            out.push_str("  undriven bones (hold rest pose):\n");
            for name in &self.unbound {
                let _ = writeln!(out, "    {name}");
            }
        }
        if self.orphaned > 0 {
            let _ = writeln!(
                out,
                "  {} curve(s) target bones this skeleton lacks; those are dropped.",
                self.orphaned
            );
        }
        out
    }
}

/// Make a hierarchy animatable by clips authored against it.
///
/// `bevy_gltf` only attaches [`AnimationTargetId`] and `AnimatedBy` to nodes
/// that sit under an *animation root*, and it only recognises animation roots
/// when the glTF file contains animation channels. A character exported as
/// geometry-plus-skeleton, with its clips living in separate files, therefore
/// arrives with none of that metadata — and every clip you play on it binds to
/// nothing, silently.
///
/// This installs the missing components, using exactly the path convention the
/// importer would have used, so ids match clips authored against the same
/// skeleton. Call it on the entity whose name is the first path segment (for a
/// Blender export, the armature object).
///
/// Returns the number of entities given animation targets. The caller still
/// needs to add [`AnimationPlayer`] and an `AnimationGraphHandle` to `root`.
pub fn install_animation_targets(world: &mut World, root: Entity) -> usize {
    let mut targets = Vec::new();
    collect_targets(world, root, &mut Vec::new(), &mut targets);
    let count = targets.len();
    for (entity, id) in targets {
        world
            .entity_mut(entity)
            .insert((id, bevy::animation::AnimatedBy(root)));
    }
    count
}

fn collect_targets(
    world: &World,
    entity: Entity,
    path: &mut Vec<Name>,
    out: &mut Vec<(Entity, AnimationTargetId)>,
) {
    let Some(name) = world.get::<Name>(entity) else {
        return;
    };
    path.push(name.clone());
    out.push((entity, AnimationTargetId::from_names(path.iter())));
    if let Some(children) = world.get::<Children>(entity) {
        let children: Vec<Entity> = children.iter().collect();
        for child in children {
            collect_targets(world, child, path, out);
        }
    }
    path.pop();
}

fn join(path: &[Name]) -> String {
    path.iter().map(Name::as_str).collect::<Vec<_>>().join("/")
}

/// Frames to pump before declaring an asset stuck. Loading is file I/O on a
/// task pool, so this is a deadlock guard, not a budget.
const MAX_LOAD_FRAMES: u32 = 900;

/// The smallest app that loads a glTF and evaluates a clip on it.
///
/// `MinimalPlugins` plus assets, transforms, the glTF loader, the world
/// spawner and animation — and **no renderer**. The spawner writes a loaded
/// scene into the world through reflection, so every component the importer
/// emits has to be registered here by hand; `DefaultPlugins` would register
/// them all, and would also demand a GPU adapter for the privilege. Meshes,
/// materials and images are still *assets* (the loader insists on their
/// stores existing) but nothing uploads them.
///
/// `finish`/`cleanup` are called here: plugin `finish` hooks never run from a
/// bare `update()`, and the glTF loader is registered in one.
pub fn headless_app(asset_root: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: asset_root.to_string_lossy().into_owned(),
            ..default()
        },
        TransformPlugin,
        bevy::world_serialization::WorldSerializationPlugin,
        bevy::gltf::GltfPlugin::default(),
        bevy::animation::AnimationPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
    .register_type::<Name>()
    .register_type::<Transform>()
    .register_type::<GlobalTransform>()
    .register_type::<Visibility>()
    .register_type::<InheritedVisibility>()
    .register_type::<ViewVisibility>()
    .register_type::<bevy::camera::visibility::VisibilityClass>()
    .register_type::<bevy::camera::visibility::NoFrustumCulling>()
    .register_type::<bevy::camera::primitives::Aabb>()
    .register_type::<Mesh3d>()
    .register_type::<MeshMaterial3d<StandardMaterial>>()
    .register_type::<bevy::mesh::skinning::SkinnedMesh>()
    .register_type::<bevy::camera::visibility::DynamicSkinnedMeshBounds>()
    .register_type::<bevy::mesh::morph::MorphWeights>()
    .register_type::<bevy::mesh::morph::MeshMorphWeights>()
    .register_type::<AnimationTargetId>()
    .register_type::<bevy::animation::AnimatedBy>()
    .register_type::<AnimationPlayer>()
    .register_type::<bevy::gltf::GltfExtras>()
    .register_type::<bevy::gltf::GltfSceneExtras>()
    .register_type::<bevy::gltf::GltfMeshExtras>()
    .register_type::<bevy::gltf::GltfMaterialExtras>()
    .register_type::<bevy::gltf::GltfMeshName>()
    .register_type::<bevy::gltf::GltfMaterialName>()
    .register_type::<bevy::gltf::GltfSceneName>()
    .register_type::<DirectionalLight>()
    .register_type::<PointLight>()
    .register_type::<SpotLight>();
    app.finish();
    app.cleanup();
    app
}

/// A model spawned into a [`headless_app`], with the clip it was asked for.
struct Loaded {
    spawned: Entity,
    clip: Handle<AnimationClip>,
}

/// Load `model` and `clip_file` and pump until both are usable.
fn load(
    app: &mut App,
    stage: &Stage,
    clip_file: &str,
    clip_index: usize,
) -> Result<Loaded, RenderError> {
    let (scene, clip) = {
        let server = app.world().resource::<AssetServer>().clone();
        (
            server.load::<WorldAsset>(stage.scene_path()),
            server.load::<AnimationClip>(format!("{clip_file}#Animation{clip_index}")),
        )
    };
    let spawned = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
    for _ in 0..MAX_LOAD_FRAMES {
        app.update();
        let server = app.world().resource::<AssetServer>();
        let scene_state = server.get_load_state(&scene);
        let clip_state = server.get_load_state(&clip);
        for (what, state) in [("model", &scene_state), ("clip", &clip_state)] {
            if let Some(LoadState::Failed(err)) = state {
                return Err(RenderError::AssetLoad(format!(
                    "{what} failed to load: {err}"
                )));
            }
        }
        let spawned_children = app
            .world()
            .get::<Children>(spawned)
            .is_some_and(|c| !c.is_empty());
        if spawned_children && matches!(clip_state, Some(LoadState::Loaded)) {
            return Ok(Loaded { spawned, clip });
        }
    }
    Err(RenderError::AssetLoad(format!(
        "assets did not load within {MAX_LOAD_FRAMES} frames"
    )))
}

/// What `forge bones` prints: whether a clip binds to a model, by name.
#[derive(Debug, Clone)]
pub struct BonesReport {
    /// The model, relative to the asset root.
    pub model: String,
    /// The clip file, relative to the asset root.
    pub clip_file: String,
    /// Animation index within the clip file.
    pub clip_index: usize,
    /// Clip length in seconds.
    pub duration: f32,
    /// The animation root's name — the first segment of every bone path.
    pub root_name: String,
    /// Named entities under the animation root.
    pub named_entities: usize,
    /// Entities given animation targets because the model carried none.
    /// `None` when the importer had already installed them.
    pub installed_targets: Option<usize>,
    /// Which bones the clip drives.
    pub diff: ClipDiff,
}

impl BonesReport {
    /// True when nothing at all connected — the exit-code case.
    #[must_use]
    pub fn is_total_mismatch(&self) -> bool {
        self.diff.is_total_mismatch()
    }

    /// The report as `forge bones` prints it.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        if let Some(installed) = self.installed_targets {
            let _ = writeln!(
                out,
                "installed animation targets on {installed} entities (model had none)"
            );
        }
        let _ = writeln!(out, "model:   {}", self.model);
        let _ = writeln!(
            out,
            "clip:    {} [{}]  ({:.3}s)",
            self.clip_file, self.clip_index, self.duration
        );
        let _ = writeln!(
            out,
            "root:    {}  ({} named entities)",
            self.root_name, self.named_entities
        );
        out.push('\n');
        out.push_str(&self.diff.report());
        out
    }
}

/// Report whether `clip_file` binds to the model on `stage`, without a GPU.
///
/// This is the check Bevy itself cannot give you: a clip whose bone names
/// miss produces no warning, no error, and a character that simply stands
/// still. A model exported without animations of its own carries no
/// animation targets; they are installed first, as the renderer and the
/// viewer do, so the answer is about the names and not about the export.
///
/// # Errors
///
/// Returns [`RenderError::AssetLoad`] when either file does not load and
/// [`RenderError::NoSkeleton`] when the model spawns no named hierarchy.
pub fn bones_report(
    stage: &Stage,
    clip_file: &str,
    clip_index: usize,
) -> Result<BonesReport, RenderError> {
    let mut app = headless_app(&stage.absolute_root());
    let loaded = load(&mut app, stage, clip_file, clip_index)?;

    let anim_root =
        find_animation_root(app.world_mut(), loaded.spawned).ok_or(RenderError::NoSkeleton)?;
    let root_name = app
        .world()
        .get::<Name>(anim_root)
        .map_or_else(|| String::from("<unnamed>"), |n| n.as_str().to_owned());

    let installed_targets = if app.world().get::<AnimationTargetId>(anim_root).is_some() {
        None
    } else {
        Some(install_animation_targets(app.world_mut(), anim_root))
    };

    let paths = SkeletonPaths::from_world(app.world(), anim_root);
    let clips = app.world().resource::<Assets<AnimationClip>>();
    let clip = clips
        .get(&loaded.clip)
        .ok_or_else(|| RenderError::AssetLoad(String::from("clip vanished after loading")))?;

    Ok(BonesReport {
        model: stage.model.clone(),
        clip_file: clip_file.to_owned(),
        clip_index,
        duration: clip.duration(),
        root_name,
        named_entities: paths.len(),
        installed_targets,
        diff: ClipDiff::new(clip, &paths),
    })
}

/// Attach a player to `anim_root` playing `clip` — the three lines every
/// caller that wants a pose writes, kept here so they are written once.
///
/// Returns the graph node to seek. The graph must round-trip through an
/// asset event before any pose applies, so pump at least one frame after.
pub fn attach_player(
    world: &mut World,
    anim_root: Entity,
    clip: Handle<AnimationClip>,
) -> AnimationNodeIndex {
    let (graph, node) = AnimationGraph::from_clip(clip);
    let handle = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
    world
        .entity_mut(anim_root)
        .insert((AnimationPlayer::default(), AnimationGraphHandle(handle)));
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_names_hash_identically() {
        // The property the whole design rests on: two skeletons built
        // independently bind to the same clip iff their name paths match.
        let a = AnimationTargetId::from_names([Name::new("Armature"), Name::new("Hips")].iter());
        let b = AnimationTargetId::from_names([Name::new("Armature"), Name::new("Hips")].iter());
        assert_eq!(a, b);
    }

    #[test]
    fn inserting_a_parent_changes_the_hash() {
        // Why the canonical rig has no root bone above Hips: adding one
        // rewrites every descendant's path and silently unbinds every clip.
        let without = AnimationTargetId::from_names([Name::new("Hips")].iter());
        let with = AnimationTargetId::from_names([Name::new("root"), Name::new("Hips")].iter());
        assert_ne!(without, with);
    }

    #[test]
    fn path_segmentation_matters() {
        // 0.19 length-prefixes each name, so these no longer collide.
        let split = AnimationTargetId::from_names([Name::new("a"), Name::new("b")].iter());
        let joined = AnimationTargetId::from_names([Name::new("ab")].iter());
        assert_ne!(split, joined);
    }

    #[test]
    fn a_diff_names_what_is_driven_and_counts_what_is_orphaned() {
        let mut world = World::new();
        let hips = world.spawn(Name::new("Hips")).id();
        let root = world.spawn(Name::new("Armature")).add_child(hips).id();
        let paths = SkeletonPaths::from_world(&world, root);
        assert_eq!(paths.len(), 2);

        let mut clip = AnimationClip::default();
        let hips_id =
            AnimationTargetId::from_names([Name::new("Armature"), Name::new("Hips")].iter());
        let stray =
            AnimationTargetId::from_names([Name::new("Armature"), Name::new("Tail")].iter());
        for id in [hips_id, stray] {
            clip.add_curve_to_target(
                id,
                bevy::animation::prelude::AnimatableCurve::new(
                    bevy::animation::animated_field!(Transform::translation),
                    bevy::math::curve::ConstantCurve::new(
                        bevy::math::curve::Interval::UNIT,
                        Vec3::ZERO,
                    ),
                ),
            );
        }
        let diff = ClipDiff::new(&clip, &paths);
        assert_eq!(diff.bound, ["Armature/Hips"]);
        assert_eq!(diff.unbound, ["Armature"]);
        assert_eq!(diff.orphaned, 1);
        assert!(!diff.is_bound());
        assert!(!diff.is_total_mismatch());
        assert!(diff.report().contains("1 of 2 skeleton bones driven"));
    }

    /// The whole `forge bones` path on the app with no renderer: the fixture
    /// mannequin, a clip baked by `forge_motion` from a fixture take, and the
    /// answer Bevy never gives — 27 driven, nothing orphaned — plus the
    /// refusal for a clip that was never there.
    #[test]
    fn a_bones_report_needs_no_gpu() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile = forge_rig::RigProfile::load(&repo.join("rigs/humanoid")).expect("profile");
        forge_rig::fixture::write_mannequin(&profile, &dir.path().join("mannequin.glb"))
            .expect("mannequin");
        let rig =
            forge_motion::RigDef::from_glb(&std::fs::read(profile.glb_path()).expect("rig.glb"))
                .expect("rig");
        let take = forge_motion::Take::read(
            repo.join("crates/forge_motion/tests/fixtures/rust/gen_jump.npz"),
        )
        .expect("take");
        let bytes =
            forge_motion::bake(&take, &forge_motion::Edit::default(), &rig, "jump").expect("bake");
        std::fs::write(dir.path().join("jump.glb"), bytes).expect("write clip");

        let stage = Stage::new(dir.path(), "mannequin.glb");
        let report = bones_report(&stage, "jump.glb", 0).expect("report");
        assert_eq!(report.root_name, "Armature");
        assert_eq!(report.diff.bound.len(), 27, "{}", report.text());
        assert_eq!(report.diff.orphaned, 0, "{}", report.text());
        assert!(
            report.installed_targets.is_some(),
            "the mannequin carries no animations"
        );
        assert!(report.duration > 0.5);
        assert!(!report.is_total_mismatch());
        assert!(report.text().contains("27 of"));

        let missing = bones_report(&stage, "nope.glb", 0);
        assert!(
            matches!(missing, Err(RenderError::AssetLoad(ref m)) if m.contains("clip")),
            "{missing:?}"
        );
    }
}
