//! Re-bake every shipped clip from its own take and recipe.
//!
//! The audit proves each clip *can* be rebuilt from its sidecar; this is the
//! command that actually does it, library-wide — the move that re-ships the
//! whole set after the bake itself changes. Each clip's existing record rides
//! along, so prompts, tags, provenance, authorship and the authored events
//! all survive; what changes is exactly what the bake owns: the bytes, the
//! measurements, the derived events, and the content hash.
//!
//! A **body or model is skipped by name**, and that skip is the headline
//! rule rather than a gap. A lifted mesh's record claims integrity and
//! provenance, never regeneration — it is the file that was rigged and
//! approved, and neither the lift nor Blender's glTF export is byte-stable —
//! so there is no recipe to re-derive and nothing this command could honestly
//! rebuild. Re-shipping one means re-rigging and `forge promote body <name>
//! --overwrite`, which is what the skip line says out loud instead of
//! reporting "no recipe recorded" like a defect.

use crate::promote::{PromoteClip, promote_clip_carrying};
use crate::schema::{Actor, Kind};
use crate::{Catalog, Project};

/// What a re-bake did, per asset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebakeReport {
    /// Clips re-baked (or, on a dry run, that would have been).
    pub baked: Vec<String>,
    /// Assets left alone, each with why.
    pub skipped: Vec<String>,
    /// Clips whose re-bake failed, each with why.
    pub failed: Vec<String>,
}

impl RebakeReport {
    /// Whether nothing failed.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }

    /// The report as text: one line per asset, then the counts.
    #[must_use]
    pub fn render(&self, dry_run: bool) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for line in &self.baked {
            out.push_str(if dry_run { "would bake " } else { "baked " });
            out.push_str(line);
            out.push('\n');
        }
        for line in &self.skipped {
            out.push_str("skipped ");
            out.push_str(line);
            out.push('\n');
        }
        for line in &self.failed {
            out.push_str("FAILED ");
            out.push_str(line);
            out.push('\n');
        }
        let _ = write!(
            out,
            "{} baked, {} skipped, {} failed",
            self.baked.len(),
            self.skipped.len(),
            self.failed.len()
        );
        out
    }
}

/// Re-bake everything, reporting per asset. With `dry_run` nothing is
/// written and the report says what would have been.
#[must_use]
pub fn run(project: &Project, dry_run: bool) -> RebakeReport {
    let catalog = Catalog::scan(project);
    let mut report = RebakeReport::default();

    for record in catalog.records() {
        let name = record.name.clone();
        match record.kind {
            Kind::Body | Kind::Model => {
                let tool = record
                    .sidecar
                    .as_ref()
                    .and_then(|s| s.generator.as_ref())
                    .map_or("no generator", crate::schema::Generator::tool);
                report.skipped.push(format!(
                    "{name}: {} ({tool}) — nothing to re-derive; re-rig and \
                     `forge promote {} {name} --overwrite` instead",
                    record.kind, record.kind
                ));
                continue;
            }
            Kind::Sfx | Kind::Music | Kind::Voice => continue,
            Kind::Clip => {}
        }
        let Some(sidecar) = record.sidecar.clone() else {
            report.skipped.push(format!("{name}: no sidecar"));
            continue;
        };
        let Some(recipe) = sidecar.recipe.clone() else {
            report.skipped.push(format!("{name}: no recipe"));
            continue;
        };
        let Some(source) = sidecar.source.path.clone() else {
            report
                .skipped
                .push(format!("{name}: no source take recorded"));
            continue;
        };
        let take = project.root.join(&source);
        if !take.is_file() {
            report.skipped.push(format!("{name}: {source} is missing"));
            continue;
        }
        if dry_run {
            report.baked.push(format!("{name} from {source}"));
            continue;
        }
        let request = PromoteClip {
            name: name.clone(),
            take_path: take,
            recipe,
            // Nothing new is claimed: prompt, tags, note, author and the
            // authored events are already in the record riding along.
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Unknown,
            take_record: None,
            overwrite: true,
        };
        match promote_clip_carrying(project, &request, Some(&sidecar)) {
            Ok(promoted) => {
                let first = promoted.report.lines().next().unwrap_or(&promoted.report);
                report.baked.push(first.to_owned());
            }
            Err(err) => report.failed.push(format!("{name}: {err}")),
        }
    }
    report
}
