//! Open the studio window on a project, the way `forge studio` will.
//!
//! ```sh
//! cargo run -p forge_studio --example studio_window -- --project . [--model vex_runner]
//!     [--audio] [--take out/sweeps/x.npz [--recipe r.json]] [--screenshot out/studio.png]
//!     [--selftest]
//! ```
//!
//! A hand-rolled flag loop rather than clap: the one binary carries the real
//! command tree, and this exists so the window can be driven before that
//! wiring lands — and after, as the shortest path to it for a test under Xvfb.

use std::path::PathBuf;

use forge_library::Project;
use forge_studio::studio::{StudioConfig, run_studio};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return std::process::ExitCode::SUCCESS;
    }

    let mut start = PathBuf::from(".");
    let mut model: Option<String> = None;
    let mut take: Option<PathBuf> = None;
    let mut recipe: Option<PathBuf> = None;
    let mut screenshot: Option<PathBuf> = None;
    let mut audio = false;
    let mut selftest = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = || {
            args.get(i + 1).cloned().ok_or_else(|| {
                eprintln!("error: {flag} needs a value");
                std::process::ExitCode::from(2)
            })
        };
        match flag {
            "--audio" => {
                audio = true;
                i += 1;
                continue;
            }
            "--selftest" => {
                selftest = true;
                i += 1;
                continue;
            }
            "--project" => match value() {
                Ok(v) => start = PathBuf::from(v),
                Err(code) => return code,
            },
            "--model" => match value() {
                Ok(v) => model = Some(v),
                Err(code) => return code,
            },
            "--take" => match value() {
                Ok(v) => take = Some(PathBuf::from(v)),
                Err(code) => return code,
            },
            "--recipe" => match value() {
                Ok(v) => recipe = Some(PathBuf::from(v)),
                Err(code) => return code,
            },
            "--screenshot" => match value() {
                Ok(v) => screenshot = Some(PathBuf::from(v)),
                Err(code) => return code,
            },
            other => {
                eprintln!("error: unknown argument '{other}'");
                print_usage();
                return std::process::ExitCode::from(2);
            }
        }
        i += 2;
    }

    let project = match Project::discover(&start) {
        Ok(project) => project,
        Err(err) => {
            eprintln!("error: {err}");
            return std::process::ExitCode::from(2);
        }
    };
    // Relative to the shell, like every other path on a command line — and
    // made absolute here because the window's asset root is elsewhere.
    let absolute = |path: PathBuf| std::path::absolute(&path).unwrap_or(path);
    run_studio(StudioConfig {
        project,
        model,
        audio,
        take: take.map(absolute),
        recipe: recipe.map(absolute),
        screenshot: screenshot.map(absolute),
        selftest,
    })
}

fn print_usage() {
    println!(
        "open the studio window on a project\n\
         \n\
         usage: studio_window [--project <dir>] [options]\n\
         \n\
         options:\n  \
           --project <dir>    a directory inside the project (default: .)\n  \
           --model <name>     the body or model to open on (default: the project's stage body)\n  \
           --audio            open on the first sound rather than the first clip\n  \
           --take <npz>       put a raw take on the stage body\n  \
           --recipe <json>    apply this recipe to the take once, at load\n  \
           --screenshot <png> capture the window after it settles, then quit\n  \
           --selftest         play every audio asset in turn, then quit"
    );
}
