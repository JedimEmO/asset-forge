//! What the asset library knows about itself: sidecars, catalog, project,
//! the four promote doors, the manifest projection and the checks.
//!
//! Three programs need to answer the same questions — the studio window, the
//! MCP server an agent drives, and the audit — and before this crate existed
//! each had its own answer. There were three sidecar readers, two of them
//! hand-rolled line parsers that could not see a nested object, and two
//! separate copies of "turn an edit recipe into generator arguments". They
//! disagreed, and the disagreement shipped: promoting a clip through the MCP
//! server silently wiped the trims of the clip it replaced, because its argv
//! omitted flags that the generator then inherited from the sidecar it was
//! about to overwrite.
//!
//! So this crate deliberately has **no engine dependency**. Anything Bevy is
//! in `forge_studio`; anything here is plain files, plain types, and one rule
//! per question.
//!
//! # The three invariants
//!
//! - **Sidecars are the source of truth.** The catalog is derived by scanning
//!   and is never persisted, so it cannot go stale. At a few hundred assets a
//!   scan is milliseconds, which is cheaper than any cache-invalidation bug.
//! - **Every flag is explicit.** A bake is a pure function of the caller's
//!   whole [`schema::ClipRecipe`] — identity values included — so nothing is
//!   ever inherited from the clip being replaced.
//! - **Payload first, record last.** An asset's file is written before its
//!   sidecar, and both land by atomic rename, so a reader scanning the library
//!   never sees half an asset.
//!
//! # Layout
//!
//! Every path is relative to the project root, the directory holding
//! `forge.toml` (see [`Project`]):
//!
//! ```text
//! assets/                       the shipped library: bodies/ models/ clips/ audio/{sfx,music,voice}/
//!                               one file + one <stem>.json sidecar per asset, library.json at the root
//! assets-src/                   what assets are made from: refs/ takes/ blender/ SOURCES.md
//! out/                          gitignored scratch: sweeps/ lifts/ props/ export/ sheets/ views/ audio/
//! rigs/<profile>/               the rig profile (a user project keeps it under assets-src/rigs/)
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

pub mod agent_kit;
pub mod audit;
pub mod backends;
pub mod bundle;
pub mod catalog;
pub mod clock;
pub mod generator_record;
pub mod hash;
pub mod manifest;
pub mod metrics_cache;
pub mod migrate;
pub mod project;
pub mod promote;
pub mod rebake;
pub mod report;
pub mod schema;
pub mod sidecar;
pub mod toolkit;
pub mod verify;

pub use catalog::{AssetRecord, Catalog, Query};
pub use generator_record::GeneratorRecord;
pub use project::Project;
pub use report::{Finding, Report, Severity};
pub use schema::{ClipRecipe, Kind, Sidecar};

/// The result of anything in this crate that touches a disk.
pub type Result<T> = std::result::Result<T, LibraryError>;

/// Everything that can go wrong reading or writing the library.
///
/// One enum rather than per-module errors because every caller is a UI or a
/// tool frame that renders the failure as one line of prose to a human or an
/// agent; the variants carry the specifics that line needs (which path, which
/// names exist) so a refusal teaches rather than just denies.
#[derive(Debug)]
#[non_exhaustive]
pub enum LibraryError {
    /// A file could not be read, written or copied.
    Io {
        /// The file in question.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A JSON document did not parse, or did not match the schema.
    Json {
        /// The file in question.
        path: PathBuf,
        /// What serde said, including line and column.
        source: serde_json::Error,
    },
    /// `forge.toml` did not parse, or did not match the schema.
    Toml {
        /// The file in question.
        path: PathBuf,
        /// What the parser said.
        detail: String,
    },
    /// No `forge.toml` above the directory a program was started from.
    ///
    /// Refused rather than guessed, so a program launched from somewhere
    /// unexpected says "I cannot find the project" instead of scanning an
    /// empty directory and reporting that the library is empty.
    NoProject {
        /// Where the walk started.
        start: PathBuf,
    },
    /// A record declared a schema version this build does not know.
    ///
    /// Reading a *newer* record with an older binary would silently drop
    /// fields on the next save, so it refuses instead. There is no older
    /// schema: this library starts at 1, and nothing before it ships.
    UnsupportedSchema {
        /// The file in question.
        path: PathBuf,
        /// The version it declared.
        schema: u64,
    },
    /// A `src:dst,...` retime specification could not be parsed.
    Retime {
        /// The spec as written.
        spec: String,
        /// Which part of it is wrong.
        detail: String,
    },
    /// Shipping would replace an existing asset and nobody said that was fine.
    WouldOverwrite {
        /// The asset's name.
        name: String,
        /// The file that already exists.
        path: PathBuf,
    },
    /// The bake ran and failed.
    Bake {
        /// What went wrong, phrased for the person who has to fix it.
        detail: String,
    },
    /// The rig profile could not be read or a file failed its contract.
    Rig(forge_rig::RigError),
    /// A caller-supplied value cannot be used: an unusable asset name, a kind
    /// that does not match the operation, a payload with no extension.
    Rejected {
        /// What is wrong, phrased for whoever has to fix it.
        detail: String,
    },
}

impl LibraryError {
    /// Attach a path to an [`std::io::Error`], which on its own never says
    /// which file it was about.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }

    /// Refuse a caller-supplied value, with the reason spelled out.
    ///
    /// Public because the CLI and the MCP server both refuse things this
    /// crate has no opinion about — an unknown preset name, a flag pair that
    /// contradicts itself — and a refusal that reads differently depending on
    /// which layer noticed it is a refusal a reader has to learn twice.
    pub fn rejected(detail: impl Into<String>) -> Self {
        Self::Rejected {
            detail: detail.into(),
        }
    }

    pub(crate) fn bake(detail: impl Into<String>) -> Self {
        Self::Bake {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Json { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Toml { path, detail } => write!(f, "{}: {detail}", path.display()),
            Self::NoProject { start } => write!(
                f,
                "no forge.toml above {} — run `forge init` there, or start from inside a project",
                start.display()
            ),
            Self::UnsupportedSchema { path, schema } => write!(
                f,
                "{} declares schema {schema}, which this build does not know (it reads schema {})",
                path.display(),
                schema::SCHEMA
            ),
            Self::Retime { spec, detail } => write!(f, "retime {spec:?}: {detail}"),
            Self::WouldOverwrite { name, path } => write!(
                f,
                "{name} already exists as {}; pass overwrite to replace it",
                path.display()
            ),
            Self::Bake { detail } => write!(f, "the bake failed: {detail}"),
            Self::Rig(source) => write!(f, "{source}"),
            Self::Rejected { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Rig(source) => Some(source),
            _ => None,
        }
    }
}

impl From<forge_rig::RigError> for LibraryError {
    fn from(source: forge_rig::RigError) -> Self {
        Self::Rig(source)
    }
}

/// Read a file, naming it in the error.
pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| LibraryError::io(path, e))
}

/// Read a file's bytes, naming it in the error.
pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| LibraryError::io(path, e))
}

/// Write a file by writing a sibling temporary and renaming it over the top.
///
/// Every durable write in this crate goes through here. A half-written
/// sidecar is indistinguishable from a record that names a file it does not
/// describe, and the studio rescans the library while a promote may be
/// running — so the window where that could be observed has to not exist.
/// `rename` within a directory is atomic on every filesystem this runs on;
/// `write` is not.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LibraryError::io(parent, e))?;
    }
    let tmp = temp_sibling(path);
    std::fs::write(&tmp, bytes).map_err(|e| LibraryError::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // Leaving the temporary behind would look like a second asset to the
        // next scan, so clear it before reporting.
        let _ = std::fs::remove_file(&tmp);
        LibraryError::io(path, e)
    })
}

/// A temporary name beside `path`, unique enough that two processes writing the
/// same record cannot land on each other's temporary file.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| String::from("tmp"), |n| n.to_string_lossy().into_owned());
    let stamp = clock::monotonic_token();
    path.with_file_name(format!(".{name}.{}.{stamp}.tmp", std::process::id()))
}

/// Turn a path into a forward-slashed string relative to `base`, the form
/// every record and every manifest entry uses.
pub(crate) fn rel_string(path: &Path, base: &Path) -> Option<String> {
    Some(
        path.strip_prefix(base)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// Shared fixtures for this crate's unit tests: a temporary project carrying
/// the toolkit's humanoid profile and nothing else.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use crate::Project;

    /// An empty project under a temporary directory with the shipped
    /// `rigs/humanoid` profile installed.
    pub(crate) fn temp_project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "test_library").expect("init");
        let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid");
        project.install_profile(&profile).expect("profile");
        (dir, project)
    }
}
