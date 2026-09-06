//! Hearing a sound by looking at it: one file's numbers and its plot.
//!
//! Audio is the one asset this server inspects in process — decode,
//! measure and draw are [`forge_audio::cli::inspect`], the same function
//! behind `forge audio inspect`, so the numbers an agent reads are the
//! numbers the terminal prints. No renderer, no GPU; a long music track is
//! seconds of CPU, which is why it runs off the async runtime.
//!
//! A sound that is not in the library yet — a generate under `out/audio`,
//! the thing an agent is deciding whether to promote — is inspected by path.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use forge_library::metrics_cache::{AudioMeasurement, MetricsCache};
use forge_library::{Catalog, Project};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util;

/// Arguments for `inspect_audio`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct InspectAudioArgs {
    /// The sound: a library name, file name or asset-relative path as
    /// `list_audio` prints it, or a path to any audio file — a generate
    /// under out/audio that has not been promoted yet.
    pub(crate) name_or_path: String,
    /// Set false to get only the measurements, for sweeping many files.
    #[serde(default)]
    pub(crate) return_image: Option<bool>,
}

#[tool_router(router = audio_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// Measure one sound and draw it.
    #[tool(
        description = "Inspect one sound: its measurements (duration, peak, RMS, loudness, \
                       crest, lead and tail silence, DC offset, clipping run), its recorded \
                       metadata when it is in the library, and a plot — waveform over a \
                       log-frequency spectrogram — as an inline image. You cannot listen, but \
                       you can see clipping as flat-topping, dead air as a gap, a truncated \
                       tail as a cliff, and over-compression as a solid block with no \
                       dynamics. Takes a name from list_audio or a path to any audio file, \
                       such as a fresh generate under out/audio."
    )]
    async fn inspect_audio(
        &self,
        Parameters(args): Parameters<InspectAudioArgs>,
    ) -> CallToolResult {
        let catalog = Catalog::scan(&self.config.project);
        let target = match resolve_sound(&self.config.project, &catalog, &args.name_or_path) {
            Ok(target) => target,
            Err(refusal) => return refusal,
        };
        let plot = args
            .return_image
            .unwrap_or(true)
            .then(|| self.config.scratch_png(&format!("audio-{}", target.stem())));
        let project = self.config.project.clone();
        // Decode, measure and plot are all blocking, and a long music track
        // is seconds of work.
        let inspected =
            tokio::task::spawn_blocking(move || inspect(&project, &target, plot.as_deref()))
                .await
                .unwrap_or_else(|err| Err(format!("the inspection task failed: {err}")));
        let (report, plot) = match inspected {
            Ok(inspected) => inspected,
            Err(message) => return util::refuse(message),
        };
        let mut blocks = vec![Content::text(report)];
        if let Some(plot) = plot {
            blocks.push(util::inline_image(&plot).into_content(&plot, "plot"));
        }
        CallToolResult::success(blocks)
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::audio_tools()
}

/// A sound to inspect: in the library, or a file somewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SoundTarget {
    /// A library asset: absolute path, asset-relative path, and the
    /// record's display pairs for the report.
    InLibrary {
        /// On disk.
        path: PathBuf,
        /// Relative to the asset root.
        rel_path: String,
        /// `key: value` lines from the sidecar, reading order.
        recorded: Vec<(String, String)>,
    },
    /// An existing audio file outside the library, by absolute path.
    File(PathBuf),
}

impl SoundTarget {
    /// The stem a scratch plot takes.
    fn stem(&self) -> String {
        let path = match self {
            Self::InLibrary { path, .. } | Self::File(path) => path,
        };
        path.file_stem().map_or_else(
            || String::from("sound"),
            |s| s.to_string_lossy().into_owned(),
        )
    }

    fn path(&self) -> &Path {
        match self {
            Self::InLibrary { path, .. } | Self::File(path) => path,
        }
    }
}

/// Resolve a sound: a file that exists is that file (and, when it lies
/// under the asset root, its library record too); otherwise a library name
/// against the audio half of the catalog — `Catalog::resolve` takes one
/// kind, and a project with a clip and a bark that share a stem must not
/// have the clip win; else a refusal listing every sound.
///
/// The file check comes first on purpose: a fresh generate under
/// `out/audio/bark.wav` and a shipped `audio/sfx/bark.wav` share a file
/// name, and a caller who typed the path meant the path.
pub(crate) fn resolve_sound(
    project: &Project,
    catalog: &Catalog,
    wanted: &str,
) -> Result<SoundTarget, CallToolResult> {
    let sounds = Catalog::from_records(
        catalog
            .records()
            .iter()
            .filter(|record| record.kind.is_audio())
            .cloned()
            .collect(),
    );
    let in_library = |record: &forge_library::AssetRecord| SoundTarget::InLibrary {
        path: record.path.clone(),
        rel_path: record.rel_path.clone(),
        recorded: record
            .sidecar
            .as_ref()
            .map(forge_library::sidecar::display_pairs)
            .unwrap_or_default(),
    };
    let wanted = wanted.trim();
    if let Some(absolute) = crate::tools::render::existing_file(wanted)? {
        if let Some(rel) = project.rel_to_assets(&absolute)
            && let Some(record) = sounds.resolve(&rel, None)
        {
            return Ok(in_library(record));
        }
        return Ok(SoundTarget::File(absolute));
    }
    // A path the caller was handed by `generate_audio` is relative to the
    // PROJECT, and this process's working directory is wherever the server
    // was launched — the toolkit checkout, for an editor's MCP client. So a
    // relative path is tried against the project root too, or an agent is
    // refused the very path the tool before it printed.
    let from_project = project.root.join(wanted);
    if !wanted.is_empty() && from_project.is_file() {
        if let Some(rel) = project.rel_to_assets(&from_project)
            && let Some(record) = sounds.resolve(&rel, None)
        {
            return Ok(in_library(record));
        }
        return Ok(SoundTarget::File(from_project));
    }
    if let Some(record) = sounds.resolve(wanted, None) {
        return Ok(in_library(record));
    }
    let names: Vec<String> = sounds
        .records()
        .iter()
        .map(|record| record.rel_path.clone())
        .collect();
    Err(util::refuse_unknown("sound", wanted, &names))
}

/// Decode, measure and (optionally) plot one file. Blocking on purpose.
///
/// Returns the report and the plot that was written, or the message to
/// refuse with.
pub(crate) fn inspect(
    project: &Project,
    target: &SoundTarget,
    plot: Option<&Path>,
) -> Result<(String, Option<PathBuf>), String> {
    let inspected = forge_audio::cli::inspect(target.path(), plot).map_err(|e| e.to_string())?;
    let mut report = inspected.summary.clone();
    if inspected.is_defective() {
        report
            .push_str("\nDEFECT: this file is silent or clipped — a promote would ship it as is.");
    }
    match target {
        SoundTarget::InLibrary {
            rel_path, recorded, ..
        } => {
            // The file has just been decoded, so the cache may as well learn
            // from it: this is the call that warms `list_audio` for a
            // freshly promoted sound.
            if let Some(metrics) = inspected.metrics() {
                let mut cache = MetricsCache::load(project);
                cache.insert(rel_path, target.path(), AudioMeasurement::from(metrics));
                let _ = cache.save();
            }
            if !recorded.is_empty() {
                report.push_str("\nrecorded:");
                for (key, value) in recorded.iter().take(12) {
                    let _ = write!(report, "\n  {key}: {}", util::first_line(value, 80));
                }
            }
        }
        SoundTarget::File(_) => {
            report.push_str(
                "\nnot in the library: no record to show. promote_audio files it with one.",
            );
        }
    }
    Ok((report, inspected.plot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::util::frame_text;
    use forge_library::Kind;
    use forge_library::promote::{PromoteAudio, promote_audio};
    use forge_library::schema::Actor;

    /// A 16-bit PCM mono WAV: a decaying 440 Hz pluck, loud and dynamic
    /// enough that no warning fires.
    fn write_pluck(path: &Path) {
        let rate: u32 = 22_050;
        let frames = rate;
        let mut bytes = Vec::with_capacity(44 + frames as usize * 2);
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
            let sample = 0.8 * (-4.0 * t).exp() * (t * 440.0 * std::f32::consts::TAU).sin();
            bytes.extend_from_slice(&((sample * 32_767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, bytes).expect("write wav");
    }

    #[tokio::test]
    async fn a_library_sound_and_a_loose_file_both_inspect() {
        let (dir, project) = testing::library();
        let loose = dir.path().join("pluck.wav");
        write_pluck(&loose);
        promote_audio(
            &project,
            &PromoteAudio {
                kind: Kind::Sfx,
                name: String::from("pluck"),
                file: loose.clone(),
                record: None,
                prompt: Some(String::from("a plucked string")),
                tags: vec![String::from("test")],
                note: None,
                created_by: Actor::Human,
                overwrite: false,
                allow_defective: false,
            },
        )
        .expect("promote the sound");
        let server = testing::server(project.clone());

        let result = server
            .inspect_audio(Parameters(InspectAudioArgs {
                name_or_path: String::from("pluck"),
                return_image: None,
            }))
            .await;
        let text = frame_text(&result);
        assert_ne!(result.is_error, Some(true), "{text}");
        assert!(text.contains("verdict:  clean"), "{text}");
        assert!(text.contains("prompt: a plucked string"), "{text}");
        assert!(
            result.content.iter().any(|c| c.as_image().is_some()),
            "the plot is inlined"
        );
        assert!(project.out.join("mcp/audio-pluck.png").is_file());
        // The cache learned from the decode.
        let cache = MetricsCache::load(&project);
        assert!(
            cache
                .get(
                    "audio/sfx/pluck.wav",
                    &project.kind_dir(Kind::Sfx).join("pluck.wav")
                )
                .is_some()
        );

        let result = server
            .inspect_audio(Parameters(InspectAudioArgs {
                name_or_path: loose.to_string_lossy().into_owned(),
                return_image: Some(false),
            }))
            .await;
        let text = frame_text(&result);
        assert_ne!(result.is_error, Some(true), "{text}");
        assert!(text.contains("not in the library"), "{text}");
        assert!(!result.content.iter().any(|c| c.as_image().is_some()));

        let result = server
            .inspect_audio(Parameters(InspectAudioArgs {
                name_or_path: String::from("bark"),
                return_image: None,
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        let text = frame_text(&result);
        assert!(text.contains("no sound matching \"bark\""), "{text}");
        assert!(text.contains("audio/sfx/pluck.wav"), "{text}");
    }

    #[test]
    fn a_stem_shared_with_a_clip_resolves_to_the_sound() {
        let (_dir, project) = testing::library();
        // A sound called `roll`, beside the clip called `roll`.
        let wav = project.kind_dir(Kind::Sfx).join("roll.wav");
        write_pluck(&wav);
        let catalog = Catalog::scan(&project);
        let target = resolve_sound(&project, &catalog, "roll").expect("the sound");
        assert!(
            matches!(target, SoundTarget::InLibrary { ref rel_path, .. } if rel_path == "audio/sfx/roll.wav")
        );
        assert_eq!(target.stem(), "roll");
        // The same file by absolute path is still the library record.
        let by_path = resolve_sound(&project, &catalog, &wav.to_string_lossy()).expect("by path");
        assert_eq!(by_path, target);
        let refusal = resolve_sound(&project, &catalog, "").expect_err("empty");
        assert_eq!(refusal.is_error, Some(true));
    }

    #[test]
    fn a_file_that_will_not_decode_is_a_refusal_with_the_reason() {
        let (dir, project) = testing::empty_project();
        let junk = dir.path().join("junk.wav");
        std::fs::write(&junk, b"not audio").expect("write");
        let error = inspect(&project, &SoundTarget::File(junk.clone()), None).expect_err("refused");
        assert!(error.contains("junk.wav"), "{error}");
    }
}
