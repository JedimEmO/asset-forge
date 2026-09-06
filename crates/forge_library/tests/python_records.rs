//! Does the Python write what the Rust reads?
//!
//! Two writers, two readers, no shared type: `python/forge_gen/records.py`
//! writes generator records and `forge_library::generator_record` reads them;
//! `python/forge_gen/npz.py` writes a `--fake` take and
//! `forge_motion::Take::read` reads it. Nothing structural stops them
//! drifting — a renamed field, a different key order, an unsorted `params`
//! object — and nothing would say so until a lift promoted with its seed
//! missing.
//!
//! So the fixtures under `tests/fixtures/python/` are **captured from the
//! Python writer itself** by `python/tests/capture_fixtures.py` — pinned
//! clock, fixed payload bytes, paths relative to a pretend project — and this
//! file makes three claims about them:
//!
//! * every record parses through [`GeneratorRecord`] at the current schema
//!   with its projection intact (`lift_params`, `ardy_params`, …);
//! * re-serialising each produces **the same bytes** the Python wrote — the
//!   stronger claim, which catches a key the Rust does not know, a key the
//!   Python forgot, a different order, a float formatted differently;
//! * the take reads as eight frames of a standing figure.
//!
//! When `python3` is on PATH the capture is also re-run into a temp dir and
//! held equal to what is committed, so the committed fixtures cannot lag the
//! writer. Re-capture after changing either side:
//!
//! ```sh
//! BLESS_PYTHON_FIXTURES=1 cargo test -p forge_library --test python_records
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use forge_library::GeneratorRecord;
use forge_library::generator_record::{RECORD_SCHEMA, RecordKind};
use forge_motion::Take;

/// One fixture per kind, plus the fake lift and one comfy run.
///
/// **Only `forge_record: 2` fixtures are held to byte equality.** A v1
/// fixture is a read-only case ([`a_v1_record_still_reads_and_is_not_promoted`]):
/// `Option` serialises as `null`, so a v1 record round-tripped through the
/// Rust writer would grow the four keys the schema added and the comparison
/// would be a test of the writer's opinion rather than of the two writers
/// agreeing.
const RECORDS: [(&str, RecordKind); 11] = [
    ("lift.json", RecordKind::Lift),
    ("prop.json", RecordKind::Prop),
    ("rig.json", RecordKind::Rig),
    ("export.json", RecordKind::Export),
    ("take.json", RecordKind::Take),
    ("sfx.json", RecordKind::Sfx),
    ("sfx_comfy.json", RecordKind::Sfx),
    ("music.json", RecordKind::Music),
    ("speech.json", RecordKind::Speech),
    ("voice.json", RecordKind::Voice),
    ("fake_lift.json", RecordKind::Lift),
];

/// A record shipped before the schema went to 2, kept by hand rather than
/// captured: `records.py` cannot write a 1 any more, which is the point.
const V1: &str = "v1_lift.json";

/// `sha256:` of the five bytes `probe`, which every stand-in file is made of.
const PROBE_SHA: &str = "sha256:ba9c736f19e7f60b7f6764adb0b7908c0a2b394e09b6c09863528c7f2bc86095";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python")
}

fn capture_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/tests/capture_fixtures.py")
}

/// Run the Python capture into `dir`; `None` when there is no `python3`.
fn capture_into(dir: &Path) -> Option<()> {
    let status = Command::new("python3")
        .arg(capture_script())
        .arg(dir)
        .status()
        .ok()?;
    assert!(status.success(), "capture_fixtures.py failed: {status}");
    Some(())
}

/// Re-capture when blessing, so the committed fixtures are what the writer
/// writes today.
fn maybe_bless() {
    if std::env::var_os("BLESS_PYTHON_FIXTURES").is_some() {
        capture_into(&fixtures()).expect("python3 is needed to bless the fixtures");
    }
}

#[test]
fn every_record_parses_and_round_trips_byte_for_byte() {
    maybe_bless();
    for (file, kind) in RECORDS {
        let path = fixtures().join(file);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let record = GeneratorRecord::load(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(record.forge_record, RECORD_SCHEMA, "{file}");
        assert_eq!(record.kind, kind, "{file}");
        assert!(!record.created_by.is_empty(), "{file}: created_by");
        assert_eq!(record.created, "2026-08-23", "{file}: the pinned clock");
        assert!(
            record.outputs.len() == 1 && record.outputs[0].sha256.as_deref() == Some(PROBE_SHA),
            "{file}: one output made of `probe`"
        );
        assert_eq!(
            record.outputs[0].bytes,
            Some(5),
            "{file}: the output's size is measured"
        );
        for input in &record.inputs {
            match (&input.path, &input.prompt) {
                (Some(_), None) => assert_eq!(
                    input.sha256.as_deref(),
                    Some(PROBE_SHA),
                    "{file}: a file input is hashed"
                ),
                (None, Some(_)) => {
                    assert!(input.sha256.is_none(), "{file}: a prompt is not hashed");
                }
                other => panic!("{file}: an input is a file or a prompt, got {other:?}"),
            }
        }
        let again = record.to_bytes().unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(
            String::from_utf8(again).expect("utf8"),
            String::from_utf8(bytes).expect("utf8"),
            "{file}: field order and formatting are the byte contract with records.py"
        );
    }
}

#[test]
fn every_v2_record_says_which_executor_ran_and_a_comfy_one_says_what_it_ran() {
    maybe_bless();
    for (file, _) in RECORDS {
        let record = GeneratorRecord::load(&fixtures().join(file)).expect("reads");
        assert!(
            matches!(record.backend.executor.as_deref(), Some("env" | "comfy")),
            "{file}: forge_record 2 says which executor ran, env ones included"
        );
    }
    // An env run has no workflow and no host, and says so with nulls rather
    // than with a default that would read as a measurement.
    let env = GeneratorRecord::load(&fixtures().join("sfx.json")).expect("sfx");
    assert_eq!(env.backend.executor.as_deref(), Some("env"));
    assert_eq!(env.backend.workflow_sha256, None);
    assert_eq!(env.backend.comfyui_commit, None);
    assert_eq!(env.backend.packs, None);

    let comfy = GeneratorRecord::load(&fixtures().join("sfx_comfy.json")).expect("sfx_comfy");
    assert_eq!(comfy.backend.executor.as_deref(), Some("comfy"));
    assert_eq!(
        comfy.backend.commit, None,
        "a comfy backend has no checkout of its own"
    );
    assert_eq!(
        comfy.backend.comfyui_commit.as_deref(),
        Some("169fcf35a2fc163fec31338b816503ddac0d3fcf")
    );
    assert!(
        comfy
            .backend
            .workflow_sha256
            .as_deref()
            .is_some_and(|h| h.starts_with("sha256:")),
        "the tracked template file is hashed"
    );
    assert_eq!(
        comfy
            .backend
            .packs
            .as_ref()
            .expect("packs")
            .get("https://github.com/diodiogod/TTS-Audio-Suite")
            .map(String::as_str),
        Some("b7e41a2c")
    );
    assert_eq!(
        comfy.param_str("workflow").as_deref(),
        Some("sfx.api.json"),
        "the patch is knobs, and knobs live in params"
    );
    let sound = comfy.sound_effect_params();
    assert_eq!(
        sound.model.as_deref(),
        Some("OpenMOSS-Team/MOSS-SoundEffect-v2.0"),
        "the same projection reads both executors"
    );
    assert_eq!(sound.seed, Some(815_273));
}

#[test]
fn a_v1_record_still_reads_and_is_not_promoted() {
    let path = fixtures().join(V1);
    let record = GeneratorRecord::load(&path).expect("a shipped v1 record still reads");
    assert_eq!(record.forge_record, 1, "read, never promoted in place");
    assert_eq!(
        record.backend.executor, None,
        "v1 predates the question; null means unknown"
    );
    assert_eq!(record.backend.packs, None);
    // Everything a v1 record did say, it still says.
    let params = record.lift_params();
    assert_eq!(params.seed, Some(42));
    assert_eq!(params.resolution, Some(1024));
    assert_eq!(
        params.texture_baker.as_deref(),
        Some("nvdiffrast (non-commercial)")
    );
    assert_eq!(record.prompt(), None);
}

#[test]
fn the_projections_read_what_the_python_stated() {
    maybe_bless();
    let lift = GeneratorRecord::load(&fixtures().join("lift.json")).expect("lift");
    let params = lift.lift_params();
    assert_eq!(params.seed, Some(42));
    assert_eq!(params.resolution, Some(1024));
    assert_eq!(params.decimation_target_vertices, Some(6000));
    assert_eq!(params.remesh, Some(true));
    assert_eq!(
        params.texture_baker.as_deref(),
        Some("nvdiffrast (non-commercial)"),
        "the licence fact travels"
    );
    assert_eq!(params.model.as_deref(), Some("microsoft/TRELLIS.2-4B"));
    assert_eq!(
        params.trellis_commit.as_deref(),
        Some("75fbf0183001ed9876c8dbb35de6b68552ee08bd")
    );
    assert_eq!(
        params.image.as_deref(),
        Some("assets-src/refs/props/barrel.png"),
        "relative to the project"
    );
    assert_eq!(params.image_sha256.as_deref(), Some(PROBE_SHA));
    assert_eq!(params.lift_sha256.as_deref(), Some(PROBE_SHA));
    assert!(!lift.fake);
    assert_eq!(
        lift.measured
            .get("triangles")
            .and_then(serde_json::Value::as_u64),
        Some(11804)
    );

    let take = GeneratorRecord::load(&fixtures().join("take.json")).expect("take");
    let ardy = take.ardy_params();
    assert_eq!(ardy.repo.as_deref(), Some("nv-tlabs/ardy"));
    assert_eq!(ardy.seed, Some(7));
    assert_eq!(ardy.duration_s, Some(4.0));
    assert_eq!(ardy.cfg, Some(2.5));
    assert_eq!(ardy.sample, Some(1));
    assert_eq!(ardy.sweep_take.as_deref(), Some("walk__d4_c2_s7_1.npz"));
    assert_eq!(take.prompt(), Some("a slow walk forward"));

    let sfx = GeneratorRecord::load(&fixtures().join("sfx.json")).expect("sfx");
    let sound = sfx.sound_effect_params();
    assert_eq!(sound.model.as_deref(), Some("OpenMOSS/MOSS-SoundEffect-v2"));
    assert_eq!(sound.seed, Some(3));
    assert_eq!(sound.duration_s, Some(1.5));
    assert_eq!(sound.steps, Some(50));
    assert_eq!(sound.cfg, Some(4.0));

    let music = GeneratorRecord::load(&fixtures().join("music.json")).expect("music");
    let ace = music.ace_step_params();
    assert_eq!(
        ace.seed.as_deref(),
        Some("1,2"),
        "ACE-Step's seed pair is text"
    );
    assert_eq!(ace.bpm, Some(96));
    assert_eq!(ace.keyscale.as_deref(), Some("F minor"));
    assert_eq!(ace.lm_model.as_deref(), Some("acestep-5Hz-lm-0.6B"));
    assert_eq!(ace.dit_model.as_deref(), Some("acestep-v15-turbo"));
    assert_eq!(ace.duration_s, Some(90.0));

    let speech = GeneratorRecord::load(&fixtures().join("speech.json")).expect("speech");
    let tts = speech.speech_params();
    assert_eq!(tts.voice.as_deref(), Some("calm"));
    assert_eq!(tts.language.as_deref(), Some("en"));
    assert_eq!(tts.reference.as_deref(), Some("assets-src/voices/calm.wav"));
    assert_eq!(tts.seed, Some(11));
    assert_eq!(
        speech.note.as_deref(),
        Some("première ligne — the accent is on purpose: ensure_ascii=False"),
        "non-ASCII is written raw on both sides"
    );

    let voice = GeneratorRecord::load(&fixtures().join("voice.json")).expect("voice");
    assert_eq!(voice.kind, RecordKind::Voice);
    assert!(
        voice.inputs.is_empty(),
        "a designed voice is handed nothing"
    );
    assert_eq!(voice.param_i64("seed"), Some(7), "every design is seeded");
    assert_eq!(
        voice.param_str("instruction").as_deref(),
        Some("Deep, slow, weathered male voice, English, low pitch, unhurried, grave and calm")
    );
    assert_eq!(voice.param_f32("audio_temperature"), Some(1.5));
    assert_eq!(
        voice.output().map(|o| o.path.as_str()),
        Some("assets-src/voices/crypt_warden/ref.wav")
    );
    assert_eq!(
        voice.backend.model.as_deref(),
        Some("OpenMOSS-Team/MOSS-VoiceGenerator")
    );

    let fake = GeneratorRecord::load(&fixtures().join("fake_lift.json")).expect("fake");
    assert!(fake.fake);
    assert_eq!(fake.backend.commit.as_deref(), Some("fake"));
    assert_eq!(fake.created_by, "unknown");
    let params = fake.lift_params();
    assert_eq!(params.seed, None, "null stays unknown, never a default");
    assert_eq!(params.texture_baker, None);
}

#[test]
fn the_fake_take_reads_as_a_standing_figure() {
    maybe_bless();
    let take = Take::read(fixtures().join("still.npz")).expect("npz.write_take is a take");
    assert_eq!(take.frames(), 8);
    assert!((take.fps - 20.0).abs() < f32::EPSILON);
    assert_eq!(take.prompt, "stand still");
    let contacts = take.contacts.as_ref().expect("contacts were written");
    assert_eq!(contacts.len(), 8);
    assert!(
        contacts
            .iter()
            .all(|frame| frame.iter().all(|planted| *planted))
    );
    let hips = take.root[0];
    assert!(
        hips.y > 0.8 && hips.y < 1.1,
        "hips at a standing height, got {hips}"
    );
    assert!(hips.x.abs() < 0.01 && hips.z.abs() < 0.01);
    for frame in &take.rotations {
        for rotation in frame {
            assert!(
                rotation.abs_diff_eq(glam::Quat::IDENTITY, 1e-6),
                "rest pose: identity on every joint"
            );
        }
    }
}

#[test]
fn the_committed_fixtures_are_what_the_writer_writes_today() {
    maybe_bless();
    let scratch = tempfile::tempdir().expect("tempdir");
    if capture_into(scratch.path()).is_none() {
        eprintln!("python3 not found: skipping the live capture comparison");
        return;
    }
    for (file, _) in RECORDS {
        let fresh = std::fs::read(scratch.path().join(file)).expect("fresh capture");
        let committed = std::fs::read(fixtures().join(file)).expect("committed fixture");
        assert_eq!(
            String::from_utf8(fresh).expect("utf8"),
            String::from_utf8(committed).expect("utf8"),
            "{file} lags records.py — BLESS_PYTHON_FIXTURES=1 cargo test -p forge_library --test python_records"
        );
    }
    let fresh = std::fs::read(scratch.path().join("still.npz")).expect("fresh take");
    let committed = std::fs::read(fixtures().join("still.npz")).expect("committed take");
    assert_eq!(fresh, committed, "still.npz lags npz.py");
}
