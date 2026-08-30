//! The queue, as an agent sees it: `wait`, `cancel`, `status`, `list_runs`.
//!
//! A generate takes minutes and a tool call cannot, so a generate is a job
//! and these four are how an agent lives with that. Every frame here is
//! shaped for a client with no shell and no memory of the last turn: the
//! job frame carries the literal `wait` call to make next, `wait` answers a
//! job that is still running with a **successful** frame rather than an
//! error, and every refusal names what does exist.

use std::fmt::Write as _;
use std::time::Duration;

use forge_serve::{Job, JobFilter, JobId, JobState, RunFilter};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::server::ForgeServer;
use crate::util;

/// What `wait` waits, unasked.
const DEFAULT_WAIT_S: u64 = 120;

/// The ceiling on `wait`, whatever was asked for. A tool call that blocks
/// longer than this is one a client gives up on, and a job that is still
/// running is a successful answer.
const MAX_WAIT_S: u64 = 600;

/// How many log lines a still-running answer carries.
const LOG_TAIL: usize = 8;

/// Arguments for `wait`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WaitArgs {
    /// The job id `generate_audio` (or `forge jobs`) gave you.
    pub(crate) job: String,
    /// Seconds to wait before answering either way. Default 120, ceiling
    /// 600. A job that is still running comes back as a successful frame
    /// you can call `wait` on again.
    pub(crate) max_s: Option<u64>,
}

/// Arguments for `cancel`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CancelArgs {
    /// The job to stop.
    pub(crate) job: String,
}

/// Arguments for `status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct StatusArgs {}

/// Arguments for `list_runs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ListRunsArgs {
    /// Only this kind: sfx, music, speech, voice, take, lift.
    pub(crate) kind: Option<String>,
    /// Only runs made on or after this date (`YYYY-MM-DD`).
    pub(crate) since: Option<String>,
    /// How many, newest first. Default 30.
    pub(crate) limit: Option<usize>,
}

#[tool_router(router = job_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// Wait for a job, and describe what it left.
    #[tool(
        description = "Wait for a job started by generate_audio (or any other generate) and \
                       return what it left: the finished row, and for a finished sound the \
                       same measurements and plot inspect_audio returns, so the common path \
                       is two calls and not three. Default 120 s, ceiling 600. A job that is \
                       still running comes back as a SUCCESSFUL frame carrying its position, \
                       elapsed time and the last log lines — call wait again; nothing was \
                       lost. An unknown id is refused with the ids that do exist."
    )]
    pub(crate) async fn wait(&self, Parameters(args): Parameters<WaitArgs>) -> CallToolResult {
        let id = JobId::from(args.job.trim().to_owned());
        let queue = self.queue.clone();
        let max = Duration::from_secs(args.max_s.unwrap_or(DEFAULT_WAIT_S).min(MAX_WAIT_S));
        let waited = {
            let id = id.clone();
            tokio::task::spawn_blocking(move || queue.wait(&id, max)).await
        };
        let job = match waited {
            Ok(Ok(job)) => job,
            Ok(Err(err)) => return self.queue_refusal(&err),
            Err(err) => return util::refuse(format!("the wait task failed: {err}")),
        };
        if !job.state.is_terminal() {
            return CallToolResult::success(vec![Content::text(pretty(&self.still_waiting(&job)))]);
        }
        self.finished_frame(&job).await
    }

    /// Stop a job.
    #[tool(
        description = "Cancel a job: SIGTERM to its process group, SIGKILL after 10 seconds, \
                       and the card comes back with it. Partial outputs under out/ are left \
                       where they are and nothing under assets/ is touched. Cancelling a job \
                       that has already finished is a refusal naming its state, not an error."
    )]
    async fn cancel(&self, Parameters(args): Parameters<CancelArgs>) -> CallToolResult {
        let id = JobId::from(args.job.trim().to_owned());
        let queue = self.queue.clone();
        let was = queue.get(&id).ok().flatten().map(|job| job.state);
        let cancelled = {
            let id = id.clone();
            tokio::task::spawn_blocking(move || queue.cancel(&id)).await
        };
        match cancelled {
            Ok(Ok(job)) => CallToolResult::success(vec![Content::text(pretty(&json!({
                "job": job.id,
                "cancelled": true,
                "was": was.map(JobState::as_str),
                "note": job.message,
            })))]),
            Ok(Err(err)) => self.queue_refusal(&err),
            Err(err) => util::refuse(format!("the cancel task failed: {err}")),
        }
    }

    /// The card, the queue and the daemon in one look.
    #[tool(
        description = "What is happening right now: free VRAM and who holds the card, the \
                       queue, the running job with a tail of its log, the last jobs that \
                       finished, and whether a daemon is up. No arguments, no GPU work, tens \
                       of milliseconds — the tool to call after a context reset, before \
                       deciding what to start."
    )]
    async fn status(&self, Parameters(_args): Parameters<StatusArgs>) -> CallToolResult {
        let queue = self.queue.clone();
        let status = match tokio::task::spawn_blocking(move || queue.status()).await {
            Ok(Ok(status)) => status,
            Ok(Err(err)) => return self.queue_refusal(&err),
            Err(err) => return util::refuse(format!("the status task failed: {err}")),
        };
        let mut text = format!("project   {}\ntier      {}\n", status.project, status.tier);
        if let Some(url) = &status.comfy_url {
            let _ = writeln!(text, "comfy     {url}");
        }
        match &status.card {
            Value::Null => text.push_str(
                "card      unknown — nvidia-smi did not answer, so nothing here is holding \
                 anything to a budget\n",
            ),
            card => {
                let gb = |key: &str| {
                    card.get(key)
                        .and_then(Value::as_f64)
                        .map_or_else(|| String::from("?"), |mb| format!("{:.1}", mb / 1024.0))
                };
                let _ = writeln!(
                    text,
                    "card      {} — {} GB free of {}",
                    card.get("name").and_then(Value::as_str).unwrap_or("?"),
                    gb("free_mb"),
                    gb("total_mb")
                );
                for app in card
                    .get("apps")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let _ = writeln!(
                        text,
                        "holding   pid {} {:.1} GB  {}{}",
                        app.get("pid").and_then(Value::as_u64).unwrap_or(0),
                        app.get("used_mb").and_then(Value::as_f64).unwrap_or(0.0) / 1024.0,
                        app.get("name").and_then(Value::as_str).unwrap_or("?"),
                        app.get("backend")
                            .and_then(Value::as_str)
                            .map_or_else(String::new, |backend| format!("  ({backend})"))
                    );
                }
            }
        }
        let _ = writeln!(
            text,
            "queue     {} queued, {} blocked, {} running",
            status.queue.queued, status.queue.blocked, status.queue.running
        );
        for job in &status.jobs {
            let _ = writeln!(
                text,
                "  {} {} {}{}",
                job.id,
                job.state,
                job.kind,
                job.elapsed_s
                    .map_or_else(String::new, |seconds| format!(" ({seconds:.0} s)"))
            );
            for line in &job.log_tail {
                let _ = writeln!(text, "      {line}");
            }
        }
        if !status.recent.is_empty() {
            text.push_str("recent\n");
            for job in &status.recent {
                let _ = writeln!(
                    text,
                    "  {} {} {} {}",
                    job.id,
                    job.state,
                    job.kind,
                    job.finished.as_deref().unwrap_or("")
                );
            }
        }
        let _ = writeln!(
            text,
            "daemon    {}",
            if status.daemon.get("up").and_then(Value::as_bool) == Some(true) {
                format!(
                    "up on port {} since {}",
                    status.daemon.get("port").unwrap_or(&Value::Null),
                    status
                        .daemon
                        .get("started")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                )
            } else {
                String::from("down — every generate still runs, in the process that asks for it")
            }
        );
        CallToolResult::success(vec![Content::text(text)])
    }

    /// Everything a generator has left under `out/`.
    #[tool(
        description = "Every file the generators have left under out/ (and the designed \
                       voices under assets-src/voices), newest first, with the record beside \
                       each one: what made it, from what prompt, at what seed, when, whether \
                       it is a --fake placeholder, and whether it has been promoted. \
                       `promoted` is decided by comparing hashes with the library, never by \
                       remembering, so it cannot go stale. No GPU, no daemon needed — this is \
                       the tool for finding yesterday's audition after a context reset."
    )]
    async fn list_runs(&self, Parameters(args): Parameters<ListRunsArgs>) -> CallToolResult {
        let filter = RunFilter {
            kind: args.kind.clone(),
            since: args.since.clone(),
            limit: Some(args.limit.unwrap_or(30)),
        };
        let queue = self.queue.clone();
        let runs = match tokio::task::spawn_blocking(move || queue.runs(&filter)).await {
            Ok(Ok(runs)) => runs,
            Ok(Err(err)) => return self.queue_refusal(&err),
            Err(err) => return util::refuse(format!("the runs task failed: {err}")),
        };
        if runs.is_empty() {
            return util::report(format!(
                "nothing under out/ yet in {}{}. generate_audio and generate_clips write \
                 there; promote_audio and promote_clip move one into the library.",
                self.config.project.root.display(),
                args.kind
                    .as_deref()
                    .map_or_else(String::new, |kind| format!(" of kind {kind}"))
            ));
        }
        CallToolResult::success(vec![Content::text(pretty(&json!(runs)))])
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    ForgeServer::job_tools()
}

impl ForgeServer {
    /// The frame a submitted job comes back as: what it is, where it will
    /// write, and **the literal call to make next**.
    ///
    /// `next` is not decoration. It is the difference between an agent that
    /// polls correctly and one that invents a tool.
    pub(crate) fn job_frame(job: &Job, position: Option<usize>, eta_s: Option<u64>) -> Value {
        json!({
            "job": job.id,
            "state": job.state.as_str(),
            "position": position,
            "eta_s": eta_s,
            "kind": job.kind,
            "backend": job.backend,
            "executor": job.executor.as_str(),
            "out": job.outputs_claimed.first(),
            "record": job.record,
            "log": job.log,
            "next": format!("wait {{\"job\":\"{}\",\"max_s\":120}}", job.id),
        })
    }

    /// The successful frame for a job that has not finished inside the
    /// caller's patience. An agent that gets this calls `wait` again and has
    /// lost nothing.
    fn still_waiting(&self, job: &Job) -> Value {
        let tail = self
            .queue
            .log(&job.id, 0)
            .ok()
            .map(|chunk| {
                chunk
                    .text
                    .lines()
                    .rev()
                    .take(LOG_TAIL)
                    .map(str::to_owned)
                    .collect::<Vec<String>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        json!({
            "job": job.id,
            "state": job.state.as_str(),
            "position": self.position_of(&job.id),
            "elapsed_s": job.elapsed_s(),
            "eta_s": Value::Null,
            "blocked_by": job.blocked_by,
            "log_tail": tail,
            "still_waiting": true,
            "next": format!("wait {{\"job\":\"{}\",\"max_s\":120}}", job.id),
        })
    }

    /// What a finished job answers with: the row, and — for a sound that
    /// came out — the measurement and plot `inspect_audio` would give.
    async fn finished_frame(&self, job: &Job) -> CallToolResult {
        let mut frame = json!({
            "job": job.id,
            "state": job.state.as_str(),
            "exit": job.exit,
            "kind": job.kind,
            "backend": job.backend,
            "executor": job.executor.as_str(),
            "record": job.record,
            "outputs": job.outputs,
            "log": job.log,
            "elapsed_s": job.elapsed_s(),
            "message": job.message,
            "hint": job.hint,
            "cached": job.cached,
            "same_as": job.same_as,
        });
        if job.cached {
            let same = job
                .same_as
                .as_ref()
                .map_or_else(String::new, |id| format!(" It is the same bytes as {id}."));
            frame["note"] = json!(format!(
                "ComfyUI served this from its node cache — it did not re-run.{same} If you \
                 meant a re-roll, change the seed."
            ));
        }
        let mut blocks = vec![Content::text(pretty(&frame))];
        if job.state != JobState::Done {
            if let Ok(chunk) = self.queue.log(&job.id, 0) {
                let tail: Vec<&str> = chunk.text.lines().rev().take(12).collect();
                if !tail.is_empty() {
                    let lines: Vec<&str> = tail.into_iter().rev().collect();
                    blocks.push(Content::text(format!(
                        "log tail:\n  {}",
                        lines.join("\n  ")
                    )));
                }
            }
            // A refusal is a successful frame carrying the Python layer's
            // own words — never an Err, which a client renders opaquely.
            return CallToolResult::error(blocks);
        }
        if let Some(sound) = self.sound_output(job) {
            blocks.extend(self.audio_block(&sound).await);
        }
        CallToolResult::success(blocks)
    }

    /// The audio file a finished job wrote, when it wrote one.
    fn sound_output(&self, job: &Job) -> Option<std::path::PathBuf> {
        let path = job.outputs.iter().find(|path| {
            matches!(
                std::path::Path::new(path)
                    .extension()
                    .and_then(std::ffi::OsStr::to_str),
                Some("wav" | "ogg" | "mp3" | "flac")
            )
        })?;
        let absolute = self.config.project.root.join(path);
        absolute.is_file().then_some(absolute)
    }

    /// The measurement and plot blocks, so a done audio job is two calls
    /// and not three.
    async fn audio_block(&self, path: &std::path::Path) -> Vec<Content> {
        let target = crate::tools::audio::SoundTarget::File(path.to_path_buf());
        let plot = self.config.scratch_png(&format!(
            "job-{}",
            path.file_stem().map_or_else(
                || String::from("sound"),
                |s| s.to_string_lossy().into_owned()
            )
        ));
        let project = self.config.project.clone();
        let inspected = tokio::task::spawn_blocking(move || {
            crate::tools::audio::inspect(&project, &target, Some(&plot))
        })
        .await
        .unwrap_or_else(|err| Err(format!("the inspection task failed: {err}")));
        match inspected {
            Ok((report, plot)) => {
                let mut blocks = vec![Content::text(report)];
                if let Some(plot) = plot {
                    blocks.push(util::inline_image(&plot).into_content(&plot, "the plot"));
                }
                blocks
            }
            Err(message) => vec![Content::text(format!(
                "the file is on disk but could not be inspected: {message}"
            ))],
        }
    }

    /// How many jobs are ahead of this one, when the queue says.
    pub(crate) fn position_of(&self, id: &JobId) -> Option<usize> {
        self.queue
            .status()
            .ok()?
            .jobs
            .iter()
            .find(|row| row.id == *id)
            .and_then(|row| row.position)
    }

    /// A queue error as a frame an agent can act on.
    pub(crate) fn queue_refusal(&self, error: &forge_serve::ServeError) -> CallToolResult {
        match error {
            forge_serve::ServeError::NoSuchJob { .. } => {
                let existing: Vec<String> = self
                    .queue
                    .list(&JobFilter {
                        limit: Some(10),
                        ..JobFilter::default()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|job| format!("{} ({})", job.id, job.state))
                    .collect();
                if existing.is_empty() {
                    util::refuse(format!(
                        "{error} — nothing has been queued in this project yet; \
                         generate_audio starts a job and hands back its id"
                    ))
                } else {
                    util::refuse(format!(
                        "no such job. the jobs that exist, newest first:\n  {}",
                        existing.join("\n  ")
                    ))
                }
            }
            other => util::refuse(other.to_string()),
        }
    }
}

/// A frame, as an agent reads it.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::frame_text;

    #[tokio::test]
    async fn waiting_on_an_id_nobody_has_names_the_ids_that_do_exist() {
        let (_dir, project) = crate::testing::empty_project();
        let server = crate::testing::server(project);
        let frame = server
            .wait(Parameters(WaitArgs {
                job: String::from("j-20260830-000000-nope"),
                max_s: Some(1),
            }))
            .await;
        assert_eq!(frame.is_error, Some(true), "a refusal is an error result");
        let text = frame_text(&frame);
        assert!(
            text.contains("nothing has been queued") || text.contains("the jobs that exist"),
            "{text}"
        );

        // With a job in the table, the refusal lists it rather than saying
        // nothing at all.
        let submitted = server
            .queue
            .submit(forge_serve::JobSpec::new(
                "generate_audio.sfx",
                vec![String::from("sfx")],
                "agent:test",
            ))
            .expect("admitted");
        let frame = server
            .wait(Parameters(WaitArgs {
                job: String::from("j-20260830-000000-nope"),
                max_s: Some(1),
            }))
            .await;
        let text = frame_text(&frame);
        assert!(text.contains(submitted.id.as_str()), "{text}");
    }

    #[tokio::test]
    async fn status_answers_with_no_daemon_and_no_card() {
        let (_dir, project) = crate::testing::empty_project();
        let server = crate::testing::server(project);
        let frame = server.status(Parameters(StatusArgs {})).await;
        assert_ne!(frame.is_error, Some(true));
        let text = frame_text(&frame);
        assert!(text.contains("queue     0 queued"), "{text}");
        assert!(text.contains("daemon    down"), "{text}");
    }

    #[test]
    fn a_job_frame_carries_the_literal_call_to_make_next() {
        let (_dir, project) = crate::testing::empty_project();
        let _server = crate::testing::server(project);
        let job = Job::admitted(
            JobId::from("j-20260830-141207-3f9a"),
            &forge_serve::JobSpec {
                kind: String::from("generate_audio.sfx"),
                backend: Some(String::from("moss_sfx")),
                argv: vec![String::from("sfx")],
                outputs_claimed: vec![String::from("out/audio/sfx/door.wav")],
                record: Some(String::from("out/audio/sfx/door.json")),
                created_by: String::from("agent:claude"),
                fake: None,
            },
            forge_serve::ExecutorKind::Comfy,
            String::from("out/serve/logs/j-20260830-141207-3f9a.log"),
            JobState::Queued,
        );
        let frame = ForgeServer::job_frame(&job, Some(1), Some(45));
        assert_eq!(frame["job"], "j-20260830-141207-3f9a");
        assert_eq!(frame["out"], "out/audio/sfx/door.wav");
        assert_eq!(
            frame["next"],
            "wait {\"job\":\"j-20260830-141207-3f9a\",\"max_s\":120}"
        );
    }
}
