//! `forge audio inspect` and `forge audio list`: measure a sound, or every
//! sound under a directory, and gate on what was found.
//!
//! Silence and clipping are defects, not observations: a build that ships
//! either has a real problem, so the exit code is 1 — the command gates in
//! CI rather than only informing. Everything else the metrics warn about is
//! worth a look, not a red build. A directory with nothing in it is not a
//! defect either: a library that ships no sound is not a broken one.

use std::path::PathBuf;

use forge_audio::{AudioError, PlotLayout};

use crate::cli::{AudioCommand, AudioInspectArgs, AudioListArgs, Cli};
use crate::outcome::{Failure, Outcome};

/// Dispatch.
pub(crate) fn run(cli: &Cli, command: &AudioCommand) -> Outcome {
    match command {
        AudioCommand::Inspect(args) => inspect(args),
        AudioCommand::List(args) => list(cli, args),
    }
}

/// Measure one file; draw it when asked.
fn inspect(args: &AudioInspectArgs) -> Outcome {
    if !args.file.is_file() {
        return Err(Failure::refused(format!(
            "no file at {}",
            args.file.display()
        )));
    }
    let layout = PlotLayout {
        width: args.width.unwrap_or(PlotLayout::default().width),
        ..PlotLayout::default()
    };
    let report = forge_audio::cli::inspect_with(&args.file, args.out.as_deref(), &layout)
        .map_err(|err| Failure::failed(err.to_string()))?;
    println!("{}", report.summary);
    if report.is_defective() {
        return Err(Failure::failed(format!(
            "{} is defective: {}",
            args.file.display(),
            report.warnings().join("; ")
        )));
    }
    Ok(())
}

/// Measure every sound under a directory — the project's `assets/audio`
/// when none is named.
fn list(cli: &Cli, args: &AudioListArgs) -> Outcome {
    let dir: PathBuf = match &args.dir {
        Some(dir) => dir.clone(),
        None => crate::project(cli)?.assets.join("audio"),
    };
    let reports = match forge_audio::list(&dir) {
        Ok(reports) => reports,
        Err(AudioError::NoAudio(dir)) => {
            println!("no audio under {} — nothing to measure", dir.display());
            return Ok(());
        }
        Err(other) => return Err(Failure::failed(other.to_string())),
    };
    println!("{}", forge_audio::cli::list_table(&reports));
    let defective: Vec<String> = reports
        .iter()
        .filter(|(_, report)| report.is_defective())
        .map(|(rel, _)| rel.display().to_string())
        .collect();
    if defective.is_empty() {
        Ok(())
    } else {
        Err(Failure::failed(format!(
            "{} defective: {}",
            defective.len(),
            defective.join(", ")
        )))
    }
}
