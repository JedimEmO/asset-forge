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
//! the directory holding `python/forge_gen` found by walking up from the
//! running executable; none. No absolute path lives in source.
//!
//! # Doctor
//!
//! `forge doctor` aggregates `forge gen doctor --json` (the in-environment
//! probe: imports, torch, CUDA, weights, licence notices) with the host
//! checks (GPU, Blender, ffmpeg, profile drift, library counts). That
//! aggregation lands in P2 with the Python layer; the types here are what it
//! reads the directory through, and [`Backends::python_launcher`] is the
//! command it runs.

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
pub const TOOLKIT_ENV: &str = "FORGE_TOOLKIT";

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    /// The directory's name, which is the backend's.
    pub name: String,
    /// Where it lives (or would).
    pub dir: PathBuf,
    /// Whether it can be used.
    pub state: BackendState,
    /// The `forge_gen` entry module `backend.toml` names, when it parsed.
    pub entry: Option<String>,
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
}

/// The backends directory and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// running somewhere that is not a checkout and [`TOOLKIT_ENV`] is not
    /// set — in which case generation is off and doctor says so.
    #[must_use]
    pub fn python_launcher(project: &Project) -> Option<Command> {
        let toolkit = toolkit_root(project)?;
        let mut command = Command::new("python3");
        command.arg(toolkit.join("python").join("forge_gen"));
        command.current_dir(&project.root);
        Some(command)
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

/// The directory holding `python/forge_gen`: the project itself when it is
/// the toolkit, else [`TOOLKIT_ENV`], else an ancestor of the running
/// executable (a checkout's `target/debug/forge` is two levels under it).
fn toolkit_root(project: &Project) -> Option<PathBuf> {
    let is_toolkit = |dir: &Path| dir.join("python").join("forge_gen").is_dir();
    if is_toolkit(&project.root) {
        return Some(project.root.clone());
    }
    if let Some(dir) = std::env::var_os(TOOLKIT_ENV) {
        let dir = PathBuf::from(dir);
        if is_toolkit(&dir) {
            return Some(dir);
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
    let override_key = format!("FORGE_BACKEND_{}_PYTHON", name.to_ascii_uppercase());
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
                    interpreter,
                };
            }
        },
        Err(_) => None,
    };
    let entry = described.as_ref().and_then(|d| d.entry.clone());
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
            interpreter,
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
        let (_dir, project) = temp_project();
        // The temp project is not the toolkit, names no backends dir, and
        // this test binary lives under the toolkit's target/ — so the walk
        // from the executable finds the checkout's backends/, which in P1
        // holds no backend.toml yet. Either way: nothing is found.
        let found = Backends::discover(&project);
        assert_eq!(found.backends.len(), KNOWN.len());
        for backend in &found.backends {
            assert_ne!(backend.state, BackendState::Found, "{backend:?}");
        }
        assert!(!found.is_found("ardy"));
        assert!(
            found
                .refusal("ardy")
                .contains("generation through it is off")
        );
        assert!(found.refusal("gpt").contains("not a backend"));
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
        // In P1 python/forge_gen does not exist yet, so the toolkit root
        // cannot be found from the checkout either and the launcher is off.
        let has_python = toolkit.root.join("python/forge_gen").is_dir();
        assert_eq!(Backends::python_launcher(&toolkit).is_some(), has_python);
    }
}
