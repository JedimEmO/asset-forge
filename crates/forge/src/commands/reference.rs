//! `forge ref import`: the human's door onto the reference importer.
//!
//! There is one implementation of the import — `python/forge_gen/reference.py`,
//! registered as `ref-import`, because the keyer it runs is `mesh.py`'s own
//! and belongs beside it. This module composes nothing: it hands the
//! importer the arguments it was given, through the same queue every other
//! generator goes through, so a human's `forge ref import`, a `just
//! ref-import` and an agent's `import_reference` are one door with three
//! ways in and cannot check a picture differently.

use crate::cli::{GenArgs, RefCommand, RefImportArgs};
use crate::outcome::Outcome;
use forge_library::project::Project;

/// Dispatch.
pub(crate) fn run(project: &Project, command: &RefCommand) -> Outcome {
    match command {
        RefCommand::Import(args) => import(project, args),
    }
}

/// Submit `forge gen ref-import` with what the flags said.
///
/// `--sources` is passed only when the project's sources directory is not
/// the default `assets-src`: the importer derives the PNG's destination,
/// the record beside it and the ledger row's key from that one directory,
/// and a caller that named the three separately could put a file somewhere
/// the row it wrote does not point.
fn import(project: &Project, args: &RefImportArgs) -> Outcome {
    let mut rest = vec![
        String::from("ref-import"),
        args.png.display().to_string(),
        String::from("--name"),
        args.name.clone(),
        String::from("--kind"),
        args.kind.clone(),
        String::from("--source"),
        args.source.clone(),
    ];
    if args.overwrite {
        rest.push(String::from("--overwrite"));
    }
    if project.sources != project.root.join("assets-src") {
        rest.push(String::from("--sources"));
        rest.push(project.sources.display().to_string());
    }
    crate::commands::generate::run(project, &GenArgs { rest })
}
