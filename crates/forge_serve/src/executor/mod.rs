//! Two executors, one spawn.
//!
//! `EnvExecutor` is today's launcher call, unmoved: `python3
//! <toolkit>/python/forge_gen <argv> --project <root> --json`, stdout
//! streamed line by line into the log with the last line held back as the
//! payload, stderr into the same log, its own process group so a cancel
//! kills the tree, and a kill by **recorded pid** — never `pkill -f`, which
//! matches the shell running it.
//!
//! `ComfyExecutor` is the same spawn wrapped in the card ladder of
//! `designs/serve.md` §5. One job shape, one log path, one cancel path, one
//! refusal shape — the daemon is a scheduler, not a second implementation,
//! and `just sfx` with no daemon up runs the identical code.
//!
//! Both are blocking and both run on a worker thread: they are
//! subprocess-shaped, and async buys nothing.

pub(crate) mod comfy;
pub(crate) mod env;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::job::ExecutorKind;
use crate::logs::LogSink;

/// What the worker settled before it took the card: which executor, which
/// backend, what it needs, and what to say about it.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The generator command line, without `--project` and `--json`.
    pub(crate) argv: Vec<String>,
    /// Which executor runs it.
    pub(crate) executor: ExecutorKind,
    /// The backend's `vram_gb` **budget**, never a measurement. `None`
    /// means the job wants no card.
    pub(crate) need_gb: Option<f64>,
    /// Where the `ComfyUI` host is, for a comfy job.
    pub(crate) comfy_url: Option<String>,
    /// The systemd unit the ladder may restart, for a comfy job.
    pub(crate) comfy_unit: Option<String>,
    /// `moss_sfx sfx`, for `card.json` and the blocked message.
    pub(crate) what: String,
    /// Whether this job is a `--fake` one: `FORGE_FAKE=1` in the child.
    pub(crate) fake: bool,
}

/// What one run left behind.
#[derive(Debug, Clone, Default)]
pub struct GenOutcome {
    /// The child's exit code, or `None` when a signal ended it.
    pub exit: Option<i32>,
    /// The terminating Unix signal, observed from the child status.
    pub signal: Option<i32>,
    /// The object on its last stdout line, when there was one.
    pub payload: Option<Value>,
    /// The pid it ran as, which is also its process group.
    pub pid: Option<u32>,
    /// What the card cost, when this job held it.
    pub card: Option<crate::job::CardFacts>,
    /// A note from the card ladder, when it had to say something.
    pub note: Option<String>,
}

/// A job's cancel flag: set by [`crate::LocalQueue::cancel`], read by the
/// worker after the child is reaped.
#[derive(Debug, Clone, Default)]
pub(crate) struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, unset token.
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Ask for the job to stop.
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether a cancel was asked for.
    pub(crate) fn cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Everything a spawn needs that is the same for every job.
#[derive(Debug)]
pub(crate) struct Launch {
    /// The project the child works on.
    pub(crate) project_root: PathBuf,
    /// The rig profile to hand down, unless the caller set one.
    pub(crate) rig_profile: PathBuf,
    /// The job's id, so a generator can name its outputs after it.
    pub(crate) job_id: String,
    /// The toolkit-relative launcher, from `forge_library::backends`.
    pub(crate) launcher: std::process::Command,
}

/// The two executors, behind one call.
pub(crate) trait Executor: Send + Sync {
    /// Run the child to completion, streaming into the log.
    fn run(
        &self,
        plan: &Plan,
        launch: Launch,
        log: &Arc<std::sync::Mutex<LogSink>>,
        cancel: &CancelToken,
        on_pid: &mut dyn FnMut(u32),
    ) -> GenOutcome;
}

/// The set the worker picks from.
#[derive(Debug, Default)]
pub(crate) struct ExecutorSet;

impl ExecutorSet {
    /// The executor for a plan.
    pub(crate) fn for_plan(plan: &Plan) -> Box<dyn Executor> {
        match plan.executor {
            ExecutorKind::Env => Box::new(env::EnvExecutor),
            ExecutorKind::Comfy => Box::new(comfy::ComfyExecutor),
        }
    }
}

/// SIGTERM a process group, then SIGKILL it ten seconds later if it is
/// still there.
///
/// By **recorded pid**, never by pattern: `pkill -f <pattern>` matches the
/// shell running the `pkill` when the pattern is on the same command line,
/// and kills it. That trap is in `CLAUDE.md` because this repository has
/// already paid for it.
pub(crate) fn terminate_group(pid: u32) {
    let Ok(group) = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap_or(0)).ok_or(())
    else {
        return;
    };
    // rustix rather than a raw libc call, because this crate denies unsafe
    // code and a signal to the wrong group is the one mistake here that
    // cannot be taken back.
    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(10));
        if crate::store::pid_alive(pid) {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    });
}

/// SIGKILL a process group now.
///
/// The last step of a stop, for the case the SIGTERM was not taken: the
/// thread [`terminate_group`] spawns dies with the process that is exiting,
/// and exiting with a generator still on the card is the one outcome a stop
/// may not have.
pub(crate) fn kill_group(pid: u32) {
    if let Ok(group) = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap_or(0)).ok_or(()) {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
}

/// A line that is one JSON object, or nothing — the same rule
/// `commands/generate.rs` has always applied to a generator's last line.
pub(crate) fn parse_object(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_json_object_on_the_last_line_is_the_payload() {
        assert!(parse_object("{\"ok\": true}").is_some());
        assert!(parse_object("  {\"ok\": true}  ").is_some());
        assert!(parse_object("[1, 2]").is_none());
        assert!(parse_object("fit: 0.83 of the frame").is_none());
        assert!(parse_object("{not json}").is_none());
    }

    #[test]
    fn a_cancel_token_is_shared_by_clone() {
        let token = CancelToken::new();
        let copy = token.clone();
        assert!(!copy.cancelled());
        token.cancel();
        assert!(copy.cancelled());
    }
}
