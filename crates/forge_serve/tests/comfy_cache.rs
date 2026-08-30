//! `cached` is observed, never inferred.
//!
//! The unit runs with `--cache-none`, so an identical graph genuinely
//! re-runs; a graph-hash-to-job index here would claim a cache hit that
//! never happened, which is the "record that lies" this design is built
//! against. Two observations are allowed: the child saying `execution_cached`
//! on its own `/history` entry, and an earlier finished job whose record
//! claims the same output bytes — which is what fills `same_as`.

mod common;

use forge_serve::{JobSpec, JobState, Queue};

/// A stub generator that writes a record claiming a fixed output hash and
/// echoes a `comfy` block back, exactly as the Python side will.
fn script(dir: &std::path::Path, cached: bool) -> std::path::PathBuf {
    common::stub(
        dir,
        &format!("comfy_gen_{cached}.py"),
        &format!(
            r#"
import json, os, sys
argv = sys.argv[1:]
name = argv[argv.index("--name") + 1]
root = argv[argv.index("--project") + 1]
out = os.path.join(root, "out", "audio", "sfx")
os.makedirs(out, exist_ok=True)
wav = os.path.join(out, name + ".wav")
open(wav, "wb").write(b"RIFF....WAVEfmt ")
record = os.path.join(out, name + ".json")
json.dump({{
    "forge_record": 1,
    "kind": "sfx",
    "tool": "moss_sound_effect",
    "created": "2026-08-30",
    "created_by": "agent:test",
    "backend": {{}},
    "inputs": [{{"role": "prompt", "prompt": "a heavy iron door"}}],
    "params": {{"seed": 815273}},
    "outputs": [{{"path": os.path.relpath(wav, root),
                 "sha256": "sha256:9f1cdeadbeef", "bytes": 16}}],
    "measured": {{}},
    "fake": False,
    "note": None,
}}, open(record, "w"))
print("[sfx] rendering")
print(json.dumps({{
    "ok": True,
    "record": os.path.relpath(record, root),
    "outputs": [os.path.relpath(wav, root)],
    "seed": 815273,
    "comfy": {{
        "template": "backends/moss_sfx/workflows/sfx.api.json",
        "template_sha256": "sha256:9f1c",
        "inputs": {{"prompt": "a heavy iron door", "seed": 815273}},
        "prompt_id": "b1f0",
        "cached": {cached}
    }}
}}))
"#,
            cached = if cached { "True" } else { "False" }
        ),
    )
}

#[test]
fn a_cached_result_says_so() {
    let (dir, project) = common::project();
    let first_script = script(dir.path(), false);
    let queue = common::queue(&project, Some(&first_script));
    let spec = |name: &str| JobSpec {
        kind: String::from("generate_audio.sfx"),
        backend: None,
        argv: vec![
            String::from("sfx"),
            String::from("--name"),
            String::from(name),
        ],
        outputs_claimed: vec![format!("out/audio/sfx/{name}.wav")],
        record: None,
        created_by: String::from("agent:test"),
        fake: None,
    };
    let first = queue.submit(spec("door")).expect("admitted");
    let first = common::finished(queue.as_ref(), &first.id, 30);
    assert_eq!(first.state, JobState::Done, "{:?}", first.message);
    assert!(!first.cached, "the first run is not a cache hit");
    assert_eq!(first.same_as, None);
    // The whole comfy block is the child's, copied and not composed.
    let comfy = first.comfy.clone().expect("the comfy block came back");
    assert_eq!(
        comfy.get("template").and_then(serde_json::Value::as_str),
        Some("backends/moss_sfx/workflows/sfx.api.json")
    );
    assert_eq!(
        comfy.get("prompt_id").and_then(serde_json::Value::as_str),
        Some("b1f0")
    );
    queue.stop();
    drop(queue);

    // The same graph again, and this time the host served it from its node
    // cache and said so.
    let cached_script = script(dir.path(), true);
    let queue = common::queue(&project, Some(&cached_script));
    let second = queue.submit(spec("door_again")).expect("admitted");
    let second = common::finished(queue.as_ref(), &second.id, 30);
    assert_eq!(second.state, JobState::Done, "{:?}", second.message);
    assert!(
        second.cached,
        "the child said execution_cached, so the row says cached"
    );
    assert_eq!(
        second.same_as.as_ref().map(forge_serve::JobId::as_str),
        Some(first.id.as_str()),
        "same_as names the earlier job whose record claims the same bytes"
    );
    queue.stop();
}
