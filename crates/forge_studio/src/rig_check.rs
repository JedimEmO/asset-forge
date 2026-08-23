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
use crate::rig_findings::{Finding, Severity, diagnose};
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
