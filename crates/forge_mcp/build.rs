//! Copy the reference format text out of its one home into this build.
//!
//! The format a reference PNG must satisfy is stated once, in
//! `python/forge_gen/reference.py` as `FORMAT`, because that is the door
//! that measures against it. The MCP tool's description has to carry the
//! same words — an agent choosing a picture reads the tool, not the Python —
//! and a second hand-maintained copy held to byte equality across a `.rs`, a
//! `--help` reflow and a markdown file is a test that fails on a rewrap and
//! teaches people to edit the fixture. So the copy is generated: this reads
//! the Python source and writes the text into `OUT_DIR`, where
//! `tools/reference.rs` includes it.
//!
//! When the Python source is not there — a package built outside the
//! checkout — the block comes out empty and the tool's description carries
//! its own prose without the format paragraph. Empty is the honest
//! degradation; a fallback copy pasted here would be exactly the second
//! source this exists to prevent.

use std::path::{Path, PathBuf};

fn main() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/forge_gen/reference.py");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=build.rs");

    let text = std::fs::read_to_string(&source)
        .ok()
        .and_then(|python| format_constant(&python));
    if text.is_none() {
        println!(
            "cargo:warning=no FORMAT in {} — import_reference's description ships without the \
             format paragraph until the reference door lands it",
            source.display()
        );
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("reference_format.txt");
    std::fs::write(&out, text.unwrap_or_default()).expect("write the generated format text");
}

/// The body of `FORMAT = """…"""`, unwrapped and trimmed.
///
/// A deliberately small parser: it looks for the assignment and takes what
/// is between the triple quotes. Anything cleverer would be a second reader
/// of Python, and anything looser would silently pick up a different
/// constant.
fn format_constant(python: &str) -> Option<String> {
    for quote in ["\"\"\"", "'''"] {
        let Some(start) = python.find(&format!("FORMAT = {quote}")) else {
            continue;
        };
        let body = &python[start + "FORMAT = ".len() + quote.len()..];
        let end = body.find(quote)?;
        let text = body[..end].trim();
        if !text.is_empty() {
            return Some(text.to_owned());
        }
    }
    None
}
