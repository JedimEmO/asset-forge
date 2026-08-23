//! Where everything lives, worked out once: `forge.toml` and the directories
//! it names.
//!
//! Four programs need the same directories and each used to derive them its
//! own way: the viewer from a `--assets` flag, the MCP server from its own
//! `--assets`, the audit from `CARGO_MANIFEST_DIR`, a script from `../..`.
//! They agreed by luck. This resolves them from one root marker so that a
//! project laid out differently is one change in one file, and so that a
//! game's asset folder can be a forge project without being a Rust crate —
//! which the old rule, "a directory holding `Cargo.toml` and `assets/`",
//! quietly required.
//!
//! Convention, not configuration, covers the rest: the kind → directory map
//! ([`Kind::dir`]), the sidecar beside each asset, `library.json` at the
//! asset root, `takes/`, `refs/` and `blender/` under the sources, and the
//! scratch subdirectories under `out/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::Kind;
use crate::{LibraryError, Result, rel_string};

/// The project root marker.
pub const PROJECT_FILE: &str = "forge.toml";

/// The reference ledger under the sources directory: every PNG under `refs/`
/// has a row in it, or `verify` fails.
pub const SOURCES_LEDGER: &str = "SOURCES.md";

/// The manifest's file name at the asset root.
pub const MANIFEST_FILE: &str = "library.json";

/// The library version a new project starts at.
pub const INITIAL_LIBRARY_VERSION: &str = "0.1.0";

/// The rig profile a new project is given.
pub const DEFAULT_RIG: &str = "humanoid";

/// The gitignored scratch subdirectories under `out/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutKind {
    /// Motion sweeps: takes, review tables, sheets.
    Sweeps,
    /// Raw TRELLIS.2 lifts, before any Blender step.
    Lifts,
    /// Normalized props, ready to promote.
    Props,
    /// Rigged-and-exported bodies, ready to promote.
    Export,
    /// Contact sheets.
    Sheets,
    /// Mesh view renders.
    Views,
    /// Generated sounds, before promote.
    Audio,
}

impl OutKind {
    /// The subdirectory name.
    #[must_use]
    pub const fn dir(self) -> &'static str {
        match self {
            Self::Sweeps => "sweeps",
            Self::Lifts => "lifts",
            Self::Props => "props",
            Self::Export => "export",
            Self::Sheets => "sheets",
            Self::Views => "views",
            Self::Audio => "audio",
        }
    }
}

/// `forge.toml` as written. Unknown keys are refused: a typo is a parse
/// error, not a setting that silently does nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForgeToml {
    project: ProjectSection,
    #[serde(default)]
    paths: PathsSection,
    #[serde(default)]
    studio: StudioSection,
    #[serde(default)]
    backends: BackendsSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSection {
    name: String,
    library_version: String,
    #[serde(default = "default_rig")]
    rig: String,
}

fn default_rig() -> String {
    String::from(DEFAULT_RIG)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathsSection {
    #[serde(default = "default_assets")]
    assets: String,
    #[serde(default = "default_sources")]
    sources: String,
    #[serde(default = "default_out")]
    out: String,
    #[serde(default = "default_rigs")]
    rigs: String,
}

fn default_assets() -> String {
    String::from("assets")
}
fn default_sources() -> String {
    String::from("assets-src")
}
fn default_out() -> String {
    String::from("out")
}
fn default_rigs() -> String {
    String::from("assets-src/rigs")
}

impl Default for PathsSection {
    fn default() -> Self {
        Self {
            assets: default_assets(),
            sources: default_sources(),
            out: default_out(),
            rigs: default_rigs(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioSection {
    #[serde(default)]
    stage_body: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendsSection {
    #[serde(default)]
    dir: Option<String>,
    /// Per-backend interpreter overrides: `[backends.interpreters]
    /// trellis2 = "/path/to/python"`. Read by `forge_library::backends`.
    #[serde(default)]
    interpreters: std::collections::BTreeMap<String, String>,
}

/// A forge project: the root that holds `forge.toml`, and every directory
/// resolved from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory holding `forge.toml`. Every relative path in a record is
    /// relative to this.
    pub root: PathBuf,
    /// The shipped library: one file + one sidecar per asset, `library.json`
    /// at its root. The only directory `promote` writes.
    pub assets: PathBuf,
    /// What assets are made from: reference PNGs and lift records, takes,
    /// `.blend` files, the `SOURCES.md` ledger.
    pub sources: PathBuf,
    /// Gitignored scratch: lifts, props, exports, sweeps, sheets, views,
    /// audio, the metrics cache.
    pub out: PathBuf,
    /// Rig profiles, one directory each. The toolkit root keeps them at
    /// `rigs/`; a user project gets `assets-src/rigs/` from `forge init`.
    pub rigs: PathBuf,
    /// The library's name, written into `library.json`.
    pub name: String,
    /// Semver of the library's content, bumped by hand when shipped assets
    /// change; written into `library.json`.
    pub library_version: String,
    /// The rig profile every body is skinned to and every clip is baked
    /// against, by directory name under [`Self::rigs`].
    pub rig_name: String,
    /// The body the studio poses clips on by default, by name under
    /// `bodies/`. `None` lets the studio pick the first body it finds.
    pub stage_body: Option<String>,
    /// Where the generator backends live, when the project says. `None`
    /// means "find the toolkit's" — see `forge_library::backends`.
    pub backends_dir: Option<PathBuf>,
    /// Per-backend interpreter overrides from `[backends.interpreters]`.
    pub backend_interpreters: std::collections::BTreeMap<String, String>,
}

impl Project {
    /// Walk up from `start` looking for `forge.toml`, and load the project
    /// it marks.
    ///
    /// A relative `start` is resolved against the current directory.
    ///
    /// # Errors
    ///
    /// [`LibraryError::NoProject`] when no ancestor holds a `forge.toml`;
    /// otherwise as [`Self::load`].
    pub fn discover(start: &Path) -> Result<Self> {
        let mut here = if start.is_absolute() {
            start.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| LibraryError::io(start, e))?
                .join(start)
        };
        loop {
            if here.join(PROJECT_FILE).is_file() {
                return Self::load(&here);
            }
            if !here.pop() {
                return Err(LibraryError::NoProject {
                    start: start.to_path_buf(),
                });
            }
        }
    }

    /// Load the project whose root is `root`: read `forge.toml` and resolve
    /// every path against the root, without checking any of them exist.
    ///
    /// # Errors
    ///
    /// Fails when `forge.toml` is missing, unreadable or does not parse.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(PROJECT_FILE);
        let text = crate::read_to_string(&path)?;
        let parsed: ForgeToml = toml::from_str(&text).map_err(|e| LibraryError::Toml {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        let root = root.to_path_buf();
        Ok(Self {
            assets: root.join(&parsed.paths.assets),
            sources: root.join(&parsed.paths.sources),
            out: root.join(&parsed.paths.out),
            rigs: root.join(&parsed.paths.rigs),
            name: parsed.project.name,
            library_version: parsed.project.library_version,
            rig_name: parsed.project.rig,
            stage_body: parsed.studio.stage_body,
            backends_dir: parsed.backends.dir.map(|dir| root.join(dir)),
            backend_interpreters: parsed.backends.interpreters,
            root,
        })
    }

    /// Create a project at `root`: write a `forge.toml` naming it, and make
    /// every directory the convention expects — the six kind directories,
    /// `takes/`, `refs/`, `blender/` and `rigs/` under the sources, `out/`.
    ///
    /// The rig profile itself is not written here: it is a directory of
    /// files the toolkit ships, copied in with [`Self::install_profile`].
    ///
    /// # Errors
    ///
    /// Refuses a root that already holds a `forge.toml` — initialising over a
    /// project would rewrite a file somebody edited — and fails when a
    /// directory or the file cannot be created.
    pub fn init(root: &Path, name: &str) -> Result<Self> {
        let marker = root.join(PROJECT_FILE);
        if marker.exists() {
            return Err(LibraryError::rejected(format!(
                "{} already exists — this is a project; edit it rather than re-initialising",
                marker.display()
            )));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::rejected("a project needs a name"));
        }
        std::fs::create_dir_all(root).map_err(|e| LibraryError::io(root, e))?;
        let text = format!(
            "# asset-forge project file. This is the root marker: `forge` walks up from the\n\
             # working directory until it finds one, and every path below is relative to\n\
             # it. Convention covers the rest — kind -> directory (bodies | models | clips |\n\
             # audio/sfx | audio/music | audio/voice), sidecar = same stem `.json`,\n\
             # `library.json` at the asset root.\n\
             \n\
             [project]\n\
             name = {name:?}\n\
             library_version = {INITIAL_LIBRARY_VERSION:?}   # bumped by hand when shipped assets change\n\
             rig = {DEFAULT_RIG:?}                # the rig profile, by directory name under paths.rigs\n\
             \n\
             [paths]\n\
             assets = \"assets\"\n\
             sources = \"assets-src\"\n\
             out = \"out\"\n\
             rigs = \"assets-src/rigs\"\n\
             \n\
             [studio]\n\
             # stage_body = \"vex_runner\"    # the body clips are posed on by default\n\
             \n\
             [backends]\n\
             # dir = \"/path/to/asset-forge/backends\"   # defaults to the toolkit's own\n"
        );
        crate::write_atomic(&marker, text.as_bytes())?;
        let project = Self::load(root)?;
        for dir in project.convention_dirs() {
            std::fs::create_dir_all(&dir).map_err(|e| LibraryError::io(&dir, e))?;
        }
        Ok(project)
    }

    /// Every directory the convention expects to exist.
    fn convention_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Kind::ALL.iter().map(|k| self.kind_dir(*k)).collect();
        dirs.extend([
            self.takes_dir(),
            self.refs_dir(),
            self.blender_dir(),
            self.rigs.clone(),
            self.out.clone(),
        ]);
        dirs
    }

    /// Copy a rig profile directory into this project as
    /// `<rigs>/<rig_name>`: every regular file in `source_dir` and its
    /// `fixture/` subdirectory, verbatim.
    ///
    /// # Errors
    ///
    /// Fails when the source is not a directory that loads as a profile, or
    /// a copy fails.
    pub fn install_profile(&self, source_dir: &Path) -> Result<()> {
        forge_rig::RigProfile::load(source_dir)?;
        let target = self.rig_dir();
        copy_tree(source_dir, &target)
    }

    /// The rig profile directory: `<rigs>/<rig_name>`.
    #[must_use]
    pub fn rig_dir(&self) -> PathBuf {
        self.rigs.join(&self.rig_name)
    }

    /// The rig profile, read and cross-checked.
    ///
    /// # Errors
    ///
    /// As [`forge_rig::RigProfile::load`].
    pub fn profile(&self) -> Result<forge_rig::RigProfile> {
        Ok(forge_rig::RigProfile::load(&self.rig_dir())?)
    }

    /// The directory assets of one kind live in.
    #[must_use]
    pub fn kind_dir(&self, kind: Kind) -> PathBuf {
        self.assets.join(kind.dir())
    }

    /// `<assets>/library.json`.
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        self.assets.join(MANIFEST_FILE)
    }

    /// Where a shipped clip's raw take is kept, which is what makes a shipped
    /// clip editable at all: the stored `.npz` is the *untrimmed* take, so a
    /// recipe replays on top of it.
    #[must_use]
    pub fn takes_dir(&self) -> PathBuf {
        self.sources.join("takes")
    }

    /// Reference images and their lift records.
    #[must_use]
    pub fn refs_dir(&self) -> PathBuf {
        self.sources.join("refs")
    }

    /// Committed `.blend` files: the authoring source of every body.
    #[must_use]
    pub fn blender_dir(&self) -> PathBuf {
        self.sources.join("blender")
    }

    /// Voices: `<sources>/voices/<name>/ref.wav` beside its `voice.json`,
    /// the durable source every line of that character is cloned from.
    #[must_use]
    pub fn voices_dir(&self) -> PathBuf {
        self.sources.join("voices")
    }

    /// `<sources>/SOURCES.md`, the reference ledger.
    #[must_use]
    pub fn sources_ledger(&self) -> PathBuf {
        self.sources.join(SOURCES_LEDGER)
    }

    /// One of the scratch subdirectories under `out/`.
    #[must_use]
    pub fn out_dir(&self, kind: OutKind) -> PathBuf {
        self.out.join(kind.dir())
    }

    /// Turn an absolute path into one relative to the asset root, with forward
    /// slashes — the form the manifest, the sidecars and every tool argument
    /// use.
    #[must_use]
    pub fn rel_to_assets(&self, path: &Path) -> Option<String> {
        rel_string(path, &self.assets)
    }

    /// The same, relative to the project root — the form a sidecar's
    /// `source.path` uses, because a durable source lives under the sources
    /// directory and so cannot be named relative to the asset root at all.
    #[must_use]
    pub fn rel_to_root(&self, path: &Path) -> Option<String> {
        rel_string(path, &self.root)
    }
}

/// Copy every regular file under `from` to `to`, one level of subdirectory
/// deep — enough for a profile's `fixture/`.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| LibraryError::io(to, e))?;
    let entries = std::fs::read_dir(from).map_err(|e| LibraryError::io(from, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| LibraryError::io(from, e))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else if source.is_file() {
            std::fs::copy(&source, &target).map_err(|e| LibraryError::io(&target, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_finds_the_toolkit_root_from_a_crate_directory() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project = Project::discover(here).expect("the toolkit root");
        assert_eq!(project.name, "asset-forge");
        assert_eq!(project.rig_name, "humanoid");
        assert!(project.rig_dir().join("contract.json").is_file());
        assert_eq!(
            project.kind_dir(Kind::Music),
            project.assets.join("audio/music")
        );
        assert_eq!(
            project.backends_dir.as_deref(),
            Some(project.root.join("backends").as_path())
        );
        assert!(project.profile().is_ok());
    }

    #[test]
    fn a_directory_with_no_marker_is_not_a_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = Project::discover(dir.path()).expect_err("no project");
        assert!(matches!(error, LibraryError::NoProject { .. }), "{error}");
    }

    #[test]
    fn init_writes_a_marker_and_the_convention_and_refuses_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "my_game").expect("init");
        assert_eq!(project.name, "my_game");
        assert_eq!(project.library_version, INITIAL_LIBRARY_VERSION);
        assert_eq!(project.rigs, dir.path().join("assets-src/rigs"));
        assert_eq!(project.stage_body, None);
        assert_eq!(project.backends_dir, None);
        for kind in Kind::ALL {
            assert!(project.kind_dir(kind).is_dir(), "{kind}");
        }
        assert!(project.takes_dir().is_dir());
        assert!(project.refs_dir().is_dir());
        assert_eq!(Project::load(dir.path()).expect("reload"), project);
        assert_eq!(
            Project::discover(&project.kind_dir(Kind::Sfx)).expect("walk up"),
            project
        );
        let error = Project::init(dir.path(), "again").expect_err("refuse");
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn an_unknown_key_is_a_parse_error_not_a_silent_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(PROJECT_FILE),
            "[project]\nname = \"x\"\nlibrary_version = \"0.1.0\"\n[paths]\nasets = \"a\"\n",
        )
        .expect("write");
        let error = Project::load(dir.path()).expect_err("refuse");
        assert!(error.to_string().contains("asets"), "{error}");
    }

    #[test]
    fn defaults_fill_what_the_file_leaves_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(PROJECT_FILE),
            "[project]\nname = \"x\"\nlibrary_version = \"2.0.0\"\n",
        )
        .expect("write");
        let project = Project::load(dir.path()).expect("load");
        assert_eq!(project.rig_name, DEFAULT_RIG);
        assert_eq!(project.assets, dir.path().join("assets"));
        assert_eq!(project.rigs, dir.path().join("assets-src/rigs"));
        assert_eq!(
            project.out_dir(OutKind::Sweeps),
            dir.path().join("out/sweeps")
        );
        assert_eq!(
            project.rel_to_root(&project.takes_dir().join("walk.npz")),
            Some(String::from("assets-src/takes/walk.npz"))
        );
    }
}
