//! Does a bundle carry the clips it was given, unchanged?
//!
//! A bundle claims to be a **merge**: the body's file with each clip's
//! channels re-pointed at the body's nodes by bone name, and every sampler's
//! values copied. So the bar here is bytes — the output accessor of every
//! channel in the bundle must be exactly the accessor the source clip
//! carried — plus the two things a merge could get wrong invisibly: a channel
//! landing on the wrong node, and the file ceasing to be self-contained.
//!
//! The inputs are the shipped library — `assets/bodies/vex_runner.glb` and
//! two shipped clips — because a merge has no oracle to compare against but
//! its own inputs, and reading them live is the point: whatever those files
//! are today, their channels have to come through.

mod common;

use common::repo_path;
use forge_motion::bundle::{ClipSource, bundle};
use serde_json::Value;

/// The body every test merges into.
const BODY: &str = "assets/bodies/vex_runner.glb";
/// Two shipped clips: a loop with root travel and a travelling roll.
const CLIPS: [&str; 2] = ["assets/clips/walk.glb", "assets/clips/roll.glb"];

fn read(relative: &str) -> Vec<u8> {
    let path = repo_path(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A GLB's two chunks, as the document and its buffer's own bytes.
fn open(bytes: &[u8]) -> (Value, Vec<u8>) {
    assert_eq!(&bytes[..4], b"glTF", "not a glb");
    let total = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes")) as usize;
    assert_eq!(
        total,
        bytes.len(),
        "the header's length is the file's length"
    );
    let (mut doc, mut bin, mut at) = (None, None, 12);
    while at + 8 <= total {
        let length = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
        let payload = &bytes[at + 8..at + 8 + length];
        match &bytes[at + 4..at + 8] {
            b"JSON" => doc = Some(serde_json::from_slice(payload).expect("the JSON chunk")),
            b"BIN\0" => bin = Some(payload.to_vec()),
            other => panic!("a third chunk: {other:?}"),
        }
        at += 8 + length;
    }
    (doc.expect("a JSON chunk"), bin.expect("a BIN chunk"))
}

/// The bytes one accessor actually reads: element by element, through its
/// view's offset and stride, so a copy that moved the data is still equal and
/// a copy that changed a value is not.
fn accessor_bytes(doc: &Value, bin: &[u8], accessor: usize) -> Vec<u8> {
    let accessor = &doc["accessors"][accessor];
    let view = &doc["bufferViews"][usize(accessor, "bufferView")];
    assert_eq!(
        accessor["componentType"].as_u64(),
        Some(5126),
        "every animation accessor in play here is float"
    );
    let components = match accessor["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        other => panic!("unexpected accessor type {other:?}"),
    };
    let size = components * 4;
    let stride = view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map_or(size, |s| s as usize);
    let base = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize
        + accessor
            .get("byteOffset")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
    (0..usize(accessor, "count"))
        .flat_map(|element| {
            let at = base + element * stride;
            bin[at..at + size].to_vec()
        })
        .collect()
}

/// Floats an accessor reads.
fn accessor_f32(doc: &Value, bin: &[u8], accessor: usize) -> Vec<f32> {
    accessor_bytes(doc, bin, accessor)
        .chunks_exact(4)
        .map(|w| f32::from_le_bytes(w.try_into().expect("4 bytes")))
        .collect()
}

fn usize(value: &Value, key: &str) -> usize {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} is an index"))
        .try_into()
        .expect("an index fits")
}

/// Node name by index.
fn node_name(doc: &Value, node: usize) -> String {
    doc["nodes"][node]["name"]
        .as_str()
        .unwrap_or_else(|| panic!("node {node} has no name"))
        .to_owned()
}

/// The channels of one animation as `(bone, path, input accessor, output
/// accessor)`, which is everything a merge is allowed to change and
/// everything it is not.
fn channels(doc: &Value, animation: usize) -> Vec<(String, String, usize, usize)> {
    let animation = &doc["animations"][animation];
    animation["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .map(|channel| {
            let sampler = &animation["samplers"][usize(channel, "sampler")];
            (
                node_name(doc, usize(&channel["target"], "node")),
                channel["target"]["path"].as_str().expect("path").to_owned(),
                usize(sampler, "input"),
                usize(sampler, "output"),
            )
        })
        .collect()
}

/// The bundle of the two shipped clips at a given scale, with its document.
fn bundled(scale: f32) -> (Vec<u8>, Value, Vec<u8>) {
    let body = read(BODY);
    let clips: Vec<Vec<u8>> = CLIPS.iter().map(|c| read(c)).collect();
    let sources: Vec<ClipSource<'_>> = CLIPS
        .iter()
        .zip(&clips)
        .map(|(name, bytes)| ClipSource {
            name: stem(name),
            bytes,
        })
        .collect();
    let bundle = bundle(&body, &sources, scale).expect("the merge");
    let (doc, bin) = open(&bundle.bytes);
    (bundle.bytes, doc, bin)
}

fn stem(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".glb"))
        .expect("a .glb path")
}

#[test]
fn each_clip_arrives_as_one_animation_with_its_own_channels() {
    let body = read(BODY);
    let clips: Vec<Vec<u8>> = CLIPS.iter().map(|c| read(c)).collect();
    let sources: Vec<ClipSource<'_>> = CLIPS
        .iter()
        .zip(&clips)
        .map(|(name, bytes)| ClipSource {
            name: stem(name),
            bytes,
        })
        .collect();
    let merged = bundle(&body, &sources, 1.0).expect("the merge");
    let (doc, bin) = open(&merged.bytes);

    assert_eq!(
        doc["animations"].as_array().expect("animations").len(),
        CLIPS.len(),
        "one animation per clip"
    );
    for (at, clip) in clips.iter().enumerate() {
        let (source, source_bin) = open(clip);
        // The name an engine binds by is the clip's name inside its own file.
        let expected = source["animations"][0]["name"]
            .as_str()
            .expect("the clip names its animation")
            .to_owned();
        assert_eq!(merged.animations[at], expected);
        assert_eq!(doc["animations"][at]["name"].as_str(), Some(&*expected));

        let theirs = channels(&source, 0);
        let ours = channels(&doc, at);
        assert_eq!(ours.len(), theirs.len(), "{expected}: channel count");
        for (ours, theirs) in ours.iter().zip(&theirs) {
            assert_eq!(ours.0, theirs.0, "{expected}: the bone a channel drives");
            assert_eq!(ours.1, theirs.1, "{expected}: the path it drives");
            assert_eq!(
                accessor_bytes(&doc, &bin, ours.3),
                accessor_bytes(&source, &source_bin, theirs.3),
                "{expected}: {} {} values are not the clip's",
                ours.0,
                ours.1
            );
            assert_eq!(
                accessor_bytes(&doc, &bin, ours.2),
                accessor_bytes(&source, &source_bin, theirs.2),
                "{expected}: {} {} key times are not the clip's",
                ours.0,
                ours.1
            );
        }
        // A baked clip's 28 channels share one input accessor; copying it 28
        // times would still pass every check above and make a fatter file.
        let inputs: std::collections::BTreeSet<usize> = ours.iter().map(|c| c.2).collect();
        assert_eq!(inputs.len(), 1, "{expected}: the key times are copied once");
    }
}

#[test]
fn the_same_inputs_give_the_same_bytes() {
    let (first, ..) = bundled(1.0);
    let (second, ..) = bundled(1.0);
    assert_eq!(first, second, "the writer is not deterministic");
}

#[test]
fn the_bundle_is_self_contained() {
    let (bytes, doc, _) = bundled(1.0);
    // One JSON chunk, one BIN chunk: `open` panics on a third and on a
    // header whose length is not the file's.
    let (_, bin) = open(&bytes);
    assert_eq!(
        doc["buffers"].as_array().expect("buffers").len(),
        1,
        "one buffer"
    );
    assert!(
        doc["buffers"][0].get("uri").is_none(),
        "a bundle carries its bytes"
    );
    assert!(
        usize(&doc["buffers"][0], "byteLength") <= bin.len(),
        "the buffer fits in the chunk"
    );
    assert!(
        !serde_json::to_string(&doc)
            .expect("json")
            .contains("\"uri\""),
        "no uri anywhere in the document"
    );
}

#[test]
fn motion_scale_multiplies_the_root_track_and_touches_no_rotation() {
    let (_, plain, plain_bin) = bundled(1.0);
    let (_, half, half_bin) = bundled(0.5);

    for at in 0..CLIPS.len() {
        let theirs = channels(&plain, at);
        let ours = channels(&half, at);
        assert_eq!(ours.len(), theirs.len());
        for (ours, theirs) in ours.iter().zip(&theirs) {
            let scaled = accessor_f32(&half, &half_bin, ours.3);
            let plain = accessor_f32(&plain, &plain_bin, theirs.3);
            if ours.1 == "translation" && ours.0 == "Hips" {
                assert_eq!(scaled.len(), plain.len());
                for (scaled, plain) in scaled.iter().zip(&plain) {
                    assert!(
                        (scaled - plain * 0.5).abs() < f32::EPSILON,
                        "{} travel: {scaled} is not half of {plain}",
                        ours.0
                    );
                }
                assert!(
                    plain.iter().any(|v| v.abs() > 1e-4),
                    "the walk's root track is not all zeros, or this proves nothing"
                );
            } else {
                assert_eq!(
                    scaled, plain,
                    "{} {} moved, and only the root track may",
                    ours.0, ours.1
                );
            }
        }
    }
}

#[test]
fn a_clip_naming_a_bone_the_body_lacks_is_refused() {
    // A prop is a body with no bones at all — the same refusal a body
    // missing one bone gets, and one the library can actually hand over.
    let barrel = read("assets/models/barrel.glb");
    let walk = read(CLIPS[0]);
    let refusal = bundle(
        &barrel,
        &[ClipSource {
            name: "walk",
            bytes: &walk,
        }],
        1.0,
    )
    .expect_err("a prop has no Hips")
    .to_string();
    assert!(
        refusal.contains("Hips") && refusal.contains("walk"),
        "the refusal must name the bone and the clip: {refusal}"
    );
}

#[test]
fn two_clips_cannot_share_one_animation_name() {
    let body = read(BODY);
    let walk = read(CLIPS[0]);
    let twice = [
        ClipSource {
            name: "walk",
            bytes: &walk,
        },
        ClipSource {
            name: "walk_again",
            bytes: &walk,
        },
    ];
    let refusal = bundle(&body, &twice, 1.0)
        .expect_err("an engine binds an animation by name")
        .to_string();
    assert!(refusal.contains("walk"), "{refusal}");
}
