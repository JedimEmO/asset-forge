//! Drawing new motion and new sound out of the local backends.
//!
//! Generation writes nothing into the library. A sweep leaves `.npz` takes,
//! their records and a review sheet under `out/sweeps/`; a sound lands
//! under `out/audio/` with its record and a plot beside it. Choosing one of
//! them is `promote_clip`'s or `promote_audio`'s job, and that is the first
//! moment anything reaches `assets/`.
//!
//! # One binary
//!
//! Every generator runs as `forge gen <cmd> … --json` — this same
//! executable re-invoked ([`crate::Config::renderer`]), which hands the
//! command line to `python/forge_gen` and relays its exit code. So the MCP
//! tool and the `just` recipe a human types go through the one launcher,
//! read the one backend table, and write the one record shape; nothing
//! here knows a generator's flags beyond the handful it exposes.
//!
//! # Refusing before the GPU
//!
//! A backend that is not installed is "generation is off", not an error,
//! and it is known in under a millisecond from the backends directory. So
//! each tool asks [`Backends`] first and refuses with the install line and
//! a pointer at `doctor` — the same words `forge gen` would print after
//! spawning an interpreter to find out. When the Python side refuses
//! anyway (exit 3: a stale `.env`, a checkout that moved) its own
//! `message` and `hint` come back verbatim, because that hint is the next
//! command to type and paraphrasing it would cost whoever types it the one
//! useful sentence.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_library::backends::{Backends, GenExit};
use forge_library::project::OutKind;
use forge_library::promote::validate_name;
use forge_library::{GeneratorRecord, Project, hash};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::server::ForgeServer;
use crate::tools::promote::{ACTOR, resolve_path, stated};
use crate::util::{self, Captured, Ran};

/// The review sheet is a plotting pass over takes already on disk.
const REVIEW_TIMEOUT: Duration = Duration::from_mins(5);
/// A music track renders on a resident server, minutes for a minute of
/// audio; the server itself may also be starting up.
const MUSIC_TIMEOUT: Duration = Duration::from_mins(30);
/// How many characters of a prompt's hash name a sweep directory.
const PROMPT_HASH_CHARS: usize = 8;
/// How many characters of a prompt's slug name a sound.
const SLUG_CHARS: usize = 24;

/// This file's tools, for the server to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    ForgeServer::generate_router()
}

/// Arguments for `generate_clips`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GenerateClipsArgs {
    /// Plain-English description of the motion, e.g. "A person waves
    /// hello." A grounded activity with both hands accounted for beats a
    /// mood; weak priors give arm-waves.
    pub(crate) prompt: String,
    /// Seconds of motion to generate. Default 4.
    pub(crate) duration_s: Option<f32>,
    /// How many takes to draw. One model load covers the whole batch, so 8
    /// is barely slower than 1. Default 8.
    pub(crate) samples: Option<u32>,
    /// Random seed. Change it to reroll the same prompt; the same pair
    /// lands in the same directory. Default 0.
    pub(crate) seed: Option<i64>,
    /// Return the review sheet inline. Default true.
    pub(crate) return_image: Option<bool>,
}

/// Arguments for `generate_audio`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GenerateAudioArgs {
    /// What to make: `sfx` (MOSS sound effect), `music` (ACE-Step track) or
    /// `speech` (MOSS-TTS line).
    pub(crate) kind: String,
    /// For sfx: the sound — material, action, environment, tail. For music:
    /// genre, mood, instrumentation. For speech, the line to speak (or use
    /// `text`).
    pub(crate) prompt: Option<String>,
    /// For speech: the line to speak; `[pause 1.5s]` is an explicit pause.
    /// Read as the prompt when `prompt` is absent.
    pub(crate) text: Option<String>,
    /// `snake_case` stem for the file under out/audio/<kind>/. Default: a
    /// slug of the prompt. Becomes the suggested library name.
    pub(crate) name: Option<String>,
    /// Length in seconds: sfx up to 30 (default 3), music 10–600 (default
    /// 30). Speech takes as long as the line does.
    pub(crate) seconds: Option<f32>,
    /// Sampler seed. Omitted, the backend draws one and records it.
    pub(crate) seed: Option<i64>,
    /// Speech only: the voice to clone — the name of one designed by
    /// `forge gen voice` (`assets-src/voices/<name>/ref.wav`), or a path
    /// to a reference clip, 5–15 s of clean speech (.wav/.mp3/.flac).
    /// Omitted, an uncloned voice.
    pub(crate) voice: Option<String>,
    /// Music only: `ogg` (default; needs ffmpeg) or `wav`. Sfx and speech
    /// are always wav.
    pub(crate) format: Option<String>,
}

/// One audio generator as the tool exposes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioKind {
    Sfx,
    Music,
    Speech,
}

impl AudioKind {
    /// The word `generate_audio` takes, which is also the `forge gen`
    /// subcommand and the directory under `out/audio/`.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Speech => "speech",
        }
    }

    /// The backend directory this kind needs.
    const fn backend(self) -> &'static str {
        match self {
            Self::Sfx => "moss_sfx",
            Self::Music => "acestep",
            Self::Speech => "moss_tts",
        }
    }

    /// The library kind the sound would ship as, for the promote hint.
    const fn library_kind(self) -> &'static str {
        match self {
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Speech => "voice",
        }
    }

    /// The generator's ceiling.
    const fn timeout(self) -> Duration {
        match self {
            Self::Music => MUSIC_TIMEOUT,
            Self::Sfx | Self::Speech => util::GENERATE_TIMEOUT,
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "sfx" => Some(Self::Sfx),
            "music" => Some(Self::Music),
            "speech" | "voice" | "tts" => Some(Self::Speech),
            _ => None,
        }
    }
}

#[tool_router(router = generate_router, vis = "pub(crate)")]
impl ForgeServer {
    /// Generate a batch of takes and return a review sheet of all of them.
    #[tool(
        description = "Generate motion takes from a text prompt with ARDY and return a \
                       contact sheet of every take side by side, plus the review table \
                       (travel speed, foot skate, jitter, flags) and each take's record. One \
                       model load covers the whole batch, so asking for 8 takes costs barely \
                       more than 1. Nothing is written to the library: the takes land under \
                       out/sweeps/; judge them on the sheet, then pass the path of the one \
                       you pick to promote_clip, and render_clip_strip shows the result on the \
                       real body. Takes \
                       1-3 minutes and needs ~16 GB of GPU memory; refuses with the install \
                       line when ARDY is not set up (doctor has the full table)."
    )]
    pub(crate) async fn generate_clips(
        &self,
        Parameters(args): Parameters<GenerateClipsArgs>,
    ) -> CallToolResult {
        let prompt = args.prompt.trim();
        if prompt.is_empty() {
            return util::refuse("prompt is empty — say what the motion is, in plain English");
        }
        if let Some(refusal) = self.backend_refusal("ardy", "generate_clips") {
            return refusal;
        }
        let seed = args.seed.unwrap_or(0);
        let samples = args.samples.unwrap_or(8).max(1);
        let duration = args.duration_s.unwrap_or(4.0);
        if !duration.is_finite() || duration <= 0.0 {
            return util::refuse(format!("duration_s must be positive, not {duration}"));
        }

        let project = &self.config.project;
        let out_dir = project
            .out_dir(OutKind::Sweeps)
            .join(sweep_dir_name(seed, prompt));
        // A re-run of the same prompt and seed lands on the same directory;
        // clearing it first keeps a shorter second sweep from leaving the
        // first one's extra takes beside its own. Nothing under out/ is a
        // source — a promoted take was copied under assets-src/.
        let _ = std::fs::remove_dir_all(&out_dir);
        if let Err(err) = std::fs::create_dir_all(&out_dir) {
            return util::refuse(format!("cannot create {}: {err}", out_dir.display()));
        }

        let mut sweep = self.gen_command();
        sweep
            .arg("motion")
            .arg("sweep")
            .arg("--out-dir")
            .arg(&out_dir)
            .arg("--prompt")
            .arg(prompt)
            .arg("--duration")
            .arg(duration.to_string())
            .arg("--samples")
            .arg(samples.to_string())
            .arg("--seeds")
            .arg(seed.to_string())
            .arg("--created-by")
            .arg(ACTOR)
            .arg("--json");
        let swept = match util::run(&mut sweep, util::GENERATE_TIMEOUT).await {
            Ran::Ok(captured) => captured,
            Ran::Failed(captured) => return gen_refusal("the sweep", "ardy", &captured),
            Ran::Unlaunchable(err) => {
                return util::refuse(format!(
                    "could not run {} for the sweep: {err}",
                    self.config.renderer.display()
                ));
            }
            Ran::TimedOut => {
                return util::refuse(format!(
                    "the sweep timed out after {} minutes — the GPU may be held by another \
                     process; doctor says who holds it. takes written so far are under {}",
                    util::minutes(util::GENERATE_TIMEOUT),
                    out_dir.display()
                ));
            }
        };

        let takes = npz_files(&out_dir);
        if takes.is_empty() {
            return util::refuse(format!(
                "the sweep reported success but wrote no takes under {}.\n{}",
                out_dir.display(),
                swept.stdout.trim()
            ));
        }
        let records: Vec<PathBuf> = takes.iter().map(|take| take_record_path(take)).collect();

        // The review owns the metrics; reimplementing thirty of them here
        // would buy nothing and drift. Its failure is reported beside the
        // takes rather than refusing them: they are on disk either way.
        let sheet = out_dir.join("sheet.png");
        let metrics = out_dir.join("metrics.json");
        let mut review = self.gen_command();
        review
            .arg("motion")
            .arg("review")
            .args(&takes)
            .arg("--sheet")
            .arg(&sheet)
            .arg("--metrics")
            .arg(&metrics)
            .arg("--intent")
            .arg("oneshot")
            .arg("--json");
        let reviewed = util::run(&mut review, REVIEW_TIMEOUT).await;

        let mut text = format!(
            "{} take(s) for {prompt:?} (seed {seed}, {duration} s, {samples} asked) under {}:\n",
            takes.len(),
            out_dir.display()
        );
        for (take, record) in takes.iter().zip(&records) {
            let _ = writeln!(
                text,
                "  {}\n    record {}",
                take.display(),
                if record.is_file() {
                    record.display().to_string()
                } else {
                    String::from("(none written)")
                }
            );
        }
        if sweep_was_fake(&swept) {
            text.push_str(
                "\nFAKE: these are --fake placeholders (a still figure in the rest pose); \
                 nothing about them is a measurement.\n",
            );
        }
        text.push_str(
            "\nnone of these are in the library. judge them on the sheet and the table, pass \
             the path of the one you pick to promote_clip with a recipe, and then \
             render_clip_strip shows the shipped clip on the real body (a human can watch the \
             raw take with `forge studio --take <npz>`).",
        );
        let mut blocks = vec![Content::text(text)];

        match reviewed {
            Ran::Ok(captured) => {
                blocks.push(Content::text(format!(
                    "review table (metrics at {}):\n{}",
                    metrics.display(),
                    review_table(&captured)
                )));
                if args.return_image.unwrap_or(true) {
                    blocks
                        .push(util::inline_image(&sheet).into_content(&sheet, "the review sheet"));
                } else {
                    blocks.push(Content::text(format!(
                        "review sheet at {}",
                        sheet.display()
                    )));
                }
            }
            Ran::Failed(captured) => blocks.push(Content::text(format!(
                "the takes are on disk but the review did not produce a sheet ({}).\n{}",
                captured
                    .code
                    .map_or_else(|| String::from("killed"), |c| format!("exit {c}")),
                captured.stderr_tail()
            ))),
            Ran::Unlaunchable(err) => blocks.push(Content::text(format!(
                "the takes are on disk but the review could not be launched: {err}"
            ))),
            Ran::TimedOut => blocks.push(Content::text(format!(
                "the takes are on disk but the review timed out after {} minutes",
                util::minutes(REVIEW_TIMEOUT)
            ))),
        }
        CallToolResult::success(blocks)
    }

    /// Draw one sound, track or spoken line into `out/audio/`.
    #[tool(
        description = "Generate one sound with the local audio backends: kind sfx (MOSS sound \
                       effect from a prompt), music (ACE-Step track from a description) or \
                       speech (MOSS-TTS from text, cloning a voice designed by `forge gen \
                       voice` when one is named). The \
                       file, its record and a plot land under out/audio/<kind>/ — never in the \
                       library; the response carries the measurements and the plot, so check \
                       it for clipping, dead air and truncation, then pass the file to \
                       promote_audio. Music leaves the ACE-Step server resident on the GPU \
                       (~8 GB) until `forge gen music --stop-server`. Refuses with the install \
                       line when the backend is not set up (doctor has the full table)."
    )]
    pub(crate) async fn generate_audio(
        &self,
        Parameters(args): Parameters<GenerateAudioArgs>,
    ) -> CallToolResult {
        let Some(kind) = AudioKind::parse(&args.kind) else {
            return util::refuse(format!(
                "{:?} is not an audio kind — use sfx, music or speech",
                args.kind
            ));
        };
        // Speech speaks `text`; the other two take `prompt`. Either word is
        // accepted for any kind so a caller that mixes them up is not sent
        // back for a retype.
        let line = match kind {
            AudioKind::Speech => {
                stated(args.text.as_deref()).or_else(|| stated(args.prompt.as_deref()))
            }
            AudioKind::Sfx | AudioKind::Music => {
                stated(args.prompt.as_deref()).or_else(|| stated(args.text.as_deref()))
            }
        };
        let Some(line) = line else {
            return util::refuse(match kind {
                AudioKind::Speech => "text is empty — give the line to speak",
                AudioKind::Sfx => {
                    "prompt is empty — say what the sound is: material, action, environment, tail"
                }
                AudioKind::Music => {
                    "prompt is empty — say what the track is: genre, mood, instrumentation"
                }
            });
        };
        if let Some(refusal) = self.backend_refusal(kind.backend(), "generate_audio") {
            return refusal;
        }
        let mut notes = String::new();
        let format = match (kind, stated(args.format.as_deref())) {
            (AudioKind::Music, None) => "ogg",
            (AudioKind::Music, Some(f)) if f.eq_ignore_ascii_case("ogg") => "ogg",
            (AudioKind::Music, Some(f)) if f.eq_ignore_ascii_case("wav") => "wav",
            (AudioKind::Music, Some(other)) => {
                return util::refuse(format!(
                    "format must be ogg or wav for music, not {other:?}"
                ));
            }
            (AudioKind::Sfx | AudioKind::Speech, Some(_)) => {
                let _ = writeln!(
                    notes,
                    "note: format is ignored for {} — it is always wav",
                    kind.as_str()
                );
                "wav"
            }
            (AudioKind::Sfx | AudioKind::Speech, None) => "wav",
        };
        if kind != AudioKind::Speech && args.voice.is_some() {
            let _ = writeln!(
                notes,
                "note: voice is ignored for {} — it clones a speaker, and only speech has one",
                kind.as_str()
            );
        }
        if kind == AudioKind::Speech && args.seconds.is_some() {
            let _ = writeln!(
                notes,
                "note: seconds is ignored for speech — the line decides its own length"
            );
        }
        if let Some(seconds) = args.seconds
            && (!seconds.is_finite() || seconds <= 0.0)
        {
            return util::refuse(format!("seconds must be positive, not {seconds}"));
        }

        let project = &self.config.project;
        let dir = project.out_dir(OutKind::Audio).join(kind.as_str());
        if let Err(err) = std::fs::create_dir_all(&dir) {
            return util::refuse(format!("cannot create {}: {err}", dir.display()));
        }
        let name = match stated(args.name.as_deref()) {
            Some(stated) => match validate_name(&stated) {
                Ok(name) => name,
                Err(err) => return util::refuse(err.to_string()),
            },
            None => free_name(&dir, &slug(&line, kind.as_str()), format),
        };
        let out = dir.join(format!("{name}.{format}"));
        let record = dir.join(format!("{name}.json"));
        let plot = dir.join(format!("{name}.png"));

        let mut command = self.gen_command();
        command.arg(kind.as_str());
        match kind {
            AudioKind::Sfx => {
                command.arg("--prompt").arg(&line);
                if let Some(seconds) = args.seconds {
                    command.arg("--seconds").arg(seconds.to_string());
                }
            }
            AudioKind::Music => {
                command.arg("--prompt").arg(&line);
                if let Some(seconds) = args.seconds {
                    command.arg("--duration").arg(seconds.to_string());
                }
                command.arg("--format").arg(format);
            }
            AudioKind::Speech => {
                command.arg("--text").arg(&line);
                if let Some(voice) = stated(args.voice.as_deref()) {
                    let voice = match resolve_voice(project, &voice) {
                        Ok(path) => path,
                        Err(refusal) => return util::refuse(refusal),
                    };
                    command.arg("--voice").arg(voice);
                }
            }
        }
        if let Some(seed) = args.seed {
            command.arg("--seed").arg(seed.to_string());
        }
        command
            .arg("--out")
            .arg(&out)
            .arg("--record")
            .arg(&record)
            .arg("--created-by")
            .arg(ACTOR)
            .arg("--json");

        let what = format!("the {} generate", kind.as_str());
        let captured = match util::run(&mut command, kind.timeout()).await {
            Ran::Ok(captured) => captured,
            Ran::Failed(captured) => return gen_refusal(&what, kind.backend(), &captured),
            Ran::Unlaunchable(err) => {
                return util::refuse(format!(
                    "could not run {} for {what}: {err}",
                    self.config.renderer.display()
                ));
            }
            Ran::TimedOut => {
                return util::refuse(format!(
                    "{what} timed out after {} minutes — the GPU may be held by another \
                     process; doctor says who holds it",
                    util::minutes(kind.timeout())
                ));
            }
        };
        // The generator says where it wrote; the paths asked for are the
        // fallback, since a wav asked for as ogg without ffmpeg would have
        // been refused rather than renamed.
        let payload = captured.last_stdout_line().and_then(parse_object);
        let written = payload
            .as_ref()
            .and_then(|p| p.get("outputs"))
            .and_then(Value::as_array)
            .and_then(|o| o.first())
            .and_then(Value::as_str)
            .map_or(out.clone(), PathBuf::from);
        let record = payload
            .as_ref()
            .and_then(|p| p.get("record"))
            .and_then(Value::as_str)
            .map_or(record, PathBuf::from);
        if !written.is_file() {
            return util::refuse(format!(
                "{what} reported success but {} is not there.\n{}",
                written.display(),
                captured.stdout.trim()
            ));
        }

        // Decode, measure and plot in this process: the file is on disk and
        // the numbers are what promote will measure again.
        let (inspect_out, inspect_plot) = (written.clone(), plot.clone());
        let inspected = tokio::task::spawn_blocking(move || {
            forge_audio::cli::inspect(&inspect_out, Some(&inspect_plot)).map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|err| Err(format!("the inspection task failed: {err}")));

        // Whether the run was a --fake one is the record's word, not the
        // summary object's: not every command repeats it there, and the
        // record is what the promote will read. The seed is there too.
        let run = GeneratorRecord::load(&record).ok();
        let fake = run.as_ref().is_some_and(|r| r.fake);
        let mut text = format!(
            "{} wrote {}\nrecord {}{}\n{notes}",
            kind.as_str(),
            written.display(),
            record.display(),
            run.as_ref()
                .and_then(|r| r.param_seed_text("seed"))
                .map_or_else(String::new, |seed| format!(" (seed {seed})"))
        );
        if fake {
            text.push_str(
                "FAKE: this is a --fake placeholder; nothing about it is a measurement.\n",
            );
        }
        let mut plot_block = None;
        match inspected {
            Ok(report) => {
                let _ = write!(text, "\n{}", report.summary);
                if report.is_defective() {
                    text.push_str(
                        "\n\nDEFECT: silent or clipped — do not promote this one; reroll the \
                         seed or change the prompt.",
                    );
                }
                if let Some(plot_path) = report.plot {
                    plot_block =
                        Some(util::inline_image(&plot_path).into_content(&plot_path, "the plot"));
                }
            }
            Err(err) => {
                let _ = write!(
                    text,
                    "\nthe file was written but could not be inspected: {err}"
                );
            }
        }
        let _ = write!(
            text,
            "\n\nnot in the library. when it reads clean, promote_audio {{\"kind\": {:?}, \
             \"name\": {name:?}, \"file\": {:?}}} ships it (the record beside it is read \
             automatically).",
            kind.library_kind(),
            written.display().to_string()
        );
        if kind == AudioKind::Music && !fake {
            text.push_str(
                "\n\nthe ACE-Step server is still resident on the GPU (~8 GB) so the next \
                 track starts faster; it does not co-reside with a lift or a sweep. \
                 `forge gen music --stop-server` frees the card.",
            );
        }
        let mut blocks = vec![Content::text(text)];
        blocks.extend(plot_block);
        CallToolResult::success(blocks)
    }
}

impl ForgeServer {
    /// `<renderer> --project <root> gen` — the launcher every generate goes
    /// through, with the project stated so the child never has to find it.
    fn gen_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.config.renderer);
        command
            .arg("--project")
            .arg(&self.config.project.root)
            .arg("gen");
        command
    }

    /// The refusal for a generate whose backend is not usable, decided
    /// before anything is spawned — or `None` when it is.
    fn backend_refusal(&self, backend: &str, tool: &str) -> Option<CallToolResult> {
        if self.config.toolkit.is_none() {
            return Some(util::refuse(format!(
                "{tool} is off: no toolkit checkout holding python/forge_gen was found from \
                 this executable or the environment, so no generator can run. set FORGE_HOME \
                 to the asset-forge checkout and restart the server; doctor has the detail. \
                 nothing was written."
            )));
        }
        let backends = Backends::discover(&self.config.project);
        if backends.is_found(backend) {
            return None;
        }
        Some(util::refuse(format!(
            "{tool} is off: {}\ncall doctor {{}} for the full table of what this machine can \
             run. nothing was written.",
            backends.refusal(backend)
        )))
    }
}

/// The refusal for a generator that exited non-zero, in the Python layer's
/// own words: its last stdout line is one JSON object carrying `message`
/// (or `reason`), `hint` and, when the backend ran and failed, `log_tail`.
///
/// Exit 3 — the backend is not installed — is the case every caller hits
/// first on a fresh machine, and its hint is the install command, so that
/// refusal says so in one line and carries the hint verbatim. A line that
/// is not JSON at all falls back to the stderr tail.
fn gen_refusal(what: &str, backend: &str, captured: &Captured) -> CallToolResult {
    let exit = captured.code.and_then(GenExit::from_code);
    let Some(payload) = captured.last_stdout_line().and_then(parse_object) else {
        let mut text = format!(
            "{what} failed ({}).\n{}",
            exit_word(captured),
            captured.stderr_tail()
        );
        let stdout = captured.stdout.trim();
        if !stdout.is_empty() {
            text.push_str("\n\n");
            text.push_str(stdout);
        }
        text.push_str(NOTHING_WRITTEN);
        return util::refuse(text);
    };
    let field = |key: &str| payload.get(key).and_then(Value::as_str);
    let message = field("reason")
        .or_else(|| field("message"))
        .unwrap_or("(no message)");
    let mut text = match exit {
        Some(GenExit::MissingBackend) => format!(
            "{what} refused: {} is not installed, so generation through it is off.\n{message}",
            field("backend").unwrap_or(backend)
        ),
        Some(GenExit::InputRejected) => {
            format!("{what} refused the input — the fix is upstream, not a flag.\n{message}")
        }
        Some(GenExit::BackendFailed) => format!("{what} ran and failed.\n{message}"),
        Some(GenExit::MissingTool) => format!(
            "{what} needs a host tool that is not there{}.\n{message}",
            field("tool").map_or_else(String::new, |t| format!(" ({t})"))
        ),
        Some(GenExit::Usage) => {
            format!("{what} was called with arguments forge-gen rejects.\n{message}")
        }
        Some(GenExit::Ok) | None => format!("{what} failed ({}).\n{message}", exit_word(captured)),
    };
    if let Some(hint) = field("hint") {
        let _ = write!(text, "\nhint: {hint}");
    }
    if let Some(tail) = payload.get("log_tail").and_then(Value::as_array) {
        let lines: Vec<&str> = tail.iter().filter_map(Value::as_str).collect();
        if !lines.is_empty() {
            let keep = lines.len().saturating_sub(12);
            text.push_str("\nlog tail:");
            for line in &lines[keep..] {
                text.push_str("\n  ");
                text.push_str(line);
            }
        }
    } else if matches!(exit, Some(GenExit::BackendFailed) | None) {
        let _ = write!(text, "\n{}", captured.stderr_tail());
    }
    text.push_str(NOTHING_WRITTEN);
    util::refuse(text)
}

/// The last line of every generator refusal.
const NOTHING_WRITTEN: &str =
    "\n\ncall doctor {} for what this machine can run. nothing was written under assets/.";

/// "exit N", or what ended a child that has no code.
fn exit_word(captured: &Captured) -> String {
    captured.code.map_or_else(
        || String::from("killed by a signal"),
        |c| format!("exit {c}"),
    )
}

/// A line that is one JSON object, or nothing.
fn parse_object(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object)
}

/// `<seed>-<8 hex of sha256(prompt)>`: two auditions of one prompt with
/// different seeds sit side by side, and a re-run of the same pair lands on
/// the same directory.
fn sweep_dir_name(seed: i64, prompt: &str) -> String {
    let digest = hash::sha256_bytes(prompt.as_bytes());
    let hex = digest.rsplit(':').next().unwrap_or(&digest);
    let short: String = hex.chars().take(PROMPT_HASH_CHARS).collect();
    format!("{seed}-{short}")
}

/// Every `.npz` directly under a sweep directory, sorted — the order the
/// sheet rows follow.
fn npz_files(dir: &Path) -> Vec<PathBuf> {
    let mut takes: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "npz"))
        .collect();
    takes.sort();
    takes
}

/// `<take>.take.json` beside a take — where the sweep writes its record.
fn take_record_path(take: &Path) -> PathBuf {
    let stem = take
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    take.with_file_name(format!("{stem}.take.json"))
}

/// Whether the sweep's result line says its takes are placeholders.
fn sweep_was_fake(captured: &Captured) -> bool {
    captured
        .last_stdout_line()
        .and_then(parse_object)
        .and_then(|p| p.get("fake").and_then(Value::as_bool))
        == Some(true)
}

/// The review table: what the review printed before its JSON line — the
/// inner prints the table as it goes — or, when it printed nothing but the
/// object, a compact table built from the object's `metrics`.
fn review_table(captured: &Captured) -> String {
    let mut lines: Vec<&str> = captured.stdout.lines().collect();
    let payload = lines.last().and_then(|l| parse_object(l));
    if payload.is_some() {
        lines.pop();
    }
    let printed: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !printed.is_empty() {
        return printed.join("\n");
    }
    let Some(metrics) = payload
        .as_ref()
        .and_then(|p| p.get("metrics"))
        .and_then(Value::as_object)
    else {
        return String::from("(the review printed no table)");
    };
    let mut out = format!(
        "{:<36} {:>6} {:>7} {:>7} {:>7}  flags",
        "take", "dur_s", "avg_mps", "skate", "jit_pct"
    );
    let number = |m: &Value, key: &str| -> String {
        m.get(key)
            .and_then(Value::as_f64)
            .map_or_else(|| String::from("null"), |v| format!("{v:.2}"))
    };
    for (name, m) in metrics {
        let flags: Vec<&str> = m
            .get("flags")
            .and_then(Value::as_array)
            .map(|f| f.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let _ = write!(
            out,
            "\n{:<36} {:>6} {:>7} {:>7} {:>7}  {}",
            util::first_line(name, 36),
            number(m, "duration_s"),
            number(m, "avg_speed_mps"),
            number(m, "foot_skate_mps"),
            number(m, "jitter_pct"),
            if flags.is_empty() {
                String::from("-")
            } else {
                flags.join(" ")
            }
        );
    }
    out
}

/// A file-name-safe stub of a prompt: lower-case alphanumerics, runs of
/// anything else collapsed to one underscore, cut at [`SLUG_CHARS`].
/// `fallback` stands in for a prompt with no letters in it.
fn slug(prompt: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in prompt.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            gap = false;
        } else {
            gap = true;
        }
        if out.len() >= SLUG_CHARS {
            break;
        }
    }
    let out = out.trim_end_matches('_').to_owned();
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

/// `stem`, or `stem_2`, `stem_3`, … — the first name with no file under
/// `dir` yet, so a second draw of the same prompt does not overwrite a
/// first one nobody has listened to.
fn free_name(dir: &Path, stem: &str, extension: &str) -> String {
    let taken = |name: &str| dir.join(format!("{name}.{extension}")).exists();
    if !taken(stem) {
        return stem.to_owned();
    }
    (2..1_000u32)
        .map(|n| format!("{stem}_{n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| stem.to_owned())
}

/// The clip a `voice` argument names: a bare name is a voice designed by
/// `forge gen voice` under `assets-src/voices/<name>/ref.wav`; anything
/// else is a path to a brought clip. A name nothing designed is refused
/// with the voices that do exist, so a wrong name costs one turn.
fn resolve_voice(project: &Project, voice: &str) -> Result<PathBuf, String> {
    let text = voice.trim();
    let is_name = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if is_name {
        let path = project.voices_dir().join(text).join("ref.wav");
        if path.is_file() {
            return Ok(path);
        }
        let designed = designed_voices(project);
        return Err(format!(
            "no designed voice named {text} — {}; `forge gen voice {text} --describe \"…\"` \
             designs one, or pass a path to a 5-15 s clip of clean speech",
            if designed.is_empty() {
                String::from("none has been designed yet")
            } else {
                format!("designed: {}", designed.join(", "))
            }
        ));
    }
    let path = resolve_path(project, text);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "no reference voice at {} — pass a 5-15 s clip of clean speech, the name of a \
             designed voice, or omit voice for an uncloned one",
            path.display()
        ))
    }
}

/// Every `<name>` under `assets-src/voices/` with a `ref.wav`, sorted.
fn designed_voices(project: &Project) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(project.voices_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("ref.wav").is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(code: i32, stdout: &str, stderr: &str) -> Captured {
        Captured {
            code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn a_voice_is_a_designed_name_or_a_brought_path_and_a_wrong_name_lists_what_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "voices").expect("init");
        let refusal = resolve_voice(&project, "crypt_warden").expect_err("nothing designed");
        assert!(refusal.contains("none has been designed yet"), "{refusal}");
        assert!(
            refusal.contains("forge gen voice crypt_warden"),
            "{refusal}"
        );

        let warden = project.voices_dir().join("crypt_warden");
        std::fs::create_dir_all(&warden).expect("mkdir");
        std::fs::write(warden.join("ref.wav"), b"RIFF").expect("clip");
        assert_eq!(
            resolve_voice(&project, "crypt_warden").expect("designed"),
            warden.join("ref.wav")
        );
        let refusal = resolve_voice(&project, "kessa").expect_err("not this one");
        assert!(refusal.contains("designed: crypt_warden"), "{refusal}");

        let brought = dir.path().join("calm.wav");
        std::fs::write(&brought, b"RIFF").expect("clip");
        assert_eq!(
            resolve_voice(&project, brought.to_str().expect("utf8")).expect("path"),
            brought
        );
        let refusal = resolve_voice(&project, "nowhere/calm.wav").expect_err("missing path");
        assert!(refusal.contains("no reference voice at"), "{refusal}");
    }

    #[test]
    fn a_sweep_directory_is_seed_then_eight_hex_of_the_prompt() {
        let name = sweep_dir_name(7, "A person waves hello.");
        assert!(name.starts_with("7-"), "{name}");
        assert_eq!(name.len(), 2 + PROMPT_HASH_CHARS, "{name}");
        assert_eq!(name, sweep_dir_name(7, "A person waves hello."));
        assert_ne!(name, sweep_dir_name(8, "A person waves hello."));
        assert_ne!(name, sweep_dir_name(7, "A person bows."));
    }

    #[test]
    fn a_missing_backend_refusal_carries_the_hint_verbatim_and_names_doctor() {
        let line = "{\"ok\":false,\"error\":\"missing_backend\",\"backend\":\"ardy\",\
                    \"message\":\"ardy is not installed — generation through it is off\",\
                    \"hint\":\"bash backends/ardy/install.sh  (or --adopt-env <prefix>)\"}";
        let refusal = gen_refusal("the sweep", "ardy", &captured(3, line, "forge-gen: x\n"));
        assert_eq!(refusal.is_error, Some(true));
        let text = util::frame_text(&refusal);
        assert!(text.contains("ardy is not installed"), "{text}");
        assert!(
            text.contains("hint: bash backends/ardy/install.sh  (or --adopt-env <prefix>)"),
            "{text}"
        );
        assert!(text.contains("doctor"), "{text}");
        assert!(text.contains("nothing was written under assets/"), "{text}");
    }

    #[test]
    fn a_backend_failure_quotes_its_log_tail_and_a_non_json_line_falls_back_to_stderr() {
        let line = "{\"ok\":false,\"error\":\"backend_failed\",\"message\":\"CUDA out of memory\",\
                    \"log_tail\":[\"a\",\"b\",\"c\"]}";
        let text = util::frame_text(&gen_refusal("the sweep", "ardy", &captured(5, line, "")));
        assert!(text.contains("ran and failed"), "{text}");
        assert!(text.contains("CUDA out of memory"), "{text}");
        assert!(
            text.ends_with("nothing was written under assets/."),
            "{text}"
        );
        assert!(text.contains("log tail:\n  a\n  b\n  c"), "{text}");

        let text = util::frame_text(&gen_refusal(
            "the sweep",
            "ardy",
            &captured(1, "not json", "Traceback\nValueError: boom\n"),
        ));
        assert!(text.contains("ValueError: boom"), "{text}");

        let line = "{\"ok\":false,\"error\":\"input_rejected\",\"reason\":\"the prompt is empty\"}";
        let text = util::frame_text(&gen_refusal(
            "the sfx generate",
            "moss_sfx",
            &captured(4, line, ""),
        ));
        assert!(text.contains("refused the input"), "{text}");
        assert!(text.contains("the prompt is empty"), "{text}");
    }

    #[test]
    fn the_review_table_is_what_was_printed_or_a_compact_one_from_the_object() {
        let printed = captured(
            0,
            "name  duration_s  flags\n----\nwalk_0  4.0  -\n{\"ok\":true,\"metrics\":{}}\n",
            "",
        );
        let table = review_table(&printed);
        assert!(table.starts_with("name  duration_s"), "{table}");
        assert!(!table.contains("{\"ok\""), "{table}");

        let only_json = captured(
            0,
            "{\"ok\":true,\"metrics\":{\"walk_0\":{\"duration_s\":4.0,\"avg_speed_mps\":1.25,\
             \"foot_skate_mps\":null,\"jitter_pct\":0.5,\"flags\":[\"SKATE\"]}}}",
            "",
        );
        let table = review_table(&only_json);
        assert!(table.contains("walk_0"), "{table}");
        assert!(table.contains("1.25"), "{table}");
        assert!(table.contains("null"), "{table}");
        assert!(table.contains("SKATE"), "{table}");

        assert!(review_table(&captured(0, "", "")).contains("no table"));
    }

    #[test]
    fn a_slug_is_a_file_stem_and_a_taken_one_is_numbered() {
        assert_eq!(slug("A person waves hello.", "x"), "a_person_waves_hello");
        assert_eq!(slug("!!!", "sfx"), "sfx");
        assert!(
            slug("a very long prompt indeed, far too long for a stem", "x").len() <= SLUG_CHARS
        );
        assert!(validate_name(&slug("Crunchy GRAVEL footstep, outdoors", "x")).is_ok());

        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(free_name(dir.path(), "bark", "wav"), "bark");
        std::fs::write(dir.path().join("bark.wav"), b"x").expect("write");
        assert_eq!(free_name(dir.path(), "bark", "wav"), "bark_2");
        std::fs::write(dir.path().join("bark_2.wav"), b"x").expect("write");
        assert_eq!(free_name(dir.path(), "bark", "wav"), "bark_3");
    }

    #[test]
    fn the_audio_kinds_name_their_backends() {
        assert_eq!(AudioKind::parse("speech"), Some(AudioKind::Speech));
        assert_eq!(AudioKind::parse("voice"), Some(AudioKind::Speech));
        assert_eq!(AudioKind::parse("SFX"), Some(AudioKind::Sfx));
        assert_eq!(AudioKind::parse("noise"), None);
        assert_eq!(AudioKind::Music.backend(), "acestep");
        assert_eq!(AudioKind::Sfx.backend(), "moss_sfx");
        assert_eq!(AudioKind::Speech.backend(), "moss_tts");
        assert_eq!(AudioKind::Speech.library_kind(), "voice");
    }

    #[test]
    fn the_take_record_sits_beside_the_take() {
        assert_eq!(
            take_record_path(Path::new("/s/walk__d4_c2_s0_1.npz")),
            PathBuf::from("/s/walk__d4_c2_s0_1.take.json")
        );
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("b.npz"), b"").expect("write");
        std::fs::write(dir.path().join("a.npz"), b"").expect("write");
        std::fs::write(dir.path().join("sheet.png"), b"").expect("write");
        let takes = npz_files(dir.path());
        assert_eq!(takes.len(), 2);
        assert!(takes[0].ends_with("a.npz"));
    }
}
