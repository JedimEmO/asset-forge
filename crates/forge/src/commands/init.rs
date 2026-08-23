//! `forge init`: a project where there was a directory.
//!
//! Writes `forge.toml` and the convention directories, installs the rig
//! profile, writes the reference ledger's header and an empty manifest — so
//! the very next `forge manifest --check` and `forge verify` pass on a
//! library that holds nothing, which is the honest starting state rather
//! than a broken one.

use std::path::Path;

use forge_library::project::SOURCES_LEDGER;
use forge_library::{Project, manifest};

use crate::cli::InitArgs;
use crate::outcome::{Failure, Outcome};
use crate::toolkit;

/// The ledger a new project starts with: the header row and the rule. The
/// same text the toolkit's own `assets-src/SOURCES.md` opens with.
pub(crate) const LEDGER_HEADER: &str = "# Reference sources\n\
\n\
Every reference image under `refs/` has a row here — where it came from, on \
what terms, and what was made from it. A reference PNG claims integrity \
(its sha256) and this row, never regeneration: the row is where its origin \
and its licence live, and a PNG without one is a file nobody can account \
for, which is why `forge verify` fails on it. Add the row when you add the \
image; the ledger is the answer to \"can we ship this?\" and has to be \
answerable from this file alone.\n\
\n\
| File | Origin | For | Date |\n\
|---|---|---|---|\n";

/// Make a project at `root` (the `--project` directory, else the working
/// directory).
pub(crate) fn run(root: Option<&Path>, args: &InitArgs) -> Outcome {
    let root = match root {
        Some(dir) => dir.to_path_buf(),
        None => crate::cwd()?,
    };
    let name = match &args.name {
        Some(name) => name.clone(),
        None => root
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .ok_or_else(|| {
                Failure::refused(format!(
                    "{} has no name to take — pass --name",
                    root.display()
                ))
            })?,
    };
    let project = Project::init(&root, &name)?;
    println!("initialised {} at {}", project.name, project.root.display());
    println!(
        "  forge.toml, assets/{{bodies,models,clips,audio/{{sfx,music,voice}}}}, \
         assets-src/{{takes,refs,blender,rigs}}, out/"
    );

    let ledger = project.sources_ledger();
    if !ledger.exists() {
        std::fs::write(&ledger, LEDGER_HEADER)
            .map_err(|e| Failure::failed(format!("{}: {e}", ledger.display())))?;
        println!("  {SOURCES_LEDGER}: the reference ledger, header only");
    }

    let profile = match &args.rig_dir {
        Some(dir) => Some(dir.clone()),
        None => toolkit::profile_dir(&project.rig_name),
    };
    match profile {
        Some(source) => {
            project.install_profile(&source)?;
            println!(
                "  rig profile {} installed from {}",
                project.rig_name,
                source.display()
            );
            let written = manifest::write(&project)?;
            println!(
                "  {}: empty, on rig {} ({} bones)",
                project
                    .rel_to_root(&project.manifest_path())
                    .unwrap_or_else(|| project.manifest_path().display().to_string()),
                written.rig.profile,
                written.rig.bone_count
            );
        }
        None => {
            // Exit non-zero: without the profile the project is half-made —
            // the very next `forge verify` and `forge manifest` both exit 1
            // on it — and an `init` that said ok anyway buried the one
            // message that names the fix.
            return Err(Failure::refused(format!(
                "no rig profile installed — the toolkit's rigs/{} could not be found from \
                 this executable. Set {} to the asset-forge checkout (or pass --rig-dir), \
                 then run `forge init` here again; forge.toml and the directories are \
                 already in place",
                project.rig_name,
                forge_library::backends::TOOLKIT_ENV,
            )));
        }
    }
    println!(
        "next: drop a reference PNG under {} and give it a row in {}",
        project.rel_to_root(&project.refs_dir()).unwrap_or_default(),
        SOURCES_LEDGER
    );
    Ok(())
}
