//! `forge studio`: the viewer window, opened on something.
//!
//! The window can open on an empty stage — a library of sounds alone is a
//! library — but this command does not let it: the model it hands over is
//! resolved here by the same rule `forge sheet` poses by. `--model` names
//! it; unstated, it is the project's `stage_body`, then the first body with
//! a warning, then the fixture mannequin written under `out/` so that a
//! fresh project still opens on a figure a clip can be watched on. A window
//! that opens on nothing costs a restart to find out why.
//!
//! The window's asset root is the library, and Bevy loads nothing from
//! outside a root; the project's `out/` is its one other source, as
//! `out://<path>`, which is how the mannequin and an export get on stage.

use std::path::PathBuf;

use forge_library::{Catalog, Project};
use forge_studio::studio::{OUT_SOURCE, StudioConfig, run_studio};

use crate::cli::StudioArgs;
use crate::commands::look::{Chosen, default_body, named_mesh, under_assets};
use crate::outcome::{Failure, Outcome};

/// Open the window and run it until it is closed; with `--screenshot` or
/// `--selftest`, until it has done that.
pub(crate) fn run(project: &Project, args: &StudioArgs) -> Outcome {
    let model = match &args.model {
        Some(wanted) => Some(named_model(project, wanted)?),
        None => Some(default_model(project)?),
    };
    // Relative to the shell, like every other path on a command line — and
    // made absolute here because the window's asset root is elsewhere.
    let absolute = |path: &PathBuf| std::path::absolute(path).unwrap_or_else(|_| path.clone());
    for (flag, path) in [("--take", &args.take), ("--recipe", &args.recipe)] {
        if let Some(path) = path
            && !path.is_file()
        {
            return Err(Failure::refused(format!(
                "{flag} {} is not a file",
                path.display()
            )));
        }
    }
    let config = StudioConfig {
        project: project.clone(),
        model,
        audio: args.audio,
        take: args.take.as_ref().map(absolute),
        recipe: args.recipe.as_ref().map(absolute),
        screenshot: args.screenshot.as_ref().map(absolute),
        selftest: args.selftest,
    };
    let code = run_studio(config);
    if code == std::process::ExitCode::SUCCESS {
        Ok(())
    } else {
        Err(Failure::failed("studio: the window did not run cleanly"))
    }
}

/// `--model` as the browser would resolve it: a body or model by name, file
/// name or asset-relative path; else a file under the asset root by
/// relative path; else a glb under the project's `out/`, which the window
/// reads through its `out://` source. Anywhere else is refused: Bevy loads
/// nothing from outside a root, and a window that opened empty over that
/// would cost a restart to find out why.
fn named_model(project: &Project, wanted: &str) -> Result<String, Failure> {
    let catalog = Catalog::scan(project);
    if let Some(rel) = under_assets(project, wanted) {
        return Ok(rel);
    }
    stage_path(project, named_mesh(&catalog, wanted)?)
}

/// What the window opens on when nobody said: `stage_body`, the first body,
/// the mannequin — the rule `forge sheet` poses by.
fn default_model(project: &Project) -> Result<String, Failure> {
    let catalog = Catalog::scan(project);
    stage_path(project, default_body(project, &catalog, "opening on")?)
}

/// The string the window loads a chosen mesh by.
fn stage_path(project: &Project, chosen: Chosen) -> Result<String, Failure> {
    match chosen {
        Chosen::InLibrary { rel_path, .. } => Ok(rel_path),
        Chosen::Elsewhere(path) => {
            let out = std::path::absolute(&project.out).unwrap_or_else(|_| project.out.clone());
            match path.strip_prefix(&out) {
                Ok(under_out) => Ok(format!(
                    "{OUT_SOURCE}://{}",
                    under_out.to_string_lossy().replace('\\', "/")
                )),
                Err(_) => Err(Failure::refused(format!(
                    "{} is outside the library and outside {} — the window loads from those two roots \
                     only; copy it under out/ to look at it",
                    path.display(),
                    out.display()
                ))),
            }
        }
    }
}
