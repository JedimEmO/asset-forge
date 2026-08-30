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

use std::path::Path;

use forge_mcp::{Config, PROJECT_ENV};

use crate::cli::Cli;
use crate::outcome::{Failure, Outcome};

/// Serve until the client hangs up.
pub(crate) fn run(cli: &Cli) -> Outcome {
    let project = match std::env::var_os(PROJECT_ENV) {
        // The flag still wins: the environment is the launcher's default,
        // the flag is what this call said.
        Some(dir) if cli.project.is_none() && !dir.is_empty() => crate::load_root(Path::new(&dir))?,
        _ => crate::project(cli)?,
    };
    let queue = crate::commands::generate::queue_for(&project)?;
    let config = Config::for_project(project).map_err(|e| Failure::refused(e.to_string()))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(format!("cannot start the async runtime: {e}")))?;
    runtime
        .block_on(forge_mcp::serve(config, queue))
        .map_err(|e| Failure::failed(e.to_string()))
}
