//! `forge` — the one command line over the library crates.
//!
//! Every verb here is wiring and printing: the project is found, the flags
//! are turned into a request, a library crate does the work, the result is
//! printed and becomes an exit code. No command has logic of its own, which
//! is what keeps this binary, the MCP server and the studio from disagreeing
//! about what a promote does or what `verify` checks. `forge mcp` is the
//! same binary serving the same library to an agent: every render and
//! generate it answers is this executable re-invoked with `sheet`, `views`,
//! `gen` or `doctor`, never a second build.
//!
//! ```text
//! forge init [--name]                       make a project here
//! forge catalog [--kind] [--filter] [--tag] what the library holds
//! forge manifest [--check]                  project the library into assets/library.json
//! forge verify | audit | rebake | migrate   the engine-free checks and repairs
//! forge promote clip|body|model|audio ...   the four doors into the library
//! forge audio inspect|list                  measure a sound, or every sound
//! forge rig export-contract|fixture|check   the profile's contract and mannequin; one mesh held to it
//! forge gen <cmd> [args…]                   a generator, through python/forge_gen
//! forge doctor [--json]                     what this machine can do, every backend probed
//! forge gpu [--json]                        who holds the card, and whether the largest backend fits
//! forge sheet <clip> [--body]               a clip on a body as a contact sheet
//! forge views <name|path.glb>               one mesh from every angle, culling off for a lift
//! forge turntable <body>                    every view of a body, posed on the reference clip
//! forge bones <clip> [--body]               which bones a clip drives, no GPU
//! forge studio [--model] [--audio] …        the viewer window
//! forge mcp                                 serve the MCP tools over stdio, for an agent
//! ```
//!
//! # Exit codes
//!
//! - `0` — it worked, or a check passed.
//! - `1` — a gate did not hold: `verify`, `audit`, `manifest --check`, an
//!   audio file that is silent or clipped, a bake that failed, a doctor
//!   with a backend that is not ok, a card without room for the largest
//!   backend.
//! - `2` — the call could not be honoured as written: a flag that does not
//!   parse, a file that is not there, a name already in use, no project
//!   above the working directory. A refusal says what does exist.
//! - `3`–`6` — `forge gen` relaying the Python layer's own table: missing
//!   backend, input rejected, backend failed, missing tool.
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
        Command::Audit(args) => commands::checks::audit(&project(cli)?, args.fit),
        Command::Rebake(args) => commands::checks::rebake(&project(cli)?, args.dry_run),
        Command::Migrate(args) => commands::checks::migrate(&project(cli)?, args.dry_run),
        Command::Promote(door) => commands::promote::run(&project(cli)?, door),
        Command::Audio(args) => commands::audio::run(cli, args),
        Command::Rig(args) => commands::rig::run(cli, args),
        Command::Gen(args) => commands::generate::run(&project(cli)?, args),
        Command::Doctor(args) => commands::doctor::run(&project(cli)?, args),
        Command::Gpu(args) => commands::gpu::run(&project(cli)?, args),
        Command::Sheet(args) => commands::look::sheet(&project(cli)?, args),
        Command::Views(args) => commands::look::views(&project(cli)?, args),
        Command::Turntable(args) => commands::look::turntable(&project(cli)?, args),
        Command::Bones(args) => commands::look::bones(&project(cli)?, args),
        Command::Studio(args) => commands::studio::run(&project(cli)?, args),
        Command::Mcp => commands::mcp::run(cli),
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
