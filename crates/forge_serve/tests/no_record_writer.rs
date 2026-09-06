//! The rule this crate is built around, held by reading its own source.
//!
//! `records.py` is the one place that knows a generator record's key order,
//! its sorted free-form maps, its atomic write and its `None`-means-unknown
//! rule, and `crates/forge_library/tests/python_records.rs` pins the two
//! writers that exist to the same bytes. A third writer in the daemon would
//! be a third dialect of one schema, and this repository's ledger already
//! records what three readers of one record format did to each other.
//!
//! So the daemon reads records and never writes one. This test is the
//! cheapest possible guard on that: the crate's own text.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/`.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(!found.is_empty(), "the crate has sources to read");
    found
}

#[test]
fn records_are_written_only_by_the_generator() {
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("read");
        let name = path.display().to_string();
        for (line_number, line) in text.lines().enumerate() {
            let line_number = line_number + 1;
            // A comment may name the schema — this file's own module doc
            // does — but nothing may write the key.
            let code = line.split("//").next().unwrap_or("").trim();
            assert!(
                !code.contains("forge_record"),
                "{name}:{line_number} names forge_record in code: {line}"
            );
            assert!(
                !code.contains("GeneratorRecord::write") && !code.contains("RecordKind::"),
                "{name}:{line_number} looks like a record writer: {line}"
            );
        }
        // The one thing this crate is allowed to do with a record is read
        // one, to answer `list_runs` and to compare hashes.
        for use_site in text.match_indices("GeneratorRecord") {
            let after = &text[use_site.0..];
            assert!(
                after.starts_with("GeneratorRecord::load")
                    || after.starts_with("GeneratorRecord,")
                    || after.starts_with("GeneratorRecord}")
                    || after.starts_with("GeneratorRecord;"),
                "{name} uses GeneratorRecord for something other than a read: {}",
                after.lines().next().unwrap_or_default()
            );
        }
    }
}

/// The other half of the same rule: the graph client is Python's. Nothing
/// here posts a prompt, reads a history or fetches a view — the card's two
/// endpoints are the whole of Rust's `ComfyUI` surface, because the card
/// must answer with no Python alive.
#[test]
fn rusts_comfy_surface_is_system_stats_and_free() {
    let banned = ["/prompt", "/history", "/view", "/object_info", "/upload"];
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("read");
        let name = path.display().to_string();
        for (line_number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            for endpoint in banned {
                assert!(
                    !code.contains(&format!("\"{endpoint}"))
                        && !code.contains(&format!("{{base}}{endpoint}")),
                    "{name}:{} calls ComfyUI's {endpoint}, which belongs to python/forge_gen/comfy.py: {line}",
                    line_number + 1
                );
            }
        }
    }
}
