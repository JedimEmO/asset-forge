//! What every test here needs: a throwaway project, a stub generator, and
//! a card reader that is not this test binary.
//!
//! The stub generator is a python script rather than a mock inside the
//! crate, because the thing under test is *spawning a child and reading its
//! last line* — a mock would test the part that was never in doubt.

// A shared helper module is compiled into every test binary that names
// it, so most of it is dead code in most of them; and `unreachable_pub`
// has nothing to say about a module nobody outside these tests can see.
#![allow(dead_code, unreachable_pub)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use forge_library::Project;
use forge_serve::{Job, JobId, LocalQueue, LocalQueueOptions, Queue};

/// A project with nothing in it.
pub fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "serve_test").expect("init");
    (dir, project)
}

/// Options that never reach the real card or the real launcher.
///
/// `forge` is a path that does not exist on purpose: the card reader
/// re-invokes the `forge` binary, and pointing it at this test binary would
/// have a test spawn the test harness with `gpu --json` as a filter.
pub fn options(script: Option<&Path>) -> LocalQueueOptions {
    LocalQueueOptions {
        forge: PathBuf::from("/nonexistent/forge-for-tests"),
        // Empty on purpose, and not `None`: `None` falls through to the
        // host backend's own `[server]` block, which on a developer's box
        // is a ComfyUI that is actually up — a test would then read its
        // `/system_stats` and POST `/free` to it, unloading a model no test
        // put there. No test in this crate reaches a real host.
        comfy_url: Some(String::new()),
        launcher: script.map(|path| vec![String::from("python3"), path.display().to_string()]),
        ..LocalQueueOptions::default()
    }
}

/// Write a python stub generator and return its path.
pub fn stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write the stub");
    path
}

/// A queue over a project, with a stub generator.
pub fn queue(project: &Project, script: Option<&Path>) -> std::sync::Arc<LocalQueue> {
    LocalQueue::open(project, options(script)).expect("queue")
}

/// Wait for a row to satisfy something, or fail saying what it was instead.
pub fn until(
    queue: &dyn Queue,
    id: &JobId,
    what: &str,
    seconds: u64,
    ready: impl Fn(&Job) -> bool,
) -> Job {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last = None;
    while Instant::now() < deadline {
        if let Ok(Some(job)) = queue.get(id) {
            if ready(&job) {
                return job;
            }
            last = Some(job);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "{id} never {what}; it is {:?}",
        last.map(|job| (job.state, job.blocked_by, job.message))
    );
}

/// Wait for a row to reach any terminal state.
pub fn finished(queue: &dyn Queue, id: &JobId, seconds: u64) -> Job {
    until(queue, id, "finished", seconds, |job| {
        job.state.is_terminal()
    })
}
