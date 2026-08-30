//! `mcp-session`: the agent's whole path through the MCP, held green the
//! way `ci-fake` holds the shell's.
//!
//! One scripted session — initialize, the tool surface, a project, the
//! licences, the setup gate, doctor, a sound made, waited on, inspected,
//! promoted and verified, then a reference brought and a body filed behind
//! the export gate and the rig check — run **twice, over both transports**,
//! because "one tool surface, two transports, one queue" is this phase's
//! central claim and a transport nothing exercises ships ungated.
//! Everything is asserted on the frame text an agent would read, never on
//! an internal: the thing under test is what the agent is told.
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
const TOOLS: [&str; 25] = [
    "cancel",
    "doctor",
    "export_bundle",
    "generate_audio",
    "generate_clips",
    "generate_mesh",
    "import_reference",
    "init_project",
    "inspect_audio",
    "licences",
    "list_audio",
    "list_clips",
    "list_models",
    "list_runs",
    "prepare_body",
    "promote_audio",
    "promote_body",
    "promote_clip",
    "promote_model",
    "render_clip_strip",
    "render_model",
    "setup",
    "skin_body",
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

    character_loop(client, project).await;

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

/// The character path, appended to the session: a reference in, a mesh out,
/// a body filed.
///
/// Two halves, and they are asserted differently on purpose.
///
/// The **making** half — `import_reference`, `generate_mesh`,
/// `prepare_body`, `skin_body` — is held to the contract this door actually
/// makes: every one of them answers with a job id and the literal next call,
/// or with a refusal that names doctor and what to install, and never with a
/// protocol error or a call that blocks for four minutes. Whether the
/// generator behind it then succeeds depends on a card and an installed
/// backend, which a runner has neither of; `ci-fake` is where the
/// placeholders run end to end.
///
/// The **shipping** half runs for real, because it needs nothing but this
/// binary: the fixture mannequin is written from the profile, filed through
/// `promote_body` behind the export gate and `rig check`, refused when the
/// name is taken, accepted when told `overwrite`, and looked at. Then the
/// session's own `verify` holds what it shipped to its record — which, for a
/// body, now includes re-deriving all 55 rest translations out of the `.glb`
/// and recomputing its motion scale.
async fn character_loop(client: &RunningService<RoleClient, ()>, project: &Path) {
    // -- a reference, brought ---------------------------------------------
    let drawn = project.join("out/refs/hero.png");
    std::fs::create_dir_all(drawn.parent().expect("a parent")).expect("out/refs");
    std::fs::write(&drawn, PNG_1X1).expect("write the picture");

    let imported = ok(
        client,
        "import_reference",
        json!({
            "png": "out/refs/hero.png",
            "name": "hero",
            "kind": "character",
            "source": "drawn by hand for this test",
        }),
    )
    .await;
    let job = job_id(&imported);
    assert!(
        !job.is_empty() && imported.contains("generate_mesh"),
        "the import hands back a job and the literal next call:\n{imported}"
    );

    // The same name twice is refused: a reference is the durable source a
    // body is re-derived from, so this door has no overwrite at all.
    let again = refused(
        client,
        "import_reference",
        json!({
            "png": "out/refs/hero.png",
            "name": "hero",
            "kind": "character",
            "source": "drawn by hand for this test",
        }),
    )
    .await;
    assert!(again.contains("hero"), "{again}");

    // -- the three card steps ---------------------------------------------
    // Held to the contract this door makes and no further: each answers
    // with a job id and the literal next call, or with a refusal that names
    // doctor and what to install. Whether the generator behind it then
    // succeeds wants a card and an installed backend, which a runner has
    // neither of; `the_whole_character_loop_on_the_fake_tier` runs the
    // whole path once the generators' own doors exist.
    for (tool, arguments) in [
        (
            "generate_mesh",
            json!({"image": "out/refs/hero.png", "name": "hero"}),
        ),
        ("prepare_body", json!({"glb": "out/lifts/hero.glb"})),
        ("skin_body", json!({"glb": "out/prepare/hero.glb"})),
    ] {
        let result = call(client, tool, arguments).await;
        let body = text(&result);
        if is_error(&result) {
            assert!(
                body.contains("doctor")
                    || body.contains("no reference PNG")
                    || body.contains("no mesh at")
                    || body.contains("no prepared mesh at"),
                "{tool} refused without naming what would have worked:\n{body}"
            );
        } else {
            assert!(
                !job_id(&body).is_empty(),
                "{tool} answered without a job id:\n{body}"
            );
        }
    }

    // -- a body, filed ----------------------------------------------------
    // Written through `forge rig fixture`, which builds the mannequin from
    // the profile itself: a real body on the real contract, with no card.
    let body = project.join("out/export/mannequin.glb");
    let wrote = Command::new(forge())
        .arg("--project")
        .arg(project)
        .arg("rig")
        .arg("fixture")
        .arg(&body)
        .env("FORGE_HOME", toolkit())
        .output()
        .expect("forge rig fixture runs");
    assert!(
        wrote.status.success(),
        "the fixture body did not build:\n{}",
        String::from_utf8_lossy(&wrote.stderr)
    );

    let filed = ok(
        client,
        "promote_body",
        json!({"name": "mannequin", "glb": "out/export/mannequin.glb",
               "prompt": "the fixture mannequin, on the profile's own skeleton"}),
    )
    .await;
    assert!(filed.contains("bodies/mannequin.glb"), "{filed}");
    assert!(
        filed.contains("motion_scale 1.0000"),
        "the sidecar's own skeleton is echoed, re-derived from the glb:\n{filed}"
    );
    assert!(
        project.join("assets/bodies/mannequin.glb").is_file(),
        "a promote is a direct write"
    );

    let taken = refused(
        client,
        "promote_body",
        json!({"name": "mannequin", "glb": "out/export/mannequin.glb"}),
    )
    .await;
    assert!(
        taken.contains("overwrite: true"),
        "a taken name is refused, and the way past it is named:\n{taken}"
    );
    assert!(
        taken.contains("bodies/mannequin.glb"),
        "and the record it would have replaced is echoed:\n{taken}"
    );

    let replaced = ok(
        client,
        "promote_body",
        json!({"name": "mannequin", "glb": "out/export/mannequin.glb", "overwrite": true}),
    )
    .await;
    assert!(
        replaced.contains("replaced the body mannequin"),
        "and an overwrite says what it replaced:\n{replaced}"
    );

    // Looking is not a gate, which is why it is a separate call: on a
    // machine with no wgpu adapter it refuses, and that refusal is a
    // successful frame like any other.
    let looked = call(client, "render_model", json!({"name_or_path": "mannequin"})).await;
    let body_text = text(&looked);
    assert!(
        body_text.contains("mannequin") || body_text.contains("adapter"),
        "render_model says what it drew or why it could not:\n{body_text}"
    );
}

/// One transparent pixel, as PNG bytes: something for the reference door to
/// take hold of that is unmistakably not a drawing.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

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

/// A stranger's very first session: a directory that is not a project yet.
///
/// `forge mcp` used to refuse to start here — exit 2, "no forge.toml in
/// &lt;dir&gt;", stdout closed before the handshake — so the one tool that makes
/// a project was reachable only from a server already bound to a different
/// one, and the way out a client with no shell was handed was a shell
/// command. The other two legs of this gate cannot see that: both run
/// `forge init` from a shell first.
#[tokio::test]
async fn mcp_session_with_no_project_yet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut command = tokio::process::Command::new(forge());
    command.arg("mcp");
    session_env(&mut command, dir.path());
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("forge mcp starts without a project");
    let stdout = child.stdout.take().expect("the server's stdout");
    let stdin = child.stdin.take().expect("the server's stdin");
    let client = ()
        .serve((stdout, stdin))
        .await
        .expect("the handshake completes where there is no forge.toml");

    // Every tool is still advertised — the surface is the toolkit's, not
    // the project's — and the seventeen that need a library refuse by
    // naming the one that fixes it.
    let blocked = refused(client_ref(&client), "list_audio", json!({})).await;
    assert!(blocked.contains("init_project"), "{blocked}");
    assert!(blocked.contains("no forge.toml"), "{blocked}");

    // The two that answer without one, because their answers are the
    // toolkit's and the machine's rather than a library's.
    let licences = ok(client_ref(&client), "licences", json!({"kinds": ["props"]})).await;
    assert!(licences.contains("nvdiffrast"), "{licences}");
    let doctor = ok(client_ref(&client), "doctor", json!({"quick": true})).await;
    assert!(!doctor.is_empty());

    // And the one that ends the condition, which then says plainly that
    // this session cannot follow it.
    let made = ok(
        client_ref(&client),
        "init_project",
        json!({ "path": dir.path().display().to_string(), "make": { "sfx": true }, "tier": "fake" }),
    )
    .await;
    assert!(dir.path().join("forge.toml").is_file(), "{made}");
    assert!(
        made.contains("Reconnect it with `--project"),
        "the frame says the running server is still holding what it started with:\n{made}"
    );

    let _ = client.cancel().await;
    let _ = child.kill().await;
}

/// The borrow every helper takes, spelled once.
fn client_ref(client: &RunningService<RoleClient, ()>) -> &RunningService<RoleClient, ()> {
    client
}

/// The whole character path on the fake tier, end to end through the MCP:
/// a picture in, a body in the library, and `verify` holding it to its own
/// record.
///
/// It runs `forge gen ref-import`, `prepare` and `skin` through the queue.
/// Those three verbs landed with the reference door and the skinner, which
/// is why this no longer carries an `#[ignore]`: every assertion below is on
/// the frame text an agent reads, and nothing here had to change when the
/// doors arrived.
#[tokio::test]
async fn the_whole_character_loop_on_the_fake_tier() {
    let dir = scratch_project();
    let mut command = tokio::process::Command::new(forge());
    command.arg("mcp");
    session_env(&mut command, dir.path());
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("forge mcp starts");
    let stdout = child.stdout.take().expect("the server's stdout");
    let stdin = child.stdin.take().expect("the server's stdin");
    let client = ().serve((stdout, stdin)).await.expect("initialize over stdio");
    let project = dir.path();

    let drawn = project.join("out/refs/knight.png");
    std::fs::create_dir_all(drawn.parent().expect("a parent")).expect("out/refs");
    std::fs::write(&drawn, PNG_1X1).expect("write the picture");

    // reference → mesh → prepare → skin, each waited out: a fake tier writes
    // placeholders through the same doors and validators, so the chain is
    // the real chain with the card taken out of it.
    let imported = ok(
        client_ref(&client),
        "import_reference",
        json!({"png": "out/refs/knight.png", "name": "knight", "kind": "character",
               "source": "drawn by hand for this test", "wait_s": 120}),
    )
    .await;
    assert!(
        imported.contains("assets-src/refs/characters/knight.png"),
        "{imported}"
    );

    for (tool, arguments, wrote) in [
        (
            "generate_mesh",
            json!({"image": "assets-src/refs/characters/knight.png", "name": "knight",
                   "wait_s": 300}),
            "out/lifts/knight.glb",
        ),
        (
            "prepare_body",
            json!({"glb": "out/lifts/knight.glb", "wait_s": 300}),
            "out/prepare/knight.glb",
        ),
        (
            "skin_body",
            json!({"glb": "out/prepare/knight.glb", "wait_s": 300}),
            "assets-src/blender/knight.blend",
        ),
    ] {
        let frame = ok(client_ref(&client), tool, arguments).await;
        assert!(frame.contains("done"), "{tool} did not finish:\n{frame}");
        assert!(
            project.join(wrote).exists(),
            "{tool} said it was done and {wrote} is not there:\n{frame}"
        );
    }

    // The body, filed behind the export gate and the rig check.
    let export = Command::new(forge())
        .arg("--project")
        .arg(project)
        .arg("gen")
        .arg("export")
        .arg("assets-src/blender/knight.blend")
        .arg("--out")
        .arg("out/export/knight.glb")
        .arg("--record")
        .arg("out/export/knight.export.json")
        .env("FORGE_HOME", toolkit())
        .env("FORGE_FAKE", "1")
        .output()
        .expect("forge gen export runs");
    assert!(
        export.status.success(),
        "the export gate refused the fake body:\n{}",
        String::from_utf8_lossy(&export.stderr)
    );

    let filed = ok(
        client_ref(&client),
        "promote_body",
        json!({"name": "knight", "glb": "out/export/knight.glb",
               "rig_record": "assets-src/blender/knight.rig.json",
               "export_record": "out/export/knight.export.json"}),
    )
    .await;
    assert!(filed.contains("bodies/knight.glb"), "{filed}");
    assert!(
        filed.contains("motion_scale"),
        "the fitted skeleton is echoed:\n{filed}"
    );

    let looked = ok(
        client_ref(&client),
        "render_model",
        json!({"name_or_path": "knight"}),
    )
    .await;
    assert!(looked.contains("knight"), "{looked}");

    let verify = Command::new(forge())
        .arg("verify")
        .arg("--project")
        .arg(project)
        .env("FORGE_HOME", toolkit())
        .output()
        .expect("forge verify runs");
    assert!(
        verify.status.success(),
        "verify failed on the body the loop shipped:\n{}\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );

    let _ = client.cancel().await;
    let _ = child.kill().await;
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
