//! The `out/` walk: every file a generator left, with the record beside it.
//!
//! No GPU, no daemon, no queue: this reads directories and records, which is
//! why `GET /v1/runs` and the `list_runs` tool are the same function and why
//! an agent that has lost its context can find yesterday's audition without
//! anything being up.
//!
//! **`promoted` is decided by hashing**, never by remembering: a library
//! sidecar whose `content_hash` (or whose source hash, for a clip baked out
//! of a take) equals what this run's record claims for its output. Nothing
//! writes a "promoted" flag anywhere, so nothing can go stale.

use std::path::Path;

use forge_library::project::OutKind;
use forge_library::{GeneratorRecord, Project};

use crate::job::{Run, RunFilter};

/// Walk the generated directories.
///
/// This cannot fail: a directory that is not there contributes nothing, on
/// the grounds that a project with no sounds is not a broken project. The
/// [`Queue`](crate::Queue) method above it keeps a `Result` because a
/// remote queue's walk goes over a socket and that one can.
pub(crate) fn walk(project: &Project, filter: &RunFilter) -> Vec<Run> {
    let shipped = shipped_hashes(project);
    let mut runs = Vec::new();
    let audio = project.out_dir(OutKind::Audio);
    for kind in ["sfx", "music", "speech", "voice"] {
        collect(project, &audio.join(kind), kind, &shipped, &mut runs);
    }
    let sweeps = project.out_dir(OutKind::Sweeps);
    if let Ok(entries) = std::fs::read_dir(&sweeps) {
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            collect(project, &entry.path(), "take", &shipped, &mut runs);
        }
    }
    collect(
        project,
        &project.out_dir(OutKind::Lifts),
        "lift",
        &shipped,
        &mut runs,
    );
    collect(
        project,
        &project.sources.join("voices"),
        "voice",
        &shipped,
        &mut runs,
    );

    if let Some(kind) = &filter.kind {
        runs.retain(|run| run.kind == *kind);
    }
    if let Some(since) = &filter.since {
        runs.retain(|run| {
            run.created
                .as_deref()
                .is_none_or(|created| created >= since.as_str())
        });
    }
    // Newest first, by what the record says and then by path so the order
    // is stable inside a day.
    runs.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.path.cmp(&a.path)));
    if let Some(limit) = filter.limit {
        runs.truncate(limit);
    }
    runs
}

/// Every generator record directly under a directory (one level of
/// subdirectory for voices), as runs.
fn collect(project: &Project, dir: &Path, kind: &str, shipped: &[String], runs: &mut Vec<Run>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `assets-src/voices/<name>/voice.json` sits one deeper.
            collect(project, &path, kind, shipped, runs);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(record) = GeneratorRecord::load(&path) else {
            continue;
        };
        for output in &record.outputs {
            let absolute = project.root.join(&output.path);
            if !absolute.is_file() {
                continue;
            }
            runs.push(Run {
                path: project
                    .rel_to_root(&absolute)
                    .unwrap_or_else(|| output.path.clone()),
                kind: kind.to_owned(),
                tool: Some(record.tool.clone()),
                prompt: record.prompt().map(str::to_owned),
                seed: record
                    .param_seed_text("seed")
                    .or_else(|| record.param_seed_text("seeds")),
                created: Some(record.created.clone()),
                created_by: Some(record.created_by.clone()),
                fake: record.fake,
                promoted: output
                    .sha256
                    .as_ref()
                    .is_some_and(|hash| shipped.iter().any(|shipped| shipped == hash)),
                record: project.rel_to_root(&path),
            });
        }
    }
}

/// Every hash the library claims: what a shipped file is, and what it was
/// made from.
fn shipped_hashes(project: &Project) -> Vec<String> {
    let mut hashes = Vec::new();
    for asset in forge_library::Catalog::scan(project).records() {
        let Some(sidecar) = &asset.sidecar else {
            continue;
        };
        hashes.push(sidecar.content_hash.clone());
        if let Some(hash) = &sidecar.source.sha256 {
            hashes.push(hash.clone());
        }
    }
    hashes
}
