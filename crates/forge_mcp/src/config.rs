//! Where the project is and which binary renders, worked out once at
//! startup.
//!
//! The project is [`forge_library::Project`] — the same `forge.toml` walk
//! every other `forge` verb does — so an agent and a human at the terminal
//! are always looking at one library. What is specific to being an MCP
//! server is here: the renderer is *this executable* re-invoked, and the
//! project may arrive by flag, by environment or by discovery.
//!
//! # The exe-relative rule
//!
//! An MCP client launches the server from wherever it likes, with whatever
//! working directory it has, and `PATH` is the client's. A renderer looked
//! up by name would find a stale system install or nothing; one looked up
//! relative to the working directory would find nothing at all. So the
//! renderer is [`std::env::current_exe`]: `forge mcp` re-invokes `forge
//! sheet`, `forge views`, `forge doctor` — one binary, one build, and a
//! render that cannot disagree with what the human ran by hand a minute
//! earlier.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use forge_library::backends::Backends;
use forge_library::project::PROJECT_FILE;
use forge_library::{LibraryError, Project};

/// The environment variable naming the project root when no `--project`
/// flag does.
pub const PROJECT_ENV: &str = "FORGE_PROJECT";

/// What the server needs to know before it answers anything.
#[derive(Debug, Clone)]
pub struct Config {
    /// The project: root, asset directories, rig profile, scratch.
    ///
    /// When [`Self::project_found`] is false this is a *provisional* one —
    /// the conventional layout under the directory the server was pointed
    /// at, held in memory and written nowhere.
    pub project: Project,
    /// Whether a `forge.toml` was actually found.
    ///
    /// `false` is a session in a directory that is not a project yet, and
    /// the server serves `init_project`, `licences` and `doctor` there and
    /// refuses every other tool naming the first. It used to refuse to
    /// *start*: exit 2 with `no forge.toml in …`, stdout closed before the
    /// handshake — so the one tool that makes a project was reachable only
    /// from a server already bound to a different project, and the hint a
    /// client with no shell got was a shell command (2026-08-30).
    pub project_found: bool,
    /// The binary that renders — this one, re-invoked with `sheet`, `views`
    /// or `doctor --json`. See the module note for why it is never looked
    /// up by name.
    pub renderer: PathBuf,
    /// The body clips are posed on by default: `[studio] stage_body` in
    /// `forge.toml`. `None` lets the renderer pick the first body, which
    /// it says out loud on stderr.
    pub stage_body: Option<String>,
    /// The toolkit checkout holding `python/forge_gen`, when one can be
    /// found from this executable or the environment. `None` means every
    /// generator is off and `doctor` says so.
    pub toolkit: Option<PathBuf>,
}

/// Why the server could not be configured. All of these are launch
/// mistakes a human fixes in `.mcp.json` or the shell, so each renders as
/// one line of prose for stderr.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// A flag that is not `--project`, or `--project` with nothing after it.
    Usage(String),
    /// The directory named by `--project` or [`PROJECT_ENV`] holds no
    /// `forge.toml`.
    NotAProject(PathBuf),
    /// No flag, no environment, and no `forge.toml` above the working
    /// directory.
    NoProject(String),
    /// `forge.toml` exists and does not load.
    Project(LibraryError),
    /// The operating system would not say where this executable is.
    NoExecutable(std::io::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(detail) => write!(f, "{detail} — usage: forge mcp [--project <DIR>]"),
            Self::NotAProject(dir) => write!(
                f,
                "no {PROJECT_FILE} in {} — pass the project root, the directory that holds it",
                dir.display()
            ),
            Self::NoProject(detail) => write!(
                f,
                "{detail} — pass --project <DIR>, set {PROJECT_ENV}, or launch from inside a project"
            ),
            Self::Project(err) => write!(f, "{err}"),
            Self::NoExecutable(err) => write!(f, "cannot locate own executable: {err}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Project(err) => Some(err),
            Self::NoExecutable(err) => Some(err),
            Self::Usage(_) | Self::NotAProject(_) | Self::NoProject(_) => None,
        }
    }
}

impl From<LibraryError> for ConfigError {
    fn from(err: LibraryError) -> Self {
        match err {
            LibraryError::NoProject { start } => {
                Self::NoProject(format!("no {PROJECT_FILE} above {}", start.display()))
            }
            other => Self::Project(other),
        }
    }
}

impl Config {
    /// Read the server's own command line: `[--project <DIR>]`, and nothing
    /// else.
    ///
    /// Precedence: the flag, then [`PROJECT_ENV`], then the walk up from the
    /// working directory to the first `forge.toml`. The flag and the
    /// variable must name the root itself — a value that named a directory
    /// and then silently used its parent would be a value that sometimes
    /// means something else.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`]: an unknown flag, a root with no `forge.toml`,
    /// no project anywhere, a `forge.toml` that does not load, or an
    /// executable the OS cannot name.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, ConfigError> {
        let flag = parse_project_flag(args)?;
        let env = std::env::var_os(PROJECT_ENV);
        let cwd = std::env::current_dir().map_err(|e| {
            ConfigError::NoProject(format!("the working directory cannot be read: {e}"))
        })?;
        let project = resolve_project(flag.as_deref(), env.as_deref(), &cwd)?;
        Self::for_project(project)
    }

    /// Configure the server for a project already in hand — what the
    /// `forge` binary does, having found the project the way every other
    /// verb does.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NoExecutable`] when the OS will not say where this
    /// binary is; nothing else can fail.
    pub fn for_project(project: Project) -> Result<Self, ConfigError> {
        let renderer = std::env::current_exe().map_err(ConfigError::NoExecutable)?;
        Ok(Self::with_renderer(project, renderer))
    }

    /// Configure the server for a directory that holds no project.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NoExecutable`] when the OS will not say where this
    /// binary is.
    pub fn for_no_project(root: &Path) -> Result<Self, ConfigError> {
        let renderer = std::env::current_exe().map_err(ConfigError::NoExecutable)?;
        Ok(Self {
            project_found: false,
            ..Self::with_renderer(Project::provisional(root), renderer)
        })
    }

    /// [`Self::for_project`] with the renderer stated — for a test that
    /// must not spawn anything, and for a harness that wants a different
    /// build to draw.
    #[must_use]
    pub fn with_renderer(project: Project, renderer: PathBuf) -> Self {
        let stage_body = project.stage_body.clone();
        let toolkit = Backends::toolkit_root(&project);
        Self {
            project,
            project_found: true,
            renderer,
            stage_body,
            toolkit,
        }
    }

    /// Where this server writes the PNGs it inlines: `<out>/mcp/<name>.png`.
    ///
    /// Under the project's scratch rather than the system temp directory, so
    /// a sheet an agent was told the path of is still there when the human
    /// opens it, and so `out/` being gitignored covers it.
    #[must_use]
    pub fn scratch_png(&self, name: &str) -> PathBuf {
        self.project.out.join("mcp").join(format!("{name}.png"))
    }

    /// The one-line description of this configuration for stderr at
    /// startup.
    #[must_use]
    pub fn banner(&self) -> String {
        if !self.project_found {
            return format!(
                "forge mcp: no forge.toml at {} — serving init_project, licences and doctor; \
                 every other tool refuses until this is a project",
                self.project.root.display()
            );
        }
        format!(
            "forge mcp: project={} assets={} renderer={} stage_body={} toolkit={}",
            self.project.root.display(),
            self.project.assets.display(),
            self.renderer.display(),
            self.stage_body.as_deref().unwrap_or("(first body)"),
            self.toolkit.as_deref().map_or_else(
                || String::from("(none: generation is off)"),
                |t| t.display().to_string()
            )
        )
    }
}

/// The `--project` value from the server's argument list, refusing anything
/// else: the flag table is one entry long and a typo should say so.
fn parse_project_flag(
    args: impl IntoIterator<Item = String>,
) -> Result<Option<PathBuf>, ConfigError> {
    let mut args = args.into_iter();
    let mut project = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => {
                let value = args
                    .next()
                    .ok_or_else(|| ConfigError::Usage(String::from("--project needs a value")))?;
                project = Some(PathBuf::from(value));
            }
            other => {
                if let Some(value) = other.strip_prefix("--project=") {
                    project = Some(PathBuf::from(value));
                } else {
                    return Err(ConfigError::Usage(format!("unknown argument {other:?}")));
                }
            }
        }
    }
    Ok(project)
}

/// Settle the project from the three sources, in precedence order. Pure, so
/// the precedence is testable without touching the process environment.
fn resolve_project(
    flag: Option<&Path>,
    env: Option<&OsStr>,
    cwd: &Path,
) -> Result<Project, ConfigError> {
    if let Some(dir) = flag {
        return load_root(&absolute(dir, cwd));
    }
    if let Some(dir) = env.filter(|v| !v.is_empty()) {
        return load_root(&absolute(Path::new(dir), cwd));
    }
    Ok(Project::discover(cwd)?)
}

/// Load the project whose root is exactly `dir`.
fn load_root(dir: &Path) -> Result<Project, ConfigError> {
    if !dir.join(PROJECT_FILE).is_file() {
        return Err(ConfigError::NotAProject(dir.to_path_buf()));
    }
    Ok(Project::load(dir)?)
}

/// A path as given, made absolute against `cwd` when it is not: every
/// subprocess this server spawns gets the project root as an argument, and
/// a relative one would be resolved against *that* process's directory.
fn absolute(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_at(dir: &Path, name: &str) -> Project {
        Project::init(dir, name).expect("init")
    }

    #[test]
    fn the_flag_wins_over_the_environment_which_wins_over_discovery() {
        let flagged = tempfile::tempdir().expect("tempdir");
        let from_env = tempfile::tempdir().expect("tempdir");
        let discovered = tempfile::tempdir().expect("tempdir");
        project_at(flagged.path(), "flagged");
        project_at(from_env.path(), "from_env");
        let inner = project_at(discovered.path(), "discovered");
        let cwd = inner.kind_dir(forge_library::Kind::Sfx);

        let env = from_env.path().as_os_str();
        let project = resolve_project(Some(flagged.path()), Some(env), &cwd).expect("flag");
        assert_eq!(project.name, "flagged");
        let project = resolve_project(None, Some(env), &cwd).expect("env");
        assert_eq!(project.name, "from_env");
        let project = resolve_project(None, None, &cwd).expect("discovery");
        assert_eq!(project.name, "discovered");
        // An empty variable is an unset one: a shell that exported nothing
        // should not turn discovery off.
        let project = resolve_project(None, Some(OsStr::new("")), &cwd).expect("empty env");
        assert_eq!(project.name, "discovered");
    }

    #[test]
    fn a_named_root_must_be_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = project_at(dir.path(), "p");
        let under = project.kind_dir(forge_library::Kind::Clip);
        let error = resolve_project(Some(&under), None, dir.path()).expect_err("refused");
        assert!(matches!(error, ConfigError::NotAProject(_)), "{error}");
        assert!(error.to_string().contains("forge.toml"), "{error}");
    }

    #[test]
    fn a_relative_flag_is_resolved_against_the_working_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        project_at(&dir.path().join("game"), "game");
        let project =
            resolve_project(Some(Path::new("game")), None, dir.path()).expect("relative flag");
        assert_eq!(project.name, "game");
        assert!(project.root.is_absolute());
    }

    #[test]
    fn nowhere_to_look_is_said_with_the_three_ways_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_project(None, None, dir.path()).expect_err("no project");
        let text = error.to_string();
        assert!(text.contains("--project"), "{text}");
        assert!(text.contains(PROJECT_ENV), "{text}");
    }

    #[test]
    fn the_flag_table_is_one_entry_long() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_project_flag(args(&["--project", "/x"])).expect("flag"),
            Some(PathBuf::from("/x"))
        );
        assert_eq!(
            parse_project_flag(args(&["--project=/y"])).expect("flag"),
            Some(PathBuf::from("/y"))
        );
        assert_eq!(parse_project_flag(args(&[])).expect("none"), None);
        assert!(parse_project_flag(args(&["--project"])).is_err());
        let error = parse_project_flag(args(&["--assets", "a"])).expect_err("unknown");
        assert!(error.to_string().contains("--assets"), "{error}");
    }

    #[test]
    fn the_config_carries_the_stage_body_and_the_scratch_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(PROJECT_FILE),
            "[project]\nname = \"x\"\nlibrary_version = \"0.1.0\"\n[studio]\nstage_body = \"vex\"\n",
        )
        .expect("write");
        let project = Project::load(dir.path()).expect("load");
        let config = Config::with_renderer(project, PathBuf::from("/bin/forge"));
        assert_eq!(config.stage_body.as_deref(), Some("vex"));
        assert_eq!(
            config.scratch_png("walk"),
            dir.path().join("out/mcp/walk.png")
        );
        assert!(
            config.banner().contains("stage_body=vex"),
            "{}",
            config.banner()
        );
    }
}
