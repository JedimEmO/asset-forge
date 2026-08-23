//! Seeing what the library holds: meshes, clips, sounds — one line each.
//!
//! All three read the scan-derived [`Catalog`], never the manifest: what an
//! agent wants about an asset is the authoring record — the prompt, the
//! tags, how much of the record to believe, the recipe — and that is what
//! the sidecar beside the file says. The manifest is the game's projection
//! of the same records, and `doctor` reports whether it is current.
//!
//! Every row carries provenance, not only the doubtful ones: `recorded` and
//! `reconstructed` are different claims, and a column that only appears
//! sometimes reads as a warning rather than as a fact.

use std::fmt::Write as _;

use forge_library::metrics_cache::{AudioMeasurement, MetricsCache};
use forge_library::schema::{Measured, RootYMode};
use forge_library::{AssetRecord, Catalog, ClipRecipe, Kind, Project, Query};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util;

/// Width the name column is padded to, so the rest lines up.
const NAME_COLUMN: usize = 22;

/// Arguments for `list_models`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ListModelsArgs {
    /// Restrict to one kind: `model` (a static mesh — prop, fixture, held
    /// weapon) or `body` (a rigged character every clip plays on). Omit for
    /// both.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// Case-insensitive substring, matched against the name *and* the
    /// prompt — "sword" finds `sword` and anything described as one.
    #[serde(default)]
    pub(crate) filter: Option<String>,
}

/// Arguments for `list_clips`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ListClipsArgs {
    /// Case-insensitive substring, matched against the clip name *and* its
    /// prompt — "roll" finds `roll` and anything whose prompt describes
    /// rolling.
    #[serde(default)]
    pub(crate) filter: Option<String>,
    /// Exact tag, case-insensitive, e.g. `loop`.
    #[serde(default)]
    pub(crate) tag: Option<String>,
}

/// Arguments for `list_audio`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ListAudioArgs {
    /// Restrict to one kind: `sfx`, `music` or `voice`. Omit for all three.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// Case-insensitive substring, matched against the name and the prompt.
    #[serde(default)]
    pub(crate) filter: Option<String>,
}

#[tool_router(router = list_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// List the meshes the library ships.
    #[tool(
        description = "List every shipped mesh: bodies (rigged characters every clip plays \
                       on) and models (static props, fixtures, held weapons), each with its \
                       prompt, tags, provenance, the generator that made it and its measured \
                       size. Optional kind (model|body) and filter (substring of name or \
                       prompt). Cheap: no GPU, no rendering. Call this before render_model \
                       to learn valid names."
    )]
    async fn list_models(&self, Parameters(args): Parameters<ListModelsArgs>) -> CallToolResult {
        let wanted = match parse_mesh_kind(args.kind.as_deref()) {
            Ok(kind) => kind,
            Err(refusal) => return refusal,
        };
        let catalog = Catalog::scan(&self.config.project);
        let kinds: Vec<Kind> = wanted.map_or_else(|| vec![Kind::Body, Kind::Model], |k| vec![k]);
        let mut out = String::new();
        let mut shown = 0usize;
        for kind in kinds {
            let query = Query {
                kind: Some(kind),
                text: args.filter.clone(),
                tag: None,
            };
            let records = catalog.find(&query);
            shown += records.len();
            let _ = writeln!(out, "{} ({}):", plural(kind), records.len());
            for record in records {
                let _ = writeln!(out, "{}", model_line(record));
            }
            out.push('\n');
        }
        if shown == 0 {
            return util::refuse(format!(
                "no mesh matches {}. the library holds {} body/ies and {} model(s) — call \
                 list_models with no arguments to see them all",
                describe_filter(args.filter.as_deref(), None),
                catalog.names(Some(Kind::Body)).len(),
                catalog.names(Some(Kind::Model)).len()
            ));
        }
        out.push_str(
            "render_model shows you one; a body is also what render_clip_strip poses a clip on.\n",
        );
        util::report(out)
    }

    /// List the animation clips in the library.
    #[tool(
        description = "List animation clips with their prompt, tags, provenance, measured \
                       length and the recipe they were baked with (trims, in-place mode, \
                       loop, exaggeration). Optional filter (substring of name or prompt) \
                       and tag. Cheap: no GPU, no rendering. Call this first to learn valid \
                       clip names for render_clip_strip, and to check whether a name you \
                       want to promote to is already taken."
    )]
    async fn list_clips(&self, Parameters(args): Parameters<ListClipsArgs>) -> CallToolResult {
        let catalog = Catalog::scan(&self.config.project);
        let query = Query {
            kind: Some(Kind::Clip),
            text: args.filter.clone(),
            tag: args.tag.clone(),
        };
        let clips = catalog.find(&query);
        let total = catalog.names(Some(Kind::Clip)).len();
        if clips.is_empty() {
            return util::refuse(format!(
                "no clip matches {}. the library holds {total} clip(s) — call list_clips \
                 with no arguments to see them all",
                describe_filter(args.filter.as_deref(), args.tag.as_deref())
            ));
        }
        let mut out = format!(
            "stage body: {}\n{} of {total} clip(s) under {}:\n\n",
            self.config
                .stage_body
                .as_deref()
                .unwrap_or("(unset — the first body)"),
            clips.len(),
            self.config.project.kind_dir(Kind::Clip).display()
        );
        for record in &clips {
            let _ = writeln!(out, "{}", clip_line(record));
        }
        out.push_str(
            "\nlength is measured from the baked file. render_clip_strip shows a clip posed \
             on a body.\n",
        );
        util::report(out)
    }

    /// List audio assets with their measurements.
    #[tool(
        description = "List audio assets with their measurements: duration, channels, peak, \
                       loudness, and any defect (! clipped or silent, ? worth a look). \
                       Optional kind (sfx|music|voice) and filter (substring of name or \
                       prompt). Measurements are cached under out/, so this is cheap after \
                       the first call. Call it before inspect_audio, to compare loudness \
                       across a library, and to check whether a name you want to \
                       promote_audio under is already taken."
    )]
    async fn list_audio(&self, Parameters(args): Parameters<ListAudioArgs>) -> CallToolResult {
        let kind = match parse_audio_kind(args.kind.as_deref()) {
            Ok(kind) => kind,
            Err(refusal) => return refusal,
        };
        let project = self.config.project.clone();
        let filter = args.filter.clone();
        // Decoding is blocking and, on a cold cache, is the whole library —
        // doing it on a runtime thread would stall every other tool call.
        let listed =
            tokio::task::spawn_blocking(move || list_audio(&project, filter.as_deref(), kind))
                .await
                .unwrap_or_else(|err| Err(format!("the measuring task failed: {err}")));
        match listed {
            Ok(text) => util::report(text),
            Err(message) => util::refuse(message),
        }
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::list_tools()
}

/// What every row says after the name: the prompt, the tags, the
/// provenance, the generator — or that there is no record to say it.
fn record_meta(record: &AssetRecord) -> String {
    let Some(sidecar) = &record.sidecar else {
        return format!(
            "  (no sidecar{})",
            record
                .sidecar_error
                .as_deref()
                .map_or(String::new(), |e| format!(": {e}"))
        );
    };
    let mut meta = String::new();
    if let Some(prompt) = &sidecar.prompt {
        let _ = write!(meta, "  \"{}\"", util::first_line(prompt, 60));
    }
    if !sidecar.tags.is_empty() {
        let _ = write!(meta, "  [{}]", sidecar.tags.join(", "));
    }
    let _ = write!(meta, "  {}", sidecar.provenance);
    if let Some(generator) = &sidecar.generator {
        let _ = write!(meta, " via {}", generator.tool());
    }
    meta
}

/// One mesh's row: name, path, record, and the measured size when the
/// record has one.
fn model_line(record: &AssetRecord) -> String {
    let mut line = format!("  {:NAME_COLUMN$}  {}", record.name, record.rel_path);
    line.push_str(&record_meta(record));
    if let Some(mesh) = record
        .sidecar
        .as_ref()
        .and_then(|s| s.measured.as_ref())
        .and_then(|m| m.mesh.as_ref())
    {
        let _ = write!(
            line,
            "  {:.2} m tall, {} verts, {} tris",
            mesh.height(),
            mesh.vertices,
            mesh.triangles
        );
    }
    line
}

/// One clip's row: name, measured length, record, recipe, events.
fn clip_line(record: &AssetRecord) -> String {
    let mut line = format!("  {:NAME_COLUMN$}", record.name);
    if let Some(length) = record
        .sidecar
        .as_ref()
        .and_then(|s| s.measured.as_ref())
        .and_then(measured_length)
    {
        let _ = write!(line, "  {length:>14}");
    } else {
        let _ = write!(line, "  {:>14}", "(unmeasured)");
    }
    line.push_str(&record_meta(record));
    if let Some(sidecar) = &record.sidecar {
        if let Some(recipe) = &sidecar.recipe {
            let _ = write!(line, "  recipe: {}", recipe_summary(recipe));
        }
        match &sidecar.events {
            Some(events) if !events.is_empty() => {
                let _ = write!(line, "  {} event(s)", events.len());
            }
            Some(_) => line.push_str("  no events"),
            None => {}
        }
    }
    line
}

/// `1.450s / 29 f` when both are measured, seconds or frames alone when
/// only one is, nothing when neither.
fn measured_length(measured: &Measured) -> Option<String> {
    match (measured.duration_s, measured.frames) {
        (Some(seconds), Some(frames)) => Some(format!("{seconds:.3}s / {frames} f")),
        (Some(seconds), None) => Some(format!("{seconds:.3}s")),
        (None, Some(frames)) => Some(format!("{frames} f")),
        (None, None) => None,
    }
}

/// The knobs of a recipe that are not at identity, in one phrase — the
/// record states every knob, and the listing shows the ones that did
/// something. A recipe at identity says so, because "nothing was done to
/// this take" is a fact and a blank is not.
fn recipe_summary(recipe: &ClipRecipe) -> String {
    let mut parts = Vec::new();
    if recipe.trim_start_s != 0.0 || recipe.trim_end_s != 0.0 {
        parts.push(format!(
            "trim {:.2}s/{:.2}s",
            recipe.trim_start_s, recipe.trim_end_s
        ));
    }
    if let Some(auto) = recipe.auto_trim {
        parts.push(format!("auto-trim {}", format!("{auto:?}").to_lowercase()));
    }
    if recipe.in_place != forge_library::schema::InPlaceMode::Off {
        parts.push(format!("in-place {}", recipe.in_place.as_str()));
    }
    if recipe.y_mode != RootYMode::Off {
        parts.push(format!("root-y {}", recipe.y_mode.as_str()));
    }
    if recipe.looping {
        parts.push(format!("loop {:.2}s", recipe.loop_blend_s));
    }
    if (recipe.exaggerate - 1.0).abs() > f32::EPSILON {
        parts.push(format!("exaggerate {:.2}", recipe.exaggerate));
    }
    if recipe.arm_bend_deg != 0.0 {
        parts.push(format!("arm-bend {:.0}°", recipe.arm_bend_deg));
    }
    if recipe.lean_deg != 0.0 {
        parts.push(format!("lean {:.0}°", recipe.lean_deg));
    }
    if recipe.shoulder_back_deg != 0.0 {
        parts.push(format!("shoulder-back {:.0}°", recipe.shoulder_back_deg));
    }
    if let Some(retime) = &recipe.retime {
        parts.push(format!("retime {retime}"));
    }
    if let Some(clip) = &recipe.clip {
        parts.push(format!("as \"{clip}\""));
    }
    if parts.is_empty() {
        String::from("identity")
    } else {
        parts.join(", ")
    }
}

/// The whole audio listing, measured from the cache where possible.
///
/// Returns the rendered table, or the message to refuse with. Blocking on
/// purpose: the caller runs it off the runtime.
fn list_audio(
    project: &Project,
    filter: Option<&str>,
    kind: Option<Kind>,
) -> Result<String, String> {
    let catalog = Catalog::scan(project);
    let query = Query {
        kind,
        text: filter.map(ToOwned::to_owned),
        tag: None,
    };
    let records: Vec<&AssetRecord> = catalog
        .find(&query)
        .into_iter()
        .filter(|r| r.kind.is_audio())
        .collect();
    if records.is_empty() {
        let total = catalog
            .records()
            .iter()
            .filter(|r| r.kind.is_audio())
            .count();
        return Err(format!(
            "no sound matches {}. the library holds {total} sound(s) — call list_audio \
             with no arguments to see them all",
            describe_filter(filter, None)
        ));
    }

    let mut cache = MetricsCache::load(project);
    let mut out = String::from("sounds (! = defect, ? = worth a look):\n\n");
    for record in &records {
        let cached = cache.get(&record.rel_path, &record.path).copied();
        let measurement = match cached {
            Some(measurement) => Some(measurement),
            None => match forge_audio::decode(&record.path) {
                Ok(audio) => {
                    let measurement = AudioMeasurement::from(&forge_audio::measure(&audio));
                    cache.insert(&record.rel_path, &record.path, measurement);
                    Some(measurement)
                }
                Err(err) => {
                    let _ = writeln!(out, "! {:32} {err}", record.rel_path);
                    None
                }
            },
        };
        let Some(measurement) = measurement else {
            continue;
        };
        let metrics = metrics_from_cache(&measurement);
        let warnings = metrics.warnings();
        let flag = if metrics.silent || metrics.longest_clip_run >= 3 {
            '!'
        } else if warnings.is_empty() {
            ' '
        } else {
            '?'
        };
        let _ = write!(
            out,
            "{flag} {:32} {:>7.2}s {}ch  peak {:>6.1}dB  lufs {:>6.1}",
            record.rel_path,
            metrics.duration,
            metrics.channels,
            metrics.peak_db,
            metrics.loudness_lufs,
        );
        out.push_str(&record_meta(record));
        if let Some(first) = warnings.first() {
            let _ = write!(out, "  <- {first}");
        }
        out.push('\n');
    }
    // Best effort: the cache is derived, and a failure to write it costs
    // only the next call's decode.
    let _ = cache.save();
    let _ = write!(
        out,
        "\n{} sound(s). inspect_audio shows one as a plot.",
        records.len()
    );
    Ok(out)
}

/// A cached measurement back as `forge_audio` states it, so
/// [`forge_audio::Metrics::warnings`] — the tested rules — decides what is
/// wrong with it rather than a second copy of the thresholds here.
pub(crate) fn metrics_from_cache(m: &AudioMeasurement) -> forge_audio::Metrics {
    forge_audio::Metrics {
        duration: m.duration,
        sample_rate: m.sample_rate,
        channels: m.channels,
        peak: m.peak,
        peak_db: m.peak_db,
        rms_db: m.rms_db,
        loudness_lufs: m.loudness_lufs,
        crest_db: m.crest_db,
        full_scale_samples: m.full_scale_samples,
        longest_clip_run: m.longest_clip_run,
        lead_silence: m.lead_silence,
        tail_silence: m.tail_silence,
        dc_offset: m.dc_offset,
        silent: m.silent,
        loop_seam_db: m.loop_seam_db,
    }
}

/// The plural a heading uses.
fn plural(kind: Kind) -> &'static str {
    match kind {
        Kind::Body => "bodies",
        Kind::Model => "models",
        Kind::Clip => "clips",
        Kind::Sfx | Kind::Music | Kind::Voice => "sounds",
    }
}

/// The `kind` argument of `list_models`, or a refusal naming the two words
/// that work.
pub(crate) fn parse_mesh_kind(text: Option<&str>) -> Result<Option<Kind>, CallToolResult> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    match text.to_ascii_lowercase().as_str() {
        "model" | "models" => Ok(Some(Kind::Model)),
        "body" | "bodies" => Ok(Some(Kind::Body)),
        other => Err(util::refuse(format!(
            "{other:?} is not a mesh kind — use model or body"
        ))),
    }
}

/// The `kind` argument of `list_audio`, or a refusal naming the three that
/// exist. `clip`, `body` and `model` are refused by name rather than as
/// "not a kind", because an agent that passes one has picked the wrong
/// tool and the useful answer says which tool it wanted.
pub(crate) fn parse_audio_kind(text: Option<&str>) -> Result<Option<Kind>, CallToolResult> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    match Kind::parse(text) {
        Some(Kind::Clip) => Err(util::refuse(
            "clip is not an audio kind — use list_clips for clips",
        )),
        Some(Kind::Body | Kind::Model) => Err(util::refuse(format!(
            "{text} is not an audio kind — use list_models for meshes"
        ))),
        Some(kind) => Ok(Some(kind)),
        None => Err(util::refuse(format!(
            "{text:?} is not an audio kind — use sfx, music or voice"
        ))),
    }
}

/// The filter, phrased for a refusal that has to say what was asked.
fn describe_filter(filter: Option<&str>, tag: Option<&str>) -> String {
    match (
        filter.map(str::trim).filter(|f| !f.is_empty()),
        tag.map(str::trim).filter(|t| !t.is_empty()),
    ) {
        (None, None) => String::from("(no filter)"),
        (Some(filter), None) => format!("filter {filter:?}"),
        (None, Some(tag)) => format!("tag {tag:?}"),
        (Some(filter), Some(tag)) => format!("filter {filter:?} and tag {tag:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::util::frame_text;
    use forge_library::schema::{InPlaceMode, Provenance, Sidecar};
    use std::path::PathBuf;

    fn record(kind: Kind, name: &str, sidecar: Option<Sidecar>) -> AssetRecord {
        AssetRecord {
            kind,
            name: name.to_owned(),
            rel_path: format!("{}/{name}.glb", kind.dir()),
            path: PathBuf::from("/assets")
                .join(kind.dir())
                .join(format!("{name}.glb")),
            sidecar,
            sidecar_error: None,
        }
    }

    #[test]
    fn a_clip_row_carries_length_prompt_tags_provenance_and_recipe() {
        let mut sidecar = Sidecar::new(Kind::Clip, "roll");
        sidecar.prompt = Some(String::from("A person dives forward into a roll."));
        sidecar.tags = vec![String::from("action"), String::from("loop")];
        sidecar.provenance = Provenance::Reconstructed;
        sidecar.recipe = Some(ClipRecipe {
            trim_start_s: 0.25,
            trim_end_s: 1.3,
            in_place: InPlaceMode::Detrend,
            looping: true,
            loop_blend_s: 0.2,
            ..ClipRecipe::default()
        });
        sidecar.measured = Some(Measured {
            frames: Some(49),
            duration_s: Some(2.45),
            ..Measured::default()
        });
        sidecar.events = Some(Vec::new());
        let line = clip_line(&record(Kind::Clip, "roll", Some(sidecar)));
        assert!(line.contains("roll"), "{line}");
        assert!(line.contains("2.450s / 49 f"), "{line}");
        assert!(
            line.contains("\"A person dives forward into a roll.\""),
            "{line}"
        );
        assert!(line.contains("[action, loop]"), "{line}");
        assert!(line.contains("reconstructed"), "{line}");
        assert!(
            line.contains("recipe: trim 0.25s/1.30s, in-place detrend, loop 0.20s"),
            "{line}"
        );
        assert!(line.ends_with("no events"), "{line}");
    }

    #[test]
    fn a_record_with_no_sidecar_says_so_rather_than_looking_ordinary() {
        let line = clip_line(&record(Kind::Clip, "walk", None));
        assert!(line.contains("(no sidecar)"), "{line}");
        assert!(line.contains("(unmeasured)"), "{line}");
        let line = model_line(&record(Kind::Model, "sword", None));
        assert!(line.contains("models/sword.glb"), "{line}");
        assert!(line.contains("(no sidecar)"), "{line}");
    }

    #[test]
    fn an_identity_recipe_says_identity() {
        assert_eq!(recipe_summary(&ClipRecipe::default()), "identity");
        let named = ClipRecipe {
            clip: Some(String::from("Walk")),
            exaggerate: 1.15,
            ..ClipRecipe::default()
        };
        assert_eq!(recipe_summary(&named), "exaggerate 1.15, as \"Walk\"");
    }

    #[test]
    fn kinds_are_parsed_and_the_wrong_tool_is_named() {
        assert_eq!(parse_mesh_kind(None).expect("none"), None);
        assert_eq!(
            parse_mesh_kind(Some(" Bodies ")).expect("bodies"),
            Some(Kind::Body)
        );
        assert!(parse_mesh_kind(Some("clip")).is_err());

        let refusal = parse_audio_kind(Some("clip")).expect_err("refuse");
        assert!(frame_text(&refusal).contains("list_clips"));
        let refusal = parse_audio_kind(Some("body")).expect_err("refuse");
        assert!(frame_text(&refusal).contains("list_models"));
        assert!(matches!(parse_audio_kind(Some("SFX")), Ok(Some(Kind::Sfx))));
        assert!(matches!(parse_audio_kind(Some("  ")), Ok(None)));
        assert!(parse_audio_kind(Some("drums")).is_err());
    }

    #[test]
    fn an_empty_result_repeats_what_was_asked_for() {
        assert_eq!(describe_filter(None, None), "(no filter)");
        assert_eq!(describe_filter(Some(" roll "), None), "filter \"roll\"");
        assert_eq!(
            describe_filter(Some("roll"), Some("loop")),
            "filter \"roll\" and tag \"loop\""
        );
    }

    #[tokio::test]
    async fn the_lists_answer_from_a_promoted_library() {
        let (_dir, project) = testing::library();
        let server = testing::server(project);

        let models = server
            .list_models(Parameters(ListModelsArgs {
                kind: None,
                filter: None,
            }))
            .await;
        let text = frame_text(&models);
        assert_ne!(models.is_error, Some(true), "{text}");
        assert!(text.contains("bodies (1)"), "{text}");
        assert!(text.contains("models (0)"), "{text}");
        assert!(text.contains("mannequin"), "{text}");
        assert!(text.contains("bodies/mannequin.glb"), "{text}");
        assert!(text.contains("[fixture]"), "{text}");
        assert!(text.contains("m tall"), "{text}");

        let clips = server
            .list_clips(Parameters(ListClipsArgs {
                filter: None,
                tag: Some(String::from("ACTION")),
            }))
            .await;
        let text = frame_text(&clips);
        assert_ne!(clips.is_error, Some(true), "{text}");
        assert!(text.contains("1 of 1 clip(s)"), "{text}");
        assert!(text.contains("49 f"), "{text}");
        assert!(text.contains("in-place detrend"), "{text}");

        let none = server
            .list_clips(Parameters(ListClipsArgs {
                filter: Some(String::from("zzz")),
                tag: None,
            }))
            .await;
        assert_eq!(none.is_error, Some(true));
        assert!(frame_text(&none).contains("filter \"zzz\""));

        let sounds = server
            .list_audio(Parameters(ListAudioArgs {
                kind: None,
                filter: None,
            }))
            .await;
        assert_eq!(
            sounds.is_error,
            Some(true),
            "an empty audio library refuses"
        );
        assert!(frame_text(&sounds).contains("0 sound(s)"));
    }

    #[test]
    fn a_measurement_survives_the_cache_unchanged() {
        let original = forge_audio::Metrics {
            duration: 1.5,
            sample_rate: 48_000,
            channels: 2,
            peak: 0.9,
            peak_db: -0.9,
            rms_db: -18.0,
            loudness_lufs: -20.0,
            crest_db: 17.1,
            full_scale_samples: 4,
            longest_clip_run: 3,
            lead_silence: 0.01,
            tail_silence: 0.05,
            dc_offset: 0.0002,
            silent: false,
            loop_seam_db: 1.2,
        };
        let round_tripped = metrics_from_cache(&AudioMeasurement::from(&original));
        assert_eq!(format!("{round_tripped:?}"), format!("{original:?}"));
        // Three consecutive full-scale samples is the clipping threshold,
        // and it has to survive the trip or a listing built from cached
        // numbers would call a clipped file clean.
        assert!(
            round_tripped
                .warnings()
                .iter()
                .any(|w| w.contains("clipped")),
            "{:?}",
            round_tripped.warnings()
        );
    }
}
