//! What a restarted daemon may and may not conclude about the rows it
//! finds.

mod common;

use forge_serve::{ExecutorKind, Job, JobId, JobSpec, JobState, JobStore, Queue};

/// A row on disk, written the way a previous daemon would have left it.
fn write_row(
    store: &JobStore,
    id: &str,
    state: JobState,
    executor: ExecutorKind,
    pid: Option<u32>,
) {
    let mut job = Job::admitted(
        JobId::from(id),
        &JobSpec::new(
            "generate_audio.sfx",
            vec![String::from("sfx"), String::from(id)],
            "cli",
        ),
        executor,
        format!("out/serve/logs/{id}.log"),
        state,
    );
    job.state = state;
    job.pid = pid;
    if state == JobState::Running {
        job.started = Some(String::from("2026-08-30T14:12:09Z"));
    }
    store.write(&job).expect("write the row");
}

/// A daemon that restarted cannot `waitpid` on a process it did not fork
/// and cannot read a pipe that died with its parent, so it can observe
/// neither the exit code nor the last JSON line. The row says so, and no
/// later pass ever moves it to `done`.
#[test]
fn an_interrupted_row_is_never_completed_by_inference() {
    let (_dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    write_row(
        &store,
        "j-20260830-141207-env0",
        JobState::Running,
        ExecutorKind::Env,
        Some(0x00ff_fffe),
    );
    // A comfy row is not the exception: finishing one out of GET /history
    // would drag /view fetching and a record write into Rust, which is the
    // second record writer this design refuses.
    write_row(
        &store,
        "j-20260830-141208-cmfy",
        JobState::Running,
        ExecutorKind::Comfy,
        Some(0x00ff_ffff),
    );

    let queue = common::queue(&project, None);
    for id in ["j-20260830-141207-env0", "j-20260830-141208-cmfy"] {
        let row = queue
            .get(&JobId::from(id))
            .expect("read")
            .expect("the row survived");
        assert_eq!(row.state, JobState::Interrupted, "{id}");
        assert_eq!(row.exit, None, "an interrupted row has no exit code: {id}");
        let message = row.message.unwrap_or_default();
        assert!(message.contains("restarted"), "{id}: {message}");
        assert!(
            message.contains("list_runs"),
            "{id}: the row says where whatever it wrote is found: {message}"
        );
    }
    queue.stop();
    drop(queue);

    // A second restart does not change its mind either.
    let queue = common::queue(&project, None);
    let row = queue
        .get(&JobId::from("j-20260830-141207-env0"))
        .expect("read")
        .expect("still there");
    assert_eq!(row.state, JobState::Interrupted);
    assert_eq!(row.exit, None);
    queue.stop();
}

/// A queued row ran nothing and derived nothing, so it goes back in the
/// line it was in — the one-way rule is about a file that exists, and there
/// is no half-written `.glb` here to repair.
#[test]
fn queued_rows_are_requeued_in_submitted_order() {
    let (dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    let trace = dir.path().join("order.txt");
    // Three rows a previous daemon admitted and never ran, one of them
    // blocked on a card that is long gone.
    write_row(
        &store,
        "j-20260830-141201-aaaa",
        JobState::Queued,
        ExecutorKind::Env,
        None,
    );
    write_row(
        &store,
        "j-20260830-141202-bbbb",
        JobState::Blocked,
        ExecutorKind::Env,
        None,
    );
    write_row(
        &store,
        "j-20260830-141203-cccc",
        JobState::Queued,
        ExecutorKind::Env,
        None,
    );

    let script = common::stub(
        dir.path(),
        "order.py",
        &format!(
            r#"
import json, os, sys
with open({trace:?}, "a") as f:
    f.write(os.environ.get("FORGE_JOB_ID", "?") + "\n")
print(json.dumps({{"ok": True, "outputs": []}}))
"#,
            trace = trace.display().to_string()
        ),
    );
    let queue = common::queue(&project, Some(&script));
    for id in [
        "j-20260830-141201-aaaa",
        "j-20260830-141202-bbbb",
        "j-20260830-141203-cccc",
    ] {
        let row = common::finished(queue.as_ref(), &JobId::from(id), 30);
        assert_eq!(row.state, JobState::Done, "{id}: {:?}", row.message);
    }
    let order = std::fs::read_to_string(&trace).expect("the trace");
    assert_eq!(
        order.lines().collect::<Vec<_>>(),
        vec![
            "j-20260830-141201-aaaa",
            "j-20260830-141202-bbbb",
            "j-20260830-141203-cccc"
        ],
        "the FIFO is submitted order, and a blocked row is not sent to the back"
    );
    queue.stop();
}

/// Only the daemon takes what a previous process left.
///
/// A `forge gen sfx` with no daemon up opens a queue of its own to run one
/// job. It used to reconcile and adopt every `queued` and `blocked` row on
/// disk first — a plain `forge jobs` had turned another session's `blocked`
/// row back into `queued`, and the next generate ran a stranger's forgotten
/// job on the card before its own, with nothing on stdout saying so
/// (2026-08-30). The row is left exactly as it is; `forge jobs` shows it,
/// and the next daemon picks it up in submitted order.
#[test]
fn an_in_process_queue_runs_its_own_job_and_adopts_nothing() {
    let (dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    write_row(
        &store,
        "j-20260830-141201-left",
        JobState::Queued,
        ExecutorKind::Env,
        None,
    );
    let trace = dir.path().join("ran.txt");
    let script = common::stub(
        dir.path(),
        "ran.py",
        &format!(
            r#"
import json, os
with open({trace:?}, "a") as f:
    f.write(os.environ.get("FORGE_JOB_ID", "?") + "\n")
print(json.dumps({{"ok": True, "outputs": []}}))
"#,
            trace = trace.display().to_string()
        ),
    );
    let queue = forge_serve::LocalQueue::open(
        &project,
        forge_serve::LocalQueueOptions {
            adopt: false,
            ..common::options(Some(&script))
        },
    )
    .expect("queue");
    let mine = queue
        .submit(JobSpec::new(
            "generate_audio.sfx",
            vec![String::from("sfx")],
            "cli",
        ))
        .expect("admitted");
    let mine = common::finished(queue.as_ref(), &mine.id, 30);
    assert_eq!(mine.state, JobState::Done, "{:?}", mine.message);

    let left = queue
        .get(&JobId::from("j-20260830-141201-left"))
        .expect("read")
        .expect("still there");
    assert_eq!(
        left.state,
        JobState::Queued,
        "the row a previous process left is untouched, not run"
    );
    let ran = std::fs::read_to_string(&trace).unwrap_or_default();
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
        vec![mine.id.as_str()],
        "exactly one job ran, and it is this door's own"
    );
    queue.stop();
}
