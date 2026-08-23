//! The model on the stage: putting one there, swapping it, and everything read
//! off it once.
//!
//! Loading is asynchronous twice over — the scene spawns over several frames,
//! and every clip resolves on its own schedule — and neither reports completion
//! synchronously. So [`finish_loading`] runs every frame until the thing stands
//! up, and then goes quiet until somebody asks for a different model.
//!
//! # Swapping the mesh
//!
//! The stage holds one model at a time and it can be changed while the studio
//! runs. Two public items are the whole contract:
//!
//! * [`ActiveModel`] — what is standing, and how many rigs have stood up.
//! * [`SwapModel`] — the request to stand up a different one.
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use forge_studio::studio::rig::{ActiveModel, SwapModel};
//! /// A model picker: one click, one message.
//! fn pick(active: Res<ActiveModel>, mut swaps: MessageWriter<SwapModel>) {
//!     if active.path != "models/barrel.glb" {
//!         swaps.write(SwapModel::new("models/barrel.glb"));
//!     }
//! }
//! ```
//!
//! A buffered message rather than a pending field on a resource: a swap is
//! consumed once and turned into state ([`Rig`]'s own in-flight load) the
//! same frame it arrives. A writer also needs no write access to the rig,
//! which is what lets a picker panel ask for a model without being able to
//! corrupt one. [`crate::studio::StudioPlugin`] registers the message; a
//! headless test driving these systems must call `add_message::<SwapModel>()`
//! itself.
//!
//! ## What a swap actually does
//!
//! Exactly what the first load does, in exactly the same order, because it is
//! the same code: find the animation root, install animation targets if the
//! glTF did not bring any, sample [`RigFrames`] **before** an
//! [`AnimationPlayer`] exists, attach the player and the *same*
//! [`AnimationGraph`] handle, read [`SkeletonPaths`], frame the camera. The
//! graph is a set of clip handles and nothing else — it names no mesh — so node
//! indices held by [`ClipLibrary`] and by a materialised take all survive a
//! swap untouched, and a clip grafted onto the graph after startup plays on
//! the new mesh with no further work.
//!
//! The incoming model is spawned **beside** the outgoing one and hidden until
//! it is known to stand; only then is the outgoing scene entity despawned and
//! the rig's facts replaced, all inside one exclusive-world step. Two
//! consequences worth knowing before building on this:
//!
//! * A swap that fails — a path that will not load, or a scene with no named
//!   hierarchy — leaves the previous model on stage and says why in
//!   [`Rig::status`]. The stage is never silently emptied and nothing panics.
//! * `Rig::is_ready` therefore stays `true` across a successful swap: it
//!   answers "is there a rig on the stage", and during the load there still
//!   is. Ask [`Rig::is_swapping`] whether one is on its way, and watch
//!   [`ActiveModel::generation`] to find out that one arrived.
//!
//! Only the model leaves. The floor, the lights and the camera are spawned by
//! [`spawn_stage`](crate::stage::spawn_stage) and `spawn_camera` once, and a
//! swap reframes the camera rather than replacing it.
//!
//! ## Playback across a swap
//!
//! The [`Selection`](crate::studio::library::Selection) is left alone and the
//! selected clip restarts from `t = 0`. Preserving the seek time was the
//! alternative and is worse: the new rig's first drawn frame would be a pose
//! from the middle of a clip on a body that has never been posed, and a walk
//! caught mid-stride reads as a broken load. Restarting is one visible, honest
//! event.
//!
//! Known limitation, for whoever needs it: clips *built* from raw takes by
//! [`npz_clip::build`](crate::npz_clip::build) bake in a correction derived
//! from the rest frames of the rig that was standing when they were built. Two
//! meshes that conform to the same rig contract have the same rest frames, so
//! in practice a take materialised before a swap still plays correctly after
//! it — but a mesh with a different rest pose would need its candidates
//! rebuilt, and nothing here does that.

use bevy::{
    animation::{AnimationClip, AnimationTargetId, graph::AnimationGraph},
    asset::LoadState,
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot},
};
use forge_library::schema::Kind;
use forge_rig::Contract;

use crate::{
    binding::{SkeletonPaths, install_animation_targets},
    npz_clip::RigFrames,
    orbit::OrbitCamera,
    rig_findings::{self, Finding, RigFindings},
    stage::{CAMERA_FOV_DEG, find_animation_root, world_bounds},
    studio::{
        StudioConfig,
        library::{ClipLibrary, ModelLibrary, clips_resolved},
        playback::Playback,
    },
    theme,
};

/// Frames a model gets to spawn its hierarchy before the load is called dead.
///
/// The same budget the headless renderer allows, and for the same reason: a
/// load that has not produced a single child entity in fifteen seconds is not
/// slow, it is stuck, and saying so is more use than waiting forever.
const MAX_LOAD_FRAMES: u32 = 900;
/// Missing rig joints named in [`Rig::status`] before it says "and N more".
const JOINTS_NAMED: usize = 3;

/// Which model the stage is showing.
///
/// Read it to know what is standing; do not write it — a swap is asked for
/// with [`SwapModel`] and this is how the answer comes back.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveModel {
    /// Asset-root-relative path, e.g. `bodies/vex_runner.glb`.
    ///
    /// Initialised from what the studio resolved to open on, so before the
    /// first rig stands up it names the model that was *asked* for and
    /// `generation` is still 0. From then on it names the model actually on
    /// stage: a swap only rewrites it once the new rig is wired, so a failed
    /// swap leaves it pointing at what a viewer can still see.
    pub path: String,
    /// How many rigs have stood up on this stage, including the first.
    ///
    /// Bumped as the last step of a completed stand-up, which makes
    /// `Local<u32>` plus a comparison the way to run something once per rig —
    /// and makes it work for the model the studio opened on, not only for the
    /// ones swapped in afterwards. 0 means nothing has stood up yet.
    pub generation: u32,
}

impl ActiveModel {
    /// Record a rig that has just finished standing up.
    fn completed(&mut self, path: String) {
        self.path = path;
        // Wrapping rather than saturating: a counter that stopped moving would
        // silently stop waking everything that watches it, and after four
        // billion swaps a wrap is the honest thing to do.
        self.generation = self.generation.wrapping_add(1);
    }
}

/// Put a different model on the stage.
///
/// `path` is asset-root-relative and names the file, not a scene label —
/// `models/barrel.glb`, and `#Scene0` is appended here. Two requests in
/// one frame are two clicks and the last one wins. A request naming the model
/// already standing is honoured rather than ignored, so that every `SwapModel`
/// ends in a [`ActiveModel::generation`] bump and reloading a mesh edited on
/// disk is possible at all.
#[derive(Message, Debug, Clone)]
pub struct SwapModel {
    /// Model path, relative to the asset root.
    pub path: String,
}

impl SwapModel {
    /// A request for the model at `path`.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// The rig contract the stage judges a model against: the project's profile,
/// read once when the window opens.
///
/// `None` when the profile would not load, which is reported then and turns
/// into one warning finding per model rather than a window that will not
/// open — a library whose rig profile is broken is still a library whose
/// sounds and clips can be looked at.
#[derive(Resource, Default)]
pub struct StageContract(pub Option<Contract>);

/// A model asked for and not yet standing.
struct Incoming {
    /// The [`WorldAssetRoot`] entity spawned for it, hidden until it stands.
    entity: Entity,
    /// Where it came from, for [`ActiveModel`] and for saying what failed.
    path: String,
    /// The scene handle, so a load that failed is reported rather than waited
    /// out — the asset server is never going to answer.
    scene: Handle<WorldAsset>,
    /// Frames spent waiting for the hierarchy to appear.
    frames: u32,
}

/// The spawned model, and the facts about it that are only readable once.
#[derive(Resource, Default)]
pub struct Rig {
    anim_root: Option<Entity>,
    graph: Option<Handle<AnimationGraph>>,
    rest_frames: Option<RigFrames>,
    paths: SkeletonPaths,
    ready: bool,
    /// The [`WorldAssetRoot`] entity of the model on stage, so a swap can
    /// despawn that and only that.
    scene: Option<Entity>,
    /// The model being loaded, if one is.
    incoming: Option<Incoming>,
    status: String,
    /// Whether [`Rig::status`] currently reports a refusal rather than
    /// progress or a note — what tells a status line to change colour.
    troubled: bool,
    /// Whether anything on the standing model actually draws. `false` for the
    /// bare rig, which is what turns the skeleton overlay on.
    visible_mesh: bool,
}

impl Rig {
    /// Whether the model is spawned and the graph is live.
    ///
    /// Stays `true` while a swap loads, because the model it will replace is
    /// still standing and still playable. [`Rig::is_swapping`] is the question
    /// about the one on its way.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Whether a different model is on its way to the stage.
    #[must_use]
    pub fn is_swapping(&self) -> bool {
        self.incoming.is_some()
    }

    /// The entity the [`AnimationPlayer`] is attached to.
    #[must_use]
    pub fn anim_root(&self) -> Option<Entity> {
        self.anim_root
    }

    /// The graph clips are played from, once every clip has resolved.
    ///
    /// Outlives any one model: it holds clip handles and names no mesh, so the
    /// same handle is attached to whatever stands on the stage next and the
    /// node indices the library hands out never move.
    #[must_use]
    pub fn graph(&self) -> Option<&Handle<AnimationGraph>> {
        self.graph.as_ref()
    }

    /// The rig's rest frames, for building clips from raw takes.
    ///
    /// Sampled during loading, because once an [`AnimationPlayer`] is attached
    /// the bone transforms hold animated values and a correction derived from
    /// them silently bakes in a frame of motion. Re-sampled on every swap, from
    /// the new model and before its player exists.
    #[must_use]
    pub fn rest_frames(&self) -> Option<&RigFrames> {
        self.rest_frames.as_ref()
    }

    /// The rig's bone paths, for reporting what a clip actually binds to.
    #[must_use]
    pub fn paths(&self) -> &SkeletonPaths {
        &self.paths
    }

    /// What the stage has to say: what it is loading, what it refused to load,
    /// or what is odd about what is standing — a model with no visible mesh,
    /// or one missing rig joints.
    ///
    /// One line, always current, and the *only* in-window account of a swap
    /// that did not happen. The browser draws it in its own fixed-height line
    /// under the mesh list ([`crate::studio::library`]'s `StageStatus`) —
    /// deliberately not the status line under the viewport, which belongs to
    /// [`crate::studio::metadata`]'s keyboard hint. [`Rig::is_troubled`] says
    /// whether the line is a refusal or mere progress. A refusal is also
    /// logged at error level, so a failed swap is never silent even headless.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Whether [`Rig::status`] reports a refusal — a model that would not
    /// load or stand — rather than progress or an informational note.
    #[must_use]
    pub fn is_troubled(&self) -> bool {
        self.troubled
    }

    /// Whether the standing model has anything to draw.
    ///
    /// `false` means the bones are real and the clips play — the bare
    /// `rig.glb` of the profile is the case — and `draw_skeleton` is showing
    /// them.
    #[must_use]
    pub fn has_visible_mesh(&self) -> bool {
        self.visible_mesh
    }

    /// Ask for `path`, spawning its scene hidden beside whatever is standing.
    ///
    /// Hidden rather than despawning the outgoing model first: the one on stage
    /// stays until its replacement is known to stand, so a bad path costs
    /// nothing, and two rigs interpenetrating for the length of a load is not
    /// what anybody asked to see.
    fn begin(&mut self, commands: &mut Commands, server: &AssetServer, path: String) {
        let scene: Handle<WorldAsset> = server.load(format!("{path}#Scene0"));
        let entity = commands
            .spawn((WorldAssetRoot(scene.clone()), Visibility::Hidden))
            .id();
        self.status = format!("loading {path}...");
        self.troubled = false;
        self.incoming = Some(Incoming {
            entity,
            path,
            scene,
            frames: 0,
        });
    }

    /// Abandon the load in flight, loudly, leaving the stage as it was.
    fn give_up(&mut self, commands: &mut Commands, reason: String) {
        if let Some(incoming) = self.incoming.take() {
            commands.entity(incoming.entity).despawn();
        }
        error!("{reason}");
        self.status = reason;
        self.troubled = true;
    }
}

/// The camera, framed on the subject once its bounds are known.
pub(super) fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: CAMERA_FOV_DEG.to_radians(),
            ..default()
        }),
        Msaa::Sample4,
        OrbitCamera::default(),
        Transform::default(),
    ));
}

/// Which mesh the studio opens on, out of what it was asked for and what the
/// library holds.
///
/// `--model` first, by any spelling the browser resolves; then the project's
/// `stage_body`; then the first body; then the first model — so a library
/// with anything in it always opens on something, and one with nothing
/// opens on an empty stage and says so. A `--model` that names nothing is a
/// refusal in the log and a fall through to the default, because a window
/// that opened empty over a typo would cost a restart to find out why.
///
/// A path that is not in the catalog but exists under the asset root is taken
/// as it is, and so is `out://<path>` for a file under the project's `out/`
/// ([`super::OUT_SOURCE`]): an export or a lift there is outside the library
/// and still a thing to look at, and the catalog's job is to list the
/// library, not to gate the stage.
pub(crate) fn opening_model(config: &StudioConfig, models: &ModelLibrary) -> Option<String> {
    if let Some(wanted) = config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|w| !w.is_empty())
    {
        if let Some(found) = models.resolve(wanted) {
            return Some(found.rel_path.clone());
        }
        let rel = wanted.trim_start_matches("./").replace('\\', "/");
        if config.project.assets.join(&rel).is_file() {
            return Some(rel);
        }
        if let Some(under_out) = rel.strip_prefix(&format!("{}://", super::OUT_SOURCE))
            && config.project.out.join(under_out).is_file()
        {
            return Some(rel);
        }
        error!(
            "no body or model named {wanted:?} — the library holds: {}",
            models
                .models()
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(found) = config
        .project
        .stage_body
        .as_deref()
        .and_then(|name| models.resolve(name))
        .filter(|m| m.kind == Kind::Body)
    {
        return Some(found.rel_path.clone());
    }
    models
        .models()
        .iter()
        .find(|m| m.kind == Kind::Body)
        .or_else(|| models.models().first())
        .map(|m| m.rel_path.clone())
}

/// Read the project's rig profile, so the stage knows what to judge by.
pub fn load_contract(config: Res<StudioConfig>, mut contract: ResMut<StageContract>) {
    match config.project.profile() {
        Ok(profile) => contract.0 = Some(profile.contract),
        Err(err) => error!(
            "the rig profile under {} would not load, so nothing on the stage is checked: {err}",
            config.project.rig_dir().display()
        ),
    }
}

/// Diagnose the stage whenever a rig stands up on it.
///
/// Runs once per [`ActiveModel::generation`], which covers the model the studio
/// opened on as well as every swap. Exclusive because
/// [`rig_findings::diagnose`] reads the whole world — the hierarchy, the mesh
/// assets and the clip — and a system that asked for all of that as
/// parameters would conflict with itself.
///
/// **Order is load-bearing**: this must run in `Update`, after
/// [`finish_loading`] and in the same frame, because it reads the rest pose
/// off live transforms. See [`crate::rig_findings`]'s module docs.
pub fn refresh_findings(world: &mut World, mut checked: Local<u32>) {
    let generation = world.resource::<ActiveModel>().generation;
    if generation == 0 || generation == *checked {
        return;
    }
    let rig = world.resource::<Rig>();
    let Some(anim_root) = rig.anim_root().filter(|_| rig.is_ready()) else {
        return;
    };
    *checked = generation;

    let model = world.resource::<ActiveModel>().path.clone();
    let has_mesh = world.resource::<Rig>().has_visible_mesh();
    let findings = if is_static_model(world, &model, has_mesh) {
        // A prop has no rig by construction, so 50-odd "contract bone
        // missing" lines would be the definition of a model recited as
        // failures — the way `check-bodies` says "no bodies … and that
        // passes", this says the one true thing and stops.
        vec![Finding::note(STATIC_MODEL_FINDING)]
    } else {
        match world.resource::<StageContract>().0.clone() {
            Some(contract) => {
                let reference = reference_clip(world.resource::<ClipLibrary>(), &contract);
                let clips = world.resource::<Assets<AnimationClip>>();
                let findings = rig_findings::diagnose(
                    world,
                    anim_root,
                    &contract,
                    reference.as_ref().and_then(|handle| clips.get(handle)),
                );
                rig_findings::log_failures(&model, &contract, &findings);
                findings
            }
            None => vec![Finding::warn(
                "no rig profile loaded - the contract checks did not run",
            )],
        }
    };
    world
        .resource_mut::<RigFindings>()
        .record(findings, u64::from(generation));
}

/// The one line the stage says about a static model instead of rig findings.
pub const STATIC_MODEL_FINDING: &str = "static model: no rig, nothing to hold to the contract";

/// Whether the subject standing on the stage is a static model.
///
/// The sidecar's word first: a library entry of kind `model` declared itself
/// unrigged, and holding it to the rig contract would flood the panel with
/// findings that are its definition, not its defects. A path outside the
/// library (a raw lift under out/, a foreign glb) has no sidecar to ask, so
/// the file answers: a mesh with no skin is decor a contract cannot bind.
/// `has_mesh` keeps the two things a missing skin can mean apart — a bare
/// skeleton (no mesh at all) is still a rig and clips still play on it — and
/// a library *body* with no skin stays a body: that flood is a real defect.
fn is_static_model(world: &mut World, model: &str, has_mesh: bool) -> bool {
    match world
        .resource::<ModelLibrary>()
        .get(model)
        .map(|entry| entry.kind)
    {
        Some(Kind::Model) => true,
        Some(_) => false,
        None => {
            let mut skins = world.query::<&bevy::mesh::skinning::SkinnedMesh>();
            has_mesh && skins.iter(world).next().is_none()
        }
    }
}

/// The clip to diff the skeleton against: the contract's reference clip if
/// the library has it, otherwise whatever is first.
///
/// Nothing is loaded to answer this. A diagnosis that pulled in an asset of its
/// own would be the one thing on the stage nobody asked for, and the studio has
/// already loaded every clip it lists by the time a rig stands up.
fn reference_clip(library: &ClipLibrary, contract: &Contract) -> Option<Handle<AnimationClip>> {
    library
        .items()
        .iter()
        .find(|item| item.entry.name == contract.reference_clip)
        .or_else(|| library.items().first())
        .map(|item| item.handle.clone())
}

/// Start loading the model the studio opens on.
///
/// A library with no meshes at all is not a failure: the stage stays empty,
/// the clip half stays inert, and the sounds are what there is to audition. A
/// [`SwapModel`] still works from there, which is what makes the stage a
/// stage rather than a property of the command line.
pub fn open_model(
    mut commands: Commands,
    config: Res<StudioConfig>,
    models: Res<ModelLibrary>,
    mut rig: ResMut<Rig>,
    mut active: ResMut<ActiveModel>,
    server: Res<AssetServer>,
) {
    let Some(path) = opening_model(&config, &models) else {
        active.path.clear();
        if config.model.is_some() || !models.models().is_empty() {
            rig.status = String::from("nothing to put on the stage");
        } else {
            rig.status = String::from("no bodies in the library - the stage is empty");
        }
        return;
    };
    active.path.clone_from(&path);
    rig.begin(&mut commands, &server, path);
}

/// Turn a [`SwapModel`] into a load in flight.
///
/// Registered before [`finish_loading`] so a request made this frame starts
/// this frame rather than one frame later, which matters only in that it keeps
/// the two systems' order the same as their order in this file.
pub fn request_swap(
    mut requests: MessageReader<SwapModel>,
    mut commands: Commands,
    mut rig: ResMut<Rig>,
    server: Res<AssetServer>,
) {
    // Last wins: two requests in one frame are two clicks, and the second one
    // is the model the person ended up on.
    let Some(request) = requests.read().last() else {
        return;
    };
    let path = request.path.trim().to_owned();
    if path.is_empty() {
        rig.give_up(
            &mut commands,
            String::from("a swap needs a model path, and this one is empty"),
        );
        return;
    }
    // A load already in flight is abandoned rather than raced: its entity would
    // otherwise stand up over the top of this one.
    if let Some(superseded) = rig.incoming.take() {
        commands.entity(superseded.entity).despawn();
    }
    rig.begin(&mut commands, &server, path);
}

/// Drive the model in flight until it stands, and wire it when it does.
///
/// Runs every frame while something is loading and returns immediately when
/// nothing is. The whole sequence — and the order within it, which is
/// load-bearing — is described in the module docs.
pub fn finish_loading(
    mut commands: Commands,
    mut rig: ResMut<Rig>,
    mut library: ResMut<ClipLibrary>,
    server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    clips: Res<Assets<AnimationClip>>,
    children: Query<&Children>,
) {
    let Some(incoming) = rig.incoming.as_mut() else {
        return;
    };
    incoming.frames += 1;
    let (entity, frames) = (incoming.entity, incoming.frames);
    let path = incoming.path.clone();
    // A load that failed is reported now rather than waited out: the asset
    // server is never going to answer, and fifteen seconds of silence is not a
    // diagnosis. The server's own reason is carried along, because "no such
    // file" and "that glTF will not parse" are different problems.
    let refused = match server.get_load_state(&incoming.scene) {
        Some(LoadState::Failed(err)) => Some(format!("cannot load {path}: {err}")),
        _ => None,
    };
    if let Some(reason) = refused {
        rig.give_up(&mut commands, reason);
        return;
    }
    // The scene spawns its hierarchy over several frames and reports nothing
    // when it is done, so the children appearing is the signal.
    if !children.get(entity).is_ok_and(|c| !c.is_empty()) {
        if frames > MAX_LOAD_FRAMES {
            rig.give_up(
                &mut commands,
                format!("{path} spawned nothing in {MAX_LOAD_FRAMES} frames"),
            );
        }
        return;
    }

    let Some(graph) = build_graph(&mut rig, &mut library, &clips, &mut graphs, &server) else {
        return;
    };
    let outgoing = rig.scene;

    commands.queue(move |world: &mut World| {
        let Some(anim_root) = find_animation_root(world, entity) else {
            world.entity_mut(entity).despawn();
            let reason = format!("{path} has no named node hierarchy, so nothing can animate it");
            error!("{reason}");
            let mut rig = world.resource_mut::<Rig>();
            rig.incoming = None;
            rig.status = reason;
            rig.troubled = true;
            return;
        };
        if world.get::<AnimationTargetId>(anim_root).is_none() {
            install_animation_targets(world, anim_root);
        }
        // Before the player: see Rig::rest_frames.
        let rest_frames = RigFrames::from_world(world, anim_root);

        // Only now does the outgoing model leave: everything above could still
        // have refused, and a stage with nothing on it explains nothing.
        if let Some(outgoing) = outgoing {
            world.entity_mut(outgoing).despawn();
        }
        world
            .entity_mut(anim_root)
            .insert((AnimationPlayer::default(), AnimationGraphHandle(graph)));
        world.entity_mut(entity).insert(Visibility::Inherited);

        let paths = SkeletonPaths::from_world(world, anim_root);
        let mesh = has_mesh(world, entity);
        // A static model is not "missing rig joints" — it never claimed any.
        let note = if is_static_model(world, &path, mesh) {
            format!("{path} on stage — {STATIC_MODEL_FINDING}")
        } else {
            describe(&path, &rest_frames, mesh)
        };
        // After the despawn above, so the model that just left does not drag
        // the framing halfway towards where it stood. A meshless rig has no
        // AABBs to frame on, so the camera frames the bones themselves —
        // which is also what the skeleton overlay will be drawing.
        let (lo, hi) = world_bounds(world)
            .or_else(|| skeleton_bounds(world))
            .unwrap_or((Vec3::ZERO, Vec3::splat(1.0)));
        let centre = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(0.05);

        let mut cameras = world.query::<(&mut OrbitCamera, &mut Transform)>();
        for (mut orbit, mut transform) in cameras.iter_mut(world) {
            orbit.frame(centre, radius);
            *transform = orbit.transform();
        }

        let mut rig = world.resource_mut::<Rig>();
        rig.anim_root = Some(anim_root);
        rig.rest_frames = Some(rest_frames);
        rig.paths = paths;
        rig.scene = Some(entity);
        rig.incoming = None;
        rig.ready = true;
        rig.status = note;
        rig.troubled = false;
        rig.visible_mesh = mesh;

        // The selection is deliberately untouched; the clip it names starts
        // again from the top on a body that has never been posed.
        let mut playback = world.resource_mut::<Playback>();
        playback.time = 0.0;
        playback.dirty = true;

        world.resource_mut::<ActiveModel>().completed(path);
    });
}

/// The graph every clip plays from, built once and kept across swaps.
///
/// `None` means the library has clips that have not resolved yet and the
/// caller should try again next frame: a graph missing nodes would silently
/// renumber the indices the library holds. A clip whose load *failed* is
/// resolved for this purpose — it gets a node it will never play through,
/// and the rig stands up regardless, because one corrupt file must not keep
/// the whole stage dark.
fn build_graph(
    rig: &mut Rig,
    library: &mut ClipLibrary,
    clips: &Assets<AnimationClip>,
    graphs: &mut Assets<AnimationGraph>,
    server: &AssetServer,
) -> Option<Handle<AnimationGraph>> {
    if let Some(graph) = &rig.graph {
        return Some(graph.clone());
    }
    if !clips_resolved(library, clips, server) {
        return None;
    }
    let handles: Vec<Handle<AnimationClip>> = library
        .items()
        .iter()
        .map(|item| item.handle.clone())
        .collect();
    let (graph, nodes) = AnimationGraph::from_clips(handles);
    // `from_clips` returns nodes in handle order, which is item order.
    library.assign_nodes(nodes);
    let handle = graphs.add(graph);
    rig.graph = Some(handle.clone());
    Some(handle)
}

/// The bounds of the bones themselves, for framing a model with no mesh.
///
/// Every [`AnimationTargetId`] in the world belongs to the rig being stood up:
/// this runs after the outgoing model is despawned and the incoming one is the
/// only scene on stage. Padded a little so the camera does not crop the
/// fingertips of a skeleton drawn as lines.
fn skeleton_bounds(world: &mut World) -> Option<(Vec3, Vec3)> {
    let mut query = world.query_filtered::<&GlobalTransform, With<AnimationTargetId>>();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for transform in query.iter(world) {
        let position = transform.translation();
        lo = lo.min(position);
        hi = hi.max(position);
        any = true;
    }
    any.then_some((lo - Vec3::splat(0.15), hi + Vec3::splat(0.15)))
}

/// Draw the skeleton when there is nothing else to see.
///
/// The bare `rig.glb` is a legitimate thing to put on stage — it is the
/// contract every mesh is skinned to — and an empty viewport says nothing
/// about it. Lines run parent-to-child between animation targets, so what
/// plays on the bones is visible exactly where a mesh would have been. Only
/// when no mesh draws: over a skinned character the overlay would read as a
/// glitch, not information.
pub(super) fn draw_skeleton(
    rig: Res<Rig>,
    mut gizmos: Gizmos,
    bones: Query<(Entity, &GlobalTransform, Option<&ChildOf>), With<AnimationTargetId>>,
) {
    if !rig.is_ready() || rig.has_visible_mesh() {
        return;
    }
    for (_, transform, child_of) in &bones {
        let position = transform.translation();
        gizmos.sphere(Isometry3d::from_translation(position), 0.008, theme::ACCENT);
        if let Some(child_of) = child_of
            && let Ok((_, parent_transform, _)) = bones.get(child_of.parent())
        {
            gizmos.line(parent_transform.translation(), position, theme::ACCENT);
        }
    }
}

/// Whether anything under `root` will actually draw.
///
/// The profile's `rig.glb` is bones and nothing else, which is a legitimate
/// thing to put on the stage — the clips bind and play, there is simply
/// nothing to see — and saying so is the difference between that and a load
/// that went wrong.
fn has_mesh(world: &World, root: Entity) -> bool {
    if world.get::<Mesh3d>(root).is_some() {
        return true;
    }
    let Some(children) = world.get::<Children>(root) else {
        return false;
    };
    let children: Vec<Entity> = children.iter().collect();
    children.into_iter().any(|child| has_mesh(world, child))
}

/// One line about the model that just stood up, worst news first.
fn describe(path: &str, rest_frames: &RigFrames, mesh: bool) -> String {
    let missing = rest_frames.missing_joints();
    if rest_frames.is_empty() {
        return format!("{path} has no named bones under its animation root");
    }
    if !missing.is_empty() {
        let named = missing
            .iter()
            .take(JOINTS_NAMED)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let rest = missing.len().saturating_sub(JOINTS_NAMED);
        let more = if rest > 0 {
            format!(" and {rest} more")
        } else {
            String::new()
        };
        return format!("{path} is missing rig joints: {named}{more}");
    }
    if mesh {
        format!("{path} on stage")
    } else {
        format!("{path} has no visible mesh - the bones are there and clips play on them")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter every "once per rig" system will hang off. It has to move
    /// for the model the studio opened on too, or such a system would never
    /// run at all in a session where nobody swapped anything.
    #[test]
    fn a_rig_standing_up_bumps_the_generation() {
        let mut active = ActiveModel::default();
        assert_eq!(active.generation, 0, "nothing has stood up yet");
        assert!(active.path.is_empty());

        active.completed(String::from("bodies/vex_runner.glb"));
        assert_eq!(active.generation, 1);
        assert_eq!(active.path, "bodies/vex_runner.glb");

        active.completed(String::from("models/barrel.glb"));
        assert_eq!(active.generation, 2);
        assert_eq!(active.path, "models/barrel.glb");
    }

    /// A library with no meshes is not a failure: nothing is loaded, nothing
    /// is refused, and the stage stays empty with a line saying so.
    #[test]
    fn opening_without_a_mesh_asks_for_nothing() {
        let (dir, project) = temp_project();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<Rig>()
            .init_resource::<ActiveModel>()
            .init_resource::<ModelLibrary>()
            .insert_resource(config(project, None))
            .add_systems(Startup, open_model);
        app.update();

        assert!(!app.world().resource::<Rig>().is_swapping());
        assert!(
            app.world().resource::<Rig>().status().contains("empty"),
            "{}",
            app.world().resource::<Rig>().status()
        );
        assert!(app.world().resource::<ActiveModel>().path.is_empty());
        assert_eq!(app.world().resource::<ActiveModel>().generation, 0);
        drop(dir);
    }

    /// What the window opens on has to be readable as the active model before
    /// anything has finished loading — that string is what the browser
    /// highlights on the very first frame. The default is the first body, by
    /// whatever spelling `--model` or the project used.
    #[test]
    fn the_opening_model_is_the_active_one_from_the_start() {
        let (dir, project) = temp_project();
        let mut models = ModelLibrary::default();
        models.push("barrel", "models/barrel.glb", Kind::Model);
        models.push("vex_runner", "bodies/vex_runner.glb", Kind::Body);

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<Rig>()
            .init_resource::<ActiveModel>()
            .insert_resource(models)
            .insert_resource(config(project, Some("VEX_RUNNER")))
            .add_systems(Startup, open_model);
        app.update();

        let active = app.world().resource::<ActiveModel>();
        assert_eq!(active.path, "bodies/vex_runner.glb");
        // Asked for, not standing: the counter is what says which.
        assert_eq!(active.generation, 0);
        assert!(app.world().resource::<Rig>().is_swapping());
        assert!(!app.world().resource::<Rig>().is_ready());
        drop(dir);
    }

    /// The fall-through order: a typo in `--model` lands on the default body
    /// rather than on an empty stage; the project's `stage_body` beats the
    /// first body; a library with only props opens on the first prop.
    #[test]
    fn the_opening_model_falls_through_to_what_the_library_has() {
        let (dir, mut project) = temp_project();
        let mut models = ModelLibrary::default();
        models.push("barrel", "models/barrel.glb", Kind::Model);
        models.push("aria", "bodies/aria.glb", Kind::Body);
        models.push("vex_runner", "bodies/vex_runner.glb", Kind::Body);

        assert_eq!(
            opening_model(&config(project.clone(), Some("nope")), &models).as_deref(),
            Some("bodies/aria.glb")
        );
        project.stage_body = Some(String::from("vex_runner"));
        assert_eq!(
            opening_model(&config(project.clone(), None), &models).as_deref(),
            Some("bodies/vex_runner.glb")
        );
        assert_eq!(
            opening_model(&config(project.clone(), Some("barrel")), &models).as_deref(),
            Some("models/barrel.glb")
        );

        let mut props = ModelLibrary::default();
        props.push("barrel", "models/barrel.glb", Kind::Model);
        assert_eq!(
            opening_model(&config(project.clone(), None), &props).as_deref(),
            Some("models/barrel.glb")
        );
        assert_eq!(
            opening_model(&config(project.clone(), None), &ModelLibrary::default()),
            None
        );

        // A file outside the catalog but under the asset root is taken as it
        // is — a lift under out/ copied in to be looked at, say.
        let stray = project.assets.join("stray.glb");
        std::fs::write(&stray, b"glb").expect("write");
        assert_eq!(
            opening_model(
                &config(project, Some("stray.glb")),
                &ModelLibrary::default()
            )
            .as_deref(),
            Some("stray.glb")
        );
        drop(dir);
    }

    /// A request with nothing in it must not spawn a scene called `#Scene0`
    /// and then wait fifteen seconds to find out it does not exist.
    #[test]
    fn an_empty_swap_is_refused_out_loud() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<Rig>()
            .add_message::<SwapModel>()
            .add_systems(Update, request_swap);
        app.world_mut().write_message(SwapModel::new("   "));
        app.update();

        let rig = app.world().resource::<Rig>();
        assert!(!rig.is_swapping(), "an empty path started a load");
        assert!(rig.status().contains("empty"), "{}", rig.status());
    }

    /// A rig with no mesh is a supported thing to look at; a rig missing
    /// joints is a defect. Both have to be said, and the defect first.
    #[test]
    fn the_report_names_what_is_wrong_before_what_is_merely_odd() {
        let complete = RigFrames::default();
        assert!(
            describe("models/x.glb", &complete, true).contains("no named bones"),
            "an empty rig is the worst news there is"
        );
    }

    fn temp_project() -> (tempfile::TempDir, forge_library::Project) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = forge_library::Project::init(dir.path(), "studio_test").expect("init");
        (dir, project)
    }

    fn config(project: forge_library::Project, model: Option<&str>) -> StudioConfig {
        StudioConfig {
            project,
            model: model.map(ToOwned::to_owned),
            audio: false,
            take: None,
            recipe: None,
            screenshot: None,
            selftest: false,
        }
    }
}
