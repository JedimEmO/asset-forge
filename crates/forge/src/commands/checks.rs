//! `forge verify`, `forge audit`, `forge rebake`, `forge migrate`: the
//! engine-free checks and repairs, each printing the library's own report
//! and turning its verdict into the exit code.

use forge_library::{Project, migrate, rebake, verify};

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

/// Every clip rebuilds from its own record, by bytes and then by pose on
/// the fixture mannequin; every body is what it claims and conforms to the
/// contract.
///
/// The engine-free four run inside [`forge_studio::audit::run`] first; the
/// pose compare and the body checks need an animation player, which is
/// `MinimalPlugins` plus animation — no renderer, no adapter — so this is
/// still a gate a runner can hold.
pub(crate) fn audit(project: &Project, fit: bool) -> Outcome {
    let audit = forge_studio::audit::run(project, fit);
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
