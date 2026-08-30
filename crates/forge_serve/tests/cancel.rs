//! ^C on a followed job cancels it, and a cancel means the tree is gone and
//! the card is back.
//!
//! A human who interrupts a 111-second image expects the card back; leaving
//! it held by a job the user believes they stopped is a silent divergence.

mod common;

use std::time::{Duration, Instant};

use forge_serve::{CardLease, CardState, JobSpec, JobState, Queue};

#[test]
fn cancel_kills_the_process_group_and_frees_the_card() {
    let (dir, project) = common::project();
    let marker = dir.path().join("grandchild.pid");
    let script = common::stub(
        dir.path(),
        "long_gen.py",
        &format!(
            r#"
import os, subprocess, sys, time
# A generator with a child of its own — a Blender step, a server call —
# so a cancel that only killed the direct child would leave this behind.
child = subprocess.Popen(["sleep", "300"])
open({marker:?}, "w").write(str(child.pid))
print("working", flush=True)
time.sleep(300)
"#,
            marker = marker.display().to_string()
        ),
    );
    let queue = common::queue(&project, Some(&script));
    let job = queue
        .submit(JobSpec {
            kind: String::from("generate_audio.music"),
            backend: Some(String::from("acestep")),
            argv: vec![String::from("music")],
            outputs_claimed: vec![String::from("out/audio/music/theme.ogg")],
            record: None,
            created_by: String::from("human"),
            fake: None,
        })
        .expect("admitted");
    let running = common::until(queue.as_ref(), &job.id, "started", 20, |job| {
        job.state == JobState::Running && job.pid.is_some()
    });
    assert!(
        CardState::read(&forge_serve::state_dir(&project.root)).is_some(),
        "a running job's projection says who holds the card"
    );

    // The grandchild exists before the cancel, so its death afterwards is
    // the process group being signalled and not a race.
    let grandchild: u32 = common::until(queue.as_ref(), &job.id, "wrote its pid", 20, |_| {
        marker.is_file()
    })
    .pid
    .map(|_| {
        std::fs::read_to_string(&marker)
            .expect("pid")
            .trim()
            .parse()
            .expect("a pid")
    })
    .expect("the row has a pid");
    assert!(alive(grandchild), "the grandchild should be running");

    let cancelled = queue.cancel(&job.id).expect("cancel");
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert_eq!(cancelled.exit, None, "a cancelled job has no exit code");
    let note = cancelled.message.unwrap_or_default();
    assert!(note.contains("SIGTERM"), "{note}");
    assert!(
        note.contains("nothing under assets/ was touched"),
        "the note says what was and was not left behind: {note}"
    );
    assert_eq!(
        running.pid, cancelled.pid,
        "the kill is by the recorded pid, never by a pattern"
    );

    let deadline = Instant::now() + Duration::from_secs(15);
    while alive(grandchild) {
        assert!(
            Instant::now() < deadline,
            "the grandchild survived the cancel: the group was not signalled"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    let state = forge_serve::state_dir(&project.root);
    let free = CardLease::try_acquire(&state, "j-after", None, None)
        .expect("the lock file opens")
        .expect("the card is free again");
    drop(free);
    queue.stop();
}

/// Whether a pid still names a process, without reaping anything.
fn alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
