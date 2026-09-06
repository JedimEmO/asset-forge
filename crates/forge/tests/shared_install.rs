//! External game projects sharing one local toolkit, without cwd-dependent launchers.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

fn toolkit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("toolkit")
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8")
}

fn init(cwd: &Path, root: &Path, key: &str, home: &Path) {
    success(
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .current_dir(cwd)
            .env_remove("FORGE_HOME")
            .env_remove("FORGE_TOOLKIT")
            .env(key, home)
            .env("FORGE_PROJECT", toolkit())
            .arg("--project")
            .arg(root)
            .args([
                "init", "--name", "external", "--make", "none", "--tier", "fake", "--yes",
            ])
            .output()
            .expect("init"),
    );
}

#[tokio::test]
async fn generated_launchers_bind_two_games_despite_unrelated_cwd_and_environment() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = toolkit();
        let first = temp.path().join("game one");
        let second = temp.path().join("game two");
        init(temp.path(), &first, "FORGE_HOME", &home);
        init(temp.path(), &second, "FORGE_TOOLKIT", &home);
        let mut children = Vec::new();
        let mut clients = Vec::new();
        for root in [&first, &second] {
            let config: Value = serde_json::from_slice(
                &std::fs::read(root.join(".forge/mcp.json")).expect("config"),
            )
            .expect("json");
            let server = &config["mcpServers"]["asset-forge"];
            assert_eq!(server["env"]["FORGE_HOME"], json!(home));
            let mut command =
                tokio::process::Command::new(server["command"].as_str().expect("command"));
            for arg in server["args"].as_array().expect("args") {
                command.arg(arg.as_str().expect("argument"));
            }
            command
                .current_dir(&home)
                .env("FORGE_PROJECT", &home)
                .env("FORGE_TOOLKIT", "/missing/legacy");
            for (key, value) in server["env"].as_object().expect("env") {
                command.env(key, value.as_str().expect("env value"));
            }
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(true);
            let mut child = command.spawn().expect("configured launcher");
            let client = ()
                .serve((
                    child.stdout.take().expect("stdout"),
                    child.stdin.take().expect("stdin"),
                ))
                .await
                .expect("MCP handshake");
            let info = client.peer_info().expect("info");
            assert!(
                info.instructions
                    .as_deref()
                    .expect("instructions")
                    .contains(root.to_str().expect("root"))
            );
            let reply = client
                .call_tool(CallToolRequestParams::new("list_audio"))
                .await
                .expect("list");
            assert!(format!("{reply:?}").contains("0 sound(s)"), "{reply:?}");
            children.push(child);
            clients.push(client);
        }
        let untouched = std::fs::read(second.join("assets/library.json")).expect("second manifest");
        success(
            Command::new(env!("CARGO_BIN_EXE_forge"))
                .current_dir(&second)
                .env("FORGE_PROJECT", &second)
                .arg("--project")
                .arg(&first)
                .args(["rig", "fixture"])
                .arg(first.join("out/first.glb"))
                .output()
                .expect("fixture"),
        );
        assert!(first.join("out/first.glb").is_file());
        assert!(!second.join("out/first.glb").exists());
        assert_eq!(
            std::fs::read(second.join("assets/library.json")).expect("manifest"),
            untouched
        );
        for root in [&first, &second] {
            for args in [vec!["verify"], vec!["audit"], vec!["manifest", "--check"]] {
                success(
                    Command::new(env!("CARGO_BIN_EXE_forge"))
                        .current_dir(&home)
                        .arg("--project")
                        .arg(root)
                        .args(args)
                        .output()
                        .expect("check"),
                );
            }
        }
        for client in clients {
            let _ = client.cancel().await;
        }
        for mut child in children {
            let _ = child.kill().await;
        }
    })
    .await
    .expect("bounded external session");
}

#[test]
fn agent_config_preserves_custom_files_and_can_be_repeated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("game");
    std::fs::create_dir(&root).expect("mkdir");
    std::fs::write(root.join("AGENTS.md"), "Keep my instructions\n").expect("instructions");
    std::fs::write(root.join(".mcp.json"), "custom client configuration\n").expect("config");
    init(temp.path(), &root, "FORGE_HOME", &toolkit());
    for _ in 0..2 {
        let output = success(
            Command::new(env!("CARGO_BIN_EXE_forge"))
                .arg("--project")
                .arg(&root)
                .arg("agent-config")
                .output()
                .expect("agent-config"),
        );
        assert!(output.contains("preserved"));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("AGENTS.md")).expect("instructions"),
        "Keep my instructions\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".mcp.json")).expect("config"),
        "custom client configuration\n"
    );
    let guide = std::fs::read_to_string(root.join(".forge/AGENT.md")).expect("guide");
    assert!(guide.contains("`forge guide`"));
    assert!(root.join(".forge/mcp.json").is_file());
}

#[test]
fn invalid_explicit_toolkit_refuses_before_creating_project() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("game");
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .env("FORGE_HOME", temp.path().join("missing"))
        .env("FORGE_TOOLKIT", toolkit())
        .arg("--project")
        .arg(&root)
        .args(["init", "--name", "game", "--tier", "fake", "--yes"])
        .output()
        .expect("init");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("nothing was written"));
    assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn relative_symlinked_toolkit_is_resolved_before_launch() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(toolkit(), temp.path().join("shared forge")).expect("symlink");
    let root = temp.path().join("game");
    init(temp.path(), &root, "FORGE_HOME", Path::new("shared forge"));
    let config: Value =
        serde_json::from_slice(&std::fs::read(root.join(".forge/mcp.json")).expect("read"))
            .expect("json");
    assert_eq!(
        config["mcpServers"]["asset-forge"]["env"]["FORGE_HOME"],
        json!(toolkit())
    );
}

#[test]
fn workflow_guide_is_embedded_and_available_before_project_initialization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let guide = success(
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .current_dir(temp.path())
            .env("FORGE_HOME", temp.path().join("missing toolkit"))
            .env("FORGE_TOOLKIT", temp.path().join("missing legacy toolkit"))
            .arg("guide")
            .output()
            .expect("guide"),
    );
    assert_eq!(
        guide,
        format!(
            "Toolkit version: {}\n\n{}",
            env!("CARGO_PKG_VERSION"),
            include_str!("../../forge_mcp/guides/workflow.md")
        )
    );
    assert_eq!(
        std::fs::read_dir(temp.path()).expect("directory").count(),
        0
    );
}
