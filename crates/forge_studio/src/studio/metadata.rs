//! Everything known about whatever is selected, in the order it is wanted.
//!
//! # Why this is not a list of key/value pairs
//!
//! It was, and the pairs came out of a parser that flattened whatever JSON it
//! could see into strings. That gave a panel where the prompt — the single most
//! useful thing in a record, the sentence that produced the asset — was one
//! truncated row among twenty, sorted wherever the file happened to put it, and
//! where `in_place: true` sat beside `in_place_mode: detrend` as if they were
//! two independent facts.
//!
//! [`forge_library::Sidecar`] is typed, so this panel can be laid out by
//! *meaning*: the prompt in full at the top, then the recipe that shaped it,
//! then how much of the record to believe, then what is measurably true of the
//! built file.
//!
//! # Reading the provenance line
//!
//! `recorded` means the generator wrote these values as it used them.
//! `reconstructed` means somebody rebuilt the record afterwards: trust the
//! recipe, disbelieve anything a writer could have defaulted. `unknown` means
//! the file exists and nothing is known about how. That distinction is the
//! whole reason the schema has the field, so it is shown rather than implied.
//!
//! # Why the stage is described here
//!
//! Every clip in this window is judged on one model, and the panel already says
//! how many bones a clip drives *on this rig*. Which rig that is, and whether it
//! is one the contract vouches for, belongs beside that number: a clip that
//! drives nothing on a mesh with an inserted root bone is not a bad clip. So the
//! STAGE block follows the selected clip, and stands alone when nothing is
//! selected — clicking a mesh row swaps the stage without moving the
//! selection, so that is where the answer has to appear. A sound is not played
//! on the model at all, so the audio half does not draw it.

use bevy::{animation::AnimationClip, ecs::hierarchy::ChildSpawnerCommands, prelude::*};

use forge_library::{
    Sidecar,
    schema::{DEFAULT_FPS, Generator, Provenance},
};

use crate::{
    binding::ClipDiff,
    rig_findings::{RigFindings, Severity},
    studio::{
        audio_view::AudioLibrary,
        library::{AssetKey, ClipLibrary, ModelLibrary, Selection},
        rig::{ActiveModel, Rig},
        shell::{MetaPanel, StatusLine},
        widgets,
    },
    theme,
};

/// What the window says it can do, when there is nothing more urgent to say.
const HINT: &str = "drag to orbit  scroll to zoom  space play/pause  arrows step  L loop";
/// The same, for a sound: there is nothing to orbit.
const AUDIO_HINT: &str = "space play/pause  up/down browse  drag the bar to seek";

/// Rebuild the metadata panel when the selection moves, or when it learns more.
///
/// Keyed on which asset *and* on whether its facts have arrived, not on a dirty
/// flag: the clip asset lands a frame or two after the library entry, and a
/// sound is only measured when it is first selected, so a panel that drew once
/// and marked itself done would show a permanently empty MEASURED section.
pub(super) fn refresh(
    mut commands: Commands,
    rig: Res<Rig>,
    library: Res<ClipLibrary>,
    audio: Res<AudioLibrary>,
    models: Res<ModelLibrary>,
    active: Res<ActiveModel>,
    selection: Res<Selection>,
    findings: Res<RigFindings>,
    clips: Res<Assets<AnimationClip>>,
    mut status: ResMut<StatusLine>,
    panel: Query<Entity, With<MetaPanel>>,
    mut shown: Local<Option<(Option<AssetKey>, bool, u64)>>,
) {
    let key = selection.key().cloned();
    let Ok(panel) = panel.single() else {
        return;
    };

    // The clip half needs the rig to say anything about bones; the audio half
    // needs nothing but the file. Both report whether what they will draw is
    // the whole story, so that arriving facts trigger exactly one redraw.
    let described = match &key {
        Some(key) if key.is_audio() => audio.get(key).is_some_and(|asset| asset.decoded),
        Some(key) => {
            rig.is_ready()
                && library
                    .get(key)
                    .is_some_and(|item| clips.get(&item.handle).is_some())
        }
        // Nothing selected is nothing to wait for.
        None => true,
    };
    // The findings generation is part of the identity of what is drawn, not
    // just of what is selected: a model swapped onto the stage changes the
    // STAGE block under a clip nobody touched.
    let drawing = (key.clone(), described, findings.generation);
    if *shown == Some(drawing.clone()) {
        return;
    }
    *shown = Some(drawing);

    commands.entity(panel).despawn_children();
    let Some(key) = key else {
        // Nothing is selected — a library with nothing in it, or the frames
        // before the first row is chosen. What is standing on the stage is
        // still worth saying, and it is the only thing left to say.
        HINT.clone_into(&mut status.0);
        let standing = models.get(&active.path).and_then(|m| m.sidecar.clone());
        let model = active.path.clone();
        commands.entity(panel).with_children(|meta| {
            describe_stage(meta, &findings, &model, standing.as_ref());
        });
        return;
    };
    if key.is_audio() {
        let Some(asset) = audio.get(&key) else {
            return;
        };
        AUDIO_HINT.clone_into(&mut status.0);
        let name = asset.name.clone();
        let sidecar = asset.sidecar.clone();
        let measurement = asset.measurement;
        let warnings = asset.warnings();
        let bus = asset.bus().name();
        commands.entity(panel).with_children(|meta| {
            meta.spawn(widgets::section_header("SOUND"));
            meta.spawn(theme::label(name, theme::FONT, theme::TEXT));
            meta.spawn(widgets::field("bus", bus));
            describe(meta, sidecar.as_ref());
            meta.spawn(widgets::section_header("MEASURED"));
            if let Some(m) = measurement {
                for line in [
                    format!(
                        "{:.3} s   {} Hz   {} ch",
                        m.duration, m.sample_rate, m.channels
                    ),
                    format!("peak {:.1} dBFS", m.peak_db),
                    format!("rms  {:.1} dBFS", m.rms_db),
                    format!("lufs {:.1}", m.loudness_lufs),
                    format!("crest {:.1} dB", m.crest_db),
                    format!(
                        "lead {:.0} ms  tail {:.0} ms",
                        m.lead_silence * 1000.0,
                        m.tail_silence * 1000.0
                    ),
                    format!("loop seam {:.1} dB", m.loop_seam_db),
                ] {
                    meta.spawn(theme::label(line, theme::FONT_SMALL, theme::TEXT_DIM));
                }
            } else {
                meta.spawn(theme::label(
                    "measuring...",
                    theme::FONT_SMALL,
                    theme::TEXT_DIM,
                ));
            }
            if warnings.is_empty() {
                meta.spawn(theme::label("clean", theme::FONT, theme::TEXT_DIM));
            } else {
                meta.spawn(widgets::section_header("ISSUES"));
                for warning in warnings {
                    meta.spawn(widgets::paragraph(warning, theme::FONT_SMALL, theme::WARN));
                }
            }
        });
        return;
    }

    // Anything missing is a reason to try again next frame, not to record this
    // clip as drawn: a candidate's asset lands a frame after its library entry.
    let Some(item) = library.get(&key) else {
        return;
    };
    let name = item.entry.name.clone();
    let sidecar = item.entry.sidecar.clone();
    let diff = clips
        .get(&item.handle)
        .filter(|_| rig.is_ready())
        .map(|clip| ClipDiff::new(clip, rig.paths()));
    let mismatch = diff.as_ref().is_some_and(ClipDiff::is_total_mismatch);
    status.0 = if mismatch {
        "NOTHING BOUND - this clip's bone names do not match the model".to_owned()
    } else {
        HINT.to_owned()
    };
    let standing = models.get(&active.path).and_then(|m| m.sidecar.clone());
    let model = active.path.clone();

    commands.entity(panel).with_children(|meta| {
        meta.spawn(widgets::section_header("CLIP"));
        meta.spawn(theme::label(name, theme::FONT, theme::TEXT));
        describe(meta, sidecar.as_ref());
        if let Some(diff) = diff {
            meta.spawn(widgets::section_header("ON THIS RIG"));
            meta.spawn(theme::label(
                format!("{} bones driven", diff.bound.len()),
                theme::FONT,
                if mismatch { theme::WARN } else { theme::TEXT },
            ));
            meta.spawn(theme::label(
                format!("{} at rest, {} orphaned", diff.unbound.len(), diff.orphaned),
                theme::FONT_SMALL,
                theme::TEXT_DIM,
            ));
        }
        // Immediately under "on this rig", because it answers the question that
        // line raises: *which* rig, and is it the one every clip was posed on.
        describe_stage(meta, &findings, &model, standing.as_ref());
    });
}

/// What is standing on the stage, what the rig contract makes of it, and what
/// its own record says.
///
/// # Why the failures are not painted as warnings
///
/// The profile's bare `rig.glb` is a legitimate thing to look at — it *is*
/// the contract — and it fails the skinned-mesh check by being what it is. A
/// block that turned orange for it would cry wolf about the reference rig
/// itself. What must not be missed is loud elsewhere anyway:
/// [`Rig::status`](crate::studio::rig::Rig::status) says a swap was refused,
/// the ON THIS RIG line above turns orange when the selected clip binds to
/// nothing, and every failed finding is logged as a warning the moment it is
/// read. This block states facts about the model, each line carrying the
/// mark the terminal report would give it; the notes and the checks that
/// could not run are dimmed because they change nothing at all.
///
/// Nothing is drawn when no model has stood up. A library with no bodies
/// leaves the stage deliberately empty, and a STAGE heading over "not checked"
/// would invent a problem out of a supported way to run the studio.
fn describe_stage(
    meta: &mut ChildSpawnerCommands,
    findings: &RigFindings,
    model: &str,
    sidecar: Option<&Sidecar>,
) {
    if findings.generation == 0 {
        return;
    }
    meta.spawn(widgets::section_header("STAGE"));
    meta.spawn(theme::label(file_name(model), theme::FONT, theme::TEXT));
    if findings.conforms() {
        meta.spawn(theme::label(
            "conforms to the rig contract",
            theme::FONT_SMALL,
            theme::TEXT_DIM,
        ));
    }
    // Verbatim, in the order they were found: these sentences say what is wrong
    // *and* what it costs, and a panel that summarised them would keep the
    // second half to itself.
    for (severity, text) in findings.lines() {
        if severity == Severity::Ok {
            continue;
        }
        meta.spawn(widgets::paragraph(
            drawable(&format!("{} {text}", severity.mark())),
            theme::FONT_SMALL,
            if severity == Severity::Fail {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            },
        ));
    }
    // The mesh's own record, because a clip is judged on a body and the body
    // has a story too: which lift it came from, what Blender measured of it,
    // and whether the file on stage is the one the record hashes.
    let Some(sidecar) = sidecar else {
        return;
    };
    if let Some(mesh) = sidecar.measured.as_ref().and_then(|m| m.mesh.as_ref()) {
        meta.spawn(widgets::field(
            "mesh",
            &format!(
                "{} verts, {} tris, {} bones, {:.2} m",
                mesh.vertices,
                mesh.triangles,
                mesh.bones_skinned,
                mesh.height()
            ),
        ));
    }
    if let Some(generator) = &sidecar.generator {
        for (key, value) in generator_fields(generator) {
            meta.spawn(widgets::field(&key, &value));
        }
    }
    if let Some(hash) = short_hash(sidecar) {
        meta.spawn(widgets::field("hash", &hash));
    }
}

/// A finding as this window can draw it.
///
/// The findings are written for a terminal, where an em dash is an em dash. The
/// default font ships no glyph for one and a missing glyph draws as a blank box
/// — the same trap [`widgets::collapse_header`] keeps its caret ASCII for — so
/// the panel spells it as a hyphen rather than showing a hole in the sentence.
/// The message itself is left alone: `forge rig check` prints it to a terminal
/// that has the glyph, and two wordings of one finding would be one too many.
fn drawable(message: &str) -> String {
    message.replace('—', "-")
}

/// The file a model path ends in — the panel is 300 px wide, and the directory
/// a mesh lives in is already its section in the browser.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The first twelve hex digits of the content hash: 48 bits, enough to tell
/// two assets apart by eye, and the whole value is in the file for anything
/// that verifies. `None` when the record has no hash at all.
fn short_hash(sidecar: &Sidecar) -> Option<String> {
    let short: String = sidecar
        .content_hash
        .trim_start_matches("sha256:")
        .chars()
        .take(12)
        .collect();
    (!short.is_empty()).then_some(short)
}

/// The record itself: prompt, recipe, provenance, tags.
///
/// Shared by both halves because the envelope is the same question regardless
/// of whether the answer is a clip or a bark — which is exactly why the schema
/// has one envelope.
fn describe(meta: &mut ChildSpawnerCommands, sidecar: Option<&Sidecar>) {
    let Some(sidecar) = sidecar else {
        meta.spawn(widgets::section_header("PROMPT"));
        meta.spawn(widgets::paragraph(
            "no sidecar - nothing is recorded about this asset",
            theme::FONT_SMALL,
            theme::WARN,
        ));
        return;
    };

    meta.spawn(widgets::section_header("PROMPT"));
    match &sidecar.prompt {
        // Whole and wrapped. A prompt is what makes a regenerate a reroll
        // rather than a blank page, and half a sentence is no use for that.
        Some(prompt) => meta.spawn(widgets::paragraph(prompt.clone(), theme::FONT, theme::TEXT)),
        // Not "" and not a guess: the record says it does not know.
        None => meta.spawn(widgets::paragraph(
            "not recorded",
            theme::FONT_SMALL,
            theme::TEXT_DIM,
        )),
    };

    if let Some(recipe) = &sidecar.recipe {
        let fps = sidecar
            .measured
            .as_ref()
            .and_then(|m| m.fps)
            .filter(|f| *f > 0.0)
            .unwrap_or(DEFAULT_FPS);
        meta.spawn(widgets::section_header("RECIPE"));
        // Seconds are what the record holds and frames are what the edit
        // moves, so both are shown rather than making anyone convert.
        meta.spawn(widgets::field(
            "trim in",
            &format!(
                "{:.2}s ({:.0}f)",
                recipe.trim_start_s,
                recipe.trim_start_s * fps
            ),
        ));
        meta.spawn(widgets::field(
            "trim out",
            &format!(
                "{:.2}s ({:.0}f)",
                recipe.trim_end_s,
                recipe.trim_end_s * fps
            ),
        ));
        if let Some(auto) = recipe.auto_trim {
            meta.spawn(widgets::field(
                "auto trim",
                &format!("{auto:?}").to_lowercase(),
            ));
        }
        meta.spawn(widgets::field("in place", recipe.in_place.as_str()));
        meta.spawn(widgets::field("y mode", recipe.y_mode.as_str()));
        meta.spawn(widgets::field(
            "loop",
            &if recipe.looping {
                format!("yes, {:.2}s blend", recipe.loop_blend_s)
            } else {
                String::from("no")
            },
        ));
        meta.spawn(widgets::field(
            "exaggerate",
            &format!("{:.2}", recipe.exaggerate),
        ));
        meta.spawn(widgets::field(
            "elbows",
            &format!("{:.0} deg", recipe.arm_bend_deg),
        ));
        meta.spawn(widgets::field(
            "lean",
            &format!("{:.0} deg", recipe.lean_deg),
        ));
        meta.spawn(widgets::field(
            "shoulder",
            &format!("{:.0} deg", recipe.shoulder_back_deg),
        ));
        if let Some(retime) = &recipe.retime {
            meta.spawn(widgets::field("retime", retime));
        }
        if let Some(clip) = &recipe.clip {
            meta.spawn(widgets::field("clip name", clip));
        }
    }

    meta.spawn(widgets::section_header("PROVENANCE"));
    // The marker first, because it says how much of the rest to believe.
    meta.spawn(theme::label(
        match sidecar.provenance {
            Provenance::Recorded => "recorded as generated",
            Provenance::Reconstructed => "reconstructed after the fact",
            Provenance::Unknown => "unknown - nothing was recorded",
        },
        theme::FONT_SMALL,
        if sidecar.provenance == Provenance::Recorded {
            theme::TEXT_DIM
        } else {
            theme::WARN
        },
    ));
    if let Some(generator) = &sidecar.generator {
        for (key, value) in generator_fields(generator) {
            meta.spawn(widgets::field(&key, &value));
        }
    } else {
        meta.spawn(widgets::field("tool", "unknown"));
    }
    meta.spawn(widgets::field(
        "created by",
        &sidecar.created_by.to_string(),
    ));
    meta.spawn(widgets::field(
        "created",
        if sidecar.created.is_empty() {
            "unknown"
        } else {
            &sidecar.created
        },
    ));
    if let Some(rig) = &sidecar.rig {
        meta.spawn(widgets::field("rig", rig));
    }
    if let Some(path) = &sidecar.source.path {
        meta.spawn(widgets::field("source", path));
    }
    if let Some(skeleton) = &sidecar.source.skeleton {
        meta.spawn(widgets::field("skeleton", skeleton));
    }
    if let Some(hash) = short_hash(sidecar) {
        meta.spawn(widgets::field("hash", &hash));
    }
    if let Some(measured) = &sidecar.measured {
        if let Some(motion) = &measured.root_motion {
            // Net travel as a distance and a heading rather than as x and z:
            // a reviewer asks "how far, and which way", and the axes are the
            // bake's business. 0° is straight ahead (the rig faces −Z).
            let [x, z] = motion.net_m;
            let heading = motion
                .direction_deg
                .map_or_else(String::new, |d| format!(" at {d:.0} deg"));
            meta.spawn(widgets::field(
                "root motion",
                &format!("{:.2} m net{heading}", x.hypot(z)),
            ));
        }
        if let Some(mesh) = &measured.mesh {
            meta.spawn(widgets::field(
                "mesh",
                &format!(
                    "{} verts, {} tris, {:.2} m",
                    mesh.vertices,
                    mesh.triangles,
                    mesh.height()
                ),
            ));
        }
    }
    if let Some(events) = &sidecar.events {
        // `Some` and empty is a fact worth a row: something looked for events
        // and found none, which is not the same as nobody having looked.
        let listed = if events.is_empty() {
            String::from("examined, none")
        } else {
            events
                .iter()
                .map(|e| format!("{} @{:.2}s", e.name, e.t))
                .collect::<Vec<_>>()
                .join(", ")
        };
        meta.spawn(widgets::field("events", &listed));
    }
    if let Some(note) = &sidecar.note {
        meta.spawn(widgets::paragraph(
            note.clone(),
            theme::FONT_SMALL,
            theme::TEXT_DIM,
        ));
    }

    if !sidecar.tags.is_empty() {
        meta.spawn(widgets::section_header("TAGS"));
        meta.spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(4.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|chips| {
            for tag in &sidecar.tags {
                chips.spawn(widgets::tag_chip(tag.clone()));
            }
        });
    }
}

/// The generator's own parameters, in the order a reviewer reads them.
///
/// A seed that was never recorded is shown as `unknown` rather than omitted:
/// "this cannot be regenerated" is the single most useful thing the panel can
/// say about a shipped asset, and an absent row says nothing at all.
fn generator_fields(generator: &Generator) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, value: String| out.push((key.to_owned(), value));
    let or_unknown = |value: Option<String>| value.unwrap_or_else(|| String::from("unknown"));

    match generator {
        Generator::Ardy(p) => {
            push(
                "tool",
                match &p.commit {
                    Some(commit) => format!("ardy@{commit}"),
                    None => String::from("ardy"),
                },
            );
            push("model", or_unknown(p.model.clone()));
            push("seed", or_unknown(p.seed.map(|s| s.to_string())));
            push(
                "asked for",
                or_unknown(p.duration_s.map(|d| format!("{d:.1}s"))),
            );
            if let Some(cfg) = p.cfg {
                push("cfg", format!("{cfg:.1}"));
            }
            if let Some(take) = &p.sweep_take {
                push("sweep take", take.clone());
            }
        }
        // A lifted mesh has no recipe at all: what a reviewer needs to see is
        // which image it came from (pinned by hash), at what resolution and
        // seed, which baker textured it — a licence fact, not a detail — and
        // which Blender step normalised it.
        Generator::Trellis2(p) => {
            push(
                "tool",
                match &p.trellis_commit {
                    Some(commit) => format!("trellis2@{commit}"),
                    None => String::from("trellis2"),
                },
            );
            push("model", or_unknown(p.model.clone()));
            push("seed", or_unknown(p.seed.map(|s| s.to_string())));
            push(
                "resolution",
                or_unknown(p.resolution.map(|r| format!("{r}^3"))),
            );
            if let Some(image) = &p.image {
                push("image", image.clone());
            }
            if let Some(baker) = &p.texture_baker {
                push("texture baker", baker.clone());
            }
            if let Some(post) = &p.post {
                push(
                    "post",
                    match &post.script {
                        Some(script) => format!("{} {script}", post.tool),
                        None => post.tool.clone(),
                    },
                );
            }
        }
        Generator::AceStep(p) => {
            push("tool", String::from("ace_step"));
            push("lm model", or_unknown(p.lm_model.clone()));
            push("dit model", or_unknown(p.dit_model.clone()));
            push("seed", or_unknown(p.seed.clone()));
            if let Some(bpm) = p.bpm {
                push("bpm", bpm.to_string());
            }
            if let Some(key) = &p.keyscale {
                push("key", key.clone());
            }
            if let Some(signature) = &p.timesignature {
                push("time signature", signature.clone());
            }
        }
        Generator::MossSoundEffect(p) => {
            push("tool", String::from("moss_sound_effect"));
            push("model", or_unknown(p.model.clone()));
            push("seed", or_unknown(p.seed.map(|s| s.to_string())));
            if let Some(steps) = p.steps {
                push("steps", steps.to_string());
            }
        }
        Generator::MossTts(p) => {
            push("tool", String::from("moss_tts"));
            push("model", or_unknown(p.model.clone()));
            push("voice", or_unknown(p.voice.clone()));
            push("seed", or_unknown(p.seed.map(|s| s.to_string())));
            if let Some(language) = &p.language {
                push("language", language.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use forge_library::schema::{ArdyParams, Kind, LiftParams};

    use super::*;

    /// The whole point of the schema is that an unrecorded value says so.
    /// A panel that omitted the row would read as "no seed was needed".
    #[test]
    fn an_unrecorded_seed_is_shown_as_unknown() {
        let fields = generator_fields(&Generator::Ardy(ArdyParams {
            commit: Some(String::from("693f74d")),
            model: Some(String::from("core")),
            seed: None,
            ..ArdyParams::default()
        }));
        let value = |key: &str| {
            fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(value("tool"), Some("ardy@693f74d"));
        assert_eq!(value("seed"), Some("unknown"));
        assert_eq!(value("model"), Some("core"));
    }

    /// A record with a real seed must not be dressed up as unknown either.
    #[test]
    fn a_recorded_seed_is_shown_as_itself() {
        let fields = generator_fields(&Generator::Ardy(ArdyParams {
            seed: Some(7),
            ..ArdyParams::default()
        }));
        assert!(fields.iter().any(|(k, v)| k == "seed" && v == "7"));
    }

    /// A lift's texture baker is a licence fact and stays on the panel.
    #[test]
    fn a_lift_names_its_baker_and_its_resolution() {
        let fields = generator_fields(&Generator::Trellis2(LiftParams {
            resolution: Some(1024),
            texture_baker: Some(String::from("nvdiffrast (non-commercial)")),
            ..LiftParams::default()
        }));
        assert!(
            fields
                .iter()
                .any(|(k, v)| k == "resolution" && v == "1024^3")
        );
        assert!(
            fields
                .iter()
                .any(|(k, v)| k == "texture baker" && v.contains("non-commercial"))
        );
        assert!(fields.iter().any(|(k, v)| k == "seed" && v == "unknown"));
    }

    /// The findings are written for a terminal. Drawn as they are, every dash
    /// in them is a blank box in the panel, which reads as a broken sentence in
    /// exactly the lines somebody is trying to act on.
    #[test]
    fn a_finding_is_redrawn_with_glyphs_this_font_has() {
        assert_eq!(
            drawable("no skinned mesh — nothing in the file binds vertices to the rig"),
            "no skinned mesh - nothing in the file binds vertices to the rig"
        );
        assert_eq!(drawable("feet at y=0.000 m"), "feet at y=0.000 m");
    }

    /// The panel is 300 px wide and the directory a mesh lives in is already
    /// its section in the browser, so the path prefix is width spent saying
    /// nothing.
    #[test]
    fn a_model_is_named_by_its_file() {
        assert_eq!(file_name("bodies/vex_runner.glb"), "vex_runner.glb");
        assert_eq!(file_name("vex_runner.glb"), "vex_runner.glb");
        assert_eq!(file_name(""), "");
    }

    /// Sanity: the sidecar type this panel is built around still carries the
    /// three things it lays out — prompt, tags and provenance — and a hash
    /// that is not there is not shown as an empty row.
    #[test]
    fn an_empty_record_is_honest_about_knowing_nothing() {
        let mut sidecar = Sidecar::new(Kind::Sfx, "bark");
        assert!(sidecar.prompt.is_none());
        assert!(sidecar.tags.is_empty());
        assert_eq!(sidecar.provenance, Provenance::Unknown);
        assert_eq!(short_hash(&sidecar), None);
        sidecar.content_hash = String::from("sha256:0123456789abcdef0123");
        assert_eq!(short_hash(&sidecar).as_deref(), Some("0123456789ab"));
    }
}
