//! Copy the reference format text out of its one home into this build.
//!
//! The format a reference PNG must satisfy is stated once, in
//! `python/forge_gen/reference.py` as `FORMAT` plus its dated
//! `FORMAT_AMENDMENT`, because that is the door that measures against it.
//! The MCP tool's description has to carry the same words — an agent
//! choosing a picture reads the tool, not the Python — and a second
//! hand-maintained copy held to byte equality across a `.rs`, a `--help`
//! reflow and a markdown file is a test that fails on a rewrap and teaches
//! people to edit the fixture. So the copy is generated: this reads the
//! Python source and writes the text into `OUT_DIR`, where
//! `tools/reference.rs` includes it. It is the only generator of the Rust
//! copy; `just ref-format` prints the text and the markdown block for the
//! docs and generates no third one.
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
        .and_then(|python| format_text(&python));
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

/// The format and its amendment, joined the way the door's own
/// `format_text()` joins them: the rule, a blank line, then what changed and
/// when. The amendment travels with the text everywhere the text is
/// generated, because the amendment — three heads, not seven — is the part
/// an agent choosing a picture has to know.
fn format_text(python: &str) -> Option<String> {
    let format = format_constant(python, "FORMAT")?;
    match format_constant(python, "FORMAT_AMENDMENT") {
        Some(amendment) => Some(format!("{format}\n\n{amendment}")),
        None => Some(format),
    }
}

/// The body of `<name> = """…"""`, unwrapped and trimmed.
///
/// A deliberately small parser: it looks for the assignment at the start of
/// a line and takes what is between the triple quotes. Anything cleverer
/// would be a second reader of Python, and anything looser would silently
/// pick up a different constant — matching at a line start is also what
/// keeps `FORMAT` from swallowing `FORMAT_AMENDMENT`.
fn format_constant(python: &str, name: &str) -> Option<String> {
    for quote in ["\"\"\"", "\'\'\'"] {
        for continued in ["\\\n", "\n"] {
            let needle = format!("\n{name} = {quote}{continued}");
            let Some(start) = python.find(&needle) else {
                continue;
            };
            let body = &python[start + needle.len()..];
            let end = body.find(quote)?;
            let text = body[..end].trim();
            if !text.is_empty() {
                return Some(text.to_owned());
            }
        }
    }
    None
}
