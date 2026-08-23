//! `forge verify`, `forge audit`, `forge rebake`, `forge migrate`: the
//! engine-free checks and repairs, each printing the library's own report
//! and turning its verdict into the exit code.

use forge_library::{Project, audit, migrate, rebake, verify};

use crate::outcome::{Failure, Outcome};

/// Every engine-free check as one report: the shipped library, the
/// reference ledger, the profile's drift.
pub(crate) fn verify(project: &Project) -> Outcome {
    let report = verify::all(project);
    println!("{report}");
    if report.ok() {
        Ok(())
    } else {
        Err(Failure::failed(format!(
            "verify: {} failure(s)",
            report.failures()
        )))
    }
}

/// Every clip rebuilds from its own record; every body is what it claims.
pub(crate) fn audit(project: &Project) -> Outcome {
    let audit = audit::run(project);
    print!("{}", audit.render());
    if audit.ok() {
        Ok(())
    } else {
        Err(Failure::failed(format!(
            "audit: {} failure(s)",
            audit.combined().failures()
        )))
    }
}

/// Re-bake every shipped clip from its own take and recipe.
///
/// A body is refused by name, and that refusal is the headline rule rather
/// than a gap: a lifted mesh claims integrity and provenance, never
/// regeneration, so there is no recipe to re-derive and nothing this command
/// could honestly rebuild. The skip line says what re-shipping one takes.
pub(crate) fn rebake(project: &Project, dry_run: bool) -> Outcome {
    let report = rebake::run(project, dry_run);
    println!("{}", report.render(dry_run));
    if report.ok() {
        Ok(())
    } else {
        Err(Failure::failed(format!(
            "rebake: {} clip(s) failed",
            report.failed.len()
        )))
    }
}

/// Bring every sidecar up to the current schema, idempotently.
pub(crate) fn migrate(project: &Project, dry_run: bool) -> Outcome {
    let report = migrate::run(project, dry_run)?;
    println!("{}", report.render(dry_run));
    if report.ok() {
        Ok(())
    } else {
        Err(Failure::failed(format!(
            "migrate: {} record(s) could not be read",
            report.unreadable.len()
        )))
    }
}
