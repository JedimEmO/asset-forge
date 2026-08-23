//! Shared fixtures: the oracle clips, their sidecars, and the humanoid rig.

// Each integration test binary compiles this module separately and uses a
// different subset of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use forge_motion::{Edit, InPlace, RigDef, Take, YMode};
use glam::{Quat, Vec4};
use serde_json::Value;

/// The frame rate a sidecar recipe's seconds are converted at when the take
/// does not say — ARDY generates at 20 fps and every fixture agrees.
const DEFAULT_FPS: f32 = 20.0;

/// Repository-relative path, anchored at this crate's manifest.
pub(crate) fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// A fixture set under `tests/fixtures/<dir>`.
pub(crate) fn fixtures(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir)
}

/// Everything one baseline clip provides: the raw take, the edit its sidecar
/// recipe resolves to, the clip name inside the `.glb`, and the baked bytes.
pub(crate) struct Case {
    pub(crate) take: Take,
    pub(crate) edit: Edit,
    pub(crate) clip_name: String,
    pub(crate) shipped: Vec<u8>,
}

/// Load a baseline clip's take, sidecar recipe and `.glb` bytes from
/// `tests/fixtures/<dir>`.
///
/// Everything is a pinned copy — the `.glb` and sidecar because the four
/// under `blender/` were baked by the Blender path this crate replaced and
/// are the oracle that proves the convention, and the takes because a live
/// source directory is NOT immutable: re-promoting a clip replaces its source
/// take in place, which once silently rewrote this oracle's input. A live
/// library is re-baked by this very crate, so reading any live file here
/// would make the test compare the writer against itself (or against a
/// different take entirely).
pub(crate) fn case(dir: &str, name: &'static str) -> Case {
    let fixtures = fixtures(dir);
    let take = Take::read(fixtures.join(format!("{name}.npz")))
        .unwrap_or_else(|e| panic!("{name} take: {e}"));
    let sidecar = std::fs::read_to_string(fixtures.join(format!("{name}.json")))
        .unwrap_or_else(|e| panic!("{name} sidecar: {e}"));
    let sidecar: Value =
        serde_json::from_str(&sidecar).unwrap_or_else(|e| panic!("{name} sidecar: {e}"));
    let recipe = sidecar
        .get("recipe")
        .filter(|r| !r.is_null())
        .unwrap_or_else(|| panic!("{name} sidecar has no recipe"));
    let edit = recipe_to_edit(recipe, take.fps);
    let clip_name = recipe
        .get("clip")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{name} recipe names no clip"))
        .to_owned();
    let shipped = std::fs::read(fixtures.join(format!("{name}.glb")))
        .unwrap_or_else(|e| panic!("{name} glb: {e}"));
    Case {
        take,
        edit,
        clip_name,
        shipped,
    }
}

/// The sidecar recipe as an [`Edit`], with trims converted from seconds to
/// frames and the `src:dst,…` retime spec parsed and sorted.
///
/// This is a transcription of the library crate's recipe conversion, kept
/// local so the bake's proof does not link the library: trims round to the
/// nearest frame, an absent `y_mode` is `off` (every sidecar written before
/// that key existed rebuilds unchanged), and retime pairs are sorted by
/// source time — the retired Python sorted before interpolating, so the sort
/// is part of the format.
fn recipe_to_edit(recipe: &Value, fps: f32) -> Edit {
    let fps = if fps > 0.0 { fps } else { DEFAULT_FPS };
    let seconds = |key: &str| {
        recipe
            .get(key)
            .and_then(Value::as_f64)
            .map_or(0.0, |v| v as f32)
    };
    let frames = |seconds: f32| (seconds * fps).round().max(0.0) as usize;
    let mode = |key: &str| {
        recipe
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("off")
            .to_owned()
    };
    let in_place = match mode("in_place").as_str() {
        "off" => InPlace::Off,
        "strip" => InPlace::Strip,
        "detrend" => InPlace::Detrend,
        other => panic!("unknown in_place {other:?}"),
    };
    let y_mode = match mode("y_mode").as_str() {
        "off" => YMode::Off,
        "strip" => YMode::Strip,
        "detrend" => YMode::Detrend,
        other => panic!("unknown y_mode {other:?}"),
    };
    let retime = recipe
        .get("retime")
        .and_then(Value::as_str)
        .map_or_else(Vec::new, parse_retime);
    let knob = |key: &str, default: f32| {
        recipe
            .get(key)
            .and_then(Value::as_f64)
            .map_or(default, |v| v as f32)
    };
    Edit {
        trim_start: frames(seconds("trim_start_s")),
        trim_end: frames(seconds("trim_end_s")),
        retime,
        in_place,
        y_mode,
        arm_bend_deg: knob("arm_bend_deg", 0.0),
        lean_deg: knob("lean_deg", 0.0),
        shoulder_back_deg: knob("shoulder_back_deg", 0.0),
        exaggerate: knob("exaggerate", 1.0),
        loop_blend_s: knob("loop_blend_s", 0.0),
    }
}

/// Parse a `src:dst,src:dst,…` retime spec into sorted keypoints.
fn parse_retime(spec: &str) -> Vec<(f32, f32)> {
    let mut pairs: Vec<(f32, f32)> = spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (src, dst) = part
                .split_once(':')
                .unwrap_or_else(|| panic!("{part:?} is not src:dst"));
            let number = |text: &str| {
                text.trim()
                    .parse::<f32>()
                    .unwrap_or_else(|e| panic!("{text:?} in {spec:?}: {e}"))
            };
            (number(src), number(dst))
        })
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pairs
}

/// The humanoid profile's rig, the rest-pose source of truth every bake
/// reads.
pub(crate) fn humanoid_rig() -> RigDef {
    let bytes = std::fs::read(repo_path("rigs/humanoid/rig.glb")).expect("rigs/humanoid/rig.glb");
    RigDef::from_glb(&bytes).expect("rigs/humanoid/rig.glb parses as a rig")
}

/// Largest per-component difference between two quaternions naming the same
/// rotation, sign-aligned first because `q` and `-q` are the same rotation.
pub(crate) fn component_gap(a: Quat, b: Quat) -> f32 {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (Vec4::from(a) - Vec4::from(b)).abs().max_element()
}
