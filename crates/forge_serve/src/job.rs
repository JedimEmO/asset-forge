//! The job table's own shape: one row per job, serde and nothing else.
//!
//! Every field here is written to `out/serve/jobs/<id>.json` and read back
//! by a later process — a restarted daemon, `forge jobs`, an agent's
//! `status` — so the file *is* the type and the type carries no behaviour.
//!
//! # A row, whole
//!
//! ```json
//! {
//!   "forge_job": 1,
//!   "id": "j-20260830-141207-3f9a",
//!   "kind": "generate_audio.sfx",
//!   "executor": "comfy",
//!   "backend": "moss_sfx",
//!   "argv": ["sfx", "--prompt", "a heavy iron door",
//!            "--out", "out/audio/sfx/door.wav",
//!            "--record", "out/audio/sfx/door.json",
//!            "--created-by", "agent:claude", "--json"],
//!   "outputs_claimed": ["out/audio/sfx/door.wav"],
//!   "state": "done",
//!   "blocked_by": null,
//!   "submitted": "2026-08-30T14:12:07Z",
//!   "started":   "2026-08-30T14:12:09Z",
//!   "finished":  "2026-08-30T14:12:41Z",
//!   "exit": 0,
//!   "record": "out/audio/sfx/door.json",
//!   "outputs": ["out/audio/sfx/door.wav"],
//!   "log": "out/serve/logs/j-20260830-141207-3f9a.log",
//!   "created_by": "agent:claude",
//!   "pid": 481233,
//!   "cached": false,
//!   "same_as": null,
//!   "message": null,
//!   "hint": null,
//!   "card": {"held_s": 32.1, "vram_before_gb": 22.4,
//!            "vram_after_gb": 22.4, "restarted": false},
//!   "comfy": {
//!     "template": "backends/moss_sfx/workflows/sfx.api.json",
//!     "template_sha256": "sha256:9f1c…",
//!     "inputs": {"prompt": "a heavy iron door", "seconds": 3.0, "seed": 815273},
//!     "prompt_id": "b1f0…"
//!   }
//! }
//! ```
//!
//! Three rules read off that example, because they are the ones a second
//! implementer has to honour blind:
//!
//! - **`exit` is never `0` by default.** It is `null` for every
//!   non-terminal state, for `cancelled` and for `interrupted`; that is
//!   `null`-means-unknown applied to a process. A `0` in this field means a
//!   child was waited on and exited 0.
//! - **`message` and `hint` are the Python refusal's own words**, relayed
//!   unchanged. Nothing here composes a hint.
//! - **The whole `comfy` block is echoed back by the child**, from the
//!   `comfy` key on its JSON last line. The one process that patched the
//!   graph is the one that writes the record; the daemon copies what it was
//!   told and observes nothing it did not run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The schema number every row carries, so a reader that is behind says so.
pub const JOB_SCHEMA: u32 = 1;

/// A job's identifier: `j-<YYYYMMDD>-<HHMMSS>-<4 hex>`.
///
/// Sortable by submission time as a string, unique inside a second by the
/// suffix, and short enough to be typed back by a human reading `forge
/// jobs`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    /// A new id for a job submitted now.
    #[must_use]
    pub fn fresh() -> Self {
        let stamp = forge_library::clock::stamp_of(forge_library::clock::unix_seconds());
        let digits: String = stamp.chars().filter(char::is_ascii_digit).collect();
        let (date, time) = digits.split_at(8.min(digits.len()));
        let token = forge_library::clock::monotonic_token();
        let tail: String = token.chars().rev().take(4).collect();
        Self(format!("j-{date}-{time}-{tail}"))
    }

    /// The id as it appears in a path, a URL and a frame.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether a string could be one of this module's ids — the check a
    /// route does before it joins the value onto a directory.
    #[must_use]
    pub fn is_wellformed(text: &str) -> bool {
        text.starts_with("j-")
            && text.len() <= 64
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
}

impl From<&str> for JobId {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl From<String> for JobId {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which of the two executors runs a job.
///
/// Read from `backend.toml`'s `executor` key, whose parser is
/// `forge_library::backends`'. `tool` is a host program like Blender and
/// never reaches the queue, so it is not a variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutorKind {
    /// `python3 <toolkit>/python/forge_gen …` in the backend's own env.
    Env,
    /// The same spawn, wrapped in the card ladder around the `ComfyUI` host.
    Comfy,
}

impl ExecutorKind {
    /// The word the row carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Comfy => "comfy",
        }
    }
}

impl std::fmt::Display for ExecutorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a job is in its life.
///
/// `refused` and `failed` are separate because the exit-code table already
/// carries the distinction, and collapsing them would cost an agent a
/// wasted retry on every missing backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    /// Admitted, waiting for the card.
    Queued,
    /// At the head of the queue with a foreign process holding the card;
    /// `blocked_by` names it.
    Blocked,
    /// The child is alive.
    Running,
    /// Exit 0.
    Done,
    /// Exit 2, 3, 4 or 6 — the call was wrong, the backend is absent, the
    /// input was rejected. The fix is the caller's next turn.
    Refused,
    /// Exit 5 — the backend ran and broke. The fix is a log.
    Failed,
    /// SIGTERM to the process group, SIGKILL after ten seconds.
    Cancelled,
    /// The daemon restarted and its child was gone. Nobody knows what
    /// happened, and nothing ever moves this row to `done`.
    Interrupted,
}

impl JobState {
    /// The word the row carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Blocked => "blocked",
            Self::Running => "running",
            Self::Done => "done",
            Self::Refused => "refused",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    /// Whether nothing more will happen to this row.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Refused | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Whether the row still holds its output paths against another submit.
    #[must_use]
    pub const fn holds_outputs(self) -> bool {
        matches!(self, Self::Queued | Self::Blocked | Self::Running)
    }

    /// The state an exit code means, per `python/forge_gen/exit_codes.py`.
    ///
    /// Nothing is translated: 0 is done, 5 is a backend that ran and broke,
    /// everything else the table speaks is a refusal the caller fixes.
    #[must_use]
    pub const fn of_exit(code: i32) -> Self {
        match code {
            0 => Self::Done,
            5 => Self::Failed,
            _ => Self::Refused,
        }
    }

    /// Read the word back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "queued" => Self::Queued,
            "blocked" => Self::Blocked,
            "running" => Self::Running,
            "done" => Self::Done,
            "refused" => Self::Refused,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a caller asks for: everything admission needs and nothing it does
/// not.
///
/// `argv` is the `forge gen` command line *without* `--project` and
/// `--json`, which the executor appends: the queue does not compose a
/// generator's flags, it schedules the ones it was handed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    /// `<tool>.<verb>`, e.g. `generate_audio.sfx` — what `list_runs`,
    /// `status` and the monitor group by without parsing `argv`.
    pub kind: String,
    /// The `backends/<name>` this runs through, when the caller knows it.
    /// Admission resolves it and refuses a missing one with exit 3.
    pub backend: Option<String>,
    /// The generator command line, verbatim.
    pub argv: Vec<String>,
    /// Every path this job will write, relative to the project root. The
    /// out-path lease is taken on these at admission.
    pub outputs_claimed: Vec<String>,
    /// The record the generator will write, when the caller stated one.
    pub record: Option<String>,
    /// `human`, `agent:claude`, `cli` — who asked.
    pub created_by: String,
}

impl JobSpec {
    /// A spec with only the fields a caller always has.
    #[must_use]
    pub fn new(kind: impl Into<String>, argv: Vec<String>, created_by: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            backend: None,
            argv,
            outputs_claimed: Vec::new(),
            record: None,
            created_by: created_by.into(),
        }
    }
}

/// What the card cost a job, as observed around it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CardFacts {
    /// Seconds the lease was held.
    pub held_s: f64,
    /// Free VRAM in GB before the child started, when it could be read.
    pub vram_before_gb: Option<f64>,
    /// Free VRAM in GB after the release ladder, when it could be read.
    pub vram_after_gb: Option<f64>,
    /// Whether the `ComfyUI` unit had to be restarted to get the card back.
    /// Loud on purpose: this is a safety net, never routine.
    pub restarted: bool,
}

/// One job, as the file says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// [`JOB_SCHEMA`].
    pub forge_job: u32,
    /// The id.
    pub id: JobId,
    /// `<tool>.<verb>`.
    pub kind: String,
    /// Which executor ran it.
    pub executor: ExecutorKind,
    /// The backend it went through, when one was resolved.
    pub backend: Option<String>,
    /// The generator command line as submitted.
    pub argv: Vec<String>,
    /// The paths the out-path lease was taken on.
    pub outputs_claimed: Vec<String>,
    /// Where the job is.
    pub state: JobState,
    /// While `blocked`: who holds the card, by pid and GB, or `comfy` when
    /// the host would not give it back.
    pub blocked_by: Option<String>,
    /// When it was admitted.
    pub submitted: String,
    /// When the child started, if it did.
    pub started: Option<String>,
    /// When it reached a terminal state, if it has.
    pub finished: Option<String>,
    /// The child's exit code. `null` for every non-terminal state, for
    /// `cancelled` and for `interrupted` — never 0 by default.
    pub exit: Option<i32>,
    /// The record the generator says it wrote.
    pub record: Option<String>,
    /// The outputs the generator says it wrote.
    pub outputs: Vec<String>,
    /// The log file, relative to the project root.
    pub log: String,
    /// Who asked.
    pub created_by: String,
    /// The child's pid while it lives, kept afterwards so a cancel note can
    /// name it.
    pub pid: Option<u32>,
    /// Whether the result was served from `ComfyUI`'s node cache. **Observed**
    /// — from the child's own `comfy.cached` — never inferred.
    pub cached: bool,
    /// The earlier job whose output hashes equal this one's, when one was
    /// found.
    pub same_as: Option<JobId>,
    /// The refusal's own message, relayed unchanged.
    pub message: Option<String>,
    /// The refusal's own hint — the next command to type.
    pub hint: Option<String>,
    /// What the card cost, when this job took the lease.
    pub card: Option<CardFacts>,
    /// The child's `comfy` block, copied verbatim from its JSON last line.
    pub comfy: Option<Value>,
    /// The child's JSON last line, whole.
    ///
    /// Every field above that a generator supplied — `record`, `outputs`,
    /// `message`, `hint`, `comfy` — is a projection of this object, and it
    /// is kept because `forge gen --json` and the MCP frames must print the
    /// generator's own words, not a summary of them: a `seed`, an
    /// `elapsed_s` or a `fake` that only the child knows would otherwise be
    /// lost the moment a daemon stood between the child and the caller.
    #[serde(default)]
    pub payload: Option<Value>,
}

impl Job {
    /// A row for a spec that has just been admitted.
    #[must_use]
    pub fn admitted(
        id: JobId,
        spec: &JobSpec,
        executor: ExecutorKind,
        log: String,
        state: JobState,
    ) -> Self {
        Self {
            forge_job: JOB_SCHEMA,
            id,
            kind: spec.kind.clone(),
            executor,
            backend: spec.backend.clone(),
            argv: spec.argv.clone(),
            outputs_claimed: spec.outputs_claimed.clone(),
            state,
            blocked_by: None,
            submitted: forge_library::clock::now_iso(),
            started: None,
            finished: None,
            exit: None,
            record: spec.record.clone(),
            outputs: Vec::new(),
            log,
            created_by: spec.created_by.clone(),
            pid: None,
            cached: false,
            same_as: None,
            message: None,
            hint: None,
            card: None,
            comfy: None,
            payload: None,
        }
    }

    /// Move the row to a terminal state, stamping `finished`.
    ///
    /// `exit` is passed rather than derived: `cancelled` and `interrupted`
    /// have no exit code and must not acquire one.
    pub fn finish(&mut self, state: JobState, exit: Option<i32>) {
        debug_assert!(state.is_terminal(), "finish is for terminal states");
        self.state = state;
        self.exit = exit;
        self.finished = Some(forge_library::clock::now_iso());
    }

    /// Seconds between `started` (or `submitted`) and now or `finished`.
    #[must_use]
    pub fn elapsed_s(&self) -> Option<f64> {
        let from = forge_library::clock::parse_stamp(self.started.as_ref()?)?;
        let to = self
            .finished
            .as_deref()
            .map_or_else(forge_library::clock::unix_seconds, |stamp| {
                forge_library::clock::parse_stamp(stamp).unwrap_or(from)
            });
        Some(to.saturating_sub(from) as f64)
    }
}

/// Which rows `list` should return.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobFilter {
    /// Only rows in this state.
    pub state: Option<JobState>,
    /// Only rows whose `kind` starts with this — `generate_audio` matches
    /// `generate_audio.sfx`.
    pub kind: Option<String>,
    /// At most this many, newest first.
    pub limit: Option<usize>,
}

impl JobFilter {
    /// Whether a row passes.
    #[must_use]
    pub fn matches(&self, job: &Job) -> bool {
        if let Some(state) = self.state
            && job.state != state
        {
            return false;
        }
        if let Some(kind) = &self.kind
            && !job.kind.starts_with(kind.as_str())
        {
            return false;
        }
        true
    }
}

/// A slice of a job's log, and where to ask from next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogChunk {
    /// The bytes read, as text.
    pub text: String,
    /// The offset to pass as `from` next time — what
    /// `X-Forge-Log-Next` carries.
    pub next: u64,
    /// Whether the job is terminal, so this is the whole of it.
    pub done: bool,
}

/// One generated file found by walking `out/`, with the record beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// The output, relative to the project root.
    pub path: String,
    /// `sfx`, `music`, `speech`, `voice`, `take`, `lift` — the directory it
    /// came out of.
    pub kind: String,
    /// The generator the record names.
    pub tool: Option<String>,
    /// The prompt, when the record carries one.
    pub prompt: Option<String>,
    /// The seed, when the record carries one.
    pub seed: Option<String>,
    /// When it was generated.
    pub created: Option<String>,
    /// Who asked.
    pub created_by: Option<String>,
    /// Whether it is a placeholder.
    pub fake: bool,
    /// Whether a library sidecar's generator hash equals this output's —
    /// decided by **hashing**, never by remembering.
    pub promoted: bool,
    /// The record beside it.
    pub record: Option<String>,
}

/// Which runs the `out/` walk should return.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFilter {
    /// Only this kind.
    pub kind: Option<String>,
    /// Only runs created at or after this `YYYY-MM-DD` (or full stamp).
    pub since: Option<String>,
    /// At most this many, newest first.
    pub limit: Option<usize>,
}

/// The card, the queue and the daemon in one object — what `/v1/status`
/// answers and the `status` tool renders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The project root.
    pub project: String,
    /// `[hardware] tier`: full, lean or fake.
    pub tier: String,
    /// Where the `ComfyUI` host is, when the project says.
    pub comfy_url: Option<String>,
    /// The card, from `forge gpu --json`, cached two seconds.
    pub card: Value,
    /// How many rows sit in each non-terminal state.
    pub queue: QueueCounts,
    /// The non-terminal rows, oldest first.
    pub jobs: Vec<StatusJob>,
    /// The last terminal rows, newest first.
    pub recent: Vec<StatusJob>,
    /// The daemon's own facts.
    pub daemon: Value,
}

/// How many rows sit in each non-terminal state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueCounts {
    /// Waiting for the card.
    pub queued: usize,
    /// Waiting for a foreign holder to let go.
    pub blocked: usize,
    /// Alive.
    pub running: usize,
}

/// One row as `status` shows it: enough to know what is happening without
/// fetching the row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusJob {
    /// The id.
    pub id: JobId,
    /// `<tool>.<verb>`.
    pub kind: String,
    /// Where it is.
    pub state: JobState,
    /// Seconds since it started, when it has.
    pub elapsed_s: Option<f64>,
    /// How many rows are ahead of it in the FIFO.
    pub position: Option<usize>,
    /// When it finished, when it has.
    pub finished: Option<String>,
    /// The last few log lines, for a running row.
    pub log_tail: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_sortable_wellformed_and_unique() {
        let a = JobId::fresh();
        let b = JobId::fresh();
        assert_ne!(a, b, "two ids in one second must differ");
        assert!(JobId::is_wellformed(a.as_str()), "{a}");
        assert!(!JobId::is_wellformed("../etc/passwd"));
        assert!(!JobId::is_wellformed("j-2026/08"));
    }

    #[test]
    fn an_exit_code_reads_as_the_table_says_and_never_as_a_default() {
        assert_eq!(JobState::of_exit(0), JobState::Done);
        assert_eq!(JobState::of_exit(5), JobState::Failed);
        for code in [2, 3, 4, 6] {
            assert_eq!(JobState::of_exit(code), JobState::Refused, "exit {code}");
        }
        let mut job = Job::admitted(
            JobId::fresh(),
            &JobSpec::new("generate_audio.sfx", vec![String::from("sfx")], "cli"),
            ExecutorKind::Env,
            String::from("out/serve/logs/x.log"),
            JobState::Queued,
        );
        job.finish(JobState::Cancelled, None);
        assert_eq!(job.exit, None, "a cancelled job has no exit code");
        job.finish(JobState::Interrupted, None);
        assert_eq!(job.exit, None, "an interrupted job has no exit code");
    }

    #[test]
    fn a_row_round_trips_through_json_with_every_key_present() {
        let job = Job::admitted(
            JobId::from("j-20260830-141207-3f9a"),
            &JobSpec::new("generate_audio.sfx", vec![String::from("sfx")], "agent:x"),
            ExecutorKind::Comfy,
            String::from("out/serve/logs/j-20260830-141207-3f9a.log"),
            JobState::Queued,
        );
        let text = serde_json::to_string(&job).expect("serialise");
        for key in [
            "forge_job",
            "outputs_claimed",
            "blocked_by",
            "exit",
            "same_as",
            "cached",
            "comfy",
            "card",
            "hint",
        ] {
            assert!(text.contains(key), "{key} is not in {text}");
        }
        assert!(text.contains("\"exit\":null"), "{text}");
        let back: Job = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, job);
    }

    #[test]
    fn only_the_three_live_states_hold_an_output_path() {
        for state in [JobState::Queued, JobState::Blocked, JobState::Running] {
            assert!(state.holds_outputs(), "{state}");
            assert!(!state.is_terminal(), "{state}");
        }
        for state in [
            JobState::Done,
            JobState::Refused,
            JobState::Failed,
            JobState::Cancelled,
            JobState::Interrupted,
        ] {
            assert!(!state.holds_outputs(), "{state}");
            assert!(state.is_terminal(), "{state}");
        }
    }
}
