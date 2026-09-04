//! The binary, driven the way a `just` recipe or an agent drives it: a fresh
//! project from `forge init`, the doors, the checks, and the exit codes.
//!
//! No GPU, no Blender, no sample library. The take is the roll fixture
//! pinned under `forge_motion`, the body is the mannequin `forge rig
//! fixture` writes from the profile, the sound is a WAV written by hand.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The toolkit checkout this crate lives in.
fn toolkit(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// A pinned copy of the roll take.
fn take_fixture() -> PathBuf {
    toolkit("crates/forge_motion/tests/fixtures/blender/gen_roll.npz")
}

/// A pinned copy of the walk take — the profile's reference clip, once
/// promoted under that name.
fn walk_fixture() -> PathBuf {
    toolkit("crates/forge_motion/tests/fixtures/blender/gen_walk.npz")
}

/// Run `forge` with these arguments from `cwd`.
fn forge(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run forge")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Run and insist on exit 0, printing both streams when it is not.
fn ok(cwd: &Path, args: &[&str]) -> String {
    let output = forge(cwd, args);
    assert_eq!(
        code(&output),
        0,
        "forge {}\n--- stdout\n{}\n--- stderr\n{}",
        args.join(" "),
        stdout(&output),
        stderr(&output)
    );
    stdout(&output)
}

/// Run and insist on this exit code; return stdout + stderr.
fn exits(cwd: &Path, args: &[&str], expected: i32) -> String {
    let output = forge(cwd, args);
    assert_eq!(
        code(&output),
        expected,
        "forge {}\n--- stdout\n{}\n--- stderr\n{}",
        args.join(" "),
        stdout(&output),
        stderr(&output)
    );
    format!("{}{}", stdout(&output), stderr(&output))
}

/// A project made by `forge init` in a temporary directory, the way a user
/// gets one: the profile found from the executable, no flags.
fn init_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("game");
    let out = ok(
        dir.path(),
        &["init", "--project", to_str(&root), "--name", "game"],
    );
    assert!(out.contains("rig profile humanoid installed"), "{out}");
    assert!(root.join("forge.toml").is_file());
    assert!(root.join("assets/library.json").is_file());
    assert!(root.join("assets-src/SOURCES.md").is_file());
    assert!(
        root.join("assets-src/rigs/humanoid/contract.json")
            .is_file()
    );
    (dir, root)
}

fn to_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

/// A 16-bit PCM mono WAV: a decaying 440 Hz pluck, or silence.
fn write_wav(path: &Path, silent: bool) {
    let rate: u32 = 22_050;
    let frames = rate / 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(frames * 2).to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        let sample = if silent {
            0.0
        } else {
            0.8 * (-4.0 * t).exp() * (t * 440.0 * std::f32::consts::TAU).sin()
        };
        bytes.extend_from_slice(&((sample * 32_767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

#[test]
fn help_lists_the_tree_without_a_project() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = ok(dir.path(), &["--help"]);
    for verb in [
        "init",
        "catalog",
        "manifest",
        "verify",
        "audit",
        "rebake",
        "migrate",
        "promote",
        "audio",
        "rig",
        "doctor",
        "gpu",
        "sheet",
        "views",
        "turntable",
        "bones",
        "studio",
        "mcp",
    ] {
        assert!(
            out.contains(&format!("\n  {verb}")),
            "{verb} missing:\n{out}"
        );
    }
    let promote = ok(dir.path(), &["promote", "--help"]);
    for door in ["clip", "body", "model", "audio"] {
        assert!(
            promote.contains(&format!("\n  {door}")),
            "{door}:\n{promote}"
        );
    }
    let rig = ok(dir.path(), &["rig", "--help"]);
    for verb in ["export-contract", "fixture", "check"] {
        assert!(rig.contains(&format!("\n  {verb}")), "{verb}:\n{rig}");
    }
}

#[test]
fn no_project_is_a_refusal_naming_the_search_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let text = exits(dir.path(), &["catalog"], 2);
    assert!(text.contains("no forge.toml above"), "{text}");
    assert!(text.contains("forge init"), "{text}");

    let text = exits(dir.path(), &["--project", to_str(dir.path()), "catalog"], 2);
    assert!(text.contains("no forge.toml in"), "{text}");
}

/// `forge mcp` driven the way a client drives it: newline-delimited JSON-RPC
/// on stdin, frames and nothing else on stdout, the banner on stderr. The
/// tool surface is pinned here by name — it is what the skills are written
/// against — and since Phase 3 that surface includes the mesh doors.
#[test]
fn mcp_handshakes_over_stdio_and_lists_exactly_its_tools() {
    use std::io::Write as _;
    use std::process::Stdio;

    let (_dir, root) = init_project();
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["--project", to_str(&root), "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn forge mcp");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for frame in [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cli-test","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ] {
            writeln!(stdin, "{frame}").expect("write a frame");
        }
        // Dropping stdin is the hang-up; the server exits on it.
    }
    let output = child.wait_with_output().expect("forge mcp");
    let out = stdout(&output);
    let err = stderr(&output);
    assert_eq!(code(&output), 0, "--- stdout\n{out}\n--- stderr\n{err}");
    assert!(
        err.contains("forge mcp: project="),
        "banner on stderr:\n{err}"
    );

    // Every stdout line is a frame: a stray print here would corrupt the
    // protocol, and this is where it would show.
    let mut listed: Vec<String> = Vec::new();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let frame: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not a frame ({e}): {line}"));
        assert_eq!(frame["jsonrpc"], "2.0", "{line}");
        if frame["id"] == 1 {
            assert_eq!(frame["result"]["serverInfo"]["name"], "forge_mcp", "{line}");
            assert!(
                frame["result"]["instructions"]
                    .as_str()
                    .is_some_and(|t| t.contains("promote")),
                "{line}"
            );
        }
        if frame["id"] == 2 {
            listed = frame["result"]["tools"]
                .as_array()
                .expect("a tools array")
                .iter()
                .map(|t| t["name"].as_str().expect("a name").to_owned())
                .collect();
        }
    }
    listed.sort();
    // Twenty-four of the twenty-seven `mcp-check` pins: `init_project`,
    // `licences` and `setup` land with tools/setup.rs. Anything else
    // appearing here is a surface change the skills are not written against.
    let mine = [
        "cancel",
        "doctor",
        "export_body",
        "export_bundle",
        "generate_audio",
        "generate_clips",
        "generate_mesh",
        "import_reference",
        "inspect_audio",
        "list_audio",
        "list_clips",
        "list_models",
        "list_runs",
        "prepare_body",
        "prepare_prop",
        "promote_audio",
        "promote_body",
        "promote_clip",
        "promote_model",
        "render_clip_strip",
        "render_model",
        "skin_body",
        "status",
        "wait",
    ];
    for name in mine {
        assert!(
            listed.iter().any(|listed| listed == name),
            "{name} is missing --- stdout\n{out}\n--- stderr\n{err}"
        );
    }
    for name in &listed {
        assert!(
            mine.contains(&name.as_str())
                || matches!(name.as_str(), "init_project" | "licences" | "setup"),
            "{name} is not one of the twenty-seven --- stdout\n{out}"
        );
    }
    // The mesh doors are here now, and their being here is the decision:
    // what protects the library is the export gate, the rig check and the
    // refused taken name, all of which promote_body runs — not a missing
    // door that only made an agent ask a human to type its own command.
    assert!(
        listed.iter().any(|name| name == "promote_body")
            && listed.iter().any(|name| name == "promote_model"),
        "the mesh doors are part of the surface: {listed:?}"
    );

    // No project is a **session**, not a refusal: this is where a stranger
    // with no shell starts, and the tool that makes a project is inside the
    // server. It used to exit 2 here with a shell command as the way out,
    // which made `init_project` reachable only from a server already bound
    // to some other project (2026-08-30). The handshake completes; the
    // banner says which three tools answer.
    let empty = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge"))
        .arg("mcp")
        .current_dir(empty.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn forge mcp with no project");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for frame in [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cli-test","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_audio","arguments":{}}}"#,
        ] {
            writeln!(stdin, "{frame}").expect("write a frame");
        }
    }
    let output = child.wait_with_output().expect("forge mcp");
    let err = stderr(&output);
    assert_eq!(code(&output), 0, "{err}");
    assert!(err.contains("no forge.toml at"), "{err}");
    assert!(
        err.contains("init_project, licences and doctor"),
        "the banner names what still answers: {err}"
    );
    let refusal = stdout(&output);
    assert!(
        refusal.contains("init_project") && refusal.contains("isError"),
        "a tool that needs a library refuses by naming the one that fixes it:\n{refusal}"
    );
}

/// The looks that need no GPU: the binding report, the rig check without a
/// sheet, and the posed half of audit — on a library whose only body is the
/// mannequin the commands write for themselves.
#[test]
fn bones_rig_check_and_the_posed_audit_run_without_a_gpu() {
    let (_dir, root) = init_project();
    let walk = walk_fixture();
    ok(&root, &["promote", "clip", to_str(&walk), "walk"]);

    // No body in the library: the clip is posed on the mannequin, written
    // under out/fixture, and every driven bone binds.
    let output = forge(&root, &["bones", "walk"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("fixture mannequin"),
        "{}",
        stderr(&output)
    );
    assert!(root.join("out/fixture/mannequin.glb").is_file());
    let out = stdout(&output);
    assert!(out.contains("27 of 58 skeleton bones driven"), "{out}");

    // A clip that is not there is a refusal naming what is.
    let text = exits(&root, &["bones", "sprint"], 2);
    assert!(text.contains("no clip named \"sprint\""), "{text}");
    assert!(text.contains("walk"), "{text}");
    let text = exits(&root, &["sheet", "sprint"], 2);
    assert!(text.contains("no clip named \"sprint\""), "{text}");

    // The mannequin holds to the contract it was written from, and the
    // reference walk binds 27/27.
    let out = ok(&root, &["rig", "check", "out/fixture/mannequin.glb"]);
    assert!(out.contains("reference: clips/walk.glb"), "{out}");
    assert!(!out.contains("FAIL:"), "{out}");
    assert!(out.contains("0 failed"), "{out}");
    let text = exits(&root, &["rig", "check", "out/nowhere.glb"], 2);
    assert!(text.contains("is not a file"), "{text}");

    // A named body that is not there is refused the same way.
    let text = exits(&root, &["bones", "walk", "--body", "nobody"], 2);
    assert!(text.contains("no body named \"nobody\""), "{text}");

    // Audit now carries the pose compare and the body checks.
    let out = ok(&root, &["audit"]);
    assert!(out.contains("clip poses"), "{out}");
    assert!(
        out.contains("1/1 clips pose the mannequin exactly as their shipped file"),
        "{out}"
    );
    assert!(out.contains("0/0 bodies conform"), "{out}");

    // Promote the mannequin as a body: audit checks it, and rig check on
    // the shipped file still passes.
    ok(
        &root,
        &["promote", "body", "out/fixture/mannequin.glb", "mannequin"],
    );
    let out = ok(&root, &["audit", "--fit"]);
    assert!(out.contains("1/1 bodies conform to the contract"), "{out}");
    let output = forge(&root, &["bones", "walk"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("first body, mannequin"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn init_refuses_twice_and_a_fresh_project_passes_every_gate() {
    let (_dir, root) = init_project();
    let text = exits(&root, &["init"], 2);
    assert!(text.contains("already exists"), "{text}");

    let out = ok(&root, &["manifest", "--check"]);
    assert!(out.contains("matches a rebuild"), "{out}");
    ok(&root, &["verify"]);
    ok(&root, &["audit"]);
    ok(&root, &["rebake", "--dry-run"]);
    ok(&root, &["migrate"]);
    let out = ok(&root, &["catalog"]);
    assert!(out.contains("0 asset(s)"), "{out}");
    let out = ok(&root, &["audio", "list"]);
    assert!(out.contains("nothing to measure"), "{out}");
    // Doctor's exit is the backends' verdict — 1 on a machine where any is
    // missing or partial, which a CI runner always is — so the project half
    // is checked on the text, not the code.
    let output = forge(&root, &["doctor", "--quick"]);
    let out = stdout(&output);
    assert!(code(&output) == 0 || code(&output) == 1, "{out}");
    assert!(out.contains("no drift"), "{out}");
    assert!(out.contains("manifest current"), "{out}");
    assert!(out.contains("backends  "), "{out}");
    assert!(out.contains("doctor: "), "{out}");
    let output = forge(&root, &["doctor", "--quick", "--json"]);
    let out = stdout(&output);
    let json: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON object");
    assert_eq!(json["quick"], serde_json::Value::Bool(true));
    assert!(
        json["rig"]["bones"].as_u64().is_some_and(|n| n > 0),
        "{out}"
    );
}

#[test]
fn gen_relays_the_python_layer_and_its_exit_codes() {
    let (_dir, root) = init_project();
    // A fake run needs no backend and no Blender: placeholder outputs and a
    // real record, promoted through the same door a real sound uses.
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "sfx",
            "--prompt",
            "a door",
            "--out",
            "out/audio/sfx/door.wav",
        ])
        .env("FORGE_FAKE", "1")
        .current_dir(&root)
        .output()
        .expect("run forge");
    let out = stdout(&output);
    assert_eq!(code(&output), 0, "{out}\n{}", stderr(&output));
    assert!(out.contains("record   "), "{out}");
    assert!(out.contains("fake     true"), "{out}");
    assert!(root.join("out/audio/sfx/door.json").is_file());
    let out = ok(
        &root,
        &[
            "promote",
            "audio",
            "sfx",
            "out/audio/sfx/door.wav",
            "door",
            "--record",
            "out/audio/sfx/door.json",
        ],
    );
    assert!(out.contains("recorded"), "{out}");

    // --json among the arguments: the object itself is the last line.
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "sfx",
            "--prompt",
            "x",
            "--out",
            "out/audio/sfx/x.wav",
            "--json",
        ])
        .env("FORGE_FAKE", "1")
        .current_dir(&root)
        .output()
        .expect("run forge");
    let last = stdout(&output);
    let last = last.trim().lines().last().unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(last).expect("a JSON line");
    assert_eq!(json["ok"], serde_json::Value::Bool(true));

    // The table: 2 usage, 4 input rejected — relayed unchanged.
    let text = exits(&root, &["gen", "sfx", "--out", "x.wav"], 2);
    assert!(text.contains("usage"), "{text}");
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "mesh",
            "nope.png",
            "--out",
            "out/l.glb",
            "--record",
            "out/l.json",
        ])
        .env("FORGE_FAKE", "1")
        .current_dir(&root)
        .output()
        .expect("run forge");
    assert_eq!(code(&output), 4, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("input_rejected"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_clip_promotes_with_its_recipe_and_an_overwrite_shows_both() {
    let (_dir, root) = init_project();
    let take = take_fixture();
    let take = to_str(&take);

    let out = ok(
        &root,
        &[
            "promote",
            "clip",
            take,
            "roll",
            "--trim-start",
            "0.25",
            "--trim-end",
            "1.3",
            "--in-place",
            "detrend",
            "--exaggerate",
            "1.15",
            "--lean",
            "4",
            "--clip",
            "Roll",
            "--event",
            "0.5:swoosh",
            "--tag",
            "combat",
            "--created-by",
            "agent:tester",
        ],
    );
    assert!(out.contains("in_place      detrend"), "{out}");
    assert!(out.contains("baked roll.glb: 49 frames"), "{out}");
    assert!(
        out.contains("-> clips/roll.glb (reconstructed, ardy"),
        "{out}"
    );
    assert!(out.contains("by agent:tester"), "{out}");
    assert!(root.join("assets/clips/roll.glb").is_file());
    assert!(root.join("assets/clips/roll.json").is_file());
    assert!(root.join("assets-src/takes/roll.npz").is_file());

    // The same name again is refused, and the refusal shows the recipe it
    // would have baked with — the shipped one, with the stated knob on top.
    let text = exits(&root, &["promote", "clip", take, "roll", "--lean", "6"], 2);
    assert!(text.contains("pass --overwrite"), "{text}");
    assert!(text.contains("lean          6.00 deg"), "{text}");
    assert!(text.contains("trim          0.250s"), "inherited:\n{text}");

    // With --overwrite the old recipe stands beside the new, and the authored
    // event that was not restated is named as dropped.
    let out = ok(
        &root,
        &[
            "promote",
            "clip",
            take,
            "roll",
            "--lean",
            "6",
            "--loop",
            "--loop-blend",
            "0.1",
            "--overwrite",
        ],
    );
    assert!(out.contains("| now"), "{out}");
    let lean = out
        .lines()
        .find(|l| l.contains("lean          4.00 deg"))
        .expect("the replaced lean");
    assert!(
        lean.contains("| ") && lean.contains("6.00 deg"),
        "old beside new:\n{out}"
    );
    assert!(out.contains("loop          yes, 0.100s blend"), "{out}");
    assert!(out.contains("authored event(s) (swoosh)"), "{out}");

    let out = ok(&root, &["catalog", "--kind", "clip", "--filter", "roll"]);
    assert!(out.contains("combat,loop"), "tags survive:\n{out}");
    ok(&root, &["manifest", "--check"]);
    ok(&root, &["verify"]);
    let out = ok(&root, &["audit"]);
    assert!(out.contains("1/1 clips rebuild byte for byte"), "{out}");
    let out = ok(&root, &["rebake"]);
    assert!(out.contains("1 baked, 0 skipped, 0 failed"), "{out}");
    ok(&root, &["audit"]);

    // A bad event, a bad kind word and a missing take are all refusals.
    let text = exits(
        &root,
        &["promote", "clip", take, "x", "--event", "swoosh"],
        2,
    );
    assert!(text.contains("--event"), "{text}");
    let text = exits(&root, &["promote", "clip", take, "Roll"], 2);
    assert!(text.contains("not a usable asset name"), "{text}");
    let text = exits(&root, &["promote", "clip", "nowhere.npz", "gone"], 2);
    assert!(text.contains("no take at"), "{text}");
    let text = exits(&root, &["catalog", "--kind", "prop"], 2);
    assert!(
        text.contains("clip, body, model, sfx, music, voice"),
        "{text}"
    );
}

#[test]
fn the_fixture_mannequin_promotes_as_a_body_and_as_a_model() {
    let (_dir, root) = init_project();
    let out = ok(&root, &["rig", "fixture", "out/mannequin.glb"]);
    assert!(out.contains("55 bones (27 driven)"), "{out}");
    let glb = root.join("out/mannequin.glb");
    assert!(glb.is_file());

    let out = ok(
        &root,
        &[
            "promote",
            "body",
            "out/mannequin.glb",
            "dummy",
            "--tag",
            "fixture",
        ],
    );
    assert!(out.contains("ingested dummy.glb"), "{out}");
    assert!(out.contains("55 bones, 1.80 m tall"), "{out}");
    let text = exits(&root, &["promote", "body", "out/mannequin.glb", "dummy"], 2);
    assert!(text.contains("pass overwrite"), "{text}");

    let out = ok(&root, &["promote", "model", "out/mannequin.glb", "statue"]);
    assert!(out.contains("ingested statue.glb"), "{out}");

    // A record of the wrong kind is this door's refusal, not the library's.
    let wrong = root.join("out/wrong.json");
    std::fs::write(
        &wrong,
        r#"{"forge_record":1,"kind":"sfx","tool":"moss_sound_effect","created":"2026-08-23","created_by":"human"}"#,
    )
    .expect("record");
    let text = exits(
        &root,
        &[
            "promote",
            "body",
            "out/mannequin.glb",
            "dummy",
            "--overwrite",
            "--lift-record",
            to_str(&wrong),
        ],
        2,
    );
    assert!(text.contains("describes a sfx run, not lift"), "{text}");

    ok(&root, &["manifest", "--check"]);
    ok(&root, &["verify"]);
    let out = ok(&root, &["audit"]);
    assert!(out.contains("2 bodies/models skipped"), "{out}");
    let out = ok(&root, &["rebake"]);
    assert!(out.contains("skipped dummy"), "{out}");
    assert!(
        out.contains("forge promote body dummy --overwrite"),
        "{out}"
    );
}

#[test]
fn audio_inspects_lists_promotes_and_gates_on_defects() {
    let (_dir, root) = init_project();
    let tone = root.join("out/tone.wav");
    let silent = root.join("out/silent.wav");
    write_wav(&tone, false);
    write_wav(&silent, true);

    let out = ok(
        &root,
        &[
            "audio",
            "inspect",
            "out/tone.wav",
            "--out",
            "out/audio/tone.png",
            "--width",
            "400",
        ],
    );
    assert!(out.contains("verdict:  clean"), "{out}");
    assert!(out.contains("(400x"), "{out}");
    assert!(root.join("out/audio/tone.png").is_file());

    let text = exits(&root, &["audio", "inspect", "out/silent.wav"], 1);
    assert!(text.contains("file is silent"), "{text}");
    let text = exits(&root, &["audio", "list", "out"], 1);
    assert!(text.contains("1 defective: silent.wav"), "{text}");
    let text = exits(&root, &["audio", "inspect", "out/missing.wav"], 2);
    assert!(text.contains("no file at"), "{text}");

    let out = ok(
        &root,
        &[
            "promote",
            "audio",
            "sfx",
            "out/tone.wav",
            "tone",
            "--prompt",
            "a pluck",
        ],
    );
    assert!(out.contains("shipped tone.wav: 0.50s"), "{out}");
    assert!(out.contains("-> audio/sfx/tone.wav (unknown"), "{out}");
    let text = exits(
        &root,
        &["promote", "audio", "voice", "out/tone.wav", "tone"],
        2,
    );
    assert!(text.contains("already a sfx"), "{text}");
    let text = exits(
        &root,
        &["promote", "audio", "clip", "out/tone.wav", "tone"],
        2,
    );
    assert!(text.contains("not an audio kind"), "{text}");
    let text = exits(
        &root,
        &["promote", "audio", "noise", "out/tone.wav", "tone"],
        2,
    );
    assert!(text.contains("is not a kind"), "{text}");

    // The door runs the same measurements `audio inspect` gates on: a
    // defective sound is refused with the defect named, and shipping it
    // anyway is a stated decision.
    let text = exits(
        &root,
        &["promote", "audio", "sfx", "out/silent.wav", "quiet"],
        2,
    );
    assert!(text.contains("defective"), "{text}");
    assert!(text.contains("file is silent"), "{text}");
    assert!(text.contains("--allow-defective"), "{text}");
    let out = ok(
        &root,
        &[
            "promote",
            "audio",
            "sfx",
            "out/silent.wav",
            "quiet",
            "--allow-defective",
        ],
    );
    assert!(out.contains("shipped quiet.wav"), "{out}");

    let text = exits(&root, &["audio", "list"], 1);
    assert!(text.contains("sfx/tone.wav"), "{text}");
    assert!(text.contains("2 file(s)"), "{text}");
    assert!(
        text.contains("1 defective: sfx/quiet.wav"),
        "the allowed defect still fails the audio gate, by design: {text}"
    );
    let out = ok(&root, &["catalog", "--kind", "sfx"]);
    assert!(out.contains("a pluck"), "{out}");
    ok(&root, &["manifest", "--check"]);
    ok(&root, &["verify"]);
}

#[test]
fn help_and_refusals_reach_outside_a_project() {
    // `forge gen <cmd> --help` is argparse text, not a library operation:
    // it answers from anywhere, without a forge.toml above the directory.
    let dir = tempfile::tempdir().expect("tempdir");
    let output = forge(dir.path(), &["gen", "sfx", "--help"]);
    assert_eq!(
        code(&output),
        0,
        "--- stdout\n{}\n--- stderr\n{}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("usage:"), "{}", stdout(&output));
    // A real call is still the ordinary refusal.
    let text = exits(dir.path(), &["gen", "doctor"], 2);
    assert!(text.contains("no forge.toml"), "{text}");
}

#[test]
fn export_contract_refused_on_a_glb_names_the_profile_directory() {
    let (_dir, root) = init_project();
    std::fs::write(root.join("out/rig.glb"), b"glb").expect("write");
    let text = exits(&root, &["rig", "export-contract", "out/rig.glb"], 2);
    assert!(text.contains("is not a directory"), "{text}");
    assert!(text.contains("profile directory"), "{text}");
    assert!(text.contains("try out"), "the parent is suggested: {text}");
    let text = exits(&root, &["rig", "export-contract", "nowhere"], 2);
    assert!(text.contains("rigs/humanoid"), "{text}");
}

#[test]
fn a_reference_png_without_a_ledger_row_fails_verify() {
    let (_dir, root) = init_project();
    let chars = root.join("assets-src/refs/characters");
    std::fs::create_dir_all(&chars).expect("mkdir");
    std::fs::write(chars.join("hero.png"), b"\x89PNG\r\n").expect("png");
    let text = exits(&root, &["verify"], 1);
    assert!(text.contains("hero.png"), "{text}");
    let ledger = root.join("assets-src/SOURCES.md");
    let mut text = std::fs::read_to_string(&ledger).expect("ledger");
    text.push_str("| `characters/hero.png` | drawn by hand | a test | 2026-08-23 |\n");
    std::fs::write(&ledger, text).expect("ledger");
    ok(&root, &["verify"]);
}

#[test]
fn export_contract_reproduces_the_shipped_profile_byte_for_byte() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile = dir.path().join("humanoid");
    std::fs::create_dir_all(&profile).expect("mkdir");
    for file in [
        "contract.json",
        "sockets.json",
        "motion_skeleton.json",
        "rig.glb",
        "rig.blend",
    ] {
        std::fs::copy(toolkit("rigs/humanoid").join(file), profile.join(file)).expect(file);
    }
    let before = std::fs::read(profile.join("contract.json")).expect("read");
    let out = ok(dir.path(), &["rig", "export-contract", to_str(&profile)]);
    assert!(out.contains("55 bones (27 driven by cskel27)"), "{out}");
    let after = std::fs::read(profile.join("contract.json")).expect("read");
    assert_eq!(before, after, "an unchanged rig reproduces the contract");

    // The fixture from an explicit profile directory, with no project at all.
    let out = ok(
        dir.path(),
        &[
            "rig",
            "fixture",
            to_str(&dir.path().join("m.glb")),
            "--rig-dir",
            to_str(&profile),
        ],
    );
    assert!(out.contains("byte-deterministic"), "{out}");
    let text = exits(
        dir.path(),
        &["rig", "export-contract", to_str(dir.path())],
        2,
    );
    assert!(text.contains("motion_skeleton.json"), "{text}");
}

/// A `daemon.json` naming a process that is not there — or one that is
/// alive and was born at another moment — is a stale file, not a daemon:
/// it is removed and the run proceeds in this process.
///
/// The failure this retires is a queue that hangs waiting on a port
/// nothing is listening to, with a file on disk insisting otherwise.
#[test]
fn a_stale_daemon_json_falls_back_in_process() {
    let (dir, project) = init_project();
    let serve = project.join("out/serve");
    std::fs::create_dir_all(&serve).expect("mkdir");
    let write_daemon = |pid: u32, start_ticks: u64| {
        std::fs::write(
            serve.join("daemon.json"),
            format!(
                "{{\"forge_serve\":1,\"pid\":{pid},\"start_ticks\":{start_ticks},\"port\":41773,\
                 \"token\":\"t0ken\",\"url\":\"http://127.0.0.1:41773\",\"version\":\"0.1.0\",\
                 \"project\":\"{}\",\"started\":\"2026-08-30T14:20:02Z\"}}",
                project.display()
            ),
        )
        .expect("write daemon.json");
    };

    // A pid nothing owns.
    write_daemon(0x00ff_ffff, 918_273);
    let out = ok(&project, &["jobs"]);
    assert!(out.contains("no jobs yet"), "{out}");
    assert!(
        !serve.join("daemon.json").exists(),
        "a daemon.json naming a dead pid is removed rather than left to mislead the next call"
    );

    // A pid that is alive — this test process — but born at another tick:
    // a stranger who inherited the number, which is exactly what a pidfile
    // with no start time cannot tell.
    write_daemon(std::process::id(), 1);
    let out = ok(&project, &["jobs"]);
    assert!(out.contains("no jobs yet"), "{out}");
    assert!(!serve.join("daemon.json").exists());

    // And with no daemon at all, a generate still runs in this process and
    // still leaves a row, so `forge jobs` and `list_runs` see it later.
    //
    // FORGE_FAKE=1, not a missing backend: `moss_sfx` runs in the comfy
    // executor now, so on a developer's own machine — where the ComfyUI unit
    // is up — an unqualified `forge gen sfx` here posted a real graph and
    // spent the card inside `just ci`. A gate never touches a real
    // generator. A fake job is an ordinary job (`designs/serve.md` §1.2): it
    // takes the queue and the lease and writes its row like any other, which
    // is the thing this leg is about.
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "sfx",
            "--prompt",
            "a door",
            "--out",
            "out/audio/sfx/door.wav",
        ])
        .env("FORGE_FAKE", "1")
        .current_dir(&project)
        .output()
        .expect("run forge");
    assert_eq!(
        code(&output),
        0,
        "--- stdout\n{}\n--- stderr\n{}",
        stdout(&output),
        stderr(&output)
    );
    let rows = ok(&project, &["jobs", "--json"]);
    assert!(
        rows.contains("generate_audio.sfx"),
        "an in-process run writes a row anyway: {rows}"
    );
    drop(dir);
}

/// `forge job` refuses an id nobody has by naming the ids that do exist,
/// and `forge serve --status` says plainly when nothing is up.
#[test]
fn a_job_id_nobody_has_is_refused_with_the_ids_that_do() {
    let (_dir, project) = init_project();
    let text = exits(&project, &["job", "show", "j-nope"], 2);
    assert!(text.contains("no job j-nope"), "{text}");
    let out = ok(&project, &["serve", "--status"]);
    assert!(out.contains("daemon    down"), "{out}");
    assert!(
        out.contains("every forge gen still runs"),
        "the answer says what still works without one: {out}"
    );
}

/// Tier `fake` is the answer the project gave, and no environment variable
/// is needed to make it true.
///
/// `serve.md` §5: *fake sets `FORGE_FAKE=1` for every job the project runs,
/// as a first-class answer and not an environment trick.* It was not:
/// every door built its queue options with `..default()`, so `tier` was
/// always `"full"`, and a `--tier fake` project with `FORGE_FAKE` unset ran
/// the **real** sfx path — against a live host it would have leased the
/// card on a project whose doctor says every row is `off`. `ci-fake` and
/// `mcp-session` could not see it because both export `FORGE_FAKE=1`, so
/// this leg removes it from the environment on purpose.
///
/// It also pins the other half of the same defect: the row names the
/// backend the command line runs on, which is what gives a terminal job the
/// budget, the admission refusal and the card ladder the agent's door had.
#[test]
fn a_tier_fake_project_writes_a_placeholder_with_no_forge_fake_in_the_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("game");
    let out = ok(
        dir.path(),
        &[
            "init",
            "--project",
            to_str(&root),
            "--name",
            "game",
            "--make",
            "sfx",
            "--tier",
            "fake",
            "--yes",
        ],
    );
    assert!(out.contains("rig profile humanoid installed"), "{out}");

    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "sfx",
            "--prompt",
            "a heavy iron door",
            "--seconds",
            "1",
            "--out",
            "out/audio/sfx/door.wav",
            "--record",
            "out/audio/sfx/door.json",
        ])
        .env_remove("FORGE_FAKE")
        // A host that is not there, so a run that reached the real path
        // fails loudly instead of quietly succeeding on the developer's own
        // ComfyUI.
        .env("FORGE_COMFY_URL", "http://127.0.0.1:9")
        .current_dir(&root)
        .output()
        .expect("run forge");
    assert_eq!(
        code(&output),
        0,
        "tier fake must not need FORGE_FAKE\n--- stdout\n{}\n--- stderr\n{}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        root.join("out/audio/sfx/door.wav").is_file(),
        "the placeholder is on disk"
    );
    let record = std::fs::read_to_string(root.join("out/audio/sfx/door.json")).expect("the record");
    assert!(
        record.contains("\"fake\": true"),
        "the record says it is a placeholder: {record}"
    );
    let rows = ok(&root, &["jobs", "--json"]);
    assert!(
        rows.contains("\"backend\":\"moss_sfx\""),
        "the terminal door names the backend the MCP door names: {rows}"
    );
    assert!(
        rows.contains("\"fake\":true"),
        "and the row says the job was a fake one: {rows}"
    );
    drop(dir);
}

/// `forge job log <id>` is the verb, and it works on a row that exists.
///
/// The `just job-log` recipe called `forge jobs log`, which does not parse
/// — `error: unexpected argument 'log' found` — and nothing in the gate ran
/// it. This is the verb the recipe now spells, held to a row a real run
/// wrote.
#[test]
fn a_job_s_log_is_read_by_its_own_verb() {
    let (dir, project) = init_project();
    let output = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args([
            "gen",
            "sfx",
            "--prompt",
            "a door",
            "--seconds",
            "1",
            "--out",
            "out/audio/sfx/door.wav",
        ])
        .env("FORGE_FAKE", "1")
        .current_dir(&project)
        .output()
        .expect("run forge");
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let rows: serde_json::Value =
        serde_json::from_str(&ok(&project, &["jobs", "--json"])).expect("rows");
    let id = rows[0]["id"].as_str().expect("an id").to_owned();
    let log = ok(&project, &["job", "log", &id]);
    assert!(
        log.contains("forge gen sfx"),
        "the log opens with the command line it ran: {log}"
    );
    // A listing is a read: it leaves every row exactly as it found it.
    let before = std::fs::read(project.join(format!("out/serve/jobs/{id}.json"))).expect("row");
    let _ = ok(&project, &["jobs"]);
    let _ = ok(&project, &["job", "show", &id]);
    let after = std::fs::read(project.join(format!("out/serve/jobs/{id}.json"))).expect("row");
    assert_eq!(before, after, "a read verb rewrites nothing");
    drop(dir);
}
