//! Does the library say true things about itself? The claims that need an
//! engine: `forge audit`.
//!
//! [`forge_library::audit`] makes the four claims that need no engine —
//! integrity, footsteps, root tracks, and a byte-for-byte re-bake of every
//! clip — and runs first, because there is no point spinning up an animation
//! player for clips whose records do not parse. This module appends the two
//! that do:
//!
//! 5. **Every clip reproduces, posed.** The library claims each clip is
//!    reproducible from its own sidecar. The byte compare proves that for a
//!    file written by this baker; it cannot say whether a file written by an
//!    *earlier* baker, whose bytes differ in ways that are allowed to differ,
//!    still moves the same. So every clip is rebuilt from its raw take plus
//!    the recorded recipe, the shipped file and the rebuild are both bound to
//!    the fixture mannequin in a headless app, and every keyed frame is
//!    sampled: each contract bone must land within a millimetre in world
//!    space and within 1e-3 per quaternion component in its local rotation.
//!    That needs an animation player, which is why this lives here and not
//!    in `forge_library`.
//! 6. **Every body conforms.** Each shipped body is spawned and held to the
//!    profile's contract by [`crate::rig_findings`] — the same checks `forge
//!    rig check` prints — with the library's reference clip bound to it when
//!    there is one. A body that fails here holds its rest pose through every
//!    clip in the library, silently, which is the whole reason the check
//!    exists.
//!
//! `--fit` is for a record that has gone wrong: when a clip does not
//! reproduce as recorded, the in-place modes and a ladder of wrap blends are
//! searched and the candidate that *does* reproduce is named beside the
//! failure — so the fix is a recipe to write down, not a guess. The failure
//! stands either way: the claim under test is the recipe as recorded.
//!
//! Parsing goes through `forge_library` — the same types the studio, the MCP
//! server and the promote path use. Auditing a private copy of the parser
//! would prove the sidecars are reproducible by *something*, which is not the
//! claim.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::{
    animation::{AnimationClip, AnimationTargetId, graph::AnimationGraph},
    asset::LoadState,
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot},
};
use forge_library::{Catalog, Kind, Project, Report};
use forge_motion::{ClipChannels, Edit, InPlace, RigDef, Take};
use forge_rig::{Contract, RigProfile};

use crate::binding::{headless_app, install_animation_targets};
use crate::rig_findings::{diagnose, quat_gap};
use crate::stage::find_animation_root;

/// Below this, in millimetres, a rebuilt bone sits where the shipped one did.
pub const MATCH_MM: f32 = 1.0;
/// Below this, per quaternion component, a rebuilt rotation is the shipped
/// one. Exporter float noise sits near 1e-6; the smallest recipe knob moves a
/// joint by more than 1e-3.
pub const MATCH_QUAT: f32 = 1e-3;
/// Wrap blends `--fit` searches. `--loop` defaults to 0.4 s, but `wrap_blend`
/// caps at half the clip, so on a 16-frame stride cycle that would rewrite half
/// the motion — the real values are much smaller.
const BLENDS: [f32; 8] = [0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4];
/// Frames to wait for assets before declaring them stuck.
const MAX_LOAD_FRAMES: u32 = 2000;
/// The mannequin's name inside the staging root.
const MANNEQUIN: &str = "mannequin.glb";

/// What the whole audit found: the engine-free claims, then the two that
/// needed a player.
#[derive(Debug, Clone, Default)]
pub struct AuditReport {
    /// Claims 1–4, from [`forge_library::audit`].
    pub library: forge_library::audit::Audit,
    /// Claim 5: every clip reproduces, posed.
    pub poses: Report,
    /// Claim 6: every body conforms to the contract.
    pub bodies: Report,
    /// Clips whose rebuild posed the mannequin exactly as the shipped file.
    pub poses_ok: usize,
    /// How many clips there were.
    pub clips: usize,
    /// Bodies that passed every contract check.
    pub bodies_ok: usize,
    /// How many bodies there were.
    pub bodies_checked: usize,
    /// Whether `--fit` searched for the recipe behind a failure.
    pub fitted: bool,
}

impl AuditReport {
    /// Whether every claim held.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.library.ok() && self.poses.ok() && self.bodies.ok()
    }

    /// Everything as one report, for a caller that wants one exit code.
    #[must_use]
    pub fn combined(&self) -> Report {
        let mut report = self.library.combined();
        report.absorb(self.poses.clone());
        report.absorb(self.bodies.clone());
        report
    }

    /// The audit as text: the library's four sections, then the two that
    /// needed an engine, each under a heading, with its score.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.library.render();
        let section = |out: &mut String, title: &str, report: &Report, held: &str| {
            out.push_str(title);
            out.push('\n');
            for line in report.render().lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
            if report.ok() {
                out.push_str("  ");
                out.push_str(held);
                out.push('\n');
            }
            out.push('\n');
        };
        section(
            &mut out,
            "clip poses",
            &self.poses,
            &format!(
                "{}/{} clips pose the mannequin exactly as their shipped file{}",
                self.poses_ok,
                self.clips,
                if self.fitted {
                    " (recipes fitted where recorded ones failed)"
                } else {
                    ""
                }
            ),
        );
        section(
            &mut out,
            "bodies",
            &self.bodies,
            &format!(
                "{}/{} bodies conform to the contract",
                self.bodies_ok, self.bodies_checked
            ),
        );
        out
    }
}

/// Run every claim over the library: the engine-free four first, then the
/// pose compare and the body checks.
///
/// Never fails outright: a library whose profile does not load, or whose rig
/// cannot be read, is a library whose claims cannot be checked, and that is
/// reported as failures under each claim rather than as an error nobody
/// reads.
#[must_use]
pub fn run(project: &Project, fit: bool) -> AuditReport {
    let mut audit = AuditReport {
        library: forge_library::audit::run(project),
        fitted: fit,
        ..AuditReport::default()
    };
    let catalog = Catalog::scan(project);
    let clips: Vec<&forge_library::AssetRecord> = catalog
        .records()
        .iter()
        .filter(|record| record.kind == Kind::Clip)
        .collect();
    let bodies: Vec<&forge_library::AssetRecord> = catalog
        .records()
        .iter()
        .filter(|record| record.kind == Kind::Body)
        .collect();
    audit.clips = clips.len();
    audit.poses.checked = clips.len();
    audit.bodies_checked = bodies.len();
    audit.bodies.checked = bodies.len();

    let profile = match project.profile() {
        Ok(profile) => profile,
        Err(error) => {
            let detail = format!("the rig profile does not load: {error}");
            for record in &clips {
                audit.poses.fail(&record.name, &detail);
            }
            for record in &bodies {
                audit.bodies.fail(&record.name, &detail);
            }
            return audit;
        }
    };

    audit_poses(&mut audit, project, &profile, &clips, fit);
    audit_bodies(&mut audit, project, &profile.contract, &catalog, &bodies);
    audit
}

// ------------------------------------------------------------------ poses ---

/// One clip staged for the pose compare: the shipped file and its rebuild,
/// both inside the staging root, plus the frame times to sample.
struct Staged {
    name: String,
    /// Times, seconds, at which the shipped clip keys its rotations.
    times: Vec<f32>,
    /// The candidates: the recorded recipe first, then — under `--fit` — the
    /// searched alternatives. Each is the file name inside the staging root
    /// and how it was built.
    candidates: Vec<(Candidate, String)>,
}

/// The two things that vary between candidate rebuilds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    in_place: InPlace,
    loop_blend_s: f32,
    /// True for the recipe as recorded — the claim under test.
    recorded: bool,
}

/// Distinguishes concurrent audits in one process — the tests run several.
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A scratch asset root that removes itself.
struct Staging {
    dir: PathBuf,
}

impl Staging {
    fn new() -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "forge_audit_{}_{}",
            std::process::id(),
            STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Claim 5 over every clip.
fn audit_poses(
    audit: &mut AuditReport,
    project: &Project,
    profile: &RigProfile,
    clips: &[&forge_library::AssetRecord],
    fit: bool,
) {
    if clips.is_empty() {
        return;
    }
    let rig = match std::fs::read(profile.glb_path())
        .map_err(|e| e.to_string())
        .and_then(|bytes| RigDef::from_glb(&bytes).map_err(|e| e.to_string()))
    {
        Ok(rig) => rig,
        Err(error) => {
            for record in clips {
                audit.poses.fail(
                    &record.name,
                    format!(
                        "the profile's rig.glb does not load, so nothing can be re-baked: {error}"
                    ),
                );
            }
            return;
        }
    };
    let staging = match Staging::new() {
        Ok(staging) => staging,
        Err(error) => {
            for record in clips {
                audit
                    .poses
                    .fail(&record.name, format!("cannot stage the compare: {error}"));
            }
            return;
        }
    };
    if let Err(error) = forge_rig::fixture::write_mannequin(profile, &staging.dir.join(MANNEQUIN)) {
        for record in clips {
            audit.poses.fail(
                &record.name,
                format!("the fixture mannequin could not be written: {error}"),
            );
        }
        return;
    }

    // Rebuild every clip first — milliseconds each, and a record that cannot
    // be replayed is a failure with no engine involved.
    let mut staged = Vec::with_capacity(clips.len());
    for record in clips {
        match stage_clip(project, record, &rig, &staging.dir, fit) {
            Ok(clip) => staged.push(clip),
            Err(detail) => audit.poses.fail(&record.name, detail),
        }
    }
    if staged.is_empty() {
        return;
    }

    let mut app = headless_app(&staging.dir);
    let server = app.world().resource::<AssetServer>().clone();
    let scene = server.load::<WorldAsset>(format!("{MANNEQUIN}#Scene0"));
    let spawned = app.world_mut().spawn(WorldAssetRoot(scene)).id();
    // Every file in one pass: the shipped clip and each candidate rebuild.
    let mut handles: Vec<(Handle<AnimationClip>, Vec<Handle<AnimationClip>>)> = staged
        .iter()
        .map(|clip| {
            (
                server.load::<AnimationClip>(format!("{}#Animation0", shipped_file(&clip.name))),
                clip.candidates
                    .iter()
                    .map(|(_, file)| server.load::<AnimationClip>(format!("{file}#Animation0")))
                    .collect(),
            )
        })
        .collect();
    if let Err(detail) = wait_for(&mut app, spawned, &handles) {
        for clip in &staged {
            audit.poses.fail(&clip.name, &detail);
        }
        return;
    }
    let Some(anim_root) = find_animation_root(app.world_mut(), spawned) else {
        for clip in &staged {
            audit
                .poses
                .fail(&clip.name, "the mannequin spawned no skeleton");
        }
        return;
    };
    if app.world().get::<AnimationTargetId>(anim_root).is_none() {
        install_animation_targets(app.world_mut(), anim_root);
    }

    // One graph holding every clip: a node added to a live graph plays fine,
    // and one player means one warm-up.
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let nodes: Vec<(AnimationNodeIndex, Vec<AnimationNodeIndex>)> = handles
        .drain(..)
        .map(|(shipped, candidates)| {
            (
                graph.add_clip(shipped, 1.0, root),
                candidates
                    .into_iter()
                    .map(|handle| graph.add_clip(handle, 1.0, root))
                    .collect(),
            )
        })
        .collect();
    let graph_handle = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(graph);
    app.world_mut().entity_mut(anim_root).insert((
        AnimationPlayer::default(),
        AnimationGraphHandle(graph_handle),
    ));
    // The graph must round-trip through an asset event before any pose
    // applies.
    for _ in 0..8 {
        app.update();
    }

    let bones: Vec<String> = profile
        .contract
        .bones
        .iter()
        .map(|bone| bone.name.clone())
        .collect();
    for (clip, (shipped, candidates)) in staged.iter().zip(&nodes) {
        let mut verdicts: Vec<(Candidate, Divergence)> = Vec::with_capacity(candidates.len());
        for ((candidate, _), node) in clip.candidates.iter().zip(candidates) {
            let divergence = compare(&mut app, anim_root, *shipped, *node, &clip.times, &bones);
            verdicts.push((*candidate, divergence));
        }
        let Some((_, recorded)) = verdicts.iter().find(|(c, _)| c.recorded) else {
            audit
                .poses
                .fail(&clip.name, "the recorded recipe was never rebuilt");
            continue;
        };
        if recorded.within_tolerance() {
            audit.poses_ok += 1;
            continue;
        }
        let mut detail = format!(
            "the rebuild from its own take and recipe poses the mannequin differently: {recorded}"
        );
        if fit {
            let best = verdicts
                .iter()
                .filter(|(c, _)| !c.recorded)
                .min_by(|(_, a), (_, b)| a.worst().total_cmp(&b.worst()));
            match best {
                Some((candidate, divergence)) if divergence.within_tolerance() => {
                    let _ = write!(
                        detail,
                        " — reproduces with in_place {:?}, loop_blend_s {}: the record is wrong, \
                         not the file; write that recipe down",
                        candidate.in_place, candidate.loop_blend_s
                    );
                }
                Some((candidate, divergence)) => {
                    let _ = write!(
                        detail,
                        " — no searched candidate reproduces it either; nearest is in_place {:?}, \
                         loop_blend_s {} at {divergence}",
                        candidate.in_place, candidate.loop_blend_s
                    );
                }
                None => {}
            }
        }
        audit.poses.fail(&clip.name, detail);
    }
}

/// The shipped copy's name inside the staging root.
fn shipped_file(name: &str) -> String {
    format!("{name}.shipped.glb")
}

/// Rebuild one clip from its take and recipe into the staging root, beside
/// a copy of the shipped file, and read the shipped file's key times.
fn stage_clip(
    project: &Project,
    record: &forge_library::AssetRecord,
    rig: &RigDef,
    dir: &Path,
    fit: bool,
) -> Result<Staged, String> {
    let sidecar = record
        .sidecar
        .as_ref()
        .ok_or_else(|| String::from("no readable sidecar, so there is no recipe to replay"))?;
    let take_path = sidecar
        .source
        .path
        .as_deref()
        .map(|p| project.root.join(p))
        .ok_or_else(|| String::from("no source take recorded in its sidecar"))?;
    let take = Take::read(&take_path)
        .map_err(|e| format!("no readable take at {}: {e}", take_path.display()))?;
    let recipe = sidecar
        .recipe
        .as_ref()
        .ok_or_else(|| String::from("no recipe in its sidecar"))?;
    // The take's own clock, exactly as promote used it.
    let edit = recipe
        .to_edit(take.fps)
        .map_err(|e| format!("recipe will not apply: {e}"))?;
    let clip_name = recipe.clip.clone().unwrap_or_else(|| record.name.clone());

    let shipped = std::fs::read(&record.path)
        .map_err(|e| format!("cannot read {}: {e}", record.path.display()))?;
    let channels = ClipChannels::from_glb(&shipped)
        .map_err(|e| format!("the shipped file holds no readable animation: {e}"))?;
    // Every keyed frame of the shipped file: the longest rotation track's
    // times. Fixed wall-clock times would spend most of their samples past
    // the end of a 15-frame stride cycle, comparing clamped tails instead of
    // motion.
    let times = channels
        .rotations
        .iter()
        .flatten()
        .map(|track| &track.times)
        .max_by_key(|times| times.len())
        .cloned()
        .filter(|times| !times.is_empty())
        .ok_or_else(|| String::from("the shipped file keys no rotation at all"))?;

    let write = |file: &str, bytes: &[u8]| {
        std::fs::write(dir.join(file), bytes).map_err(|e| format!("cannot stage {file}: {e}"))
    };
    write(&shipped_file(&record.name), &shipped)?;

    let mut candidates = Vec::new();
    let mut rebuild = |in_place: InPlace, loop_blend_s: f32, recorded: bool| {
        let edit = Edit {
            in_place,
            loop_blend_s,
            ..edit.clone()
        };
        let bytes = forge_motion::bake(&take, &edit, rig, &clip_name)
            .map_err(|e| format!("re-bake failed: {e}"))?;
        let file = format!("{}.rebuilt{}.glb", record.name, candidates.len());
        write(&file, &bytes)?;
        candidates.push((
            Candidate {
                in_place,
                loop_blend_s,
                recorded,
            },
            file,
        ));
        Ok::<(), String>(())
    };
    rebuild(edit.in_place, edit.loop_blend_s, true)?;
    if fit {
        // By default the recipe is checked as recorded — that is the claim
        // under test. `--fit` searches instead, which is how a lost value is
        // recovered: the mode family the record names, and every blend.
        let modes: &[InPlace] = if edit.in_place == InPlace::Off {
            &[InPlace::Off]
        } else {
            &[InPlace::Strip, InPlace::Detrend]
        };
        let blends: &[f32] = if edit.loop_blend_s > 0.0 {
            &BLENDS
        } else {
            &[0.0]
        };
        for &mode in modes {
            for &blend in blends {
                if mode == edit.in_place && (blend - edit.loop_blend_s).abs() < f32::EPSILON {
                    continue;
                }
                rebuild(mode, blend, false)?;
            }
        }
    }
    Ok(Staged {
        name: record.name.clone(),
        times,
        candidates,
    })
}

/// Pump until the mannequin stands and every clip has loaded.
fn wait_for(
    app: &mut App,
    spawned: Entity,
    handles: &[(Handle<AnimationClip>, Vec<Handle<AnimationClip>>)],
) -> Result<(), String> {
    for _ in 0..MAX_LOAD_FRAMES {
        app.update();
        let server = app.world().resource::<AssetServer>();
        let mut all_loaded = true;
        for handle in handles
            .iter()
            .flat_map(|(shipped, rest)| std::iter::once(shipped).chain(rest))
        {
            match server.get_load_state(handle) {
                Some(LoadState::Loaded) => {}
                Some(LoadState::Failed(error)) => {
                    return Err(format!("a clip failed to load: {error}"));
                }
                _ => all_loaded = false,
            }
        }
        let standing = app
            .world()
            .get::<Children>(spawned)
            .is_some_and(|c| !c.is_empty());
        if all_loaded && standing {
            return Ok(());
        }
    }
    Err(format!(
        "the mannequin and the clips did not load within {MAX_LOAD_FRAMES} frames"
    ))
}

/// One bone's pose: where it is in the world and how it is turned locally.
struct Sample {
    name: String,
    position: Vec3,
    rotation: Quat,
}

/// Every contract bone's pose while `node` plays at `time`.
fn sample(
    app: &mut App,
    root: Entity,
    node: AnimationNodeIndex,
    time: f32,
    bones: &[String],
) -> Vec<Sample> {
    if let Some(mut player) = app.world_mut().get_mut::<AnimationPlayer>(root) {
        // `play` ADDS an active animation rather than replacing what is there.
        // Without this the previously sampled clip stays active and the two
        // blend — which reads as a near-match and quietly passes.
        player.stop_all();
        let active = player.play(node);
        active.set_seek_time(time);
        active.pause();
    }
    app.update();
    let mut out = Vec::with_capacity(bones.len());
    let mut query = app
        .world_mut()
        .query::<(&Name, &Transform, &GlobalTransform)>();
    for (name, local, global) in query.iter(app.world()) {
        if bones.iter().any(|bone| bone == name.as_str()) {
            out.push(Sample {
                name: name.as_str().to_owned(),
                position: global.translation(),
                rotation: local.rotation,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The worst a rebuild strayed from the shipped file, and where.
#[derive(Debug, Clone, Default)]
struct Divergence {
    /// Largest world-space gap, millimetres.
    position_mm: f32,
    /// Largest per-component local rotation gap.
    rotation: f32,
    /// The bone and time the worst of either happened at.
    at: Option<(String, f32)>,
}

impl Divergence {
    fn within_tolerance(&self) -> bool {
        self.position_mm < MATCH_MM && self.rotation < MATCH_QUAT
    }

    /// One number to rank candidates by: each axis in units of its tolerance.
    fn worst(&self) -> f32 {
        (self.position_mm / MATCH_MM).max(self.rotation / MATCH_QUAT)
    }
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1} mm, {:.4} quat", self.position_mm, self.rotation)?;
        if let Some((bone, time)) = &self.at {
            write!(f, " (worst at {bone}, {time:.3}s)")?;
        }
        Ok(())
    }
}

/// Worst pose divergence between two clips across `times`, per bone.
fn compare(
    app: &mut App,
    root: Entity,
    shipped: AnimationNodeIndex,
    rebuilt: AnimationNodeIndex,
    times: &[f32],
    bones: &[String],
) -> Divergence {
    let mut worst = Divergence::default();
    for &time in times {
        let theirs = sample(app, root, shipped, time, bones);
        let mine = sample(app, root, rebuilt, time, bones);
        for (a, b) in theirs.iter().zip(&mine) {
            let position_mm = (a.position - b.position).length() * 1000.0;
            let rotation = quat_gap(a.rotation, b.rotation);
            let score = (position_mm / MATCH_MM).max(rotation / MATCH_QUAT);
            if score > worst.worst() {
                worst.at = Some((a.name.clone(), time));
            }
            worst.position_mm = worst.position_mm.max(position_mm);
            worst.rotation = worst.rotation.max(rotation);
        }
    }
    worst
}

// ----------------------------------------------------------------- bodies ---

/// Claim 6 over every body: spawn it, bind the reference clip when the
/// library has one, and hold it to the contract.
fn audit_bodies(
    audit: &mut AuditReport,
    project: &Project,
    contract: &Contract,
    catalog: &Catalog,
    bodies: &[&forge_library::AssetRecord],
) {
    if bodies.is_empty() {
        return;
    }
    let reference = catalog
        .resolve(&contract.reference_clip, Some(Kind::Clip))
        .map(|record| record.rel_path.clone());
    if reference.is_none() {
        audit.bodies.warn(
            &contract.reference_clip,
            format!(
                "no reference clip '{}' in the library; binding not checked on any body",
                contract.reference_clip
            ),
        );
    }
    for record in bodies {
        match check_body(
            &project.assets,
            &record.rel_path,
            reference.as_deref(),
            contract,
        ) {
            Ok(findings) => {
                let mut failed = false;
                for finding in findings {
                    match finding.severity {
                        crate::rig_findings::Severity::Fail => {
                            failed = true;
                            audit.bodies.fail(&record.name, finding.text);
                        }
                        crate::rig_findings::Severity::Note => {
                            audit.bodies.note(&record.name, finding.text);
                        }
                        crate::rig_findings::Severity::Warn => {
                            audit.bodies.warn(&record.name, finding.text);
                        }
                        crate::rig_findings::Severity::Ok => {}
                    }
                }
                if !failed {
                    audit.bodies_ok += 1;
                }
            }
            Err(detail) => audit.bodies.fail(&record.name, detail),
        }
    }
}

/// Spawn one body with no renderer and run every contract check on it.
fn check_body(
    asset_root: &Path,
    body: &str,
    reference: Option<&str>,
    contract: &Contract,
) -> Result<Vec<crate::rig_findings::Finding>, String> {
    let mut app = headless_app(asset_root);
    let (scene, clip) = {
        let server = app.world().resource::<AssetServer>().clone();
        (
            server.load::<WorldAsset>(format!("{body}#Scene0")),
            reference.map(|file| server.load::<AnimationClip>(format!("{file}#Animation0"))),
        )
    };
    let spawned = app.world_mut().spawn(WorldAssetRoot(scene)).id();
    for _ in 0..MAX_LOAD_FRAMES {
        app.update();
        let clip_state = clip
            .as_ref()
            .and_then(|c| app.world().resource::<AssetServer>().get_load_state(c));
        if let Some(LoadState::Failed(error)) = &clip_state {
            return Err(format!("the reference clip failed to load: {error}"));
        }
        let standing = app
            .world()
            .get::<Children>(spawned)
            .is_some_and(|c| !c.is_empty());
        if standing && (clip.is_none() || matches!(clip_state, Some(LoadState::Loaded))) {
            break;
        }
    }
    let anim_root = find_animation_root(app.world_mut(), spawned)
        .ok_or_else(|| String::from("spawned no named node hierarchy — is it a rigged scene?"))?;
    if app.world().get::<AnimationTargetId>(anim_root).is_none() {
        install_animation_targets(app.world_mut(), anim_root);
    }
    // No player is ever attached: the rest pose is read off live transforms.
    let world = app.world();
    let reference = clip
        .as_ref()
        .and_then(|handle| world.resource::<Assets<AnimationClip>>().get(handle));
    Ok(diagnose(world, anim_root, contract, reference))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_divergence_is_judged_on_both_axes_and_ranked_in_tolerance_units() {
        let fine = Divergence {
            position_mm: 0.4,
            rotation: 5e-4,
            at: None,
        };
        assert!(fine.within_tolerance());
        let moved = Divergence {
            position_mm: 1.5,
            rotation: 0.0,
            at: Some((String::from("Hips"), 0.25)),
        };
        assert!(!moved.within_tolerance());
        let turned = Divergence {
            position_mm: 0.0,
            rotation: 2e-3,
            at: None,
        };
        assert!(!turned.within_tolerance());
        assert!(turned.worst() > moved.worst());
        assert!(moved.to_string().contains("worst at Hips, 0.250s"));
    }

    #[test]
    fn an_empty_library_holds_every_engine_claim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "empty").expect("project");
        project
            .install_profile(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid"))
            .expect("profile");
        let audit = run(&project, false);
        assert!(audit.poses.ok() && audit.bodies.ok(), "{}", audit.render());
        assert_eq!(audit.clips, 0);
        assert_eq!(audit.bodies_checked, 0);
        assert!(audit.render().contains("0/0 clips pose the mannequin"));
    }
}
