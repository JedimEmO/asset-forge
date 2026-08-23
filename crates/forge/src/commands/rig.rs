//! `forge rig export-contract`, `forge rig fixture` and `forge rig check`:
//! the profile's contract derived from its artifact, the mannequin built
//! from the contract, and one mesh held to it.

use std::path::{Path, PathBuf};

use forge_rig::{
    CONTRACT_FILE, Contract, MOTION_SKELETON_FILE, MotionSkeleton, RigProfile, Stature, export,
};

use crate::cli::{Cli, ExportContractArgs, FixtureArgs, RigCheckArgs, RigCommand};
use crate::outcome::{Failure, Outcome};

/// Dispatch.
pub(crate) fn run(cli: &Cli, command: &RigCommand) -> Outcome {
    match command {
        RigCommand::ExportContract(args) => export_contract(args),
        RigCommand::Fixture(args) => fixture(cli, args),
        RigCommand::Check(args) => check(cli, args),
    }
}

/// Hold one rigged glb to the project's contract; with `--out`, also render
/// it playing the reference clip.
///
/// The report prints whole even when it failed — a human deciding what to
/// send back wants the findings, not the exit code — and a mesh the check
/// could not run on at all (not a file, no skeleton spawned) is a refusal
/// or a failure with the reason, never a pass.
fn check(cli: &Cli, args: &RigCheckArgs) -> Outcome {
    let project = crate::project(cli)?;
    let report = forge_studio::rig_check::run(&project, &args.glb, args.out.as_deref()).map_err(
        |error| match error {
            forge_studio::rig_check::RigCheckError::NotAFile(_) => {
                Failure::refused(error.to_string())
            }
            _ => Failure::failed(error.to_string()),
        },
    )?;
    println!("{report}");
    if report.failed() {
        Err(Failure::failed(format!(
            "rig check: {} finding(s) failed",
            report.count(forge_studio::rig_findings::Severity::Fail)
        )))
    } else {
        Ok(())
    }
}

/// The humanoid profile's scalars: 1.80 m reference stature with a 1.4–2.2 m
/// band, feet within 5 cm of the ground, a 1e-3 rest-rotation tolerance, and
/// the walk as the binding reference. What a directory with no contract yet
/// starts from.
fn humanoid_defaults(dir: &Path) -> export::Options {
    export::Options {
        name: String::from("humanoid"),
        version: 1,
        front: String::from("+Z"),
        stature_m: Stature {
            reference: 1.8,
            min: 1.4,
            max: 2.2,
        },
        foot_tolerance_m: 0.05,
        rest_rotation_tolerance: 1e-3,
        reference_clip: String::from("walk"),
        glb: String::from("rig.glb"),
        blend: dir
            .join("rig.blend")
            .is_file()
            .then(|| String::from("rig.blend")),
    }
}

/// Write `contract.json` from the profile's `rig.glb`.
///
/// The scalars the artifact cannot decide come from the existing contract
/// when there is one — a re-export keeps every field the rig does not
/// decide — else from the humanoid defaults; a flag overrides either. An
/// unchanged rig reproduces the file byte for byte; a changed one shows up
/// as a diff in the bones.
fn export_contract(args: &ExportContractArgs) -> Outcome {
    let dir = &args.dir;
    if !dir.is_dir() {
        return Err(Failure::refused(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let motion_path = dir.join(MOTION_SKELETON_FILE);
    if !motion_path.is_file() {
        return Err(Failure::refused(format!(
            "{} has no {MOTION_SKELETON_FILE} — a profile directory holds rig.glb and the \
             driven layout beside it",
            dir.display()
        )));
    }
    let motion = MotionSkeleton::load(&motion_path)?;
    let out = dir.join(CONTRACT_FILE);
    let mut options = if out.is_file() {
        export::Options::from_contract(&Contract::load(&out)?)
    } else {
        humanoid_defaults(dir)
    };
    if let Some(name) = &args.name {
        options.name.clone_from(name);
    }
    if let Some(version) = args.version {
        options.version = version;
    }
    if let Some(front) = &args.front {
        options.front.clone_from(front);
    }
    if let Some(reference) = args.stature {
        options.stature_m.reference = reference;
    }
    if let Some(min) = args.stature_min {
        options.stature_m.min = min;
    }
    if let Some(max) = args.stature_max {
        options.stature_m.max = max;
    }
    if let Some(tolerance) = args.foot_tolerance {
        options.foot_tolerance_m = tolerance;
    }
    if let Some(tolerance) = args.rest_rotation_tolerance {
        options.rest_rotation_tolerance = tolerance;
    }
    if let Some(clip) = &args.reference_clip {
        options.reference_clip.clone_from(clip);
    }
    if let Some(glb) = &args.glb {
        options.glb.clone_from(glb);
    }
    if let Some(blend) = &args.blend {
        options.blend = Some(blend.clone());
    }
    if !dir.join(&options.glb).is_file() {
        return Err(Failure::refused(format!(
            "{} has no {} — pass --glb to name the rig artifact",
            dir.display(),
            options.glb
        )));
    }
    let contract = export::write(dir, &motion, &options, &out)?;
    println!(
        "wrote {}: {} bones ({} driven by {}), root {}, stature {:.2} m ({:.2}–{:.2}), glb {}{}",
        out.display(),
        contract.bones.len(),
        contract.driven().count(),
        contract.driven_layout,
        contract.root,
        contract.stature_m.reference,
        contract.stature_m.min,
        contract.stature_m.max,
        &contract.sources.glb_sha256[..12.min(contract.sources.glb_sha256.len())],
        contract
            .sources
            .blend_sha256
            .as_deref()
            .map_or_else(String::new, |sha| format!(
                ", blend {}",
                &sha[..12.min(sha.len())]
            )),
    );
    Ok(())
}

/// Write the fixture mannequin for the project's profile (or `--rig-dir`).
fn fixture(cli: &Cli, args: &FixtureArgs) -> Outcome {
    let dir: PathBuf = match &args.rig_dir {
        Some(dir) => dir.clone(),
        None => crate::project(cli)?.rig_dir(),
    };
    let profile = RigProfile::load(&dir)?;
    forge_rig::fixture::write_mannequin(&profile, &args.out)?;
    let bytes = std::fs::metadata(&args.out).map_or(0, |m| m.len());
    println!(
        "wrote {}: the {} mannequin, {} bones ({} driven), {bytes} bytes, byte-deterministic",
        args.out.display(),
        profile.contract.name,
        profile.contract.bones.len(),
        profile.contract.driven().count(),
    );
    Ok(())
}
