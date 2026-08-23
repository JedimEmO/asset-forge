//! `forge catalog`: the shipped library, one line per asset.
//!
//! The columns are the questions asked most often of a library at this
//! size: what is it, is its record trustworthy, and what was it asked for.

use forge_library::schema::Provenance;
use forge_library::{Catalog, Kind, Project, Query};

use crate::cli::CatalogArgs;
use crate::commands::first_line;
use crate::outcome::{Failure, Outcome};

/// Parse a `--kind` word, refusing with the six that exist.
pub(crate) fn parse_kind(text: &str) -> Result<Kind, Failure> {
    Kind::parse(text).ok_or_else(|| {
        Failure::refused(format!(
            "{text:?} is not a kind — the kinds are {}",
            Kind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// List the library.
pub(crate) fn run(project: &Project, args: &CatalogArgs) -> Outcome {
    let kind = args.kind.as_deref().map(parse_kind).transpose()?;
    let catalog = Catalog::scan(project);
    let query = Query {
        kind,
        text: args.filter.clone(),
        tag: args.tag.clone(),
    };
    let records = catalog.find(&query);
    println!(
        "{:<8} {:<20} {:<14} {:<12} prompt",
        "kind", "name", "provenance", "tags"
    );
    let mut unrecorded = 0;
    for record in &records {
        let provenance = record
            .sidecar
            .as_ref()
            .map_or(Provenance::Unknown, |s| s.provenance);
        if provenance == Provenance::Unknown {
            unrecorded += 1;
        }
        let tags = record.tags().join(",");
        let prompt = record.prompt().unwrap_or("—");
        println!(
            "{:<8} {:<20} {:<14} {:<12} {}",
            record.kind.as_str(),
            record.name,
            provenance.as_str(),
            first_line(&tags, 12),
            first_line(prompt, 72),
        );
    }
    println!(
        "\n{} asset(s), {unrecorded} with no provenance",
        records.len()
    );
    Ok(())
}
