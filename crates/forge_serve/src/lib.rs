//! The queue behind every generate: one FIFO, one worker, one card lock.
//!
//! > **Nothing in Rust learns what a graph is, and nothing in Python learns
//! > what the queue is.**
//!
//! Two doors now share one 24 GB card and one library — a human at a
//! terminal and an agent over MCP — and a generate that takes four minutes
//! cannot be a blocking tool call. So a generate is a *job*: admitted,
//! queued, given the card when the card is free, run as the same
//! `python/forge_gen` child both doors have always run, and left on disk as
//! a row anyone can read afterwards.
//!
//! # What a caller holds
//!
//! One thing: an [`Arc<dyn Queue>`](Queue). [`LocalQueue`] owns a worker and
//! the state directory in this process; [`RemoteQueue`] is an HTTP client of
//! a `forge serve` daemon. The CLI and `forge_mcp` never learn which they
//! have — [`discovery::find`] decides, and a machine with no daemon runs the
//! identical code path with a queue of one.
//!
//! # What it never does
//!
//! **It never writes a generator record.** `records.py` is the one writer of
//! that schema; this crate copies what a child told it and observes nothing
//! it did not run. `forge_serve/tests/no_record_writer.rs` reads this
//! crate's own source to keep that true.
//!
//! # The state directory
//!
//! ```text
//! <project>/out/serve/
//!   daemon.json      the endpoint file: pid, kernel start time, port, token (mode 0600)
//!   daemon.lock      exclusive flock while a daemon is up
//!   card.lock        the card lease — the truth about who holds the GPU
//!   card.json        who holds it, for humans and for `forge gpu`
//!   jobs/<id>.json   the row, rewritten atomically on every transition
//!   logs/<id>.log    the child's interleaved stdout+stderr, capped at 8 MB
//! ```
//!
//! Under `out/` and not under XDG, because the queue is per project and its
//! logs name paths under `out/`: `rm -rf out/` should lose a job log and the
//! artefact it describes *together* rather than leave one orphaned story
//! about the other.

mod backend_facts;
mod card;
mod client;
pub mod daemon;
pub mod discovery;
mod executor;
pub mod http;
mod job;
mod logs;
mod queue;
mod runs;
mod store;
mod wire;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use card::{
    CardLease, CardReader, CardRelease, CardState, card_json_path, comfy_free_gb, release_comfy,
    release_withhold, withhold,
};
pub use client::RemoteQueue;
pub use executor::GenOutcome;
pub use job::{
    CardFacts, ExecutorKind, JOB_SCHEMA, Job, JobFilter, JobId, JobSpec, JobState, LogChunk,
    QueueCounts, Run, RunFilter, Status, StatusJob,
};
pub use queue::{LocalQueue, LocalQueueOptions};
pub use store::{Daemon, JobStore};

/// The environment variable that forces an in-process queue.
///
/// Set in **every** child this crate spawns, so a re-entered `forge gen`
/// never rediscovers the daemon and recurses.
pub const NO_DAEMON_ENV: &str = "FORGE_NO_DAEMON";

/// The environment variable naming a daemon to talk to, or `off` to force
/// an in-process queue.
pub const SERVE_URL_ENV: &str = "FORGE_SERVE_URL";

/// The environment variable carrying a job's id into its child, so a
/// generator can name its outputs after the job that asked for them.
pub const JOB_ID_ENV: &str = "FORGE_JOB_ID";

/// How long a row and its log are kept before both are pruned, together.
pub const LOG_KEEP_DAYS: u64 = 14;

/// `<project>/out/serve` — where every file this crate writes lives.
#[must_use]
pub fn state_dir(project_root: &Path) -> PathBuf {
    project_root.join("out").join("serve")
}

/// Why a queue call could not be answered.
///
/// A `Refused` is the caller's to fix and carries the sentence that says
/// how; everything else is this side's. A refusal that names alternatives
/// beats a failure that names none, which is why [`Self::NoSuchJob`] carries
/// the ids that do exist.
#[derive(Debug)]
#[non_exhaustive]
pub enum ServeError {
    /// The call cannot be honoured as written: a claimed output path, a
    /// backend that is not installed, a cancel of a terminal job.
    Refused(String),
    /// No job by that id; the ids that do exist, newest first.
    NoSuchJob {
        /// What was asked for.
        id: JobId,
        /// The ten most recent ids, so the next call can be right.
        existing: Vec<JobId>,
    },
    /// The state directory would not read or write.
    Io(String),
    /// The daemon answered, and not with what its own contract promises.
    Protocol(String),
    /// The daemon could not be reached.
    Unreachable(String),
}

impl ServeError {
    /// A refusal with the sentence that fixes the call.
    #[must_use]
    pub fn refused(message: impl Into<String>) -> Self {
        Self::Refused(message.into())
    }

    /// An I/O failure, named with the path it was on.
    #[must_use]
    pub fn io(path: &Path, err: &std::io::Error) -> Self {
        Self::Io(format!("{}: {err}", path.display()))
    }
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Io(message) => f.write_str(message),
            Self::NoSuchJob { id, existing } => {
                if existing.is_empty() {
                    write!(f, "no job {id} — this project has no jobs yet")
                } else {
                    let names: Vec<&str> = existing.iter().map(JobId::as_str).collect();
                    write!(f, "no job {id} — the jobs that exist: {}", names.join(", "))
                }
            }
            Self::Protocol(detail) => write!(f, "the daemon answered oddly: {detail}"),
            Self::Unreachable(detail) => write!(f, "no daemon answered: {detail}"),
        }
    }
}

impl std::error::Error for ServeError {}

/// The queue, as everything above it sees it.
///
/// Eight methods, all blocking, no async: the CLI is synchronous, the MCP
/// tools call these from `spawn_blocking`, and a trait that is one shape for
/// an in-process worker and a remote daemon is the whole point.
pub trait Queue: Send + Sync {
    /// Admit a job, or refuse it before it can cost anything: a missing
    /// backend (exit 3, in under a millisecond) or an output path another
    /// live row already claims.
    ///
    /// # Errors
    ///
    /// [`ServeError::Refused`] for either refusal; [`ServeError::Io`] when
    /// the row cannot be written.
    fn submit(&self, spec: JobSpec) -> Result<Job, ServeError>;

    /// One row by id, or `None`.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the state directory cannot be read.
    fn get(&self, id: &JobId) -> Result<Option<Job>, ServeError>;

    /// The rows a filter passes, newest first.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the state directory cannot be read.
    fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, ServeError>;

    /// Stop a job: SIGTERM to its process group, SIGKILL after ten seconds.
    /// Cancelling a terminal job is a refusal naming its state.
    ///
    /// # Errors
    ///
    /// [`ServeError::NoSuchJob`], or [`ServeError::Refused`] when the job is
    /// already terminal.
    fn cancel(&self, id: &JobId) -> Result<Job, ServeError>;

    /// The job's log from a byte offset.
    ///
    /// # Errors
    ///
    /// [`ServeError::NoSuchJob`] or [`ServeError::Io`].
    fn log(&self, id: &JobId, from: u64) -> Result<LogChunk, ServeError>;

    /// The card, the queue and the daemon in one object.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the state directory cannot be read.
    fn status(&self) -> Result<Status, ServeError>;

    /// The `out/` walk: every generated file with the record beside it.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when `out/` cannot be read.
    fn runs(&self, filter: &RunFilter) -> Result<Vec<Run>, ServeError>;

    /// Block until the job is terminal or `max` elapses. The ceiling is the
    /// caller's: a still-running job is a successful answer, not a timeout.
    ///
    /// # Errors
    ///
    /// [`ServeError::NoSuchJob`] or [`ServeError::Io`].
    fn wait(&self, id: &JobId, max: Duration) -> Result<Job, ServeError>;
}

/// A queue for this project: the daemon's if one is up, else one of our own.
///
/// # Errors
///
/// [`ServeError::Io`] when the state directory cannot be made.
pub fn queue_for(project: &forge_library::Project) -> Result<Arc<dyn Queue>, ServeError> {
    discovery::queue_for(project)
}
