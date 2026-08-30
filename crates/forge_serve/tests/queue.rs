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
        backend: None,
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
        backend: None,
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
            fake: None,
        })
        .expect("a refusal is a row, not an error");
    assert_eq!(job.state, JobState::Refused);
    assert_eq!(job.exit, Some(3));
    let message = job.message.unwrap_or_default();
    assert!(message.contains("moss_sfx"), "{message}");
    assert!(job.hint.unwrap_or_default().contains("doctor"));
}
