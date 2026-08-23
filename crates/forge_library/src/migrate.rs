//! Bringing every sidecar in the library to the current writer's bytes, once
//! and idempotently.
//!
//! # What this is allowed to change
//!
//! The migration rewrites durable, git-tracked records, so it is deliberately
//! narrow about what it will touch:
//!
//! * A **schema 1** record is read, re-serialised by this build's writer,
//!   and written back only if the bytes differ — key order, indentation, a
//!   float's spelling. Nothing recorded is changed: a clip whose generator
//!   block records a real seed keeps it, and a migration that nulled it on
//!   the next run would undo the only honest provenance in the file.
//! * A record with an **empty content hash** gets one, because that is the
//!   one claim that can be made about any file. A *stale* hash is not
//!   repaired: that is `verify`'s finding, and quietly re-hashing over it
//!   would launder a file somebody replaced by hand.
//! * An asset with **no record at all** gets an honest empty one:
//!   `generator: null`, `provenance: "unknown"`, and a note saying why there
//!   is nothing to say.
//! * A record at any **other schema** cannot be read — there is no legacy
//!   reader, by design — and is reported, not rewritten.
//!
//! # Idempotence
//!
//! The command builds the record it wants, serialises it, and writes only when
//! the bytes differ from what is on disk. A second run therefore produces no
//! diff at all — which is the property that makes it safe to wire into a
//! justfile recipe a human will run more than once.

use crate::schema::{Provenance, SCHEMA, Sidecar};
use crate::{AssetRecord, Catalog, Project, Result, hash, sidecar};

/// What happened to one asset's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Its bytes were not what this writer produces; they are now.
    Rewritten,
    /// There was no record; there is one now.
    Backfilled,
    /// Already current, but the content hash was missing.
    Hashed,
    /// Already right — the second run of `migrate`, and every run after.
    Unchanged,
}

impl Outcome {
    /// The word the report uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rewritten => "rewritten",
            Self::Backfilled => "backfilled",
            Self::Hashed => "hashed",
            Self::Unchanged => "unchanged",
        }
    }
}

/// What a migration did, per asset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrateReport {
    /// One `(name, outcome)` per asset, in catalog order.
    pub outcomes: Vec<(String, Outcome)>,
    /// Assets whose record could not be read, each with why.
    pub unreadable: Vec<String>,
}

impl MigrateReport {
    /// Whether every record could be read.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.unreadable.is_empty()
    }

    /// How many assets had this outcome.
    #[must_use]
    pub fn count(&self, outcome: Outcome) -> usize {
        self.outcomes.iter().filter(|(_, o)| *o == outcome).count()
    }

    /// Whether anything was (or would be) written.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.outcomes.iter().any(|(_, o)| *o != Outcome::Unchanged)
    }

    /// The report as text.
    #[must_use]
    pub fn render(&self, dry_run: bool) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        if dry_run {
            out.push_str("dry run — nothing will be written\n\n");
        }
        for (name, outcome) in &self.outcomes {
            if *outcome != Outcome::Unchanged {
                let _ = writeln!(out, "{name:<20} {}", outcome.as_str());
            }
        }
        for line in &self.unreadable {
            let _ = writeln!(out, "CANNOT READ {line}");
        }
        let _ = write!(
            out,
            "{} rewritten, {} backfilled, {} hashed, {} already current{}",
            self.count(Outcome::Rewritten),
            self.count(Outcome::Backfilled),
            self.count(Outcome::Hashed),
            self.count(Outcome::Unchanged),
            if dry_run { " (nothing written)" } else { "" }
        );
        out
    }
}

/// Migrate the whole library. With `dry_run` nothing is written.
///
/// # Errors
///
/// Fails only when a record that should be written cannot be. An unreadable
/// record is a finding in the report, not an error.
pub fn run(project: &Project, dry_run: bool) -> Result<MigrateReport> {
    let catalog = Catalog::scan(project);
    let mut report = MigrateReport::default();
    for record in catalog.records() {
        match plan(record) {
            Err(error) => report.unreadable.push(format!("{}: {error}", record.name)),
            Ok((wanted, outcome)) => {
                if outcome != Outcome::Unchanged && !dry_run {
                    sidecar::save(&record.sidecar_path(), &wanted)?;
                }
                report.outcomes.push((record.name.clone(), outcome));
            }
        }
    }
    Ok(report)
}

/// The record this asset should have, and how far it is from the one it has.
///
/// Reads the file itself rather than reusing the catalog's parsed copy, so
/// the comparison is against the bytes on disk and not against a round trip
/// through the types.
fn plan(record: &AssetRecord) -> Result<(Sidecar, Outcome)> {
    let path = record.sidecar_path();
    if !path.is_file() {
        let mut wanted = Sidecar::new(record.kind, &record.name);
        wanted.provenance = Provenance::Unknown;
        wanted.content_hash = hash::sha256_file(&record.path)?;
        wanted.note = Some(String::from(
            "backfilled by `forge migrate`: the file was in the library with no record, so \
             nothing is known about how it was made",
        ));
        return Ok((wanted, Outcome::Backfilled));
    }
    let on_disk = crate::read_bytes(&path)?;
    let mut wanted = Sidecar::from_slice(&on_disk, &path)?;
    wanted.schema = SCHEMA;
    let mut outcome = Outcome::Unchanged;
    if wanted.content_hash.is_empty() {
        wanted.content_hash = hash::sha256_file(&record.path)?;
        outcome = Outcome::Hashed;
    }
    if outcome == Outcome::Unchanged && wanted.to_bytes()? != on_disk {
        outcome = Outcome::Rewritten;
    }
    Ok((wanted, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Kind;
    use crate::testing::temp_project;

    #[test]
    fn migrate_backfills_hashes_rewrites_and_then_does_nothing() {
        let (_dir, project) = temp_project();
        let sfx = project.kind_dir(Kind::Sfx);
        // No record at all.
        std::fs::write(sfx.join("bare.wav"), b"RIFF").expect("bare");
        // A record with no hash.
        std::fs::write(sfx.join("unhashed.wav"), b"RIFF").expect("unhashed");
        let mut unhashed = Sidecar::new(Kind::Sfx, "unhashed");
        unhashed.content_hash = String::new();
        sidecar::save(&sfx.join("unhashed.json"), &unhashed).expect("save");
        // A record whose bytes are not this writer's.
        std::fs::write(sfx.join("compact.wav"), b"RIFF").expect("compact");
        let mut compact = Sidecar::new(Kind::Sfx, "compact");
        compact.content_hash = hash::sha256_bytes(b"RIFF");
        std::fs::write(
            sfx.join("compact.json"),
            serde_json::to_vec(&compact).expect("compact json"),
        )
        .expect("write");
        // A record from another schema.
        std::fs::write(sfx.join("old.wav"), b"RIFF").expect("old");
        std::fs::write(sfx.join("old.json"), br#"{"schema": 5, "kind": "sfx"}"#).expect("old");

        let dry = run(&project, true).expect("dry run");
        assert!(!dry.ok(), "the schema-5 record cannot be read");
        assert_eq!(dry.count(Outcome::Backfilled), 1);
        assert_eq!(dry.count(Outcome::Hashed), 1);
        assert_eq!(dry.count(Outcome::Rewritten), 1);
        assert!(!sfx.join("bare.json").exists(), "a dry run writes nothing");

        let wet = run(&project, false).expect("run");
        assert_eq!(wet.outcomes, dry.outcomes);
        assert!(sfx.join("bare.json").is_file());
        let hashed = sidecar::load(&sfx.join("unhashed.json")).expect("reload");
        assert_eq!(hashed.content_hash, hash::sha256_bytes(b"RIFF"));

        let again = run(&project, false).expect("again");
        assert!(!again.changed(), "{}", again.render(false));
        assert_eq!(again.count(Outcome::Unchanged), 3);
    }
}
