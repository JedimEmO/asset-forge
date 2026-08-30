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

use std::fmt::Write as _;
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

/// The reference ledger a new project starts with: the header row and the
/// rule. The same text the toolkit's own `assets-src/SOURCES.md` opens
/// with, and the same text both doors — `forge init` and the MCP
/// `init_project` — write, because a project made by an agent and a project
/// made by a person have to be the same project.
pub const LEDGER_HEADER: &str = "# Reference sources\n\
\n\
Every reference image under `refs/` has a row here — where it came from, on \
what terms, and what was made from it. A reference PNG claims integrity \
(its sha256) and this row, never regeneration: the row is where its origin \
and its licence live, and a PNG without one is a file nobody can account \
for, which is why `forge verify` fails on it. Add the row when you add the \
image; the ledger is the answer to \"can we ship this?\" and has to be \
answerable from this file alone.\n\
\n\
| File | Origin | For | Date |\n\
|---|---|---|---|\n";

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
    /// What this project makes. Absent means every kind — see [`MakeKinds`].
    #[serde(default)]
    make: Option<MakeKinds>,
    /// What card it runs on, and where the comfy host is. Absent means
    /// detect the tier — see [`Hardware`].
    #[serde(default)]
    hardware: Option<Hardware>,
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
    /// What this project makes, from `[make]`. A `forge.toml` written before
    /// the table existed reads as every kind chosen, so no existing
    /// project's doctor goes quiet.
    pub make: MakeKinds,
    /// What card it runs on and where the comfy host is, from `[hardware]`.
    /// A file that does not say reads as "detect the tier".
    pub hardware: Hardware,
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
            // No table is every kind, and a tier to be detected: a project
            // written before this phase must not lose a doctor row.
            make: parsed.make.unwrap_or_default(),
            hardware: parsed.hardware.unwrap_or_default(),
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
        Self::init_with(root, name, MakeKinds::default(), &Hardware::default())
    }

    /// [`Self::init`] with the three questions already answered: what you
    /// make, what card this is, where `ComfyUI` is.
    ///
    /// # Errors
    ///
    /// As [`Self::init`].
    pub fn init_with(
        root: &Path,
        name: &str,
        make: MakeKinds,
        hardware: &Hardware,
    ) -> Result<Self> {
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
             # dir = \"/path/to/asset-forge/backends\"   # defaults to the toolkit's own\n\
             \n\
             {make}\n\
             {hardware}",
            make = make.to_toml(),
            hardware = hardware.to_toml(),
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

    /// Rewrite **only** `[make]` and `[hardware]` in this project's
    /// `forge.toml`, leaving every other line — including the comments
    /// somebody wrote — exactly as it was.
    ///
    /// This is what `init --adopt` (and the MCP `init_project` with
    /// `adopt: true`) does to a directory that is already a project: the
    /// answers to the three questions are re-given, and nothing else is
    /// touched. Both tables are replaced whole rather than patched key by
    /// key, so a half-written table cannot survive an adopt.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read or written, or when what is
    /// written back does not load.
    pub fn set_make_hardware(&self, make: MakeKinds, hardware: &Hardware) -> Result<Self> {
        let path = self.root.join(PROJECT_FILE);
        let text = crate::read_to_string(&path)?;
        let kept = strip_tables(&text, &["make", "hardware"]);
        let mut out = kept.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(&make.to_toml());
        out.push('\n');
        out.push_str(&hardware.to_toml());
        crate::write_atomic(&path, out.as_bytes())?;
        Self::load(&self.root)
    }

    /// The tier in force: what `[hardware]` says, else what the card says.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.hardware.tier()
    }

    /// One line saying what this project answered to the three questions,
    /// so they are visible whether they were asked, flagged or assumed.
    #[must_use]
    pub fn answered_line(&self) -> String {
        let chosen = self.make.chosen();
        let made = if chosen.is_empty() {
            String::from("nothing (every backend reads off)")
        } else {
            chosen
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "makes {made}; tier {}; comfy at {}",
            self.tier(),
            self.hardware.comfy_url
        )
    }

    /// What to do next, which depends on what was chosen. The last line
    /// `forge init` prints and the last line `init_project` returns.
    #[must_use]
    pub fn next_step(&self) -> String {
        if self.make.is_empty() {
            return String::from(
                "nothing is chosen yet — edit [make] in forge.toml, then `forge setup`",
            );
        }
        if self.tier().is_fake() {
            return String::from(
                "tier fake: every `forge gen` writes a branded placeholder through the same \
                 validators, so the whole path works with no card. `forge setup` when a card \
                 arrives",
            );
        }
        String::from(
            "`forge setup` — it prints what it installs, and what it costs, before a byte \
             downloads",
        )
    }

    /// Every backend the chosen kinds need — what doctor is told to hold to
    /// `ok`, and what `setup` installs. See [`MakeKinds::backends`].
    ///
    /// **Tier `fake` chooses nothing.** `FORGE_FAKE=1` makes every
    /// `forge gen` write a branded placeholder through the same doors and
    /// validators, so no backend is needed and none is held to `ok`: every
    /// doctor row reads `off` and doctor exits 0. That is how the gate runs
    /// green on a machine with no card, and it is a first-class answer to
    /// the second question rather than a way of switching the check off.
    #[must_use]
    pub fn chosen_backends(&self) -> Vec<&'static str> {
        if self.tier().is_fake() {
            return Vec::new();
        }
        self.make.backends()
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

// ---------------------------------------------------- what you make, on what --

/// One of the six things a project can make.
///
/// This is the vocabulary of `[make]`, and the only vocabulary the stranger
/// meets: `forge init` asks what you will make, never which models you want,
/// because a model name is a fact about this month's backends and
/// "characters" is a fact about the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MakeKind {
    /// Static props: a lift, normalised in Blender.
    Props,
    /// Rigged characters: a lift, prepared, skinned to the rig profile.
    Characters,
    /// Animation clips.
    Clips,
    /// Sound effects.
    Sfx,
    /// Music.
    Music,
    /// Spoken lines, and the voices they are cloned from.
    Voice,
}

impl MakeKind {
    /// Every kind, in the order `[make]` writes them and doctor lists them.
    pub const ALL: [Self; 6] = [
        Self::Props,
        Self::Characters,
        Self::Clips,
        Self::Sfx,
        Self::Music,
        Self::Voice,
    ];

    /// The word `forge.toml` and `--make` spell it with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Props => "props",
            Self::Characters => "characters",
            Self::Clips => "clips",
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Voice => "voice",
        }
    }

    /// The kind that word names.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        let word = word.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|kind| kind.as_str() == word)
    }

    /// **The kind → backend map, one fact in one place.** What this kind
    /// cannot be made without, before `+ comfy` is added for any backend the
    /// `comfy` executor hosts (see [`MakeKinds::backends`]).
    ///
    /// The same fact each `backend.toml` states with `executor =`; the file
    /// is what doctor renders a row from, and this table is what `init`,
    /// `licences` and `setup` reason about *before* a backend directory
    /// exists on the machine at all.
    #[must_use]
    pub const fn backends(self) -> &'static [&'static str] {
        match self {
            Self::Props => &["trellis2", "qwen_image"],
            Self::Characters => &["trellis2", "skintokens", "qwen_image"],
            Self::Clips => &["ardy"],
            Self::Sfx => &["moss_sfx"],
            Self::Music => &["acestep"],
            Self::Voice => &["moss_tts"],
        }
    }

    /// One line for the stranger: what choosing this gets them.
    #[must_use]
    pub const fn blurb(self) -> &'static str {
        match self {
            Self::Props => "static props from a reference image",
            Self::Characters => "rigged characters from a reference image",
            Self::Clips => "animation clips from a prompt",
            Self::Sfx => "sound effects from a prompt",
            Self::Music => "music from a prompt",
            Self::Voice => "spoken lines, in a voice you design",
        }
    }
}

impl std::fmt::Display for MakeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which executor runs a backend: the `env` launcher this repo has always
/// had, the `ComfyUI` service over HTTP, or a host program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Executor {
    /// A per-backend interpreter and checkout the launcher execs.
    Env,
    /// A workflow posted to the `ComfyUI` service at `[hardware] comfy_url`.
    Comfy,
    /// A host program (Blender), or a service described so it has a row.
    Tool,
}

impl Executor {
    /// The word `backend.toml` spells it with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Comfy => "comfy",
            Self::Tool => "tool",
        }
    }

    /// The executor that word names.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "env" => Some(Self::Env),
            "comfy" => Some(Self::Comfy),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

/// The host every `comfy` backend needs, added to the chosen set whenever
/// one of them is chosen.
pub const COMFY_BACKEND: &str = "comfy";

/// The host program a mesh kind needs, added to the chosen set whenever
/// props or characters is chosen.
///
/// Blender is not in [`MakeKind::backends`] because that map is about
/// generators — what makes the thing — and Blender makes nothing. It is
/// what *normalises* a prop and what prepares a body's geometry before the
/// skinner, so a props-only project whose Blender is missing must not read
/// green; it is added here, beside the comfy host, for the same reason and
/// in the same way.
pub const BLENDER_BACKEND: &str = "blender";

/// **The `forge gen <verb>` → backend map, one fact in one place.**
///
/// [`MakeKind::backends`] says which backends a *kind* needs; this says
/// which backend a *command line* runs on, and the two are the same fact
/// read from opposite ends. It exists because a job that does not name its
/// backend is a job with no budget, no admission refusal and no card
/// ladder: the terminal door submitted every `forge gen sfx` with
/// `backend: null` while the MCP door named `moss_sfx`, so one row said
/// `executor: "env"` for a run whose own record said `comfy` (2026-08-30).
/// Both doors read this function now.
///
/// The Blender verbs (`prop`, `rig`, `export`, `rig-build`, `prepare`)
/// name Blender, which is a `tool` backend with no budget: naming it costs
/// nothing and keeps "every generate names its backend" true without an
/// exception nobody can see. A verb that is not a generator at all —
/// `doctor`, `motion review` — is `None`, and the queue does not refuse it.
/// `motion` is the one verb whose subcommand decides: `sweep` and `keys`
/// draw motion out of ARDY, `review` draws a contact sheet out of takes
/// that are already on disk and must not wait for a 16 GB budget.
#[must_use]
pub fn backend_for_verb(verb: &str, sub: Option<&str>) -> Option<&'static str> {
    Some(match (verb, sub) {
        ("sfx", _) => "moss_sfx",
        ("music", _) => "acestep",
        ("speech" | "voice", _) => "moss_tts",
        ("mesh", _) => "trellis2",
        ("motion", Some("sweep" | "keys")) => "ardy",
        ("skin", _) => "skintokens",
        ("prop" | "rig" | "export" | "rig-build" | "prepare", _) => BLENDER_BACKEND,
        _ => return None,
    })
}

/// One backend a kind can need: which executor runs it, what it costs on
/// disk, and which licences it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackendNeed {
    /// The directory name under `backends/`.
    pub name: &'static str,
    /// Who runs it.
    pub executor: Executor,
    /// Approximate disk, in GB, for the environment, the clone and the
    /// tools — **never the weights**, which are [`Self::weights_gb`].
    /// An estimate, and said to be one.
    pub env_gb: f64,
    /// What the weights cost, in GB: the sum of the `gb` figures in
    /// `backends/<name>/backend.toml`'s `[[models]]`, and held to it by
    /// `the_disk_bill_is_read_out_of_each_backend_toml`. `0.0` for a
    /// backend whose weights are counted against another (the `comfy`
    /// host's image models belong to `qwen_image`) or whose models state no
    /// size, because `null` means unknown and a guess in a bill is how
    /// `acestep` came to say 7.5 GB for a 10.03 GB checkpoint.
    pub weights_gb: f64,
    /// Where both numbers came from, so nobody re-quotes an estimate as a
    /// measurement. None of them is a VRAM figure: a `vram_gb` is a budget
    /// and is never quoted as one (`designs/decisions.md`, 2026-08-30).
    pub disk_note: &'static str,
    /// The licence ids this backend carries, by [`Licence::id`].
    pub licences: &'static [&'static str],
    /// The licence ids this backend's own `install.sh` stops and asks about
    /// through `confirm_license`.
    ///
    /// **This is what decides whether `forge setup` may pass `--yes`.** It
    /// used to pass a blanket one to every installer — the door the CLI
    /// itself refuses from a human — and `backends/comfy/install.sh` then
    /// accepted the Shakker-Labs `FLUX.1-dev` `ControlNet` under a
    /// **non-commercial** licence that is not one of the five ids, was
    /// never on the screen and landed in no receipt (2026-08-30). An
    /// installer whose prompts are not all covered by the receipt gets no
    /// `--yes` and asks for itself.
    pub installer_prompts: &'static [&'static str],
}

impl BackendNeed {
    /// What this backend costs on disk in total: the environment plus the
    /// weights.
    #[must_use]
    pub const fn disk_gb(&self) -> f64 {
        self.env_gb + self.weights_gb
    }
}

/// Every backend the six kinds can need, with its executor, its disk and
/// its licences. The counterpart to [`MakeKind::backends`]: that says which
/// backends a kind needs, this says what each backend costs and carries.
pub const BACKEND_NEEDS: [BackendNeed; 9] = [
    BackendNeed {
        name: "trellis2",
        executor: Executor::Env,
        env_gb: 20.0,
        weights_gb: 0.0,
        disk_note: "conda env (CUDA 12.4) + clone + TRELLIS.2-4B and DINOv3 — an estimate \
                    (backends/README.md); backend.toml states no size for either weight, \
                    and null means unknown",
        licences: &["nvdiffrast", "dinov3"],
        installer_prompts: &["nvdiffrast"],
    },
    BackendNeed {
        name: "skintokens",
        executor: Executor::Env,
        env_gb: 3.0,
        weights_gb: 0.0,
        disk_note: "venv + clone + ~1.6 GB of weights — an estimate (backends/README.md); \
                    backend.toml states no size for the three",
        licences: &["skintokens_encoder"],
        installer_prompts: &[],
    },
    BackendNeed {
        name: "ardy",
        executor: Executor::Env,
        env_gb: 35.0,
        weights_gb: 0.0,
        disk_note: "venv + clone + the assembled Llama-3/LLM2Vec encoder (~16 GB \
                    downloaded, ~31 GB written) — an estimate (backends/README.md)",
        licences: &["llama3"],
        installer_prompts: &["llama3"],
    },
    BackendNeed {
        name: "qwen_image",
        executor: Executor::Comfy,
        env_gb: 0.0,
        weights_gb: 33.6,
        disk_note: "Qwen-Image fp8 20.43 + text encoder 9.38 + VAE 0.25 + InstantX \
                    ControlNet-Union 3.54, read out of backends/comfy/backend.toml \
                    (lean substitutes the 13.07 GB Q4_K_M GGUF for the fp8 model). It has \
                    no environment: it is a model group inside the host, fetched with \
                    `install.sh --models qwen_image`",
        licences: &[],
        installer_prompts: &[],
    },
    BackendNeed {
        name: "moss_sfx",
        executor: Executor::Comfy,
        env_gb: 0.0,
        weights_gb: 10.46,
        disk_note: "MOSS-SoundEffect-v2.0, read out of backends/moss_sfx/backend.toml and \
                    measured in the host's models/TTS/ tree; the pack fetches it on the \
                    node's first run, so no installer downloads it",
        licences: &[],
        installer_prompts: &[],
    },
    BackendNeed {
        name: "acestep",
        executor: Executor::Comfy,
        env_gb: 0.0,
        weights_gb: 10.03,
        disk_note: "the ACE-Step 1.5 turbo all-in-one checkpoint, read out of \
                    backends/acestep/backend.toml — one file, and the only thing this \
                    backend installs. The 7.5 that stood here was an estimate of a model \
                    set that is not what ships",
        licences: &[],
        installer_prompts: &[],
    },
    BackendNeed {
        name: "moss_tts",
        executor: Executor::Comfy,
        env_gb: 0.0,
        weights_gb: 16.28,
        disk_note: "MOSS-TTS 1.7B 5.72 + MOSS-VoiceGenerator 3.95 + MOSS-Audio-Tokenizer \
                    6.61, read out of backends/moss_tts/backend.toml and measured in the \
                    host's models/TTS/ tree; the pack fetches them on first run",
        licences: &[],
        installer_prompts: &[],
    },
    BackendNeed {
        name: BLENDER_BACKEND,
        executor: Executor::Tool,
        env_gb: 0.0,
        weights_gb: 0.0,
        disk_note: "a host program: `$BLENDER_BIN` or `blender` on PATH, >= 4.2. \
                    Nothing installs it here and nothing of it ships in an asset",
        licences: &[],
        installer_prompts: &[],
    },
    BackendNeed {
        name: COMFY_BACKEND,
        executor: Executor::Tool,
        env_gb: 2.0,
        weights_gb: 0.0,
        disk_note: "venv + the pinned ComfyUI clone and its two node packs; an estimate, \
                    not a measurement. **The models it hosts are counted against the \
                    backends that name them**, and `forge setup` tells its installer which \
                    group to fetch (`--models qwen_image`, `--models none`) rather than \
                    letting it pull all 73.67 GB of them",
        licences: &["comfyui_gpl"],
        // `install.sh --models flux` is the only path that reaches
        // `confirm_license`, and `forge setup` never passes it — see
        // `installer_prompts`.
        installer_prompts: &["flux_dev_controlnet"],
    },
];

/// What one backend needs, by name.
#[must_use]
pub fn backend_need(name: &str) -> Option<&'static BackendNeed> {
    BACKEND_NEEDS.iter().find(|need| need.name == name)
}

/// The `[make]` table: which of the six a project makes.
///
/// An absent table reads as every kind — a `forge.toml` written before the
/// table existed must not go quiet, and a project that says nothing wants
/// everything checked. A *present* table states each kind, so an unstated
/// one is off; `forge init` always writes all six.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MakeKinds {
    /// Static props.
    #[serde(default)]
    pub props: bool,
    /// Rigged characters.
    #[serde(default)]
    pub characters: bool,
    /// Animation clips.
    #[serde(default)]
    pub clips: bool,
    /// Sound effects.
    #[serde(default)]
    pub sfx: bool,
    /// Music.
    #[serde(default)]
    pub music: bool,
    /// Spoken lines.
    #[serde(default)]
    pub voice: bool,
}

impl Default for MakeKinds {
    /// Every kind — what an absent `[make]` means.
    fn default() -> Self {
        Self::all()
    }
}

impl MakeKinds {
    /// What `forge init` offers when nobody answers: the three the toolkit's
    /// own library is made of.
    pub const DEFAULT: Self = Self {
        props: true,
        characters: true,
        clips: true,
        sfx: false,
        music: false,
        voice: false,
    };

    /// Every kind chosen — `--make all`, and what an absent `[make]` reads as.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            props: true,
            characters: true,
            clips: true,
            sfx: true,
            music: true,
            voice: true,
        }
    }

    /// Nothing chosen — `--make none`, and what tier `fake` chooses.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            props: false,
            characters: false,
            clips: false,
            sfx: false,
            music: false,
            voice: false,
        }
    }

    /// Whether one kind is chosen.
    #[must_use]
    pub const fn has(&self, kind: MakeKind) -> bool {
        match kind {
            MakeKind::Props => self.props,
            MakeKind::Characters => self.characters,
            MakeKind::Clips => self.clips,
            MakeKind::Sfx => self.sfx,
            MakeKind::Music => self.music,
            MakeKind::Voice => self.voice,
        }
    }

    /// Choose or unchoose one kind.
    pub const fn set(&mut self, kind: MakeKind, on: bool) {
        match kind {
            MakeKind::Props => self.props = on,
            MakeKind::Characters => self.characters = on,
            MakeKind::Clips => self.clips = on,
            MakeKind::Sfx => self.sfx = on,
            MakeKind::Music => self.music = on,
            MakeKind::Voice => self.voice = on,
        }
    }

    /// The chosen kinds, in [`MakeKind::ALL`] order.
    #[must_use]
    pub fn chosen(&self) -> Vec<MakeKind> {
        MakeKind::ALL
            .into_iter()
            .filter(|kind| self.has(*kind))
            .collect()
    }

    /// Whether nothing at all is chosen — every doctor row reads `off`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chosen().is_empty()
    }

    /// **The chosen backend set**: every backend the chosen kinds need, plus
    /// [`COMFY_BACKEND`] when any of them is hosted by the `comfy` executor
    /// and [`BLENDER_BACKEND`] when a mesh kind is chosen.
    /// Sorted, deduplicated — this is what `forge gen doctor --chosen` is
    /// handed, and what decides which rows may vote on the exit code.
    #[must_use]
    pub fn backends(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        for kind in self.chosen() {
            for name in kind.backends() {
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
        if names
            .iter()
            .any(|name| backend_need(name).map(|need| need.executor) == Some(Executor::Comfy))
            && !names.contains(&COMFY_BACKEND)
        {
            names.push(COMFY_BACKEND);
        }
        // The two host rows follow what chose them: a mesh kind cannot be
        // normalised or prepared without Blender, so a props-only project
        // whose Blender is missing is not green.
        if (self.props || self.characters) && !names.contains(&BLENDER_BACKEND) {
            names.push(BLENDER_BACKEND);
        }
        names.sort_unstable();
        names
    }

    /// Read a `--make` value: `all`, `none`, or a comma-separated list of
    /// kinds. The error names every word that would have worked, because a
    /// refusal that does not is a second turn.
    ///
    /// # Errors
    ///
    /// [`LibraryError::Rejected`] naming the unknown word and the six kinds.
    pub fn parse_list(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("all") {
            return Ok(Self::all());
        }
        if value.is_empty() || value.eq_ignore_ascii_case("none") {
            return Ok(Self::none());
        }
        let mut make = Self::none();
        for word in value.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            let kind = MakeKind::parse(word).ok_or_else(|| {
                LibraryError::rejected(format!(
                    "{word:?} is not something this makes — use one or more of {}, or all, or none",
                    MakeKind::ALL
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            make.set(kind, true);
        }
        Ok(make)
    }

    /// The `[make]` table as `forge.toml` writes it, comments and all.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "[make]\n\
             # What you make here. Everything after reads this: `forge setup` installs\n\
             # only what a chosen kind needs, and `forge doctor` prints `off` for a kind\n\
             # you did not choose rather than probing it.\n",
        );
        for kind in MakeKind::ALL {
            let _ = writeln!(
                out,
                "{:<11}= {:<6}# {}",
                kind.as_str(),
                self.has(kind),
                kind.backends().join(" + ")
            );
        }
        out
    }
}

// --------------------------------------------------------- what card is this --

/// Which register the generators run in. Tiers change registers and
/// variants, never features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// A 24 GB card: every model in its full register.
    Full,
    /// A 16 GB card: the `Q4_K_M` GGUF reference model and `MOSS-TTS` 1.7B.
    /// **Lifts at 1024³ like the full tier** — a 1024³ lift measured 4.7 GB
    /// (`designs/decisions.md`, 2026-08-30), and 512³ costs the face.
    Lean,
    /// No card: `FORGE_FAKE=1` as a first-class answer. Every `forge gen`
    /// writes branded placeholders through the same doors and validators.
    Fake,
}

impl Tier {
    /// Every tier, in the order the prompt offers them.
    pub const ALL: [Self; 3] = [Self::Full, Self::Lean, Self::Fake];

    /// A card of at least this many MiB is [`Tier::Full`].
    pub const FULL_MIB: u64 = 22 * 1024;
    /// A card of at least this many MiB is [`Tier::Lean`].
    pub const LEAN_MIB: u64 = 14 * 1024;

    /// The word `forge.toml` and `--tier` spell it with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Lean => "lean",
            Self::Fake => "fake",
        }
    }

    /// The tier that word names.
    ///
    /// # Errors
    ///
    /// [`LibraryError::Rejected`] naming the three words.
    pub fn parse(word: &str) -> Result<Self> {
        let word = word.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|tier| tier.as_str() == word)
            .ok_or_else(|| {
                LibraryError::rejected(format!(
                    "{word:?} is not a tier — full (24 GB), lean (16 GB) or fake (no card)"
                ))
            })
    }

    /// One line for the stranger.
    #[must_use]
    pub const fn blurb(self) -> &'static str {
        match self {
            Self::Full => "a 24 GB card: every model in its full register",
            Self::Lean => {
                "a 16 GB card: the Q4_K_M reference model and MOSS-TTS 1.7B; \
                 lifts at 1024 cubed like full"
            }
            Self::Fake => {
                "no card: every generator writes a branded placeholder through \
                 the same validators"
            }
        }
    }

    /// Whether this tier means `FORGE_FAKE=1`.
    #[must_use]
    pub const fn is_fake(self) -> bool {
        matches!(self, Self::Fake)
    }

    /// The tier this machine's card suggests: the largest card `nvidia-smi`
    /// reports, held against [`Self::FULL_MIB`] and [`Self::LEAN_MIB`]. No
    /// `nvidia-smi`, no card, or a card too small is [`Tier::Fake`].
    ///
    /// Offered, never assumed: `forge init` prints what it detected and lets
    /// the answer be overridden, because the card that is busy today is
    /// still the card this project runs on.
    #[must_use]
    pub fn detect() -> Self {
        Self::from_total_mib(largest_gpu_mib())
    }

    /// The tier a card of this size is, for a caller that already asked the
    /// driver. `None` is no card at all.
    #[must_use]
    pub fn from_total_mib(total_mib: Option<u64>) -> Self {
        match total_mib {
            Some(mib) if mib >= Self::FULL_MIB => Self::Full,
            Some(mib) if mib >= Self::LEAN_MIB => Self::Lean,
            _ => Self::Fake,
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The largest card `nvidia-smi` reports, in MiB. `None` when the program is
/// not there, does not answer, or lists no GPU — all of which mean the same
/// thing to a tier.
fn largest_gpu_mib() -> Option<u64> {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .max()
}

/// Where the `ComfyUI` service answers when nobody says otherwise.
pub const DEFAULT_COMFY_URL: &str = "http://127.0.0.1:8188";

/// The `[hardware]` table: the register, and where the comfy host is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hardware {
    /// The tier as the file states it. `None` means the file did not say, so
    /// it is [`Tier::detect`]ed — which is what a `forge.toml` written
    /// before this table reads as.
    #[serde(default)]
    pub tier: Option<Tier>,
    /// Where the `ComfyUI` service answers. Another machine's is fine: the
    /// `env` backends and Blender stay local.
    #[serde(default = "default_comfy_url")]
    pub comfy_url: String,
}

fn default_comfy_url() -> String {
    String::from(DEFAULT_COMFY_URL)
}

impl Default for Hardware {
    fn default() -> Self {
        Self {
            tier: None,
            comfy_url: default_comfy_url(),
        }
    }
}

impl Hardware {
    /// The tier in force: what the file says, else what the card says.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.tier.unwrap_or_else(Tier::detect)
    }

    /// The `[hardware]` table as `forge.toml` writes it.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let tier = self.tier.unwrap_or(Tier::Full);
        format!(
            "[hardware]\n\
             # Detected from the card and overridable. Tiers change registers and\n\
             # variants, never features: lean runs the reference image model quantised\n\
             # and MOSS-TTS at 1.7B, and lifts at 1024 cubed exactly like full.\n\
             tier = {:?}     # {}\n\
             comfy_url = {:?}   # where the ComfyUI service answers; another machine's is fine\n",
            tier.as_str(),
            tier.blurb(),
            self.comfy_url
        )
    }
}

// ------------------------------------------------------------------ licences --

/// One licence fact a backend carries, as the stranger is shown it.
///
/// [`Self::terms`] is the whole notice — the same words `install.sh` prints
/// and the CLI asks about, not a summary of them — because neither a human
/// nor an agent can accept what it was not shown. Where the complete legal
/// text runs longer than a screen, the notice quotes the operative clause
/// verbatim and [`Self::full_text`] names where the rest lives; that is the
/// form this repo's installers have used since the first one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Licence {
    /// The id `--yes <id>` and the MCP `accept: [...]` name it by.
    pub id: &'static str,
    /// What it is called.
    pub name: &'static str,
    /// The backend that carries it.
    pub backend: &'static str,
    /// Whether nothing installs until this one is accepted by name. `false`
    /// is a fact to be told, not a bargain to be struck.
    pub needs_accept: bool,
    /// The notice, in full.
    pub terms: &'static str,
    /// Where the complete text is.
    pub full_text: &'static str,
}

/// The five licence facts the six kinds can carry.
pub const LICENCES: [Licence; 5] = [
    Licence {
        id: "nvdiffrast",
        name: "NVIDIA Source Code License (1-Way Commercial)",
        backend: "trellis2",
        needs_accept: true,
        terms: "nvdiffrast 0.4.0 — NVIDIA Source Code License (1-Way Commercial)\n\
                \n\
                TRELLIS.2's texture bake (o_voxel.postprocess.to_glb) rasterises through\n\
                nvdiffrast. Nothing in `forge gen mesh` produces a textured mesh without\n\
                it. Its licence, section 3.3, Use Limitation:\n\
                \n\
                \x20 \"The Work and any derivative works thereof only may be used or intended\n\
                \x20  for use non-commercially. [...] As used herein, 'non-commercially'\n\
                \x20  means for research or evaluation purposes only and not for any direct\n\
                \x20  or indirect monetary gain.\"\n\
                \n\
                If your project is commercial, a lifted mesh's texture came through\n\
                software you are not licensed to use for it. `forge doctor` warns for as\n\
                long as nvdiffrast is installed, and every lift record carries\n\
                texture_baker naming it — the fact travels with the asset.\n\
                \n\
                Declining leaves an env that does everything but bake: doctor reports\n\
                \"partial\" and `forge gen mesh` exits 6 pointing back here.",
        full_text: "https://github.com/NVlabs/nvdiffrast/blob/v0.4.0/LICENSE.txt",
    },
    Licence {
        id: "dinov3",
        name: "DINOv3 License (Meta) — gated",
        backend: "trellis2",
        needs_accept: true,
        terms: "facebook/dinov3-vitl16-pretrain-lvd1689m — DINOv3 License (Meta)\n\
                \n\
                TRELLIS.2 encodes the reference image with DINOv3. The weights are gated\n\
                on the Hugging Face hub: the licence is accepted by a human on the model\n\
                page, and the token that proves it is a thing only a human holds.\n\
                \n\
                Two commands, in this order, and neither can be run for you:\n\
                \x20 1. accept at https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m\n\
                \x20 2. hf auth login --token <tok>\n\
                \n\
                Never the interactive `hf auth login`: it waits on a TTY an agent does\n\
                not have, and hangs (designs/hosting.md, Common).",
        full_text: "https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m",
    },
    Licence {
        id: "llama3",
        name: "Llama 3 Community License — attribution required",
        backend: "ardy",
        needs_accept: true,
        terms: "Meta-Llama-3-8B-Instruct (ARDY's text encoder base) — Llama 3 Community License\n\
                \n\
                ARDY's text encoder is built from Meta-Llama-3-8B-Instruct (via the\n\
                NousResearch mirror) under the Llama 3 Community License, which asks that\n\
                anything built with it say:\n\
                \n\
                \x20 \"Built with Meta Llama 3\"\n\
                \n\
                Here the encoder only embeds the prompt — no Llama weights ship inside a\n\
                clip — and the backend's notice carries the attribution so it travels\n\
                with the backend. The assembly downloads ~16 GB and writes ~31 GB.",
        full_text: "https://llama.meta.com/llama3/license/",
    },
    Licence {
        id: "skintokens_encoder",
        name: "SkinTokens Michelangelo encoder — an open question",
        backend: "skintokens",
        needs_accept: false,
        terms: "SkinTokens src/model/michelangelo/ — a licence question, not a licence\n\
                \n\
                SkinTokens' code and weights are MIT. Its shape encoder is derived from\n\
                NeuralCarver/Michelangelo, which is GPL-3.0 upstream and is shipped by\n\
                SkinTokens under MIT; the authors have not answered (issue #9).\n\
                \n\
                What this project does about it: the skinner runs in its own process,\n\
                nothing of it ships inside an asset, and every rig record names its\n\
                `skinner` the way a lift record names its texture baker. Doctor warns for\n\
                as long as it is installed. This is a warning, so nothing waits on your\n\
                accepting it — but you were told, and the fact travels.",
        full_text: "https://github.com/VAST-AI-Research/SkinTokens/issues/9",
    },
    Licence {
        id: "comfyui_gpl",
        name: "ComfyUI — GPL-3.0-or-later",
        backend: COMFY_BACKEND,
        needs_accept: false,
        terms: "ComfyUI — GPL-3.0-or-later\n\
                \n\
                The audio models and the reference image model run inside ComfyUI, which\n\
                is GPL-3.0-or-later. It runs as its own service and is driven over HTTP\n\
                from a separate process: nothing of it is linked into this toolkit, and\n\
                what it writes is your project's own. It is started with\n\
                --disable-api-nodes, so no node in it can call a paid API or reach the\n\
                internet from a graph.\n\
                \n\
                Nothing here waits on your accepting this; you are told because the\n\
                service is a program that lands on your machine.",
        full_text: "https://github.com/comfyanonymous/ComfyUI/blob/master/LICENSE",
    },
];

/// One licence by id.
#[must_use]
pub fn licence(id: &str) -> Option<&'static Licence> {
    LICENCES.iter().find(|licence| licence.id == id)
}

/// Every licence the chosen kinds carry, in [`LICENCES`] order, without
/// repeats — props and characters share nvdiffrast and it is shown once.
#[must_use]
pub fn licences_for(kinds: &[MakeKind]) -> Vec<&'static Licence> {
    let mut make = MakeKinds::none();
    for kind in kinds {
        make.set(*kind, true);
    }
    let backends = make.backends();
    LICENCES
        .iter()
        .filter(|licence| backends.contains(&licence.backend))
        .collect()
}

/// The licence ids the chosen kinds cannot be installed without — the ones
/// `--yes <id>` and `accept: [...]` must name.
#[must_use]
pub fn required_licences(kinds: &[MakeKind]) -> Vec<&'static Licence> {
    licences_for(kinds)
        .into_iter()
        .filter(|licence| licence.needs_accept)
        .collect()
}

// ------------------------------------------------------------- the one screen --

/// What `setup` would do, worked out **before a byte downloads**: per chosen
/// kind the backends it needs, what they cost on disk, the total, and every
/// licence fact they carry.
///
/// One screen, one place: the CLI prints it and asks once; the MCP `setup`
/// tool returns the same thing and refuses until `accept` names every id
/// that needs one. Neither invents a number — the disk figures come from
/// [`BACKEND_NEEDS`], each with the file it was read out of.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupPlan {
    /// The kinds this plan covers, in [`MakeKind::ALL`] order.
    pub kinds: Vec<MakeKind>,
    /// Every backend they need, deduplicated, in [`BACKEND_NEEDS`] order.
    pub backends: Vec<&'static BackendNeed>,
    /// Every licence fact those backends carry, in [`LICENCES`] order.
    pub licences: Vec<&'static Licence>,
    /// The register this project runs in — it changes which weights are
    /// fetched, so it belongs on the bill.
    pub tier: Tier,
}

impl SetupPlan {
    /// The plan for a set of kinds at a tier.
    #[must_use]
    pub fn for_kinds(kinds: &[MakeKind], tier: Tier) -> Self {
        let mut make = MakeKinds::none();
        for kind in kinds {
            make.set(*kind, true);
        }
        let names = make.backends();
        Self {
            kinds: make.chosen(),
            backends: BACKEND_NEEDS
                .iter()
                .filter(|need| names.contains(&need.name))
                .collect(),
            licences: licences_for(kinds),
            tier,
        }
    }

    /// The plan for what a project's `[make]` chose.
    #[must_use]
    pub fn for_project(project: &Project) -> Self {
        Self::for_kinds(&project.make.chosen(), project.tier())
    }

    /// What the whole thing costs on disk, in GB.
    #[must_use]
    pub fn total_disk_gb(&self) -> f64 {
        self.backends.iter().map(|need| need.disk_gb()).sum()
    }

    /// The licences that must be accepted by name before anything installs.
    #[must_use]
    pub fn required(&self) -> Vec<&'static Licence> {
        self.licences
            .iter()
            .copied()
            .filter(|licence| licence.needs_accept)
            .collect()
    }

    /// The required licence ids that are neither already on this machine's
    /// receipt nor named in `accepted`. Empty means setup may proceed.
    #[must_use]
    pub fn missing_accepts(
        &self,
        receipt: &licences::Receipt,
        accepted: &[String],
    ) -> Vec<&'static Licence> {
        self.required()
            .into_iter()
            .filter(|licence| {
                !receipt.has(licence.id) && !accepted.iter().any(|id| id == licence.id)
            })
            .collect()
    }

    /// **The one screen**, printed before a byte downloads: what each kind
    /// needs, what it costs, the total, and every licence in full.
    #[must_use]
    pub fn screen(&self) -> String {
        let mut out = String::new();
        if self.kinds.is_empty() {
            return String::from(
                "nothing is chosen, so there is nothing to install. `forge init --make \
                 props,characters,clips` (or edit [make] in forge.toml) says what you \
                 make here.\n",
            );
        }
        let _ = writeln!(
            out,
            "setup — what this installs, before a byte downloads. tier {} ({}).\n",
            self.tier,
            self.tier.blurb()
        );
        if self.tier.is_fake() {
            let _ = writeln!(
                out,
                "  On tier fake nothing below is *needed*: every generator writes a branded\n\
                 \x20 placeholder through the same doors and validators, and doctor reads every\n\
                 \x20 row `off`. Install it anyway if a card is coming.\n"
            );
        }
        for kind in &self.kinds {
            let _ = writeln!(out, "  {} — {}", kind.as_str(), kind.blurb());
            for need in self
                .backends
                .iter()
                .filter(|need| kind.backends().contains(&need.name))
            {
                let _ = writeln!(
                    out,
                    "      {:<12} {:>6.1} GB  [{}]  {}",
                    need.name,
                    need.disk_gb(),
                    need.executor.as_str(),
                    need.disk_note
                );
            }
        }
        let hosts: Vec<&&BackendNeed> = self
            .backends
            .iter()
            .filter(|need| {
                !self
                    .kinds
                    .iter()
                    .any(|kind| kind.backends().contains(&need.name))
            })
            .collect();
        if !hosts.is_empty() {
            let _ = writeln!(out, "  hosts — what those run inside");
            for need in hosts {
                let _ = writeln!(
                    out,
                    "      {:<12} {:>6.1} GB  [{}]  {}",
                    need.name,
                    need.disk_gb(),
                    need.executor.as_str(),
                    need.disk_note
                );
            }
        }
        let _ = writeln!(
            out,
            "\n  total        {:>6.1} GB under ${} (default ~/{}), plus the Hugging Face cache.",
            self.total_disk_gb(),
            licences::BACKENDS_HOME_ENV,
            licences::DEFAULT_BACKENDS_HOME
        );
        let _ = writeln!(
            out,
            "  Every figure above is disk, read out of the file named beside it. None of \
             them is a VRAM number: a backend's vram_gb is a budget and is never quoted \
             as a measurement."
        );
        if self.licences.is_empty() {
            let _ = writeln!(out, "\nNo licence here asks anything of you.");
            return out;
        }
        let _ = writeln!(
            out,
            "\n{} licence fact(s) come with that. Read them; they travel with every asset \
             you make.\n",
            self.licences.len()
        );
        for licence in &self.licences {
            let _ = writeln!(
                out,
                "── {} ({}) — {} ────",
                licence.id,
                licence.backend,
                if licence.needs_accept {
                    "needs your yes"
                } else {
                    "told, not asked"
                }
            );
            let _ = writeln!(out, "{}", licence.terms);
            let _ = writeln!(out, "  Full text: {}\n", licence.full_text);
        }
        out
    }

    /// The refusal an agent gets when `accept` does not name every id: the
    /// ids that are missing, and the one sentence that says what to do.
    ///
    /// A tool that does not exist, rather than a prompt asking an agent to
    /// behave: the call cannot succeed until the ids are in it.
    #[must_use]
    pub fn refusal(missing: &[&'static Licence]) -> String {
        let ids: Vec<&str> = missing.iter().map(|licence| licence.id).collect();
        format!(
            "refused: nothing was installed. {} licence(s) here need accepting by name, and \
             this call named none of them: {}.\n{}\ncall licences first and pass each id in \
             accept",
            missing.len(),
            ids.join(", "),
            missing
                .iter()
                .map(|licence| format!("  {} — {} ({})", licence.id, licence.name, licence.backend))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

/// The licence receipt: who accepted what, when, and at which door.
///
/// It lives at `$FORGE_BACKENDS_HOME/licences.json` — **beside the installs,
/// because the install is what is licensed**. Deliberately not in
/// `forge.toml`: that file is hand-edited, and a hand-edited acceptance is
/// one that was *typed* rather than *given*. A machine that has accepted
/// nvdiffrast has accepted it for every project on it; a machine that has
/// not accepted it has an empty file, and `forge setup` asks.
///
/// ```json
/// {"forge_licences":1,"accepted":[{"id":"nvdiffrast",
///   "name":"NVIDIA Source Code License (1-Way Commercial)","backend":"trellis2",
///   "by":"human","at":"2026-08-30T14:02:11Z","via":"cli"}]}
/// ```
pub mod licences {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};

    use crate::{LibraryError, Result};

    /// The environment variable naming where the backends install
    /// themselves — envs, clones, weights, and this receipt.
    pub const BACKENDS_HOME_ENV: &str = "FORGE_BACKENDS_HOME";

    /// Where the installs go when `$FORGE_BACKENDS_HOME` is unset, relative
    /// to the home directory.
    pub const DEFAULT_BACKENDS_HOME: &str = ".cache/asset-forge/backends";

    /// The receipt's file name.
    pub const RECEIPT_FILE: &str = "licences.json";

    /// The receipt's schema version.
    pub const RECEIPT_SCHEMA: u32 = 1;

    /// Which door an acceptance came through.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Via {
        /// `forge setup` at a terminal, or `--yes <id>`.
        Cli,
        /// The MCP `setup` tool's `accept` argument.
        Mcp,
        /// A backend's own `install.sh`, run directly.
        Install,
    }

    impl Via {
        /// The word the receipt writes.
        #[must_use]
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Cli => "cli",
                Self::Mcp => "mcp",
                Self::Install => "install",
            }
        }
    }

    /// One acceptance, as written.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Accepted {
        /// The licence id, by [`super::Licence::id`].
        pub id: String,
        /// What it is called, copied in so the receipt reads on its own.
        pub name: String,
        /// The backend it belongs to.
        pub backend: String,
        /// Who accepted: `human` at a TTY, or the actor an agent's door
        /// named (`agent:claude`). Never invented — the door that writes
        /// the row knows which it was.
        pub by: String,
        /// When, as `YYYY-MM-DDTHH:MM:SSZ`.
        pub at: String,
        /// Which door.
        pub via: Via,
    }

    /// The file.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Receipt {
        /// The schema version, so a later shape can be read.
        #[serde(rename = "forge_licences")]
        pub schema: u32,
        /// Every acceptance, in the order it was given.
        #[serde(default)]
        pub accepted: Vec<Accepted>,
    }

    impl Default for Receipt {
        fn default() -> Self {
            Self {
                schema: RECEIPT_SCHEMA,
                accepted: Vec::new(),
            }
        }
    }

    impl Receipt {
        /// Whether this licence has been accepted on this machine.
        #[must_use]
        pub fn has(&self, id: &str) -> bool {
            self.accepted.iter().any(|row| row.id == id)
        }

        /// The row for a licence, when there is one.
        #[must_use]
        pub fn row(&self, id: &str) -> Option<&Accepted> {
            self.accepted.iter().find(|row| row.id == id)
        }
    }

    /// Where the backends install themselves: `$FORGE_BACKENDS_HOME`, else
    /// `~/.cache/asset-forge/backends`. The same rule `_lib/common.sh` uses,
    /// so the shell installers and this agree without either being told.
    #[must_use]
    pub fn backends_home() -> PathBuf {
        if let Ok(dir) = std::env::var(BACKENDS_HOME_ENV)
            && !dir.trim().is_empty()
        {
            return PathBuf::from(dir);
        }
        home_dir().join(DEFAULT_BACKENDS_HOME)
    }

    /// `$HOME`, or the current directory when even that is unset — a
    /// receipt that lands in the wrong place is better than a panic in a
    /// container with no environment.
    fn home_dir() -> PathBuf {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
    }

    /// The receipt's path under a backends home.
    #[must_use]
    pub fn path_in(home: &Path) -> PathBuf {
        home.join(RECEIPT_FILE)
    }

    /// The receipt's path on this machine.
    #[must_use]
    pub fn path() -> PathBuf {
        path_in(&backends_home())
    }

    /// Read the receipt at `path`. A file that is not there is an empty
    /// receipt — nothing accepted yet is a normal state, not an error.
    ///
    /// # Errors
    ///
    /// Fails when the file exists and does not parse: a receipt nobody can
    /// read must not silently become "you accepted nothing", because the
    /// next thing that happens is a second prompt for something already
    /// agreed to, and the row that recorded it is gone.
    pub fn load_from(path: &Path) -> Result<Receipt> {
        if !path.is_file() {
            return Ok(Receipt::default());
        }
        let text = crate::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| {
            LibraryError::rejected(format!(
                "{} does not parse as a licence receipt: {e}",
                path.display()
            ))
        })
    }

    /// Read this machine's receipt.
    ///
    /// # Errors
    ///
    /// As [`load_from`].
    pub fn load() -> Result<Receipt> {
        load_from(&path())
    }

    /// Append acceptances to the receipt at `path` and write it back,
    /// creating the directory if the installers have not yet.
    ///
    /// Appending, never rewriting: an id already in the file keeps its
    /// original row, because the question "when was this accepted, and by
    /// whom" has one true answer and a re-run of `setup` is not it.
    ///
    /// # Errors
    ///
    /// As [`load_from`], or when the file cannot be written.
    pub fn accept_in(
        path: &Path,
        ids: &[&str],
        by: &str,
        via: Via,
    ) -> Result<(Receipt, Vec<String>)> {
        let mut receipt = load_from(path)?;
        let at = crate::clock::now_iso();
        let mut added = Vec::new();
        for id in ids {
            if receipt.has(id) {
                continue;
            }
            let Some(licence) = super::licence(id) else {
                return Err(LibraryError::rejected(format!(
                    "{id:?} is not a licence this toolkit knows — the ids are {}",
                    super::LICENCES
                        .iter()
                        .map(|l| l.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            };
            receipt.accepted.push(Accepted {
                id: licence.id.to_string(),
                name: licence.name.to_string(),
                backend: licence.backend.to_string(),
                by: by.to_string(),
                at: at.clone(),
                via,
            });
            added.push(licence.id.to_string());
        }
        if !added.is_empty() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| LibraryError::io(parent, e))?;
            }
            let text = serde_json::to_string_pretty(&receipt)
                .map_err(|e| LibraryError::rejected(e.to_string()))?;
            crate::write_atomic(path, format!("{text}\n").as_bytes())?;
        }
        Ok((receipt, added))
    }

    /// Append acceptances to this machine's receipt.
    ///
    /// # Errors
    ///
    /// As [`accept_in`].
    pub fn accept(ids: &[&str], by: &str, via: Via) -> Result<(Receipt, Vec<String>)> {
        accept_in(&path(), ids, by, via)
    }
}

/// Make a project, whole: `forge.toml` with the three answers in it, the
/// convention directories, the reference ledger's header, the rig profile
/// copied in and an empty manifest written — so the very next
/// `forge manifest --check` and `forge verify` pass on a library that holds
/// nothing, which is the honest starting state rather than a broken one.
///
/// Both doors call this: `forge init` at a terminal and the MCP
/// `init_project`. A project an agent made and a project a person made are
/// the same project, down to the ledger's wording, because the alternative
/// is two subtly different starting states and a verify that passes in one
/// of them.
///
/// `profile_source` is the toolkit's `rigs/<rig>` directory. `None` leaves
/// the project half-made — `forge.toml` and the directories are there, the
/// profile and the manifest are not — and says so in the returned lines,
/// which is a state the caller must treat as a failure.
///
/// # Errors
///
/// As [`Project::init_with`], or when the ledger, the profile or the
/// manifest cannot be written.
pub fn create(
    root: &Path,
    name: &str,
    make: MakeKinds,
    hardware: &Hardware,
    profile_source: Option<&Path>,
) -> Result<(Project, Vec<String>)> {
    let project = Project::init_with(root, name, make, hardware)?;
    let mut lines = vec![
        format!("initialised {} at {}", project.name, project.root.display()),
        String::from(
            "forge.toml, assets/{bodies,models,clips,audio/{sfx,music,voice}}, \
             assets-src/{takes,refs,blender,rigs}, out/",
        ),
        project.answered_line(),
    ];
    let ledger = project.sources_ledger();
    if !ledger.exists() {
        crate::write_atomic(&ledger, LEDGER_HEADER.as_bytes())?;
        lines.push(format!(
            "{SOURCES_LEDGER}: the reference ledger, header only"
        ));
    }
    match profile_source {
        Some(source) => {
            project.install_profile(source)?;
            lines.push(format!(
                "rig profile {} installed from {}",
                project.rig_name,
                source.display()
            ));
            let written = crate::manifest::write(&project)?;
            lines.push(format!(
                "{}: empty, on rig {} ({} bones)",
                project
                    .rel_to_root(&project.manifest_path())
                    .unwrap_or_else(|| project.manifest_path().display().to_string()),
                written.rig.profile,
                written.rig.bone_count
            ));
        }
        None => lines.push(format!(
            "NO RIG PROFILE INSTALLED — the toolkit's rigs/{} could not be found. \
             forge.toml and the directories are in place; nothing else is.",
            project.rig_name
        )),
    }
    Ok((project, lines))
}

/// Drop the named top-level tables from a TOML document, keeping every
/// other line — and every comment — byte for byte.
///
/// A comment line immediately above a dropped header goes with it: it is
/// that table's comment, and leaving it stranded over the next table is
/// worse than losing it. Line surgery rather than a re-serialise, because
/// `toml::to_string` would throw away every comment in the file, and
/// `forge.toml` is mostly comments on purpose.
fn strip_tables(text: &str, tables: &[&str]) -> String {
    let headers: Vec<String> = tables.iter().map(|name| format!("[{name}]")).collect();
    let mut out: Vec<&str> = Vec::new();
    let mut dropping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            dropping = headers.iter().any(|header| trimmed == header);
            if dropping {
                // Take the comment block that introduces it with it.
                while out
                    .last()
                    .is_some_and(|prev| prev.trim_start().starts_with('#'))
                {
                    out.pop();
                }
            }
        }
        if !dropping {
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
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

    /// Every weights figure on the setup screen is the one its own
    /// `backend.toml` states.
    ///
    /// The table has to exist as a constant — `init`, `licences` and
    /// `setup` reason about a backend *before* its directory is on the
    /// machine — so the file cannot be the source at run time. It is the
    /// source at test time instead, which is what stops the drift that put
    /// `acestep 7.5 GB` on a screen in front of a 10.03 GB checkpoint
    /// (2026-08-30).
    #[test]
    fn the_disk_bill_is_read_out_of_each_backend_toml() {
        #[derive(serde::Deserialize)]
        struct Weights {
            #[serde(default)]
            models: Vec<Weight>,
        }
        #[derive(serde::Deserialize)]
        struct Weight {
            gb: Option<f64>,
        }

        let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backends");
        for need in &BACKEND_NEEDS {
            let file = tree.join(need.name).join("backend.toml");
            let Ok(text) = std::fs::read_to_string(&file) else {
                // No directory here yet (qwen_image is Phase 3's), so the
                // constant is all there is and it says where it came from.
                assert!(
                    !need.disk_note.is_empty(),
                    "{}: a figure with no file behind it must at least say where it came from",
                    need.name
                );
                continue;
            };
            let parsed: Weights = toml::from_str(&text).expect("backend.toml parses");
            if need.name == COMFY_BACKEND {
                assert!(
                    need.weights_gb.abs() < f64::EPSILON,
                    "the host's own weights are counted against the backends that name them, \
                     and `forge setup` tells its installer which group to fetch"
                );
                continue;
            }
            if parsed.models.iter().any(|model| model.gb.is_none()) {
                assert!(
                    need.weights_gb.abs() < f64::EPSILON,
                    "{}: a model that states no size is unknown, and unknown is not a number \
                     to put on a bill",
                    need.name
                );
                continue;
            }
            let stated: f64 = parsed.models.iter().filter_map(|model| model.gb).sum();
            assert!(
                (need.weights_gb - stated).abs() < 0.005,
                "{}: the table says {:.2} GB of weights and backend.toml says {stated:.2}",
                need.name,
                need.weights_gb
            );
        }
    }

    /// An installer is handed `--yes` only for licences this machine has
    /// agreed to by name, so every prompt an installer makes is either
    /// covered by an id in the table or suppressed by a flag.
    #[test]
    fn every_installer_prompt_is_a_licence_id_or_a_declined_group() {
        for need in &BACKEND_NEEDS {
            for id in need.installer_prompts {
                if *id == "flux_dev_controlnet" {
                    // Deliberately not in LICENCES: no kind in the map needs
                    // the FLUX pose ControlNet, `forge setup` always passes
                    // `--no-flux-controlnet`, and an id in the table would
                    // put a non-commercial licence on the screen of every
                    // project that makes a sound.
                    assert_eq!(need.name, COMFY_BACKEND);
                    assert!(licence(id).is_none());
                    continue;
                }
                let known = licence(id).unwrap_or_else(|| panic!("{id} is not a licence id"));
                assert!(
                    known.needs_accept,
                    "{id} is prompted for at install time, so it must be one setup demands"
                );
                assert!(
                    need.licences.contains(id),
                    "{}: {id} is prompted for but not listed among its licences",
                    need.name
                );
            }
        }
    }

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
    fn a_forge_toml_written_before_this_phase_makes_everything_and_detects_its_tier() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(PROJECT_FILE),
            "[project]\nname = \"old\"\nlibrary_version = \"0.1.0\"\n",
        )
        .expect("write");
        let project = Project::load(dir.path()).expect("load");
        assert_eq!(project.make, MakeKinds::all(), "no [make] is every kind");
        assert!(!project.make.is_empty());
        assert_eq!(
            project.hardware.tier, None,
            "no [hardware] tier is detected"
        );
        assert_eq!(project.hardware.comfy_url, DEFAULT_COMFY_URL);
    }

    #[test]
    fn init_writes_both_tables_and_they_read_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let hardware = Hardware {
            tier: Some(Tier::Fake),
            comfy_url: String::from("http://box:8188"),
        };
        let make = MakeKinds::parse_list("sfx,voice").expect("parse");
        let project = Project::init_with(dir.path(), "my_game", make, &hardware).expect("init");
        assert_eq!(project.make, make);
        assert_eq!(project.hardware, hardware);
        assert_eq!(project.tier(), Tier::Fake);
        assert_eq!(
            project.chosen_backends(),
            Vec::<&str>::new(),
            "tier fake chooses nothing: every generator writes a placeholder"
        );
        assert_eq!(
            make.backends(),
            vec!["comfy", "moss_sfx", "moss_tts"],
            "anything comfy adds the host, and no mesh kind adds no Blender"
        );
        assert!(
            MakeKinds::parse_list("props")
                .expect("parse")
                .backends()
                .contains(&BLENDER_BACKEND),
            "a prop is normalised in Blender"
        );
        let text = std::fs::read_to_string(dir.path().join(PROJECT_FILE)).expect("read");
        assert!(text.contains("sfx        = true"), "{text}");
        assert!(text.contains("props      = false"), "{text}");
        assert!(text.contains("tier = \"fake\""), "{text}");
    }

    #[test]
    fn adopt_replaces_only_the_two_tables_and_keeps_every_other_comment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "my_game").expect("init");
        let before = std::fs::read_to_string(dir.path().join(PROJECT_FILE)).expect("read");
        assert!(before.contains("# stage_body ="));
        let adopted = project
            .set_make_hardware(
                MakeKinds::parse_list("clips").expect("parse"),
                &Hardware {
                    tier: Some(Tier::Lean),
                    comfy_url: String::from(DEFAULT_COMFY_URL),
                },
            )
            .expect("adopt");
        assert_eq!(adopted.make.chosen(), vec![MakeKind::Clips]);
        assert_eq!(adopted.tier(), Tier::Lean);
        assert_eq!(adopted.name, project.name);
        let after = std::fs::read_to_string(dir.path().join(PROJECT_FILE)).expect("read");
        assert!(after.contains("# stage_body ="), "{after}");
        assert_eq!(after.matches("[make]").count(), 1, "{after}");
        assert_eq!(after.matches("[hardware]").count(), 1, "{after}");
        assert!(after.contains("tier = \"lean\""), "{after}");
    }

    #[test]
    fn the_kind_to_backend_map_and_the_licence_table_agree() {
        // Every backend a kind names is described in BACKEND_NEEDS, and
        // every licence names a backend that exists there. The two tables
        // are the one fact this phase publishes; a name in one and not the
        // other is the drift this test is here to catch.
        for kind in MakeKind::ALL {
            for name in kind.backends() {
                assert!(backend_need(name).is_some(), "{kind} names {name}");
            }
        }
        for licence in LICENCES {
            assert!(
                backend_need(licence.backend).is_some(),
                "{} names {}",
                licence.id,
                licence.backend
            );
            assert!(
                backend_need(licence.backend)
                    .expect("need")
                    .licences
                    .contains(&licence.id),
                "{} is not listed by {}",
                licence.id,
                licence.backend
            );
        }
        assert_eq!(
            MakeKinds::none().backends(),
            Vec::<&str>::new(),
            "--make none chooses nothing, so every row is off"
        );
        let props = licences_for(&[MakeKind::Props]);
        assert_eq!(
            props.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec!["nvdiffrast", "dinov3", "comfyui_gpl"]
        );
        assert_eq!(
            required_licences(&[MakeKind::Sfx])
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            Vec::<&str>::new(),
            "sfx carries a fact to be told, not one to be accepted"
        );
        assert_eq!(
            required_licences(&[MakeKind::Characters])
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            vec!["nvdiffrast", "dinov3"]
        );
    }

    #[test]
    fn a_tier_is_detected_from_the_card_and_named_in_a_refusal() {
        assert_eq!(Tier::from_total_mib(Some(24_564)), Tier::Full);
        assert_eq!(Tier::from_total_mib(Some(22 * 1024)), Tier::Full);
        assert_eq!(Tier::from_total_mib(Some(16_376)), Tier::Lean);
        assert_eq!(Tier::from_total_mib(Some(8_192)), Tier::Fake);
        assert_eq!(Tier::from_total_mib(None), Tier::Fake);
        let error = Tier::parse("big").expect_err("refuse");
        assert!(error.to_string().contains("full"), "{error}");
        let error = MakeKinds::parse_list("props,widgets").expect_err("refuse");
        assert!(error.to_string().contains("characters"), "{error}");
    }

    #[test]
    fn the_one_screen_names_every_backend_its_disk_and_every_licence_in_full() {
        let plan = SetupPlan::for_kinds(&[MakeKind::Characters], Tier::Full);
        let screen = plan.screen();
        for name in ["trellis2", "skintokens", "qwen_image", "comfy", "blender"] {
            assert!(screen.contains(name), "{name} missing from:\n{screen}");
        }
        assert!(screen.contains("total"), "{screen}");
        assert!(
            screen.contains("non-commercially"),
            "the operative clause is quoted, not summarised:\n{screen}"
        );
        assert!(screen.contains("Full text: https://github.com/NVlabs/nvdiffrast"));
        assert!(
            screen.contains("is a budget and is never quoted as a measurement"),
            "{screen}"
        );
        assert!(
            (plan.total_disk_gb() - 58.6).abs() < 0.05,
            "{}",
            plan.total_disk_gb()
        );

        // The refusal names the ids and the sentence that fixes the call.
        let receipt = licences::Receipt::default();
        let missing = plan.missing_accepts(&receipt, &[]);
        assert_eq!(
            missing.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec!["nvdiffrast", "dinov3"]
        );
        let refusal = SetupPlan::refusal(&missing);
        assert!(refusal.contains("nvdiffrast"), "{refusal}");
        assert!(refusal.contains("dinov3"), "{refusal}");
        assert!(
            refusal.contains("call licences first and pass each id in accept"),
            "{refusal}"
        );
        assert!(refusal.contains("nothing was installed"), "{refusal}");

        // Accepting one leaves the other named, and an acceptance already on
        // the machine's receipt counts.
        let missing = plan.missing_accepts(&receipt, &[String::from("nvdiffrast")]);
        assert_eq!(
            missing.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec!["dinov3"]
        );

        // sfx asks nothing: its one licence fact is told, not asked.
        let plan = SetupPlan::for_kinds(&[MakeKind::Sfx], Tier::Fake);
        assert!(plan.missing_accepts(&receipt, &[]).is_empty());
        assert!(plan.screen().contains("GPL-3.0-or-later"));
        assert!(
            SetupPlan::for_kinds(&[], Tier::Fake)
                .screen()
                .contains("nothing is chosen")
        );
    }

    #[test]
    fn the_receipt_appends_and_never_relaunders_an_acceptance() {
        use super::licences::{Via, accept_in, load_from};
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("licences.json");
        assert!(load_from(&path).expect("empty").accepted.is_empty());

        let (receipt, added) =
            accept_in(&path, &["nvdiffrast"], "human", Via::Cli).expect("accept");
        assert_eq!(added, vec![String::from("nvdiffrast")]);
        assert!(receipt.has("nvdiffrast"));
        let first = receipt.row("nvdiffrast").expect("row").clone();
        assert_eq!(first.backend, "trellis2");
        assert_eq!(first.by, "human");

        let (receipt, added) =
            accept_in(&path, &["nvdiffrast", "llama3"], "agent:claude", Via::Mcp).expect("again");
        assert_eq!(added, vec![String::from("llama3")], "already given stands");
        assert_eq!(receipt.row("nvdiffrast"), Some(&first));
        assert_eq!(receipt.accepted.len(), 2);

        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"forge_licences\": 1"), "{text}");
        let error = accept_in(&path, &["invented"], "human", Via::Cli).expect_err("refuse");
        assert!(error.to_string().contains("nvdiffrast"), "{error}");
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
