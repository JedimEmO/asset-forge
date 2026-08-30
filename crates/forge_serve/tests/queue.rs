//! Admission and the FIFO: what the queue refuses before anything can cost
//! a GPU minute, and that a placeholder run is not a second path through it.

mod common;

use forge_serve::{JobSpec, JobState, Queue};

/// The out-path lease. Two doors asking for `out/audio/sfx/door.wav` a
/// second apart is the ordinary case, not the exotic one, and the second
/// one has to be told which row it is waiting on.
#[test]
fn two_jobs_for_one_output_path_are_refused() {
    let (_dir, project) = common::project();
    let queue = common::queue(&project, None);
    let spec = |name: &str| JobSpec {
        kind: String::from("generate_audio.sfx"),
        backend: Some(String::from("moss_sfx")),
        argv: vec![
            String::from("sfx"),
            String::from("--prompt"),
            String::from("a door"),
        ],
        outputs_claimed: vec![format!("out/audio/sfx/{name}.wav")],
        record: Some(format!("out/audio/sfx/{name}.json")),
        created_by: String::from("cli"),
        fake: None,
    };
    let first = queue.submit(spec("door")).expect("the first is admitted");
    let refusal = queue
        .submit(spec("door"))
        .expect_err("the second is refused");
    let text = refusal.to_string();
    assert!(
        text.contains(first.id.as_str()),
        "the refusal must name the row that holds the path: {text}"
    );
    assert!(text.contains("out/audio/sfx/door.wav"), "{text}");
    // Another name is not the same path, so it is admitted.
    queue.submit(spec("gate")).expect("another name is free");
}

/// A fake job is an ordinary job: same admission, same FIFO, same lease.
/// Letting fakes skip the queue would quietly lose `ci-fake` the
/// serialisation it has today, which is most of what it proves.
#[test]
fn a_fake_job_takes_the_queue_like_any_other() {
    let (dir, project) = common::project();
    let trace = dir.path().join("trace.txt");
    let script = common::stub(
        dir.path(),
        "fake_gen.py",
        &format!(
            r#"
import json, os, sys, time
trace = {trace:?}
job = os.environ.get("FORGE_JOB_ID", "?")
with open(trace, "a") as f:
    f.write("start " + job + "\n")
print("fake: writing a placeholder")
time.sleep(0.4)
with open(trace, "a") as f:
    f.write("stop " + job + "\n")
print(json.dumps({{"ok": True, "fake": True, "outputs": [], "no_daemon": os.environ.get("FORGE_NO_DAEMON")}}))
"#,
            trace = trace.display().to_string()
        ),
    );
    let queue = common::queue(&project, Some(&script));
    let spec = |name: &str| JobSpec {
        kind: String::from("generate_audio.sfx"),
        backend: Some(String::from("moss_sfx")),
        argv: vec![
            String::from("sfx"),
            String::from("--name"),
            String::from(name),
        ],
        outputs_claimed: vec![format!("out/audio/sfx/{name}.wav")],
        record: None,
        created_by: String::from("cli"),
        fake: None,
    };
    let first = queue.submit(spec("one")).expect("admitted");
    let second = queue.submit(spec("two")).expect("admitted");
    let first = common::finished(queue.as_ref(), &first.id, 30);
    let second = common::finished(queue.as_ref(), &second.id, 30);
    assert_eq!(first.state, JobState::Done, "{:?}", first.message);
    assert_eq!(first.exit, Some(0));
    assert_eq!(second.state, JobState::Done);

    let trace = std::fs::read_to_string(&trace).expect("the trace");
    let lines: Vec<&str> = trace.lines().collect();
    assert_eq!(lines.len(), 4, "{trace}");
    assert!(lines[0].starts_with("start"), "{trace}");
    assert!(
        lines[1].starts_with("stop") && lines[1].ends_with(lines[0].trim_start_matches("start ")),
        "the second job must not start before the first stopped:\n{trace}"
    );

    // Every child sees FORGE_NO_DAEMON, so a re-entered `forge gen` cannot
    // rediscover the daemon that started it.
    assert_eq!(
        first
            .payload
            .as_ref()
            .and_then(|p| p.get("no_daemon"))
            .and_then(serde_json::Value::as_str),
        Some("1"),
        "{:?}",
        first.payload
    );
    // The card was taken and released around each of them.
    assert!(first.card.is_some(), "a fake job holds the lease too");
}

/// The exit-code table is relayed, never translated: 5 is a backend that
/// ran and broke, and 4 is an input the caller has to fix.
#[test]
fn the_exit_code_decides_refused_or_failed() {
    let (dir, project) = common::project();
    let script = common::stub(
        dir.path(),
        "refuse.py",
        r#"
import json, sys
print(json.dumps({"ok": False, "error": "input_rejected", "reason": "no flat border",
                  "hint": "re-key the PNG"}))
sys.exit(4)
"#,
    );
    let queue = common::queue(&project, Some(&script));
    let job = queue
        .submit(JobSpec::new(
            "generate_mesh.lift",
            vec![String::from("mesh")],
            "cli",
        ))
        .expect("admitted");
    let job = common::finished(queue.as_ref(), &job.id, 30);
    assert_eq!(job.state, JobState::Refused);
    assert_eq!(job.exit, Some(4));
    assert_eq!(job.message.as_deref(), Some("no flat border"));
    assert_eq!(job.hint.as_deref(), Some("re-key the PNG"));
}

/// A backend nobody installed cannot run, so the queue does not hold a job
/// that cannot run: the row is refused with the table's own exit 3 before
/// anything is spawned.
///
/// The project is pointed at an EMPTY backends directory rather than left to
/// find the toolkit's own. Absence is a property of the machine, and this
/// phase moved `moss_sfx` into the comfy executor — on a developer's box,
/// where the `ComfyUI` unit is up, the old form of this test admitted the job
/// and spent the card inside `just ci`. A gate never depends on what happens
/// not to be installed.
#[test]
fn a_missing_backend_is_refused_with_exit_three_before_the_queue() {
    let (dir, project) = common::project();
    let empty = dir.path().join("no-backends-here");
    std::fs::create_dir_all(&empty).expect("an empty backends directory");
    let toml = project.root.join("forge.toml");
    let text = std::fs::read_to_string(&toml).expect("forge.toml");
    let pointed = text.replace(
        "[backends]\n",
        &format!("[backends]\ndir = \"{}\"\n", empty.display()),
    );
    assert_ne!(pointed, text, "the template still has a [backends] table");
    std::fs::write(&toml, pointed).expect("point the project at it");
    let project = forge_library::Project::load(&project.root).expect("reload");
    let queue = common::queue(&project, None);
    let job = queue
        .submit(JobSpec {
            kind: String::from("generate_audio.sfx"),
            backend: Some(String::from("moss_sfx")),
            argv: vec![String::from("sfx")],
            outputs_claimed: Vec::new(),
            record: None,
            created_by: String::from("agent:test"),
            // Stated, never inherited from the environment: a `just ci` run
            // with FORGE_FAKE exported would otherwise skip the very check
            // this test is about, because a fake job needs no backend.
            fake: Some(false),
        })
        .expect("a refusal is a row, not an error");
    assert_eq!(job.state, JobState::Refused);
    assert_eq!(job.exit, Some(3));
    let message = job.message.unwrap_or_default();
    assert!(message.contains("moss_sfx"), "{message}");
    assert!(job.hint.unwrap_or_default().contains("doctor"));
}

/// The terminal door plans like the agent's door, because there is one map.
///
/// A `just sfx` command line used to reach admission with `backend: null`,
/// so it planned as `executor: env` with no budget: no exit-3 refusal, no
/// `blocked` behind a foreign holder, and the comfy free/restart/withhold
/// ladder never ran for it, while the same work through `generate_audio`
/// got all three. The row and the card's own projection are where that
/// shows, so they are what this reads.
#[test]
fn a_cli_shaped_sfx_command_plans_as_comfy_with_the_backend_s_budget() {
    let (dir, project) = common::project();
    let budget = forge_library::backends::Backends::discover(&project)
        .get("moss_sfx")
        .and_then(|backend| backend.vram_gb)
        .expect("this checkout describes moss_sfx and states its budget");
    let script = common::stub(
        dir.path(),
        "slow.py",
        r#"
import json, time
print("working", flush=True)
time.sleep(1.5)
print(json.dumps({"ok": True, "outputs": []}))
"#,
    );
    let queue = common::queue(&project, Some(&script));
    let argv: Vec<String> = "sfx --prompt a-heavy-iron-door --out out/audio/sfx/door.wav"
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let spec = forge_serve::JobSpec {
        // Stated so an exported FORGE_FAKE cannot turn this into the fake
        // path, which needs no card and no backend.
        fake: Some(false),
        ..forge_serve::spec::spec_for(&argv, &project.root, "cli")
    };
    assert_eq!(spec.backend.as_deref(), Some("moss_sfx"));
    let job = queue.submit(spec).expect("admitted");
    assert_eq!(
        job.executor,
        forge_serve::ExecutorKind::Comfy,
        "moss_sfx is a comfy backend, and the row says so whichever door queued it"
    );
    let state = common::until(queue.as_ref(), &job.id, "took the card", 20, |job| {
        job.state == JobState::Running
    });
    assert_eq!(state.state, JobState::Running);
    let card = forge_serve::CardState::read(&forge_serve::state_dir(&project.root))
        .expect("a running job's projection");
    assert_eq!(
        card.need_gb,
        Some(budget),
        "the lease is taken against the backend's budget, never against nothing"
    );
    let done = common::finished(queue.as_ref(), &job.id, 30);
    assert_eq!(done.state, JobState::Done, "{:?}", done.message);
}

/// A stop never leaves a child alive with the card lock dropped.
///
/// The daemon used to set a flag and exit: the generator went on running in
/// its own process group, `card.lock` was released with the card still
/// held, and the row was stamped `interrupted` on the next start although
/// nothing had interrupted it (2026-08-30).
#[test]
fn a_stop_with_a_running_job_does_not_release_the_card() {
    let (dir, project) = common::project();
    let script = common::stub(
        dir.path(),
        "long.py",
        r#"
import sys, time
print("working", flush=True)
time.sleep(300)
"#,
    );
    let queue = common::queue(&project, Some(&script));
    let job = queue
        .submit(forge_serve::JobSpec::new(
            "generate_audio.sfx",
            vec![String::from("sfx")],
            "cli",
        ))
        .expect("admitted");
    let running = common::until(queue.as_ref(), &job.id, "started", 20, |job| {
        job.state == JobState::Running && job.pid.is_some()
    });
    let pid = running.pid.expect("a running job has a pid");

    queue.stop();

    let row = queue.get(&job.id).expect("read").expect("the row");
    assert!(
        row.state.is_terminal(),
        "stop waits for the row it cancelled: {:?}",
        row.state
    );
    assert_eq!(row.state, JobState::Cancelled);
    assert!(row.exit.is_none(), "a cancelled job has no exit code");
    // The child is gone before the process that spawned it may exit.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "pid {pid} outlived the stop that was supposed to end it"
    );
    assert!(
        forge_serve::CardState::read(&forge_serve::state_dir(&project.root)).is_none(),
        "the lease went with the job, and the projection with the lease"
    );
}
