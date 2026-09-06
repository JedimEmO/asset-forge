//! `forge mcp`: the MCP server an agent drives, over this process's stdin
//! and stdout.
//!
//! The binary's part is small on purpose. It finds the project the way every
//! other verb does — `--project`, else `$FORGE_PROJECT`, else the walk up
//! from the working directory — hands it to [`forge_mcp::Config`], which
//! sets the renderer to *this executable*, and awaits [`forge_mcp::serve`]
//! on a runtime. Everything the server then does — a contact sheet, a
//! doctor table, a motion sweep — is this same binary re-invoked with
//! `sheet`, `views`, `gen` or `doctor`, so an agent's picture and a human's
//! cannot disagree.
//!
//! The queue it serves is found the same way `forge gen` finds one: a
//! daemon's if `forge serve` is up for this project, else a `LocalQueue`
//! in this process — a queue of one, taking the same `card.lock` as every
//! other door. The server never learns which it holds, which is what makes
//! a stranger's first session and a busy machine's tenth one code path.
//!
//! From the moment `serve` is called, stdout is the JSON-RPC stream and
//! nothing else. The one thing printed here is the failure line, and it
//! goes to stderr through the usual exit path.

use std::path::PathBuf;

use forge_library::Project;
use forge_mcp::{Config, PROJECT_ENV};

use crate::cli::Cli;
use crate::outcome::{Failure, Outcome};

/// Serve until the client hangs up.
///
/// **A directory with no `forge.toml` is a session, not a refusal.** This
/// used to exit 2 with `no forge.toml in <dir>` before the handshake, which
/// made `init_project` — the one tool that makes a project — reachable only
/// from a server already bound to some *other* project, and handed a client
/// with no shell a shell command as its way out (2026-08-30). With no
/// project found the server starts anyway over a
/// [`forge_serve::NoQueue`], serving `init_project`, `licences` and
/// `doctor` and refusing every other tool with a frame that names the
/// first.
pub(crate) fn run(cli: &Cli) -> Outcome {
    let named = match std::env::var_os(PROJECT_ENV) {
        // The flag still wins: the environment is the launcher's default,
        // the flag is what this call said.
        Some(dir) if cli.project.is_none() && !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => cli.project.clone(),
    };
    // Absent is a session; **broken is still an error**. A `forge.toml`
    // that does not parse must not read as "there is no project here" —
    // that would answer a typo with a fresh empty session.
    let found = match &named {
        Some(dir) if dir.join(forge_library::project::PROJECT_FILE).is_file() => {
            Some(crate::load_root(dir)?)
        }
        Some(_) => None,
        None => match Project::discover(&crate::cwd()?) {
            Ok(project) => Some(project),
            Err(forge_library::LibraryError::NoProject { .. }) => None,
            Err(other) => return Err(other.into()),
        },
    };
    let Some(project) = found else {
        let root = match &named {
            Some(dir) => std::path::absolute(dir).unwrap_or_else(|_| dir.clone()),
            None => crate::cwd()?,
        };
        let config = Config::for_no_project(&root).map_err(|e| Failure::refused(e.to_string()))?;
        return serve_with(config, std::sync::Arc::new(forge_serve::NoQueue));
    };
    let queue = crate::commands::generate::queue_for(&project)?;
    let config = Config::for_project(project).map_err(|e| Failure::refused(e.to_string()))?;
    serve_with(config, queue)
}

/// Serve one configuration over stdio until the client hangs up.
fn serve_with(config: Config, queue: std::sync::Arc<dyn forge_serve::Queue>) -> Outcome {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(format!("cannot start the async runtime: {e}")))?;
    runtime
        .block_on(forge_mcp::serve(config, queue))
        .map_err(|e| Failure::failed(e.to_string()))
}
