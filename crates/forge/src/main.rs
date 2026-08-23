//! `forge` — the one command line over the library crates.
//!
//! Every verb here is wiring and printing: the project is found, the flags
//! are turned into a request, a library crate does the work, the result is
//! printed and becomes an exit code. No command has logic of its own, which
//! is what keeps this binary, the MCP server and the studio from disagreeing
//! about what a promote does or what `verify` checks.
//!
//! ```text
//! forge init [--name]                       make a project here
//! forge catalog [--kind] [--filter] [--tag] what the library holds
//! forge manifest [--check]                  project the library into assets/library.json
//! forge verify | audit | rebake | migrate   the engine-free checks and repairs
//! forge promote clip|body|model|audio ...   the four doors into the library
//! forge audio inspect|list                  measure a sound, or every sound
//! forge rig export-contract|fixture         the profile's contract and mannequin
//! forge doctor                              what this machine can do
//! forge studio | mcp | gpu                  not yet: P3, P4, P2
//! ```
//!
//! # Exit codes
//!
//! - `0` — it worked, or a check passed.
//! - `1` — a gate did not hold: `verify`, `audit`, `manifest --check`, an
//!   audio file that is silent or clipped, a bake that failed.
//! - `2` — the call could not be honoured as written: a flag that does not
//!   parse, a file that is not there, a name already in use, no project
//!   above the working directory. A refusal says what does exist.
//!
//! # The project
//!
//! Every command that touches a library finds it the same way: `--project <dir>`
//! names the root, otherwise the walk up from the working directory
//! stops at the first `forge.toml`. `forge init` is the one command that
//! expects not to find one.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use forge_library::Project;

mod cli;
mod commands;
mod outcome;
mod toolkit;

use cli::{Cli, Command};
use outcome::{Failure, Outcome};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("forge: {}", failure.message());
            failure.code()
        }
    }
}

/// Dispatch one parsed command line.
fn run(cli: &Cli) -> Outcome {
    match &cli.command {
        Command::Init(args) => commands::init::run(cli.project.as_deref(), args),
        Command::Catalog(args) => commands::catalog::run(&project(cli)?, args),
        Command::Manifest(args) => commands::manifest::run(&project(cli)?, args),
        Command::Verify => commands::checks::verify(&project(cli)?),
        Command::Audit => commands::checks::audit(&project(cli)?),
        Command::Rebake(args) => commands::checks::rebake(&project(cli)?, args.dry_run),
        Command::Migrate(args) => commands::checks::migrate(&project(cli)?, args.dry_run),
        Command::Promote(door) => commands::promote::run(&project(cli)?, door),
        Command::Audio(args) => commands::audio::run(cli, args),
        Command::Rig(args) => commands::rig::run(cli, args),
        Command::Doctor => commands::doctor::run(&project(cli)?),
        Command::Gpu(_) => Err(Failure::later("P2", "forge gpu over nvidia-smi")),
        Command::Studio(_) => Err(Failure::later("P3", "forge studio, the viewer window")),
        Command::Mcp(_) => Err(Failure::later(
            "P4",
            "forge mcp, the server an agent drives",
        )),
    }
}

/// The project a command works on: `--project` when given, else the walk
/// up from the working directory.
///
/// `--project` has to *be* the root rather than be under it — a flag that
/// named a directory and then silently used its parent would be a flag that
/// sometimes means something else.
fn project(cli: &Cli) -> Result<Project, Failure> {
    if let Some(dir) = &cli.project {
        return load_root(dir);
    }
    let here = cwd()?;
    Ok(Project::discover(&here)?)
}

/// Load the project whose root is exactly `dir`.
fn load_root(dir: &Path) -> Result<Project, Failure> {
    if !dir.join(forge_library::project::PROJECT_FILE).is_file() {
        return Err(Failure::refused(format!(
            "no {} in {} — `forge init --project {}` makes one, or pass the project root",
            forge_library::project::PROJECT_FILE,
            dir.display(),
            dir.display()
        )));
    }
    Ok(Project::load(dir)?)
}

/// The working directory, named in the error when it cannot be read.
fn cwd() -> Result<PathBuf, Failure> {
    std::env::current_dir()
        .map_err(|e| Failure::refused(format!("the working directory cannot be read: {e}")))
}
