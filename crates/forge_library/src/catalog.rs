//! The library, derived by scanning, never stored.
//!
//! At a few hundred assets a full scan is milliseconds — a `read_dir` per kind
//! and a small JSON parse per asset — so there is no database and no index
//! file to fall out of date. That is a deliberate trade: a stale index is a
//! class of bug that cannot happen here, and the scan is cheaper than the
//! code that would keep an index honest.
//!
//! [`Catalog::resolve`] is the other reason this exists. An agent that saw
//! `roll` in a listing should be able to pass back exactly that, or
//! `roll.glb`, or `clips/roll.glb`, and be understood. The MCP server once
//! had that logic twice — once for clips, once for audio, with subtly
//! different tie-breaking — and a wrong name cost the agent a turn.

use std::path::{Path, PathBuf};

use crate::schema::{Kind, Sidecar};
use crate::{Project, sidecar};

/// Extensions treated as a glTF asset: a clip, a body, a model.
const GLTF_EXTENSIONS: [&str; 2] = ["glb", "gltf"];
/// Extensions treated as audio. Matches what `forge_audio` decodes, so a file
/// the library lists is a file that measures.
const AUDIO_EXTENSIONS: [&str; 5] = ["wav", "ogg", "mp3", "flac", "oga"];

/// One asset in the shipped library.
#[derive(Debug, Clone)]
pub struct AssetRecord {
    /// What kind it is, from where it lives.
    pub kind: Kind,
    /// File stem — the name everything else refers to it by.
    pub name: String,
    /// Path relative to the asset root, forward-slashed.
    pub rel_path: String,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// The sidecar beside it, when there is one that parses.
    pub sidecar: Option<Sidecar>,
    /// Why the sidecar did not parse, when it exists and did not.
    ///
    /// Kept rather than swallowed: an unreadable sidecar is exactly what the
    /// audit is for, and an asset that silently loses its provenance looks
    /// identical to one that never had any.
    pub sidecar_error: Option<String>,
}

impl AssetRecord {
    /// The prompt, when the sidecar recorded one.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        self.sidecar.as_ref()?.prompt.as_deref()
    }

    /// The tags, when the sidecar recorded any.
    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        self.sidecar
            .as_ref()
            .map(|s| s.tags.clone())
            .unwrap_or_default()
    }

    /// The sidecar path, whether or not it exists.
    #[must_use]
    pub fn sidecar_path(&self) -> PathBuf {
        sidecar::path_for(&self.path)
    }
}

/// What to look for in the library.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Restrict to one kind.
    pub kind: Option<Kind>,
    /// Case-insensitive substring of the name or the prompt.
    pub text: Option<String>,
    /// Exact tag, case-insensitive.
    pub tag: Option<String>,
}

impl Query {
    /// Everything of one kind.
    #[must_use]
    pub fn of_kind(kind: Kind) -> Self {
        Self {
            kind: Some(kind),
            ..Self::default()
        }
    }

    /// Everything matching a substring.
    #[must_use]
    pub fn matching(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    /// Restrict this query to one kind.
    #[must_use]
    pub fn and_kind(mut self, kind: Option<Kind>) -> Self {
        self.kind = kind;
        self
    }

    /// Restrict this query to one tag.
    #[must_use]
    pub fn and_tag(mut self, tag: Option<String>) -> Self {
        self.tag = tag;
        self
    }
}

/// Every asset under the asset root, sorted by kind then path.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    records: Vec<AssetRecord>,
}

impl Catalog {
    /// Scan the library.
    ///
    /// Never fails: a kind whose directory is missing contributes nothing, on
    /// the grounds that a project with no voice lines is not a broken project.
    #[must_use]
    pub fn scan(project: &Project) -> Self {
        let mut records = Vec::new();
        for kind in Kind::ALL {
            let root = project.kind_dir(kind);
            // Asked as "is this audio", not "is this a clip": the negative
            // form silently gave every kind added later the audio extension
            // list.
            let extensions: &[&str] = if kind.is_audio() {
                &AUDIO_EXTENSIONS
            } else {
                &GLTF_EXTENSIONS
            };
            let mut files = Vec::new();
            collect(&root, extensions, &mut files);
            files.sort();
            for path in files {
                let Some(rel_path) = project.rel_to_assets(&path) else {
                    continue;
                };
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let (loaded, error) = match sidecar::load_beside(&path) {
                    Ok(loaded) => (loaded, None),
                    Err(err) => (None, Some(err.to_string())),
                };
                records.push(AssetRecord {
                    kind,
                    name,
                    rel_path,
                    path,
                    sidecar: loaded,
                    sidecar_error: error,
                });
            }
        }
        Self { records }
    }

    /// A catalog built from records already in hand, for tests and for the
    /// studio's in-memory views.
    #[must_use]
    pub fn from_records(records: Vec<AssetRecord>) -> Self {
        Self { records }
    }

    /// Everything, in scan order.
    #[must_use]
    pub fn records(&self) -> &[AssetRecord] {
        &self.records
    }

    /// How many assets there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the library is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Everything matching a query, in scan order.
    #[must_use]
    pub fn find(&self, query: &Query) -> Vec<&AssetRecord> {
        self.records
            .iter()
            .filter(|record| {
                if query.kind.is_some_and(|k| k != record.kind) {
                    return false;
                }
                if let Some(text) = query
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    let hit = record
                        .sidecar
                        .as_ref()
                        .is_some_and(|s| s.matches_text(text))
                        || record
                            .name
                            .to_ascii_lowercase()
                            .contains(&text.to_ascii_lowercase());
                    if !hit {
                        return false;
                    }
                }
                if let Some(tag) = query
                    .tag
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    && !record.sidecar.as_ref().is_some_and(|s| s.has_tag(tag))
                {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Resolve a caller's reference to one asset.
    ///
    /// Accepts, in order: the exact relative path, the file name, the stem.
    /// The order matters — a stem match is the loosest and must not win over
    /// an exact path when a project has `audio/sfx/hit.wav` and
    /// `audio/voice/hit.wav`. Case-insensitive from the file name down,
    /// because an agent re-typing a name should not be refused over
    /// capitalisation.
    ///
    /// `kind` narrows the search when the caller knows what it wants, which is
    /// what makes those two `hit`s distinguishable at all.
    #[must_use]
    pub fn resolve(&self, wanted: &str, kind: Option<Kind>) -> Option<&AssetRecord> {
        let wanted = wanted.trim().trim_start_matches("./").replace('\\', "/");
        if wanted.is_empty() {
            return None;
        }
        let candidates = || {
            self.records
                .iter()
                .filter(move |r| kind.is_none_or(|k| k == r.kind))
        };
        let file_name = |path: &str| {
            Path::new(path)
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        let stem = Path::new(&wanted)
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());

        candidates()
            .find(|r| r.rel_path == wanted)
            .or_else(|| candidates().find(|r| file_name(&r.rel_path).eq_ignore_ascii_case(&wanted)))
            .or_else(|| candidates().find(|r| r.name.eq_ignore_ascii_case(&stem)))
    }

    /// Every name of one kind, for a refusal that lists what does exist.
    #[must_use]
    pub fn names(&self, kind: Option<Kind>) -> Vec<String> {
        self.records
            .iter()
            .filter(|r| kind.is_none_or(|k| k == r.kind))
            .map(|r| r.name.clone())
            .collect()
    }

    /// A refusal for a name that resolved to nothing: what was asked for,
    /// and what does exist of that kind — so the next call can be right.
    #[must_use]
    pub fn refusal(&self, wanted: &str, kind: Option<Kind>) -> String {
        let names = self.names(kind);
        let what = kind.map_or_else(|| String::from("asset"), |k| k.to_string());
        if names.is_empty() {
            format!("no {what} named {wanted:?} — the library holds no {what}s")
        } else {
            format!(
                "no {what} named {wanted:?} — the library holds: {}",
                names.join(", ")
            )
        }
    }
}

/// Depth-first collect of files with one of `extensions` under `root`.
///
/// Recursive because `clips/` is only the convention: a project that sorts
/// its clips into subdirectories should still have them found.
fn collect(root: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
            {
                out.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: Kind, rel_path: &str) -> AssetRecord {
        let name = Path::new(rel_path)
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        AssetRecord {
            kind,
            name,
            rel_path: rel_path.to_owned(),
            path: PathBuf::from("/library").join(rel_path),
            sidecar: None,
            sidecar_error: None,
        }
    }

    fn catalog() -> Catalog {
        Catalog::from_records(vec![
            record(Kind::Clip, "clips/roll.glb"),
            record(Kind::Sfx, "audio/sfx/hit.wav"),
            record(Kind::Voice, "audio/voice/hit.ogg"),
        ])
    }

    #[test]
    fn a_name_resolves_as_stem_file_name_or_path() {
        let catalog = catalog();
        for wanted in [
            "roll",
            "roll.glb",
            "clips/roll.glb",
            "ROLL",
            "./clips/roll.glb",
        ] {
            let found = catalog.resolve(wanted, None).expect(wanted);
            assert_eq!(found.name, "roll", "{wanted}");
        }
        assert!(catalog.resolve("", None).is_none());
        assert!(catalog.resolve("walk", None).is_none());
    }

    #[test]
    fn an_exact_path_beats_a_loose_stem_and_a_kind_disambiguates() {
        let catalog = catalog();
        let voice = catalog
            .resolve("audio/voice/hit.ogg", None)
            .expect("exact path");
        assert_eq!(voice.kind, Kind::Voice);
        let first = catalog.resolve("hit", None).expect("stem");
        assert_eq!(
            first.kind,
            Kind::Sfx,
            "scan order wins when nothing narrows"
        );
        let narrowed = catalog.resolve("hit", Some(Kind::Voice)).expect("narrowed");
        assert_eq!(narrowed.kind, Kind::Voice);
        assert!(catalog.resolve("roll", Some(Kind::Sfx)).is_none());
    }

    #[test]
    fn a_refusal_lists_what_exists() {
        let catalog = catalog();
        let text = catalog.refusal("walk", Some(Kind::Clip));
        assert!(text.contains("roll"), "{text}");
        let text = catalog.refusal("x", Some(Kind::Body));
        assert!(
            text.contains("no bodys") || text.contains("no body"),
            "{text}"
        );
    }

    #[test]
    fn queries_filter_by_kind_text_and_tag() {
        let mut tagged = record(Kind::Clip, "clips/walk.glb");
        let mut sidecar = Sidecar::new(Kind::Clip, "walk");
        sidecar.prompt = Some(String::from("A person strolls."));
        sidecar.tags = vec![String::from("loop")];
        tagged.sidecar = Some(sidecar);
        let mut records = catalog().records.clone();
        records.push(tagged);
        let catalog = Catalog::from_records(records);
        assert_eq!(catalog.find(&Query::of_kind(Kind::Clip)).len(), 2);
        assert_eq!(catalog.find(&Query::matching("strolls")).len(), 1);
        assert_eq!(
            catalog
                .find(&Query::default().and_tag(Some(String::from("LOOP"))))
                .len(),
            1
        );
        assert_eq!(catalog.names(Some(Kind::Clip)), ["roll", "walk"]);
    }
}
