//! Reading and writing the `<stem>.json` beside an asset.
//!
//! This module is the only place that touches a sidecar file, which is the
//! point of it. Before, three programs each had their own reader: a
//! twelve-line splitter on `:` in the viewer, a copy of it in the audio
//! tool, and `serde_json::Value` poking in the MCP server. The line splitters
//! could not see inside a nested object, silently produced `"metas"` as a key
//! with an empty value, and would have read a new schema as gibberish rather
//! than as an error.

use std::path::{Path, PathBuf};

use crate::schema::{Generator, RootYMode, Sidecar};
use crate::{Result, read_bytes, write_atomic};

/// The sidecar path for an asset file: same directory, same stem, `.json`.
#[must_use]
pub fn path_for(asset: &Path) -> PathBuf {
    asset.with_extension("json")
}

/// Load a sidecar at schema 1.
///
/// # Errors
///
/// Fails when the file cannot be read, does not parse as JSON, declares a
/// schema this build does not know, or does not match the types.
pub fn load(path: &Path) -> Result<Sidecar> {
    Sidecar::from_slice(&read_bytes(path)?, path)
}

/// Load the sidecar beside an asset, or `None` when there is none.
///
/// A missing sidecar is not an error: a file dropped into the library by
/// hand has to be listed anyway — an asset you cannot see is worse than an
/// asset you know nothing about — and `verify` is where its absence is
/// reported.
///
/// # Errors
///
/// Fails only when a sidecar exists and is unreadable.
pub fn load_beside(asset: &Path) -> Result<Option<Sidecar>> {
    let path = path_for(asset);
    if path.is_file() {
        load(&path).map(Some)
    } else {
        Ok(None)
    }
}

/// Write a sidecar, atomically.
///
/// Pretty-printed at two-space indent with a trailing newline. These files
/// are git-tracked and reviewed as diffs, so the writer's bytes are a
/// contract: the same record always serialises to the same bytes, and a
/// re-save of an unchanged record is no diff at all.
///
/// # Errors
///
/// Fails when the file cannot be written or the rename over the old one fails.
pub fn save(path: &Path, sidecar: &Sidecar) -> Result<()> {
    write_atomic(path, &sidecar.to_bytes()?)
}

/// The record as label/value pairs, in reading order, for a metadata panel.
///
/// Ordered by what a reviewer looks at first: the prompt, then who and when,
/// then how it was made, then the recipe, then the numbers. Nulls are dropped
/// — except in the recipe, where every knob is shown including the ones at
/// identity, because "this clip has no lean" is information and a blank row is
/// not.
#[must_use]
pub fn display_pairs(sidecar: &Sidecar) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, value: String| out.push((key.to_owned(), value));

    if let Some(prompt) = &sidecar.prompt {
        push("prompt", prompt.clone());
    }
    push("kind", sidecar.kind.to_string());
    push("name", sidecar.name.clone());
    if !sidecar.tags.is_empty() {
        push("tags", sidecar.tags.join(", "));
    }
    push("created", sidecar.created.clone());
    push("created by", sidecar.created_by.to_string());
    push("provenance", sidecar.provenance.to_string());
    if let Some(rig) = &sidecar.rig {
        push("rig", rig.clone());
    }

    if let Some(generator) = &sidecar.generator {
        push("tool", generator.tool().to_owned());
        for (key, value) in generator_pairs(generator) {
            push(&key, value);
        }
    }
    if let Some(path) = &sidecar.source.path {
        push("source", path.clone());
    }
    if let Some(skeleton) = &sidecar.source.skeleton {
        push("skeleton", skeleton.clone());
    }

    if let Some(recipe) = &sidecar.recipe {
        push(
            "trim",
            format!("{:.2}s / {:.2}s", recipe.trim_start_s, recipe.trim_end_s),
        );
        if let Some(auto) = recipe.auto_trim {
            push("auto trim", format!("{auto:?}").to_lowercase());
        }
        push("in place", recipe.in_place.as_str().to_owned());
        // Only when it says something: a clip at `off` is the common case,
        // and a row of them is noise a reviewer scrolls past rather than
        // reads.
        if recipe.y_mode != RootYMode::Off {
            push("root y", recipe.y_mode.as_str().to_owned());
        }
        push(
            "loop",
            if recipe.looping {
                format!("yes, {:.2}s blend", recipe.loop_blend_s)
            } else {
                String::from("no")
            },
        );
        push("exaggerate", format!("{:.2}", recipe.exaggerate));
        push("elbows", format!("{:.0}°", recipe.arm_bend_deg));
        push("lean", format!("{:.0}°", recipe.lean_deg));
        push("shoulder", format!("{:.0}°", recipe.shoulder_back_deg));
        if let Some(retime) = &recipe.retime {
            push("retime", retime.clone());
        }
        if let Some(clip) = &recipe.clip {
            push("clip", clip.clone());
        }
    }

    if let Some(measured) = &sidecar.measured {
        if let Some(frames) = measured.frames {
            push("frames", frames.to_string());
        }
        if let Some(fps) = measured.fps {
            push("fps", format!("{fps:.0}"));
        }
        if let Some(duration) = measured.duration_s {
            push("duration", format!("{duration:.2}s"));
        }
        if let Some(speed) = measured.avg_speed_mps {
            push("avg speed", format!("{speed:.3} m/s"));
        }
        if let Some(motion) = &measured.root_motion {
            push("root motion", root_motion_summary(motion));
        }
        if let Some(mesh) = &measured.mesh {
            push(
                "mesh",
                format!(
                    "{} verts, {} tris, {} bones, {:.2} m tall",
                    mesh.vertices,
                    mesh.triangles,
                    mesh.bones_skinned,
                    mesh.height()
                ),
            );
        }
    }

    if let Some(events) = &sidecar.events {
        // `Some` and empty is a fact worth a row: something looked for events
        // and found none, which is not the same as nobody having looked.
        if events.is_empty() {
            push("events", String::from("examined, none"));
        } else {
            let listed: Vec<String> = events
                .iter()
                .map(|e| format!("{} @{:.2}s", e.name, e.t))
                .collect();
            push(
                "events",
                format!("{} — {}", events.len(), listed.join(", ")),
            );
        }
    }

    // Twelve hex digits is 48 bits: enough to tell two assets apart by eye
    // in a panel, and the full value is in the file for anything that
    // needs to actually verify.
    let short: String = sidecar
        .content_hash
        .trim_start_matches(crate::hash::PREFIX)
        .chars()
        .take(12)
        .collect();
    if !short.is_empty() {
        push("hash", short);
    }
    if let Some(note) = &sidecar.note {
        push("note", note.clone());
    }
    out
}

/// The root-motion block in one cell: how far, which way, how fast at peak.
///
/// The track itself is summarised as a point count — a hundred coordinate
/// pairs are exactly what a metadata panel is not for.
fn root_motion_summary(motion: &crate::schema::RootMotion) -> String {
    let [x, z] = motion.net_m;
    let mut parts = vec![format!("net {:.2} m", x.hypot(z))];
    if let Some(direction) = motion.direction_deg {
        parts.push(format!("{direction:.0}°"));
    }
    if let Some(peak) = motion.peak_speed_mps {
        parts.push(format!("peak {peak:.2} m/s"));
    }
    parts.push(format!("{}-point track", motion.track_xz_m.len()));
    parts.join(", ")
}

/// The generator's own parameters, flattened for display.
fn generator_pairs(generator: &Generator) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            out.push((key.to_owned(), value));
        }
    };
    match generator {
        Generator::Ardy(p) => {
            push("model", p.model.clone());
            push("commit", p.commit.clone());
            push("seed", p.seed.map(|s| s.to_string()));
            push("generated for", p.duration_s.map(|d| format!("{d:.1}s")));
            push("cfg", p.cfg.map(|c| format!("{c:.1}")));
            push("sweep take", p.sweep_take.clone());
        }
        Generator::Trellis2(p) => {
            push("model", p.model.clone());
            push("commit", p.trellis_commit.clone());
            push("resolution", p.resolution.map(|r| format!("{r}³")));
            push("seed", p.seed.map(|s| s.to_string()));
            push("texture baker", p.texture_baker.clone());
            push("image", p.image.clone());
            if let Some(post) = &p.post {
                push("post", post.script.clone());
                push("blender", post.version.clone());
            }
        }
        Generator::AceStep(p) => {
            push("lm model", p.lm_model.clone());
            push("dit model", p.dit_model.clone());
            push("seed", p.seed.clone());
            push("bpm", p.bpm.map(|b| b.to_string()));
            push("key", p.keyscale.clone());
            push("time signature", p.timesignature.clone());
            push("lyrics", p.lyrics.clone());
        }
        Generator::MossSoundEffect(p) => {
            push("model", p.model.clone());
            push("seed", p.seed.map(|s| s.to_string()));
            push("steps", p.steps.map(|s| s.to_string()));
            push("cfg", p.cfg.map(|c| format!("{c:.1}")));
        }
        Generator::MossTts(p) => {
            push("model", p.model.clone());
            push("voice", p.voice.clone());
            push("reference", p.reference.clone());
            push("language", p.language.clone());
            push("seed", p.seed.map(|s| s.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Kind;

    #[test]
    fn a_saved_record_reloads_and_a_missing_one_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = dir.path().join("walk.glb");
        assert_eq!(load_beside(&asset).expect("no sidecar"), None);
        let mut record = Sidecar::new(Kind::Clip, "walk");
        record.content_hash = String::from("sha256:00");
        save(&path_for(&asset), &record).expect("save");
        assert_eq!(load_beside(&asset).expect("sidecar"), Some(record));
        let text = std::fs::read_to_string(path_for(&asset)).expect("read");
        assert!(text.starts_with("{\n  \"schema\": 1,"), "{text}");
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn display_pairs_lead_with_the_prompt_and_show_every_recipe_knob() {
        let mut record = Sidecar::new(Kind::Clip, "walk");
        record.prompt = Some(String::from("A person walks."));
        record.recipe = Some(crate::schema::ClipRecipe::default());
        let pairs = display_pairs(&record);
        assert_eq!(pairs[0].0, "prompt");
        assert!(pairs.iter().any(|(k, v)| k == "lean" && v == "0°"));
        assert!(!pairs.iter().any(|(k, _)| k == "hash"), "no hash, no row");
    }
}
