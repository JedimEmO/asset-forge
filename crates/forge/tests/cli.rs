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
        "init", "catalog", "manifest", "verify", "audit", "rebake", "migrate", "promote", "audio",
        "rig", "doctor", "gpu", "studio", "mcp",
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

#[test]
fn the_placeholders_say_their_phase_and_exit_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (verb, phase) in [("gpu", "P2"), ("studio", "P3"), ("mcp", "P4")] {
        let text = exits(dir.path(), &[verb, "--whatever", "x"], 1);
        assert!(
            text.contains(&format!("not yet: lands in {phase}")),
            "{text}"
        );
    }
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
    let out = ok(&root, &["doctor"]);
    assert!(out.contains("no drift"), "{out}");
    assert!(out.contains("manifest current"), "{out}");
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

    let out = ok(&root, &["audio", "list"]);
    assert!(out.contains("sfx/tone.wav"), "{out}");
    assert!(out.contains("1 file(s)"), "{out}");
    let out = ok(&root, &["catalog", "--kind", "sfx"]);
    assert!(out.contains("a pluck"), "{out}");
    ok(&root, &["manifest", "--check"]);
    ok(&root, &["verify"]);
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
