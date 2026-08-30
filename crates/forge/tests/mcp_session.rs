//! `mcp-session`: the agent's whole path through the MCP, held green the
//! way `ci-fake` holds the shell's.
//!
//! One scripted session — initialize, the tool surface, a project, the
//! licences, the setup gate, doctor, a sound made, waited on, inspected,
//! promoted and verified — run **twice, over both transports**, because
//! "one tool surface, two transports, one queue" is this phase's central
//! claim and a transport nothing exercises ships ungated. Everything is
//! asserted on the frame text an agent would read, never on an internal:
//! the thing under test is what the agent is told.
//!
//! # Why Rust, and why rmcp's own client
//!
//! `rmcp` is already pinned in this workspace, so the client speaks exactly
//! the protocol the server does; a second implementation in CI would be a
//! second thing to keep current, and the day it drifted it would fail for a
//! reason that is not this repo's. `env!("CARGO_BIN_EXE_forge")` is the
//! binary Cargo just built — no `just` step in front of it, no stale
//! `target/debug/forge` from last week, no PATH.
//!
//! # What it needs
//!
//! No GPU, no display, no backend, no secret and no network. The project is
//! a `tempfile::tempdir()` at tier `fake`, so every generator writes a
//! branded placeholder through the same doors and validators, and every
//! doctor row reads `off`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, RawContent};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use serde_json::{Value, json};

/// The whole tool surface, sorted. `mcp-check` pins the same list against a
/// raw handshake; this pins it against a real client, so the two cannot
/// drift apart without one of them saying so.
const TOOLS: [&str; 18] = [
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

/// The binary under test: this build, always.
fn forge() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_forge"))
}

/// The toolkit checkout, for `FORGE_HOME` — `crates/forge/` up two.
fn toolkit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the toolkit root is two above crates/forge")
        .to_path_buf()
}

/// A project to run a session in: a real one, made through the real door.
///
/// `forge init` writes `forge.toml`, the directories, the ledger header,
/// the rig profile and an empty manifest — so the `verify` at the end of
/// the script has something honest to hold. The session's own
/// `init_project` then re-answers the three questions through the MCP,
/// which is the step being tested.
fn scratch_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = Command::new(forge())
        .arg("init")
        .arg("--project")
        .arg(dir.path())
        .arg("--name")
        .arg("mcp_session")
        .env("FORGE_HOME", toolkit())
        .env("FORGE_FAKE", "1")
        .stdin(std::process::Stdio::null())
        .status()
        .expect("forge init runs");
    assert!(status.success(), "forge init exited {status}");
    dir
}

/// The text of a frame, joined — what an agent reads.
fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the frame carried an error result. A refusal is a *successful*
/// frame with this set: that is the shape the whole surface promises.
fn is_error(result: &CallToolResult) -> bool {
    result.is_error.unwrap_or(false)
}

/// Call one tool.
async fn call(
    client: &RunningService<RoleClient, ()>,
    name: &'static str,
    arguments: Value,
) -> CallToolResult {
    let object = arguments
        .as_object()
        .cloned()
        .expect("tool arguments are an object");
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(object))
        .await
        .unwrap_or_else(|err| panic!("{name} did not answer: {err}"))
}

/// A successful frame, or a panic naming what came back instead.
async fn ok(
    client: &RunningService<RoleClient, ()>,
    name: &'static str,
    arguments: Value,
) -> String {
    let result = call(client, name, arguments).await;
    let body = text(&result);
    assert!(!is_error(&result), "{name} refused:\n{body}");
    body
}

/// A refusal, or a panic naming what came back instead.
async fn refused(
    client: &RunningService<RoleClient, ()>,
    name: &'static str,
    arguments: Value,
) -> String {
    let result = call(client, name, arguments).await;
    let body = text(&result);
    assert!(
        is_error(&result),
        "{name} was expected to refuse and did not:\n{body}"
    );
    body
}

/// The one script, run over whichever transport the caller connected with.
///
/// It is the stranger's whole path: make a project, read the licences,
/// meet the gate, ask what this machine can do, make a sound, wait for it,
/// look at it, ship it, and hold the library to its own claims.
async fn session(client: &RunningService<RoleClient, ()>, project: &Path) {
    // -- the surface ------------------------------------------------------
    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("tools/list")
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        TOOLS.iter().map(|n| (*n).to_string()).collect::<Vec<_>>(),
        "the tool surface moved; mcp-check pins the same list and must move with it"
    );

    // -- the three questions ---------------------------------------------
    let already = refused(
        client,
        "init_project",
        json!({ "path": project.display().to_string(), "make": { "sfx": true } }),
    )
    .await;
    assert!(
        already.contains("already a project") && already.contains("adopt"),
        "an existing forge.toml must not be rewritten uninvited:\n{already}"
    );

    let made = ok(
        client,
        "init_project",
        json!({
            "path": project.display().to_string(),
            "make": { "sfx": true },
            "tier": "fake",
            "adopt": true,
        }),
    )
    .await;
    assert!(made.contains("sfx"), "{made}");
    assert!(made.contains("fake"), "{made}");
    assert!(
        made.contains("[make]"),
        "the file it wrote is shown:\n{made}"
    );

    // -- the licences, in full --------------------------------------------
    let licences = ok(client, "licences", json!({ "kinds": ["sfx"] })).await;
    assert!(licences.contains("comfyui_gpl"), "{licences}");
    assert!(
        licences.contains("GPL-3.0-or-later"),
        "the notice itself, not a summary:\n{licences}"
    );
    assert!(licences.contains("needs_accept:"), "{licences}");

    // -- the gate ---------------------------------------------------------
    // sfx carries a fact that is told, not asked, so it is not gated.
    let sfx = ok(
        client,
        "setup",
        json!({ "kinds": ["sfx"], "accept": [], "dry_run": true }),
    )
    .await;
    assert!(sfx.contains("moss_sfx"), "{sfx}");

    // characters is. The refusal must name the id, or an agent that is told
    // "no" without being told which word to say next burns a turn and then
    // repeats the same call.
    let gated = refused(
        client,
        "setup",
        json!({ "kinds": ["characters"], "accept": [] }),
    )
    .await;
    assert!(gated.contains("nvdiffrast"), "{gated}");
    assert!(
        gated.contains("call licences first and pass each id in accept"),
        "{gated}"
    );
    assert!(gated.contains("nothing was installed"), "{gated}");

    // -- what this machine can do -----------------------------------------
    let doctor = ok(client, "doctor", json!({})).await;
    assert!(
        doctor.contains("off — "),
        "tier fake chooses nothing, so every row reads off:\n{doctor}"
    );
    assert!(
        doctor.contains("exit 0")
            || doctor.contains("doctor: ok")
            || doctor.contains("nothing is chosen"),
        "an off row never votes on the exit code:\n{doctor}"
    );

    // -- make one sound ---------------------------------------------------
    let started = ok(
        client,
        "generate_audio",
        json!({ "kind": "sfx", "name": "door", "prompt": "a heavy door closing", "seconds": 1 }),
    )
    .await;
    let job = job_id(&started);
    // A generate returns a JOB, not a finished sound. The frame names where
    // the file WILL be — `designs/serve.md` §7 prints `out` and `record` in
    // it, and `next` is the literal call to make — but its state is not
    // terminal and nothing has been measured yet.
    assert!(
        matches!(
            job_state(&started).as_str(),
            "queued" | "blocked" | "running"
        ),
        "a generate must not block until the sound exists:\n{started}"
    );
    assert!(
        started.contains("wait"),
        "and it hands back the literal call to make next:\n{started}"
    );

    // -- wait on it -------------------------------------------------------
    let done = ok(client, "wait", json!({ "job": job, "max_s": 120 })).await;
    assert!(done.contains("done"), "{done}");
    assert!(done.contains("fake"), "the placeholder says so:\n{done}");
    assert!(done.contains("out/audio/sfx/door.wav"), "{done}");
    assert!(
        project.join("out/audio/sfx/door.wav").is_file(),
        "the sound is on disk where the frame said it is"
    );
    assert!(
        project.join("out/audio/sfx/door.json").is_file()
            || project.join("out/audio/sfx/door.wav.json").is_file(),
        "and its record is beside it"
    );

    // Two negative legs, in the same script, because they rot silently.
    let unknown = refused(client, "wait", json!({ "job": "job_nothing", "max_s": 5 })).await;
    assert!(
        unknown.contains(&job),
        "an unknown job id must list the ids that DO exist:\n{unknown}"
    );

    // -- look at it -------------------------------------------------------
    let plot = ok(
        client,
        "inspect_audio",
        json!({ "name_or_path": "out/audio/sfx/door.wav" }),
    )
    .await;
    assert!(plot.contains("door"), "{plot}");

    // -- ship it ----------------------------------------------------------
    let shipped = ok(
        client,
        "promote_audio",
        json!({ "kind": "sfx", "name": "door", "file": "out/audio/sfx/door.wav" }),
    )
    .await;
    assert!(shipped.contains("door"), "{shipped}");
    assert!(
        project.join("assets/audio/sfx/door.wav").is_file(),
        "a promote is a direct write"
    );

    let taken = refused(
        client,
        "promote_audio",
        json!({ "kind": "sfx", "name": "door", "file": "out/audio/sfx/door.wav" }),
    )
    .await;
    assert!(
        taken.contains("overwrite"),
        "a taken name is refused, and the way past it is named:\n{taken}"
    );
    assert!(
        taken.contains("door"),
        "and the record it would have replaced is echoed:\n{taken}"
    );

    // -- hold the library to its own claims -------------------------------
    let verify = Command::new(forge())
        .arg("verify")
        .arg("--project")
        .arg(project)
        .env("FORGE_HOME", toolkit())
        .output()
        .expect("forge verify runs");
    assert!(
        verify.status.success(),
        "verify failed on what the session shipped:\n{}\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
}

/// The job id out of a frame, read the way an agent reads it: the frame is
/// one JSON object and the id is under `job`, exactly as `designs/serve.md`
/// §7 prints it. Reading the key rather than scanning for a word shape is
/// the point — an agent that had to guess the shape of an id would be the
/// bug this gate exists to catch.
fn job_id(frame: &str) -> String {
    let object: serde_json::Value = serde_json::from_str(frame)
        .unwrap_or_else(|err| panic!("a job frame is JSON ({err}):\n{frame}"));
    let Some(id) = object.get("job").and_then(serde_json::Value::as_str) else {
        panic!("no `job` key in the frame a generate returned:\n{frame}")
    };
    assert!(
        id.starts_with("j-"),
        "a job id is the daemon's own `j-<stamp>-<nonce>`:\n{frame}"
    );
    id.to_string()
}

/// The state a frame reports, for the legs that care whether a call blocked.
fn job_state(frame: &str) -> String {
    let object: serde_json::Value = serde_json::from_str(frame)
        .unwrap_or_else(|err| panic!("a job frame is JSON ({err}):\n{frame}"));
    object
        .get("state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no `state` key in a job frame:\n{frame}"))
        .to_string()
}

/// The environment every session runs in: no card, no display, no network.
fn session_env(command: &mut tokio::process::Command, project: &Path) {
    command
        .env("FORGE_HOME", toolkit())
        .env("FORGE_FAKE", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .arg("--project")
        .arg(project);
}

/// The script over stdio: the transport an editor's MCP client uses.
#[tokio::test]
async fn mcp_session_stdio() {
    let dir = scratch_project();
    let mut command = tokio::process::Command::new(forge());
    command.arg("mcp");
    session_env(&mut command, dir.path());
    // The child's own pipes are the transport. rmcp's `TokioChildProcess`
    // would do the same thing and pulls `process-wrap` in behind
    // `transport-child-process`, which this workspace's index cannot
    // resolve; a pair of pipes is the same protocol over the same bytes and
    // one fewer dependency in a test.
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("forge mcp starts");
    let stdout = child.stdout.take().expect("the server's stdout");
    let stdin = child.stdin.take().expect("the server's stdin");
    let client = ().serve((stdout, stdin)).await.expect("initialize over stdio");
    session(&client, dir.path()).await;
    let _ = client.cancel().await;
    let _ = child.kill().await;
}

/// The same script over streamable HTTP, against the daemon that owns the
/// queue. One tool surface, two transports, one queue — and the queue is
/// the same one `just sfx` at a terminal goes through.
#[tokio::test]
async fn mcp_session_http() {
    let dir = scratch_project();
    let mut serve = tokio::process::Command::new(forge());
    serve
        .arg("serve")
        .arg("--port")
        .arg("0")
        .arg("--foreground");
    session_env(&mut serve, dir.path());
    let mut daemon = serve.spawn().expect("forge serve starts");

    let (port, token) = daemon_details(dir.path()).await;
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(token),
    );
    let client = ().serve(transport).await.expect("initialize over http");
    session(&client, dir.path()).await;
    let _ = client.cancel().await;

    let stopped = Command::new(forge())
        .arg("stop")
        .arg("--project")
        .arg(dir.path())
        .env("FORGE_HOME", toolkit())
        .status()
        .expect("forge stop runs");
    assert!(stopped.success(), "forge stop exited {stopped}");
    let _ = daemon.wait().await;
}

/// The port and the token the daemon wrote, once it has written them.
///
/// `--port 0` means the kernel picks, so the port is only knowable from
/// `out/serve/daemon.json`; polling for the file is how a client that did
/// not start the daemon finds it too.
async fn daemon_details(project: &Path) -> (u16, String) {
    let path = project.join("out/serve/daemon.json");
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(value) = serde_json::from_str::<Value>(&text)
            && let Some(port) = value.get("port").and_then(Value::as_u64)
        {
            let token = value
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            return (u16::try_from(port).expect("a port"), token);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never appeared", path.display());
}
