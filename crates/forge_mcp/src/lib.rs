//! The MCP server an agent drives: look at the library, run a generator,
//! ship a clip or a sound — served over stdio by `forge mcp`.
//!
//! ```text
//! { "mcpServers": { "forge": { "command": "./target/debug/forge", "args": ["mcp"] } } }
//! ```
//!
//! This is a library crate with one entry point, [`serve`] (and its
//! blocking twin [`run`]), so the `forge` binary owns the command line and
//! this crate owns the protocol. Every render shells out to that same
//! binary — [`Config::renderer`] is `current_exe()` — so Bevy is linked
//! once, the server builds in seconds, and a sheet an agent sees is the
//! sheet `forge sheet` would have drawn.
//!
//! # Three rules this server lives by
//!
//! **stdout is the JSON-RPC frame stream and nothing else.** Every
//! diagnostic goes to stderr. A stray `println!` corrupts the protocol and
//! the failure looks like the server crashing for no reason — which is why
//! [`serve`] installs a panic hook that reports on stderr.
//!
//! **Refusals are successful frames.** A clip that does not exist comes
//! back as an error *result* carrying the list of clips that *do*, so the
//! agent fixes its own call next turn. An `Err(ErrorData)` would be
//! rendered opaquely by the client and teach it nothing.
//!
//! **The server never decides what ships without a human.** There is no
//! review queue: `promote_clip` and `promote_audio` write the library
//! directly, and so they refuse a name that is already taken unless told
//! `overwrite` — a replacement is a decision, never an accident. And there
//! is no promote for a mesh at all: a body or a model goes through the
//! genart skills, where a human looks at the lift, the rig and the views
//! before anything is filed.
//!
//! # Layout
//!
//! ```text
//! lib.rs       this: serve over stdio, and the error a caller sees
//! config.rs    where the project is, and the exe-relative renderer rule
//! server.rs    the state, the instructions text, the sum of the routers
//! util.rs      refusals, inline images, supervised subprocesses
//! tools/       one file per verb: list, render, audio, doctor, generate, promote
//! ```

use std::fmt;
use std::sync::Arc;

use forge_serve::Queue;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

mod config;
mod server;
mod tools;
mod util;

pub use config::{Config, ConfigError, PROJECT_ENV};

/// Why serving stopped before the client hung up.
///
/// Every rmcp type stays inside this crate — the workspace pins rmcp
/// because its macros churn between minors, and a bump has to be one
/// crate's problem — so what the caller gets is the phase that failed and
/// the message, not the SDK's enum.
#[derive(Debug)]
#[non_exhaustive]
pub enum ServeError {
    /// The client never completed the initialize handshake, or the
    /// transport failed during it.
    Handshake(String),
    /// The session task ended abnormally — a panic inside a tool, or a
    /// cancelled runtime.
    Session(String),
    /// A tokio runtime could not be built, for [`run`].
    Runtime(std::io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handshake(detail) => write!(f, "MCP handshake failed: {detail}"),
            Self::Session(detail) => write!(f, "MCP session ended abnormally: {detail}"),
            Self::Runtime(err) => write!(f, "cannot start the async runtime: {err}"),
        }
    }
}

impl std::error::Error for ServeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(err) => Some(err),
            Self::Handshake(_) | Self::Session(_) => None,
        }
    }
}

/// Serve the tools over this process's stdin and stdout until the client
/// closes the connection.
///
/// From the moment this is called, stdout belongs to the protocol: the
/// configuration banner goes to stderr, and a panic anywhere in the
/// process is reported there too rather than where the default hook would
/// put it.
///
/// # Errors
///
/// [`ServeError::Handshake`] when the client never initialises,
/// [`ServeError::Session`] when the session task ends abnormally. A client
/// that simply hangs up is not an error.
pub async fn serve(config: Config, queue: Arc<dyn Queue>) -> Result<(), ServeError> {
    install_panic_hook();
    eprintln!("{}", config.banner());
    let service = server::ForgeServer::new(config, queue)
        .serve(stdio())
        .await
        .map_err(|e| ServeError::Handshake(e.to_string()))?;
    service
        .waiting()
        .await
        .map_err(|e| ServeError::Session(e.to_string()))?;
    Ok(())
}

/// [`serve`] on a runtime of its own — what a synchronous `main` calls.
///
/// # Errors
///
/// [`ServeError::Runtime`] when tokio will not start, else as [`serve`].
pub fn run(config: Config, queue: Arc<dyn Queue>) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?;
    runtime.block_on(serve(config, queue))
}

/// One server, ready to answer — what a transport wraps.
///
/// The `forge` binary never names an rmcp type: it hands over a [`Config`]
/// and an `Arc<dyn Queue>` and gets back something that serves, whichever
/// door it came through.
#[must_use]
pub fn handler(config: Config, queue: Arc<dyn Queue>) -> impl rmcp::ServerHandler + 'static {
    server::ForgeServer::new(config, queue)
}

/// The same tools over streamable HTTP, as a router the daemon nests at
/// `/mcp`.
///
/// **The router does not move and is not duplicated.** This is the same
/// `ForgeServer`, the same tool sum and the same instructions text `forge
/// mcp` serves over stdio, holding the same queue the daemon's worker is
/// draining. Same tools, same frames, whichever door the client came
/// through.
pub fn http_service(config: Config, queue: Arc<dyn Queue>) -> axum::Router {
    use rmcp::transport::streamable_http_server::StreamableHttpService;
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

    let service = StreamableHttpService::new(
        move || Ok(server::ForgeServer::new(config.clone(), Arc::clone(&queue))),
        LocalSessionManager::default().into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );
    axum::Router::new().fallback_service(service)
}

/// Report panics on stderr. A panic message on stdout would be
/// indistinguishable from a corrupt frame.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{} panicked: {info}", env!("CARGO_PKG_NAME"));
    }));
}

#[cfg(test)]
pub(crate) mod testing {
    //! A temporary project with the toolkit's humanoid profile, the fixture
    //! mannequin promoted as a body and the pinned roll take promoted as a
    //! clip — the smallest library every tool test can resolve names
    //! against, built through the same doors a user's assets go through.

    use std::path::{Path, PathBuf};

    use forge_library::Project;
    use forge_library::promote::{PromoteBody, PromoteClip, promote_body, promote_clip};
    use forge_library::schema::{Actor, ClipRecipe, InPlaceMode};

    use crate::config::Config;
    use crate::server::ForgeServer;

    /// A path in the toolkit checkout this crate lives in.
    pub(crate) fn toolkit(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// An empty project with the humanoid profile installed.
    pub(crate) fn empty_project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "mcp_test").expect("init");
        project
            .install_profile(&toolkit("rigs/humanoid"))
            .expect("install the profile");
        (dir, project)
    }

    /// A project holding one body (`mannequin`) and one clip (`roll`).
    pub(crate) fn library() -> (tempfile::TempDir, Project) {
        let (dir, project) = empty_project();
        let profile = project.profile().expect("profile");
        let glb = dir.path().join("mannequin.glb");
        forge_rig::fixture::write_mannequin(&profile, &glb).expect("mannequin");
        promote_body(
            &project,
            &PromoteBody {
                name: String::from("mannequin"),
                glb_path: glb,
                blend_path: None,
                lift_record: None,
                rig_record: None,
                export_record: None,
                prompt: Some(String::from("the fixture mannequin")),
                tags: vec![String::from("fixture")],
                note: None,
                created_by: Actor::Human,
                overwrite: false,
            },
        )
        .expect("promote the body");
        promote_clip(
            &project,
            &PromoteClip {
                name: String::from("roll"),
                take_path: toolkit("crates/forge_motion/tests/fixtures/blender/gen_roll.npz"),
                recipe: ClipRecipe {
                    trim_start_s: 0.25,
                    trim_end_s: 1.3,
                    in_place: InPlaceMode::Detrend,
                    ..ClipRecipe::default()
                },
                prompt: None,
                tags: vec![String::from("action")],
                note: None,
                events: Vec::new(),
                created_by: Actor::Agent(String::from("tester")),
                take_record: None,
                overwrite: false,
            },
        )
        .expect("promote the clip");
        (dir, project)
    }

    /// A server over a project, with a renderer that does not exist — so a
    /// test that reaches the renderer gets an unlaunchable refusal rather
    /// than a Bevy window — and a queue of its own that spawns nothing.
    ///
    /// The queue is real: it writes rows under the project's `out/serve/`
    /// the way every other door does. What it is not given is a launcher
    /// that exists, so a job admitted in a test stays a row.
    pub(crate) fn server(project: Project) -> ForgeServer {
        let queue = forge_serve::LocalQueue::open(
            &project,
            forge_serve::LocalQueueOptions {
                forge: PathBuf::from("/nonexistent/forge-for-tests"),
                launcher: Some(vec![String::from("/nonexistent/forge-gen-for-tests")]),
                run_worker: false,
                ..forge_serve::LocalQueueOptions::default()
            },
        )
        .expect("a queue over the test project");
        ForgeServer::new(
            Config::with_renderer(project, PathBuf::from("/nonexistent/forge-for-tests")),
            queue,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_surface_is_the_eighteen_names_mcp_check_pins() {
        let (_dir, project) = testing::empty_project();
        let server = testing::server(project);
        // The list is asserted rather than counted so a rename shows up as a
        // diff of names, which is what `just mcp-check` compares against and
        // what `mcp_session.rs` asserts over both transports.
        let eighteen = [
            "cancel",
            "doctor",
            "generate_audio",
            "generate_clips",
            "init_project",
            "inspect_audio",
            "licences",
            "list_audio",
            "list_clips",
            "list_models",
            "list_runs",
            "promote_audio",
            "promote_clip",
            "render_clip_strip",
            "render_model",
            "setup",
            "status",
            "wait",
        ];
        let mut names = server.tool_names();
        names.sort();
        assert_eq!(names, eighteen, "the surface drifted from mcp-check's pin");
    }

    #[test]
    fn the_instructions_name_the_verbs_and_the_two_rules() {
        let (_dir, project) = testing::empty_project();
        let server = testing::server(project);
        let text = server.instructions();
        for verb in [
            "list_models",
            "list_clips",
            "list_audio",
            "render_model",
            "render_clip_strip",
            "inspect_audio",
            "generate_clips",
            "generate_audio",
            "promote_clip",
            "promote_audio",
            "doctor",
        ] {
            assert!(text.contains(verb), "instructions do not mention {verb}");
        }
        assert!(text.contains("direct write"), "{text}");
        assert!(text.contains("overwrite"), "{text}");
        assert!(text.contains("no promote for a body or a model"), "{text}");
        assert!(text.contains("LOOK BEFORE YOU PROMOTE"), "{text}");
    }

    #[test]
    fn serve_errors_render_as_one_line() {
        let text = ServeError::Handshake(String::from("connection closed")).to_string();
        assert!(text.contains("handshake"), "{text}");
        let text = ServeError::Session(String::from("panicked")).to_string();
        assert!(text.contains("session"), "{text}");
    }
}
