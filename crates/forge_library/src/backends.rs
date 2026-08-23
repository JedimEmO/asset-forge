//! Where the generator backends are, and whether each is there.
//!
//! A backend is a directory under `backends/` — `trellis2`, `ardy`, `acestep`,
//! `moss_sfx`, `moss_tts` — holding a `backend.toml` that describes it, an
//! `install.sh` that sets it up, and, once installed, a gitignored `.env`
//! symlink to the interpreter prefix the Python launcher execs. This module
//! finds that directory and says, per backend, found / missing / broken. It
//! does not run anything: **a missing backend is "generation is off", not
//! an error.** A clone of this toolkit that only wants to browse a library,
//! or a game project that only promotes what an artist handed it, is a
//! normal state, and every door that needs a backend asks here first so it
//! can refuse with the backend's name rather than a traceback.
//!
//! # Where the directory comes from
//!
//! In order: `forge.toml`'s `[backends] dir`; the `FORGE_BACKENDS`
//! environment variable; `<toolkit root>/backends`, the toolkit root being
//! the directory holding `python/forge_gen` — the project itself when it is
//! the toolkit, else `$FORGE_HOME` (or the older `$FORGE_TOOLKIT`), else an
//! ancestor of the running executable; none. No absolute path lives in
//! source.
//!
//! # The Python layer
//!
//! `forge gen <cmd>` is [`Backends::python_launcher`] with the command line
//! appended and `--json` on the end: `python3 <toolkit>/python/forge_gen
//! <cmd> … --json`. The child is told where this module found the backends
//! (`FORGE_BACKENDS`) and any per-backend interpreter override from
//! `forge.toml`, so the two sides never disagree about which directory is
//! in force. Its last stdout line is one JSON object and its exit code is one
//! of [`GenExit`], the table `python/forge_gen/exit_codes.py` speaks.
//!
//! `forge doctor` aggregates `forge gen doctor --json` (the in-environment
//! probe: imports, torch, CUDA, weights, licence notices) with the host
//! checks (GPU, Blender, ffmpeg, profile drift, library counts); the types
//! here are what it reads the directory through.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::Project;

/// The backends the toolkit knows, in the order doctor lists them.
pub const KNOWN: [&str; 5] = ["trellis2", "ardy", "acestep", "moss_sfx", "moss_tts"];

/// The environment variable naming the backends directory.
pub const BACKENDS_ENV: &str = "FORGE_BACKENDS";

/// The environment variable naming the toolkit root (the directory holding
/// `python/forge_gen`), for an installed `forge` that is not running out of
/// its checkout.
pub const HOME_ENV: &str = "FORGE_HOME";

/// The older spelling of [`HOME_ENV`], still honoured.
pub const TOOLKIT_ENV: &str = "FORGE_TOOLKIT";

/// The exit codes the Python layer speaks, mirrored from
/// `python/forge_gen/exit_codes.py`. A caller that reads the code knows
/// whether to install something (3), fix its input (4), read a log (5) or
/// put a tool on PATH (6) without parsing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenExit {
    /// Everything went as asked.
    Ok,
    /// The command line was wrong.
    Usage,
    /// The backend this command needs is not installed — generation is off.
    MissingBackend,
    /// The input was read and refused: a PNG with no flat border, a mesh
    /// that is not in the T-pose, an empty prompt.
    InputRejected,
    /// The backend ran and failed; the log tail says why.
    BackendFailed,
    /// A host tool (Blender, ffmpeg, nvidia-smi) is not where it was looked for.
    MissingTool,
}

impl GenExit {
    /// Every code, in order.
    pub const ALL: [Self; 6] = [
        Self::Ok,
        Self::Usage,
        Self::MissingBackend,
        Self::InputRejected,
        Self::BackendFailed,
        Self::MissingTool,
    ];

    /// The process exit code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Usage => 2,
            Self::MissingBackend => 3,
            Self::InputRejected => 4,
            Self::BackendFailed => 5,
            Self::MissingTool => 6,
        }
    }

    /// The code's word, as the JSON line's `error` field spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Usage => "usage",
            Self::MissingBackend => "missing_backend",
            Self::InputRejected => "input_rejected",
            Self::BackendFailed => "backend_failed",
            Self::MissingTool => "missing_tool",
        }
    }

    /// The code a process exited with, when it is one of the table's. A
    /// signal death or an unknown code is `None`: the caller treats it as a
    /// backend failure, which is what an interpreter that died is.
    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|c| i32::from(c.code()) == code)
    }
}

impl std::fmt::Display for GenExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The file that describes a backend.
pub const BACKEND_FILE: &str = "backend.toml";

/// The symlink `install.sh` leaves pointing at the interpreter prefix.
pub const ENV_LINK: &str = ".env";

/// Whether a backend can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendState {
    /// Described and installed: its interpreter is resolvable.
    Found,
    /// Not installed — no `.env`, no override — or not even present.
    /// Generation through it is off.
    Missing,
    /// Present but unusable as described: `backend.toml` does not parse, or
    /// the interpreter it points at is not there.
    Broken(String),
}

impl BackendState {
    /// The word doctor prints.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Missing => "missing",
            Self::Broken(_) => "broken",
        }
    }
}

/// One backend as found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Backend {
    /// The directory's name, which is the backend's.
    pub name: String,
    /// Where it lives (or would).
    pub dir: PathBuf,
    /// Whether it can be used.
    pub state: BackendState,
    /// The `forge_gen` entry module `backend.toml` names, when it parsed.
    pub entry: Option<String>,
    /// What it makes — `mesh`, `motion`, `music`, `sfx`, `speech`, or `tool`
    /// for a host program like Blender — when the file says.
    pub role: Option<String>,
    /// The VRAM one call peaks at, in GB, when the file says. What `forge
    /// gpu` holds the card's free memory against.
    pub vram_gb: Option<f64>,
    /// Whether it stays on the GPU after a call (the ACE-Step server).
    pub resident: bool,
    /// The interpreter the launcher would exec, when one resolved: an
    /// override, or the `.env` link.
    pub interpreter: Option<PathBuf>,
}

/// The part of `backend.toml` this module reads. Everything else — models,
/// notices, env — is the Python launcher's and doctor's business.
#[derive(Debug, Deserialize)]
struct BackendToml {
    name: Option<String>,
    entry: Option<String>,
    role: Option<String>,
    vram_gb: Option<f64>,
    resident: Option<bool>,
}

/// The backends directory and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Backends {
    /// The directory, when one was found.
    pub dir: Option<PathBuf>,
    /// Where the directory came from, for doctor's first line.
    pub origin: String,
    /// Every known backend, in [`KNOWN`] order, plus any unknown directory
    /// found under `dir` after them.
    pub backends: Vec<Backend>,
}

impl Backends {
    /// Find the backends directory for a project and read it.
    ///
    /// Never fails: no directory means every backend is missing, which is a
    /// fact the result carries rather than an error it raises.
    #[must_use]
    pub fn discover(project: &Project) -> Self {
        let (dir, origin) = backends_dir(project);
        let mut backends: Vec<Backend> = KNOWN
            .iter()
            .map(|name| describe(project, dir.as_deref(), name))
            .collect();
        if let Some(dir) = &dir
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            let mut extra: Vec<String> = entries
                .flatten()
                .filter(|e| e.path().is_dir() && e.path().join(BACKEND_FILE).is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| !KNOWN.contains(&name.as_str()))
                .collect();
            extra.sort();
            backends.extend(extra.iter().map(|name| describe(project, Some(dir), name)));
        }
        Self {
            dir,
            origin,
            backends,
        }
    }

    /// One backend by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Backend> {
        self.backends.iter().find(|b| b.name == name)
    }

    /// Whether a backend is found, so a door can refuse before any work.
    #[must_use]
    pub fn is_found(&self, name: &str) -> bool {
        self.get(name)
            .is_some_and(|b| b.state == BackendState::Found)
    }

    /// The refusal a door gives when its backend is not found: what is
    /// missing and what installs it.
    #[must_use]
    pub fn refusal(&self, name: &str) -> String {
        match self.get(name) {
            Some(backend) => match &backend.state {
                BackendState::Found => format!("{name} is installed"),
                BackendState::Missing => format!(
                    "{name} is not installed — generation through it is off. \
                     `just setup {name}` installs it under {}",
                    backend.dir.display()
                ),
                BackendState::Broken(why) => {
                    format!("{name} is present but unusable: {why} — `just doctor` has the detail")
                }
            },
            None => format!(
                "{name} is not a backend this toolkit knows ({})",
                KNOWN.join(", ")
            ),
        }
    }

    /// The command that runs the Python layer: `python3 <toolkit>/python/forge_gen`.
    ///
    /// `None` when no toolkit root can be found — the `forge` binary is
    /// running somewhere that is not a checkout and [`HOME_ENV`] is not
    /// set — in which case generation is off and doctor says so.
    ///
    /// The child inherits the environment plus what keeps the two sides in
    /// agreement: `FORGE_BACKENDS` naming the directory this module settled
    /// on (only when the project named one — otherwise the Python side's own
    /// lookup lands on the same checkout), and `FORGE_BACKEND_<NAME>_PYTHON`
    /// for every `[backends.interpreters]` entry in `forge.toml`, without
    /// overriding a value the user exported.
    #[must_use]
    pub fn python_launcher(project: &Project) -> Option<Command> {
        let toolkit = toolkit_root(project)?;
        let mut command = Command::new("python3");
        command.arg(toolkit.join("python").join("forge_gen"));
        command.current_dir(&project.root);
        if let Some(dir) = &project.backends_dir
            && std::env::var_os(BACKENDS_ENV).is_none()
        {
            command.env(BACKENDS_ENV, dir);
        }
        for (name, python) in &project.backend_interpreters {
            let key = override_var(name);
            if std::env::var_os(&key).is_none() {
                command.env(key, project.root.join(python));
            }
        }
        Some(command)
    }

    /// The largest VRAM peak any described backend declares, in GB: what
    /// the card has to have free for every generate to be possible.
    #[must_use]
    pub fn largest_vram_gb(&self) -> Option<f64> {
        self.backends
            .iter()
            .filter_map(|b| b.vram_gb)
            .filter(|v| *v > 0.0)
            .fold(None, |best: Option<f64>, v| {
                Some(best.map_or(v, |b| b.max(v)))
            })
    }

    /// The backend that declares the largest VRAM peak, by name.
    #[must_use]
    pub fn largest(&self) -> Option<&Backend> {
        self.backends
            .iter()
            .filter(|b| b.vram_gb.is_some_and(|v| v > 0.0))
            .max_by(|a, b| {
                a.vram_gb
                    .partial_cmp(&b.vram_gb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// The toolkit root the launcher would use, for doctor's report.
    #[must_use]
    pub fn toolkit_root(project: &Project) -> Option<PathBuf> {
        toolkit_root(project)
    }
}

/// The backends directory, and where it came from.
fn backends_dir(project: &Project) -> (Option<PathBuf>, String) {
    if let Some(dir) = &project.backends_dir {
        return (Some(dir.clone()), String::from("forge.toml [backends] dir"));
    }
    if let Some(dir) = std::env::var_os(BACKENDS_ENV) {
        return (Some(PathBuf::from(dir)), format!("${BACKENDS_ENV}"));
    }
    if let Some(toolkit) = toolkit_root(project) {
        let dir = toolkit.join("backends");
        if dir.is_dir() {
            return (
                Some(dir),
                String::from("the toolkit root, found from the executable"),
            );
        }
    }
    (None, String::from("nowhere — generation is off"))
}

/// `FORGE_BACKEND_<NAME>_PYTHON` for a backend: the interpreter override
/// the Python launcher honours first.
#[must_use]
pub fn override_var(name: &str) -> String {
    format!("FORGE_BACKEND_{}_PYTHON", name.to_ascii_uppercase())
}

/// The directory holding `python/forge_gen`: the project itself when it is
/// the toolkit, else [`HOME_ENV`] or [`TOOLKIT_ENV`], else an ancestor of
/// the running executable (a checkout's `target/debug/forge` is two levels
/// under it).
fn toolkit_root(project: &Project) -> Option<PathBuf> {
    let is_toolkit = |dir: &Path| dir.join("python").join("forge_gen").is_dir();
    if is_toolkit(&project.root) {
        return Some(project.root.clone());
    }
    for key in [HOME_ENV, TOOLKIT_ENV] {
        if let Some(dir) = std::env::var_os(key) {
            let dir = PathBuf::from(dir);
            if is_toolkit(&dir) {
                return Some(dir);
            }
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut here = exe.parent()?.to_path_buf();
    loop {
        if is_toolkit(&here) {
            return Some(here);
        }
        if !here.pop() {
            return None;
        }
    }
}

/// One backend's state, from its directory and the overrides.
fn describe(project: &Project, backends: Option<&Path>, name: &str) -> Backend {
    let dir = backends.map_or_else(|| PathBuf::from(name), |b| b.join(name));
    let override_key = override_var(name);
    let interpreter = std::env::var_os(&override_key)
        .map(PathBuf::from)
        .or_else(|| {
            project
                .backend_interpreters
                .get(name)
                .map(|p| project.root.join(p))
        });
    let toml_path = dir.join(BACKEND_FILE);
    let described = match std::fs::read_to_string(&toml_path) {
        Ok(text) => match toml::from_str::<BackendToml>(&text) {
            Ok(parsed) => Some(parsed),
            Err(err) => {
                return Backend {
                    name: name.to_owned(),
                    dir,
                    state: BackendState::Broken(format!("{BACKEND_FILE} does not parse: {err}")),
                    entry: None,
                    role: None,
                    vram_gb: None,
                    resident: false,
                    interpreter,
                };
            }
        },
        Err(_) => None,
    };
    let entry = described.as_ref().and_then(|d| d.entry.clone());
    let role = described.as_ref().and_then(|d| d.role.clone());
    let vram_gb = described.as_ref().and_then(|d| d.vram_gb);
    let resident = described.as_ref().and_then(|d| d.resident).unwrap_or(false);
    if let Some(declared) = described.as_ref().and_then(|d| d.name.as_deref())
        && declared != name
    {
        return Backend {
            name: name.to_owned(),
            dir,
            state: BackendState::Broken(format!(
                "{BACKEND_FILE} calls itself {declared:?} but lives in {name}/"
            )),
            entry,
            role,
            vram_gb,
            resident,
            interpreter,
        };
    }
    // A tool backend — Blender — has no environment to install; the Python
    // launcher finds its binary through $BLENDER_BIN or PATH, and doctor's
    // probe says whether it answers. Described is as found as it gets here.
    if role.as_deref() == Some("tool") {
        return Backend {
            name: name.to_owned(),
            dir,
            state: BackendState::Found,
            entry,
            role,
            vram_gb,
            resident,
            interpreter: None,
        };
    }
    // An override may name the interpreter binary itself or its prefix; the
    // `.env` link is always a prefix, so a regular file where the link should
    // be is an install that did not finish.
    let overridden = interpreter.is_some();
    let interpreter = interpreter.or_else(|| {
        let link = dir.join(ENV_LINK);
        link.exists().then_some(link)
    });
    let state = match (&described, &interpreter) {
        (None, _) | (Some(_), None) => BackendState::Missing,
        (Some(_), Some(python)) => {
            let prefix = python.canonicalize().unwrap_or_else(|_| python.clone());
            let usable = (overridden && prefix.is_file())
                || prefix.join("bin").join("python").is_file()
                || prefix.join("bin").join("python3").is_file();
            if usable {
                BackendState::Found
            } else {
                BackendState::Broken(format!(
                    "interpreter {} has no bin/python — the install did not finish, or the \
                     environment moved",
                    python.display()
                ))
            }
        }
    };
    Backend {
        name: name.to_owned(),
        dir,
        state,
        entry,
        role,
        vram_gb,
        resident,
        interpreter,
    }
}

/// Every backend's state as doctor's first table: `name  state  dir`.
#[must_use]
pub fn table(backends: &Backends) -> String {
    use std::fmt::Write as _;
    let mut out = format!("backends: {}\n", backends.origin);
    for backend in &backends.backends {
        let _ = writeln!(
            out,
            "  {:<10} {:<8} {}",
            backend.name,
            backend.state.as_str(),
            match &backend.state {
                BackendState::Broken(why) => why.clone(),
                _ => backend.dir.display().to_string(),
            }
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_project;

    #[test]
    fn a_project_with_no_backends_has_every_backend_missing() {
        let (dir, mut project) = temp_project();
        // An empty backends directory named by the project: the walk from
        // this test binary would otherwise land on the checkout's own
        // backends/, which on a developer's machine are installed.
        let empty = dir.path().join("backends");
        std::fs::create_dir_all(&empty).expect("mkdir");
        project.backends_dir = Some(empty);
        let found = Backends::discover(&project);
        assert_eq!(found.backends.len(), KNOWN.len());
        for backend in &found.backends {
            assert_eq!(backend.state, BackendState::Missing, "{backend:?}");
        }
        assert!(!found.is_found("ardy"));
        assert!(
            found
                .refusal("ardy")
                .contains("generation through it is off")
        );
        assert!(found.refusal("gpt").contains("not a backend"));
        assert_eq!(found.largest_vram_gb(), None);
    }

    #[test]
    fn the_exit_table_round_trips_and_unknown_codes_are_none() {
        for exit in GenExit::ALL {
            assert_eq!(GenExit::from_code(i32::from(exit.code())), Some(exit));
        }
        assert_eq!(GenExit::from_code(1), None);
        assert_eq!(GenExit::from_code(-9), None);
        assert_eq!(GenExit::MissingBackend.to_string(), "missing_backend");
    }

    #[test]
    fn vram_and_role_are_read_and_a_tool_backend_needs_no_env() {
        let (dir, mut project) = temp_project();
        let backends = dir.path().join("backends");
        for (name, text) in [
            (
                "trellis2",
                "name = \"trellis2\"\nrole = \"mesh\"\nentry = \"mesh\"\nvram_gb = 22\n",
            ),
            (
                "acestep",
                "name = \"acestep\"\nrole = \"music\"\nentry = \"audio.music\"\nvram_gb = 8\nresident = true\n",
            ),
            (
                "blender",
                "name = \"blender\"\nrole = \"tool\"\nentry = \"forge_gen.blender\"\nvram_gb = 0\n",
            ),
        ] {
            let sub = backends.join(name);
            std::fs::create_dir_all(&sub).expect("mkdir");
            std::fs::write(sub.join(BACKEND_FILE), text).expect("toml");
        }
        project.backends_dir = Some(backends);
        let found = Backends::discover(&project);
        assert_eq!(found.largest_vram_gb(), Some(22.0));
        assert_eq!(found.largest().map(|b| b.name.as_str()), Some("trellis2"));
        assert!(found.get("acestep").expect("listed").resident);
        let blender = found.get("blender").expect("listed after the known five");
        assert_eq!(
            blender.state,
            BackendState::Found,
            "a tool has no env to install"
        );
        assert_eq!(blender.role.as_deref(), Some("tool"));
        assert_eq!(found.backends.len(), KNOWN.len() + 1);
    }

    #[test]
    fn a_described_backend_with_an_env_link_is_found_and_a_bad_toml_is_broken() {
        let (dir, mut project) = temp_project();
        let backends = dir.path().join("backends");
        let ardy = backends.join("ardy");
        std::fs::create_dir_all(&ardy).expect("mkdir");
        std::fs::write(
            ardy.join(BACKEND_FILE),
            "name = \"ardy\"\nentry = \"motion\"\n",
        )
        .expect("toml");
        project.backends_dir = Some(backends.clone());
        let found = Backends::discover(&project);
        assert_eq!(found.origin, "forge.toml [backends] dir");
        let backend = found.get("ardy").expect("listed");
        assert_eq!(
            backend.state,
            BackendState::Missing,
            "described but not installed"
        );
        assert_eq!(backend.entry.as_deref(), Some("motion"));

        // A fake prefix with bin/python, linked as .env.
        let prefix = dir.path().join("venv");
        std::fs::create_dir_all(prefix.join("bin")).expect("mkdir");
        std::fs::write(prefix.join("bin/python"), b"#!/bin/sh\n").expect("python");
        std::fs::write(ardy.join(ENV_LINK), prefix.display().to_string()).expect("env");
        // A plain file where the symlink should be is an install that did
        // not finish: the check looks for bin/python under the link…
        let found = Backends::discover(&project);
        assert!(matches!(
            found.get("ardy").expect("listed").state,
            BackendState::Broken(_)
        ));
        // …so make it a real link.
        std::fs::remove_file(ardy.join(ENV_LINK)).expect("rm");
        std::os::unix::fs::symlink(&prefix, ardy.join(ENV_LINK)).expect("symlink");
        let found = Backends::discover(&project);
        assert!(found.is_found("ardy"), "{:?}", found.get("ardy"));
        assert!(table(&found).contains("ardy       found"));

        std::fs::write(ardy.join(BACKEND_FILE), "name = [").expect("bad toml");
        let found = Backends::discover(&project);
        assert!(matches!(
            found.get("ardy").expect("listed").state,
            BackendState::Broken(_)
        ));
    }

    #[test]
    fn the_launcher_needs_a_toolkit_root() {
        let toolkit = Project::discover(Path::new(env!("CARGO_MANIFEST_DIR"))).expect("toolkit");
        // The checkout is the toolkit: python/forge_gen is beside this crate.
        let has_python = toolkit.root.join("python/forge_gen").is_dir();
        assert_eq!(Backends::python_launcher(&toolkit).is_some(), has_python);
        let command = Backends::python_launcher(&toolkit).expect("a checkout");
        let program = command.get_program().to_string_lossy().into_owned();
        assert_eq!(program, "python3");
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args[0].ends_with("python/forge_gen"), "{args:?}");
    }
}
