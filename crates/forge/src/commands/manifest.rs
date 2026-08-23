//! `forge manifest`: write the consumer manifest, or with `--check` hold the
//! committed one to a rebuild from a fresh scan — the check that keeps a
//! committed projection of a scan-derived catalog honest.

use forge_library::{Project, manifest};

use crate::cli::ManifestArgs;
use crate::outcome::{Failure, Outcome};

/// Write or check `assets/library.json`.
pub(crate) fn run(project: &Project, args: &ManifestArgs) -> Outcome {
    let rel = project
        .rel_to_root(&project.manifest_path())
        .unwrap_or_else(|| project.manifest_path().display().to_string());
    if args.check {
        let report = manifest::check(project);
        print!("{report}");
        println!();
        if report.ok() {
            println!("{rel} matches a rebuild of the library");
            return Ok(());
        }
        return Err(Failure::failed(format!("{rel} is stale")));
    }
    let written = manifest::write(project)?;
    println!(
        "wrote {rel}: {} bodies, {} models, {} clips, {} audio, rig {} ({} bones), library {}",
        written.bodies.len(),
        written.models.len(),
        written.clips.len(),
        written.audio.len(),
        written.rig.profile,
        written.rig.bone_count,
        written.library_version,
    );
    Ok(())
}
