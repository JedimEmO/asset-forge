//! Which queue this process holds, decided once and never asked again.
//!
//! 1. `$FORGE_SERVE_URL` — a daemon elsewhere, or `off` to force
//!    in-process.
//! 2. `<project>/out/serve/daemon.json` — its pid is alive **and** its
//!    `/proc` start time equals the recorded one **and** `GET /v1/health`
//!    answers within 300 ms with a matching project. → a
//!    [`RemoteQueue`](crate::RemoteQueue).
//! 3. Otherwise the file is removed when its pid is dead, and the queue is a
//!    [`LocalQueue`](crate::LocalQueue) in this process — a queue of one,
//!    taking the same `card.lock`.
//!
//! `FORGE_NO_DAEMON=1` forces (3), and every child this crate spawns has it,
//! so a re-entered `forge gen` cannot recurse into the daemon that started
//! it. `FORGE_FAKE=1` does not change discovery: a fake job is an ordinary
//! job.
//!
//! The start time is not decoration. A pid file without one eventually
//! signals a stranger — `music.py`'s resident server learned that the
//! expensive way — and this file hands out a bearer token that drives a GPU.

use std::sync::Arc;
use std::time::Duration;

use forge_library::Project;

use crate::client::{HEALTH_TIMEOUT, RemoteQueue};
use crate::queue::{LocalQueue, LocalQueueOptions};
use crate::store::{JobStore, pid_alive, proc_start_ticks};
use crate::{NO_DAEMON_ENV, Queue, SERVE_URL_ENV, ServeError};

/// The daemon serving this project, when one is up and answering.
#[must_use]
pub fn find(project: &Project) -> Option<RemoteQueue> {
    if std::env::var_os(NO_DAEMON_ENV).is_some_and(|value| value == "1") {
        return None;
    }
    match std::env::var(SERVE_URL_ENV) {
        Ok(url) if url.trim().eq_ignore_ascii_case("off") => return None,
        Ok(url) if !url.trim().is_empty() => {
            let (url, token) = url
                .trim()
                .split_once('#')
                .map_or((url.trim(), String::new()), |(url, token)| {
                    (url, token.to_owned())
                });
            let remote = RemoteQueue::new(url, &token);
            return remote.health(Duration::from_secs(2)).ok().map(|_| remote);
        }
        _ => {}
    }
    let store = JobStore::open(&project.root).ok()?;
    let daemon = store.daemon()?;
    if !pid_alive(daemon.pid) || proc_start_ticks(daemon.pid) != Some(daemon.start_ticks) {
        // The file names a process that is not there, or a stranger who
        // inherited its number. Either way it is stale.
        store.remove_daemon();
        return None;
    }
    let remote = RemoteQueue::new(&daemon.url, &daemon.token);
    let health = remote.health(HEALTH_TIMEOUT).ok()?;
    let says = |key: &str| {
        health
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    (says("project") == project.root.display().to_string()).then_some(remote)
}

/// The queue for this project: the daemon's if one answers, else our own.
///
/// # Errors
///
/// [`ServeError::Io`] when the state directory cannot be made.
pub fn queue_for(project: &Project) -> Result<Arc<dyn Queue>, ServeError> {
    queue_with(project, LocalQueueOptions::default())
}

/// As [`queue_for`], with the local queue's options stated — what the CLI
/// uses to hand down `[hardware] tier` and the `ComfyUI` url.
///
/// # Errors
///
/// [`ServeError::Io`] when the state directory cannot be made.
pub fn queue_with(
    project: &Project,
    options: LocalQueueOptions,
) -> Result<Arc<dyn Queue>, ServeError> {
    if let Some(remote) = find(project) {
        return Ok(Arc::new(remote));
    }
    Ok(LocalQueue::open(project, options)?)
}
