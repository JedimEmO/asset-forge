//! `EnvExecutor`: `python3 <toolkit>/python/forge_gen <argv> --project
//! <root> --json`, lifted out of `commands/generate.rs::spawn_with` so the
//! daemon and the CLI call one function.
//!
//! The rules it keeps, unchanged from the day the CLI was the only door:
//!
//! - `--project <root>` and `--json` are appended, so the child's last
//!   stdout line is one JSON object and its record carries project-relative
//!   paths.
//! - `FORGE_RIG_PROFILE` is set from the project **unless the caller set
//!   it**: two readers of one fact with different defaults is the shape of
//!   every silent mismatch this repository's ledger records.
//! - stdout is read line by line with the **last line held back** as the
//!   payload; stderr joins the same log, interleaved as it arrived.
//! - The child gets its own process group, so a cancel kills the tree.
//! - `FORGE_NO_DAEMON=1`, so a re-entered `forge gen` never rediscovers the
//!   daemon and recurses.

use std::io::{BufRead as _, BufReader};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use crate::logs::LogSink;

use super::{CancelToken, Executor, GenOutcome, Launch, Plan, parse_object};

/// The generator, run in its own environment.
pub(crate) struct EnvExecutor;

impl Executor for EnvExecutor {
    fn run(
        &self,
        plan: &Plan,
        launch: Launch,
        log: &Arc<Mutex<LogSink>>,
        cancel: &CancelToken,
        on_pid: &mut dyn FnMut(u32),
    ) -> GenOutcome {
        spawn(plan, launch, log, cancel, on_pid)
    }
}

/// Run one generator command to completion.
///
/// Shared with [`super::comfy`], which wraps this in the card ladder rather
/// than spawning differently: one job shape, one log path, one cancel path.
pub(crate) fn spawn(
    plan: &Plan,
    launch: Launch,
    log: &Arc<Mutex<LogSink>>,
    cancel: &CancelToken,
    on_pid: &mut dyn FnMut(u32),
) -> GenOutcome {
    use std::os::unix::process::CommandExt as _;

    let Launch {
        project_root,
        rig_profile,
        job_id,
        mut launcher,
    } = launch;
    launcher.args(&plan.argv);
    launcher.arg("--project").arg(&project_root);
    launcher.arg("--json");
    if std::env::var_os("FORGE_RIG_PROFILE").is_none() {
        launcher.env("FORGE_RIG_PROFILE", &rig_profile);
    }
    launcher.env(crate::NO_DAEMON_ENV, "1");
    launcher.env(crate::JOB_ID_ENV, &job_id);
    if plan.fake {
        launcher.env("FORGE_FAKE", "1");
    }
    launcher.stdin(Stdio::null());
    launcher.stdout(Stdio::piped());
    launcher.stderr(Stdio::piped());
    // The whole tree, so a cancel reaches a generator's own children.
    launcher.process_group(0);

    say(log, &format!("$ forge gen {}", plan.argv.join(" ")));
    let mut child = match launcher.spawn() {
        Ok(child) => child,
        Err(err) => {
            say(
                log,
                &format!(
                    "python3 could not be started: {err} — the Python layer needs python3 >= \
                     3.11 on PATH"
                ),
            );
            return GenOutcome {
                // 6 is the table's missing-tool code, which is what this is.
                exit: Some(6),
                payload: None,
                pid: None,
                card: None,
                note: None,
            };
        }
    };
    let pid = child.id();
    on_pid(pid);

    let stderr = child.stderr.take().map(|stderr| {
        let log = Arc::clone(log);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                say(&log, &line);
            }
        })
    });
    let mut held: Option<String> = None;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(previous) = held.replace(line) {
                say(log, &previous);
            }
        }
    }
    let status = child.wait();
    if let Some(stderr) = stderr {
        let _ = stderr.join();
    }
    let payload = held.as_deref().and_then(parse_object);
    // A last line that is not the payload is still the child's own word,
    // and the log is where it belongs.
    if payload.is_none()
        && let Some(last) = &held
    {
        say(log, last);
    }
    if cancel.cancelled() {
        say(log, "cancelled: the process group was signalled");
    }
    GenOutcome {
        exit: status.ok().and_then(|status| status.code()),
        payload,
        pid: Some(pid),
        card: None,
        note: None,
    }
}

/// One line into the log, whoever holds it.
pub(crate) fn say(log: &Arc<Mutex<LogSink>>, line: &str) {
    if let Ok(mut sink) = log.lock() {
        sink.line(line);
    }
}
