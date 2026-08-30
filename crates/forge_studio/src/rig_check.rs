//! Validate a skinned mesh against the project's rig contract: `forge rig
//! check <mesh.glb> [--out sheet.png]`.
//!
//! The failure mode this tool exists for is silent: Bevy binds animation
//! curves by hashing each bone's full name path from the animation root, so a
//! mesh whose rig merely *resembles* the profile's — one inserted root bone,
//! one renamed joint — spawns fine, renders fine, and then holds its rest
//! pose forever with no warning at any log level. Every check here turns one
//! way that can happen into a named finding, printed one per line; the exit
//! code is non-zero when any of them fails.
//!
//! The mesh is spawned for real in a headless Bevy app rather than parsed
//! from the file, so what gets checked is the hierarchy the engine will
//! actually bind against — importer quirks included. Without `--out` that app
//! has no renderer at all ([`headless_app`]), so the check runs anywhere
//! `cargo test` does. With `--out` the mesh is also rendered playing the
//! profile's reference clip as a contact sheet, because "binds correctly" and
//! "deforms acceptably" are different bars and only a picture judges the
//! second; that render needs a wgpu adapter, which is the one reason the two
//! paths are separate.
//!
//! The reference clip is the one the contract names ([`Contract::reference_clip`]),
//! looked up in the project's library by name. A library that has not
//! promoted it yet is not a failure of the mesh: the binding finding is
//! replaced by a warning saying what was not checked, and the sheet, if
//! asked for, shows the rest pose from the walk-around views instead.
//!
//! The checks themselves live in [`crate::rig_findings`], because the studio
//! runs the same ones on whatever is standing on its stage. This module owns
//! the world the subject is spawned into, the staging, the report and the
//! exit-code rule, and nothing else.

use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::{
    animation::{AnimationClip, AnimationTargetId},
    asset::LoadState,
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot},
};
use forge_library::{Catalog, Kind, Project};
use forge_rig::Contract;

use crate::binding::{headless_app, install_animation_targets};
use crate::render::{
    RenderError, SheetRequest, Stage, ViewsRequest, render_clip_sheet, render_views,
};
use crate::rig_findings::{self, Finding, Severity, diagnose};
use crate::stage::find_animation_root;

/// Frames to wait for assets before declaring them stuck.
const MAX_LOAD_FRAMES: u32 = 900;

/// The subject's name inside the staging root.
const SUBJECT: &str = "subject.glb";
/// The reference clip's name inside the staging root.
const REFERENCE: &str = "reference.glb";

/// Why a check could not be run at all — as opposed to a check that ran and
/// failed, which is a [`Finding`] in the report.
#[derive(Debug)]
pub enum RigCheckError {
    /// The subject is not a file.
    NotAFile(PathBuf),
    /// The project's rig profile does not load.
    Profile(String),
    /// The scratch root the subject is spawned from could not be made.
    Staging(String),
    /// The subject or the reference clip never loaded, or the subject spawned
    /// no skeleton.
    Spawn(RenderError),
    /// The `--out` sheet could not be rendered.
    Render(RenderError),
    /// The `--out` sheet could not be written.
    Write(String),
}

impl fmt::Display for RigCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFile(path) => write!(f, "{} is not a file", path.display()),
            Self::Profile(detail) => write!(f, "the rig profile does not load: {detail}"),
            Self::Staging(detail) => write!(f, "cannot stage the subject: {detail}"),
            Self::Spawn(error) => write!(f, "the subject did not spawn: {error}"),
            Self::Render(error) => write!(f, "sheet render failed: {error}"),
            Self::Write(detail) => write!(f, "cannot write the sheet: {detail}"),
        }
    }
}

impl std::error::Error for RigCheckError {}

/// What `forge rig check` says about one mesh.
#[derive(Debug, Clone)]
pub struct CheckReport {
    /// The mesh that was checked, as given.
    pub subject: PathBuf,
    /// The profile the contract came from, as `name v<version>`.
    pub profile: String,
    /// The reference clip, relative to the project's asset root, when the
    /// library had one to bind.
    pub reference: Option<String>,
    /// Every finding, in report order.
    pub findings: Vec<Finding>,
    /// The sheet that was written, when one was asked for.
    pub sheet: Option<PathBuf>,
    /// The sheet render's own summary — bounds, adapter, bones driven — when
    /// one was rendered.
    pub sheet_summary: Option<String>,
}

impl CheckReport {
    /// True when any finding failed. Notes and warnings never decide this.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.findings.iter().any(|finding| !finding.passed())
    }

    /// How many findings carry `severity`.
    #[must_use]
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }

    /// The report as the command prints it: the subject and the reference,
    /// one line per finding marked `ok:` / `note:` / `WARN:` / `FAIL:`, the
    /// sheet's summary when one was rendered, and the tally.
    #[must_use]
    pub fn text(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for CheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "subject:   {}", self.subject.display())?;
        writeln!(f, "profile:   {}", self.profile)?;
        match &self.reference {
            Some(reference) => writeln!(f, "reference: {reference}")?,
            None => writeln!(f, "reference: none in the library")?,
        }
        for finding in &self.findings {
            // Padded so the lines align: `ok:   `, `note: `, `WARN: `, `FAIL: `.
            writeln!(f, "{:<5} {}", finding.severity.mark(), finding.text)?;
        }
        if let Some(summary) = &self.sheet_summary {
            f.write_str(summary)?;
        }
        if let Some(sheet) = &self.sheet {
            writeln!(f, "sheet: {}", sheet.display())?;
        }
        write!(
            f,
            "{} finding(s) passed, {} failed, {} note(s), {} warning(s)",
            self.count(Severity::Ok),
            self.count(Severity::Fail),
            self.count(Severity::Note),
            self.count(Severity::Warn)
        )
    }
}

/// Run every check on `glb` against `project`'s contract; render it playing
/// the reference clip to `out` when asked.
///
/// The report's [`CheckReport::failed`] is the exit-code rule. A sheet is
/// rendered even when findings failed: a human deciding what to send back to
/// an artist wants to see the failure, not imagine it.
///
/// # Errors
///
/// Returns [`RigCheckError`] when the check cannot be run at all — the
/// subject is not a file, the profile does not load, the subject never
/// spawns, or the sheet cannot be rendered or written. A check that ran and
/// found the mesh wanting is a finding in the report, never an error.
pub fn run(
    project: &Project,
    glb: &Path,
    out: Option<&Path>,
) -> Result<CheckReport, RigCheckError> {
    if !glb.is_file() {
        return Err(RigCheckError::NotAFile(glb.to_path_buf()));
    }
    let profile = project
        .profile()
        .map_err(|error| RigCheckError::Profile(error.to_string()))?;
    let contract = &profile.contract;
    let reference = reference_clip(project, contract);

    // Bevy loads assets below one root, and the subject can live anywhere, so
    // both files are staged into a scratch root of their own. Copying the
    // reference clip beside the mesh instead would write into the artist's
    // directory, which a validator has no business doing.
    let staging = Staging::new()?;
    let stage = staging.stage(glb, reference.as_ref().map(|r| r.path.as_path()))?;

    let mut findings = check_spawned_mesh(&stage, contract, reference.is_some())?;
    // The contact-feet gate needs the rig *posed*, and every other check
    // needs it at rest, so it runs in a world of its own. The cost is one
    // more spawn of the same two files, which is cheaper than the class of
    // bug a posed rest-rotation check would let through.
    let (samples, whole_clip) = if reference.is_some() {
        contacts::sample(&stage, &profile).map_or((None, None), |m| (Some(m.feet), m.whole_clip))
    } else {
        (None, None)
    };
    rig_findings::check_contact_feet(contract, samples.as_deref(), whole_clip, &mut findings);
    if reference.is_none() {
        findings.push(Finding::warn(format!(
            "no reference clip '{}' in the library; {} binding not checked — promote it, \
             or bind this mesh in the studio to see what moves",
            contract.reference_clip, contract.reference_clip
        )));
    }

    let mut report = CheckReport {
        subject: glb.to_path_buf(),
        profile: format!("{} v{}", contract.name, contract.version),
        reference: reference.as_ref().map(|r| r.rel_path.clone()),
        findings,
        sheet: None,
        sheet_summary: None,
    };
    if let Some(out) = out {
        report.sheet_summary = Some(render_sheet(&stage, reference.is_some(), out)?);
        report.sheet = Some(out.to_path_buf());
    }
    Ok(report)
}

/// The library's copy of the contract's reference clip.
struct Reference {
    /// On disk.
    path: PathBuf,
    /// Relative to the asset root, for the report.
    rel_path: String,
}

/// Look the contract's reference clip up in the project's library by name.
fn reference_clip(project: &Project, contract: &Contract) -> Option<Reference> {
    let catalog = Catalog::scan(project);
    let record = catalog.resolve(&contract.reference_clip, Some(Kind::Clip))?;
    Some(Reference {
        path: record.path.clone(),
        rel_path: record.rel_path.clone(),
    })
}

/// Distinguishes concurrent checks in one process — the tests run several.
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A scratch asset root that removes itself.
struct Staging {
    dir: PathBuf,
}

impl Staging {
    fn new() -> Result<Self, RigCheckError> {
        let dir = std::env::temp_dir().join(format!(
            "forge_rig_check_{}_{}",
            std::process::id(),
            STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)
            .map_err(|error| RigCheckError::Staging(format!("{}: {error}", dir.display())))?;
        Ok(Self { dir })
    }

    /// Copy the subject and the reference clip in, and describe the result as
    /// a [`Stage`].
    fn stage(&self, subject: &Path, reference: Option<&Path>) -> Result<Stage, RigCheckError> {
        let copy = |from: &Path, name: &str| {
            std::fs::copy(from, self.dir.join(name))
                .map(|_| ())
                .map_err(|error| RigCheckError::Staging(format!("{}: {error}", from.display())))
        };
        copy(subject, SUBJECT)?;
        if let Some(reference) = reference {
            copy(reference, REFERENCE)?;
        }
        Ok(Stage::new(&self.dir, SUBJECT))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Spawn the subject with no renderer and run every check on the result.
fn check_spawned_mesh(
    stage: &Stage,
    contract: &Contract,
    with_reference: bool,
) -> Result<Vec<Finding>, RigCheckError> {
    let mut app = headless_app(&stage.absolute_root());

    let (scene, clip) = {
        let server = app.world().resource::<AssetServer>().clone();
        (
            server.load::<WorldAsset>(stage.scene_path()),
            with_reference.then(|| server.load::<AnimationClip>(format!("{REFERENCE}#Animation0"))),
        )
    };
    let spawned = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
    wait_for(&mut app, spawned, &scene, clip.as_ref()).map_err(RigCheckError::Spawn)?;

    let anim_root = find_animation_root(app.world_mut(), spawned)
        .ok_or(RigCheckError::Spawn(RenderError::NoSkeleton))?;
    // A body exported without animations of its own carries no animation
    // targets, and a clip would bind to nothing on it however right its
    // names are. Installed here exactly as the renderer and the viewer do, so
    // the binding finding is about the names and not about the export.
    if app.world().get::<AnimationTargetId>(anim_root).is_none() {
        install_animation_targets(app.world_mut(), anim_root);
    }

    // The rest pose must be read before any AnimationPlayer exists, or the
    // transforms would already carry a frame of animation. This app never
    // creates one — playback belongs to the --out render.
    let world = app.world();
    let reference = clip
        .as_ref()
        .and_then(|handle| world.resource::<Assets<AnimationClip>>().get(handle));
    let mut findings = diagnose(world, anim_root, contract, reference);
    // The direction rule reads the same live transforms the rest-rotation
    // check does, so it belongs in this app and no other.
    rig_findings::check_rest_directions(
        contract,
        &rig_findings::collect_bones(world, anim_root),
        &mut findings,
    );
    if with_reference && reference.is_none() {
        // `diagnose` simply leaves the binding finding out when it has no clip.
        // Here that is a defect and not a choice: this app loaded one and waited
        // for it, so its absence means the asset went away underneath us.
        findings.push(Finding::fail("the reference clip vanished after loading"));
    }
    Ok(findings)
}

/// Wait until the scene has spawned and the reference clip, if any, has loaded.
fn wait_for(
    app: &mut App,
    spawned: Entity,
    scene: &Handle<WorldAsset>,
    clip: Option<&Handle<AnimationClip>>,
) -> Result<(), RenderError> {
    for _ in 0..MAX_LOAD_FRAMES {
        app.update();
        let (scene_state, clip_state) = {
            let server = app.world().resource::<AssetServer>();
            (
                server.get_load_state(scene),
                clip.and_then(|clip| server.get_load_state(clip)),
            )
        };
        for (what, state) in [("mesh", &scene_state), ("reference clip", &clip_state)] {
            if let Some(LoadState::Failed(error)) = state {
                return Err(RenderError::AssetLoad(format!(
                    "the {what} failed to load: {error}"
                )));
            }
        }
        let has_children = app
            .world()
            .get::<Children>(spawned)
            .is_some_and(|c| !c.is_empty());
        let clip_ready = clip.is_none() || matches!(clip_state, Some(LoadState::Loaded));
        if has_children && clip_ready {
            return Ok(());
        }
    }
    Err(RenderError::AssetLoad(format!(
        "the mesh did not spawn within {MAX_LOAD_FRAMES} frames — does the glb hold a scene?"
    )))
}

/// CPU-skinning the reference clip to find out where a planted foot's own
/// lowest vertex sits.
///
/// # Why a second world, and why on the CPU
///
/// The rest-pose checks must read a rig nothing has posed, so they run in an
/// app with no `AnimationPlayer` at all. This one needs the opposite: the
/// clip bound and played, frame by frame. And it has to skin, because the
/// question is about a **vertex**, not a bone — a heel joint at y = 0.01 m
/// says nothing about whether the sole is through the floor.
///
/// Everything here is arithmetic on data the engine already computed: Bevy
/// poses the skeleton and propagates transforms, and this reads the joint
/// matrices back out and applies them to the mesh the way any skinning
/// shader would. No renderer, so it runs wherever `cargo test` does.
mod contacts {
    use std::collections::{HashMap, HashSet};

    use bevy::{
        animation::{AnimationClip, AnimationTargetId},
        asset::LoadState,
        mesh::{
            Mesh, Mesh3d, VertexAttributeValues,
            skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        },
        prelude::*,
        world_serialization::{WorldAsset, WorldAssetRoot},
    };
    use forge_rig::{Contract, MotionSkeleton, RigProfile};

    use crate::binding::{attach_player, headless_app, install_animation_targets};
    use crate::render::Stage;
    use crate::rig_findings::FootContact;
    use crate::stage::{find_animation_root, seek};

    use super::{MAX_LOAD_FRAMES, REFERENCE};

    /// Frames a second the clip is walked at. Finer than any clip in the
    /// library is baked at, so no contact falls between two samples.
    const SAMPLE_FPS: f32 = 60.0;
    /// A ceiling on the walk, so a pathological clip cannot turn a check
    /// into a minute. Ten seconds at [`SAMPLE_FPS`].
    const MAX_SAMPLES: usize = 600;
    /// A vertex belongs to a foot when that foot's bones carry more than
    /// this much of its weight. Over a half, so at most one foot can claim
    /// a vertex and an ankle shared with a shin is counted once or not at
    /// all.
    const FOOT_WEIGHT: f32 = 0.5;

    /// What one walk of the reference clip measured.
    pub(super) struct Measured {
        /// Every foot on every frame after the first.
        pub(super) feet: Vec<FootContact>,
        /// The lowest vertex anywhere in the mesh over the whole clip — the
        /// number the studio's own sheet reports, kept as a note because it
        /// answers a different question from the gate.
        pub(super) whole_clip: Option<f32>,
    }

    /// One foot: the heel bone that names it, and every contract bone whose
    /// weights count as that foot's skin.
    struct Foot {
        /// The heel bone's name — what the finding says.
        name: String,
        /// The heel and everything under it in the contract.
        bones: HashSet<String>,
    }

    /// One skinned mesh, reduced to the arithmetic.
    struct Skin {
        /// Bind-pose positions.
        positions: Vec<Vec3>,
        /// Per vertex, four `(joint index, weight)` pairs — glTF's fixed
        /// four, zero weights included.
        influences: Vec<[(usize, f32); 4]>,
        /// The joint entities, by joint index.
        joints: Vec<Entity>,
        /// The inverse bind matrix per joint index.
        inverse_bind: Vec<Mat4>,
        /// Per foot (by index into the foot table), the vertices it carries.
        foot_vertices: Vec<(usize, Vec<usize>)>,
    }

    /// Pose the reference clip on the subject and measure every foot on
    /// every frame, or `None` when the clip cannot be posed at all — no
    /// skinned mesh, no skeleton, a clip that never loads.
    pub(super) fn sample(stage: &Stage, profile: &RigProfile) -> Option<Measured> {
        let feet = feet_of(&profile.contract, &profile.motion);
        if feet.is_empty() {
            return None;
        }

        let mut app = headless_app(&stage.absolute_root());
        let (scene, clip) = {
            let server = app.world().resource::<AssetServer>().clone();
            (
                server.load::<WorldAsset>(stage.scene_path()),
                server.load::<AnimationClip>(format!("{REFERENCE}#Animation0")),
            )
        };
        let spawned = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
        let mut ready = false;
        for _ in 0..MAX_LOAD_FRAMES {
            app.update();
            let loaded = app.world().resource::<AssetServer>().get_load_state(&clip);
            let spawned_children = app
                .world()
                .get::<Children>(spawned)
                .is_some_and(|c| !c.is_empty());
            if spawned_children && matches!(loaded, Some(LoadState::Loaded)) {
                ready = true;
                break;
            }
        }
        if !ready {
            return None;
        }
        let anim_root = find_animation_root(app.world_mut(), spawned)?;
        if app.world().get::<AnimationTargetId>(anim_root).is_none() {
            install_animation_targets(app.world_mut(), anim_root);
        }
        let duration = app
            .world()
            .resource::<Assets<AnimationClip>>()
            .get(&clip)?
            .duration();
        if !(duration.is_finite() && duration > 0.0) {
            return None;
        }
        let node = attach_player(app.world_mut(), anim_root, clip);

        let named = named_entities(app.world_mut());
        let heels: Vec<Option<Entity>> = feet
            .iter()
            .map(|foot| named.get(&foot.name).copied())
            .collect();
        let skins = read_skins(app.world_mut(), &feet, &named);
        if skins.is_empty() {
            return None;
        }

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the frame count is clamped into a small range on the next line"
        )]
        let frames = ((duration * SAMPLE_FPS).ceil() as usize).clamp(2, MAX_SAMPLES);
        let mut measured: Vec<FootContact> = Vec::new();
        let mut whole_clip = f32::INFINITY;
        let mut previous: Vec<Option<(f32, Vec3)>> = vec![None; feet.len()];

        for frame in 0..frames {
            #[expect(
                clippy::cast_precision_loss,
                reason = "frame counts here are in the hundreds"
            )]
            let time = duration * (frame as f32) / ((frames - 1) as f32);
            seek(app.world_mut(), anim_root, node, time);
            app.update();

            let world = app.world_mut();
            for skin in &skins {
                let matrices = skin_matrices(world, skin);
                for vertex in 0..skin.positions.len() {
                    whole_clip = whole_clip.min(skinned(skin, &matrices, vertex).y);
                }
                for (foot, vertices) in &skin.foot_vertices {
                    let lowest = vertices
                        .iter()
                        .map(|&vertex| skinned(skin, &matrices, vertex).y)
                        .fold(f32::INFINITY, f32::min);
                    let Some(heel) = heels[*foot] else { continue };
                    let Some(here) = world
                        .get::<GlobalTransform>(heel)
                        .map(GlobalTransform::translation)
                    else {
                        continue;
                    };
                    // The first frame has nothing to difference against, so
                    // it has no speed and is not a sample: a zero written
                    // there would read as a contact wherever the foot
                    // happened to start.
                    if let Some((then, there)) = previous[*foot]
                        && lowest.is_finite()
                    {
                        let dt = (time - then).max(1e-4);
                        measured.push(FootContact {
                            foot: feet[*foot].name.clone(),
                            time,
                            speed_mps: Vec3::new(here.x - there.x, 0.0, here.z - there.z).length()
                                / dt,
                            lowest_y: lowest,
                        });
                    }
                    previous[*foot] = Some((time, here));
                }
            }
        }
        Some(Measured {
            feet: measured,
            whole_clip: whole_clip.is_finite().then_some(whole_clip),
        })
    }

    /// One vertex, skinned: the weighted sum of its joints' matrices applied
    /// to its bind position — what any skinning shader computes, in f32, on
    /// the CPU.
    fn skinned(skin: &Skin, matrices: &[Mat4], vertex: usize) -> Vec3 {
        let bind = skin.positions[vertex];
        let mut out = Vec3::ZERO;
        for &(joint, weight) in &skin.influences[vertex] {
            if weight > 0.0
                && let Some(matrix) = matrices.get(joint)
            {
                out += weight * matrix.transform_point3(bind);
            }
        }
        out
    }

    /// Each joint's skinning matrix this frame: its world transform times
    /// its inverse bind pose. The armature is identity — `check_root` fails
    /// a mesh whose is not — so this is world space.
    fn skin_matrices(world: &mut World, skin: &Skin) -> Vec<Mat4> {
        skin.joints
            .iter()
            .zip(&skin.inverse_bind)
            .map(|(&joint, &inverse)| {
                world
                    .get::<GlobalTransform>(joint)
                    .map_or(Mat4::IDENTITY, |at| at.to_matrix() * inverse)
            })
            .collect()
    }

    /// Every named entity, by name. Names are unique on a conforming rig —
    /// `check_contract_bones` fails a mesh where they are not — so the last
    /// writer winning is not a case that reaches a passing body.
    fn named_entities(world: &mut World) -> HashMap<String, Entity> {
        world
            .iter_entities()
            .filter_map(|entity| Some((entity.get::<Name>()?.as_str().to_owned(), entity.id())))
            .collect()
    }

    /// Every skinned mesh in the world, with each foot's vertices already
    /// picked out.
    fn read_skins(world: &mut World, feet: &[Foot], named: &HashMap<String, Entity>) -> Vec<Skin> {
        let by_entity: HashMap<Entity, &String> =
            named.iter().map(|(name, entity)| (*entity, name)).collect();
        let found: Vec<(Handle<Mesh>, SkinnedMesh)> = world
            .iter_entities()
            .filter_map(|entity| {
                Some((
                    entity.get::<Mesh3d>()?.0.clone(),
                    entity.get::<SkinnedMesh>()?.clone(),
                ))
            })
            .collect();

        let meshes = world.resource::<Assets<Mesh>>();
        let bindposes = world.resource::<Assets<SkinnedMeshInverseBindposes>>();
        let mut skins = Vec::new();
        for (handle, skinned) in found {
            let Some(mesh) = meshes.get(&handle) else {
                continue;
            };
            let Some(inverse) = bindposes.get(&skinned.inverse_bindposes) else {
                continue;
            };
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                continue;
            };
            let Some(VertexAttributeValues::Float32x4(weights)) =
                mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
            else {
                continue;
            };
            let Some(indices) = joint_indices(mesh) else {
                continue;
            };
            let influences: Vec<[(usize, f32); 4]> = indices
                .iter()
                .zip(weights)
                .map(|(joints, weight)| {
                    [
                        (joints[0], weight[0]),
                        (joints[1], weight[1]),
                        (joints[2], weight[2]),
                        (joints[3], weight[3]),
                    ]
                })
                .collect();
            let mut foot_vertices = Vec::new();
            for (index, foot) in feet.iter().enumerate() {
                let mine: Vec<usize> = influences
                    .iter()
                    .enumerate()
                    .filter(|(_, influence)| {
                        influence
                            .iter()
                            .filter(|(joint, _)| {
                                skinned
                                    .joints
                                    .get(*joint)
                                    .and_then(|entity| by_entity.get(entity))
                                    .is_some_and(|name| foot.bones.contains(name.as_str()))
                            })
                            .map(|(_, weight)| *weight)
                            .sum::<f32>()
                            > FOOT_WEIGHT
                    })
                    .map(|(vertex, _)| vertex)
                    .collect();
                if !mine.is_empty() {
                    foot_vertices.push((index, mine));
                }
            }
            skins.push(Skin {
                positions: positions.iter().map(|p| Vec3::from_array(*p)).collect(),
                influences,
                joints: skinned.joints.clone(),
                inverse_bind: inverse.iter().copied().collect(),
                foot_vertices,
            });
        }
        skins
    }

    /// The joint index attribute, whichever integer width the exporter
    /// chose.
    fn joint_indices(mesh: &Mesh) -> Option<Vec<[usize; 4]>> {
        let widen = |v: &[u16; 4]| [v[0] as usize, v[1] as usize, v[2] as usize, v[3] as usize];
        match mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)? {
            VertexAttributeValues::Uint16x4(values) => Some(values.iter().map(widen).collect()),
            VertexAttributeValues::Uint32x4(values) => Some(
                values
                    .iter()
                    .map(|v| [v[0] as usize, v[1] as usize, v[2] as usize, v[3] as usize])
                    .collect(),
            ),
            _ => None,
        }
    }

    /// The feet of a profile: one per side, named by its heel.
    ///
    /// Read out of the driven layout's own `contact_columns` — the joints a
    /// take's per-frame contact flags belong to — and reduced to the
    /// ancestor of each side, because a heel and its toe are one foot and
    /// two numbers for one plant would say the same thing twice. Each
    /// foot's skin is the heel's whole subtree in the contract, so a toe's
    /// vertices count as the foot's and a shin's do not.
    fn feet_of(contract: &Contract, motion: &MotionSkeleton) -> Vec<Foot> {
        let mut heels: Vec<&str> = Vec::new();
        for &column in &motion.contact_columns {
            let Some(name) = motion.joints.get(column) else {
                continue;
            };
            let Some(index) = contract.find(name) else {
                continue;
            };
            // A column whose bone sits under another column's bone is that
            // one's toe, not a foot of its own.
            if motion.contact_columns.iter().any(|&other| {
                other != column
                    && motion
                        .joints
                        .get(other)
                        .and_then(|name| contract.find(name))
                        .is_some_and(|ancestor| is_under(contract, index, ancestor))
            }) {
                continue;
            }
            if !heels.contains(&name.as_str()) {
                heels.push(name.as_str());
            }
        }
        heels
            .into_iter()
            .map(|heel| {
                let root = contract.find(heel).unwrap_or_default();
                Foot {
                    name: heel.to_owned(),
                    bones: contract
                        .bones
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index == root || is_under(contract, *index, root))
                        .map(|(_, bone)| bone.name.clone())
                        .collect(),
                }
            })
            .collect()
    }

    /// Whether `index` sits somewhere below `ancestor` in the contract.
    fn is_under(contract: &Contract, index: usize, ancestor: usize) -> bool {
        let mut current = contract.bones[index].parent;
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = contract.bones[parent].parent;
        }
        false
    }
}

/// Render the subject playing the reference clip as a contact sheet — or,
/// with no reference clip to play, the walk-around views at rest — and
/// return the render's summary.
fn render_sheet(stage: &Stage, with_reference: bool, out: &Path) -> Result<String, RigCheckError> {
    let shot = if with_reference {
        render_clip_sheet(stage, &SheetRequest::new(REFERENCE))
    } else {
        render_views(stage, &ViewsRequest::default())
    }
    .map_err(RigCheckError::Render)?;
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| RigCheckError::Write(format!("{}: {error}", parent.display())))?;
    }
    shot.save_png(out)
        .map_err(|error| RigCheckError::Write(format!("{}: {error}", out.display())))?;
    let mut summary = shot.summary();
    if !with_reference {
        let _ = writeln!(
            summary,
            "rendered at rest: no reference clip in the library to play"
        );
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_marks_every_line_and_tallies_by_weight() {
        let report = CheckReport {
            subject: PathBuf::from("out/export/hero.glb"),
            profile: String::from("humanoid v1"),
            reference: None,
            findings: vec![
                Finding::ok("animation root is named Armature"),
                Finding::note("extra leaf bone Holster"),
                Finding::warn("no reference clip 'walk' in the library"),
            ],
            sheet: None,
            sheet_summary: None,
        };
        assert!(!report.failed());
        let text = report.text();
        assert!(text.contains("\nok:   animation root"), "{text}");
        assert!(text.contains("\nnote: extra leaf"), "{text}");
        assert!(text.contains("\nWARN: no reference"), "{text}");
        assert!(text.contains("reference: none in the library"), "{text}");
        assert!(
            text.ends_with("1 finding(s) passed, 0 failed, 1 note(s), 1 warning(s)"),
            "{text}"
        );

        let mut failed = report;
        failed.findings.push(Finding::fail("Hips missing"));
        assert!(failed.failed());
        assert!(failed.text().contains("\nFAIL: Hips missing"));
    }

    #[test]
    fn a_subject_that_is_not_a_file_is_refused_before_anything_spawns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "t").expect("project");
        let error = run(&project, &dir.path().join("missing.glb"), None).expect_err("refused");
        assert!(matches!(error, RigCheckError::NotAFile(_)), "{error}");
    }
}
