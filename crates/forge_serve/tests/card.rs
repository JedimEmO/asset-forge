//! The card lease, across process boundaries — which is the only place it
//! matters. A worker being singular holds nothing against a second
//! terminal; a `flock(2)` does.

mod common;

use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use forge_serve::{CardLease, CardState, JobSpec, JobState, Queue};

/// A second process holding `card.lock` with a real `flock`, as another
/// `forge gen` in another terminal would.
fn holder(state_dir: &Path) -> Child {
    let lock = state_dir.join("card.lock");
    std::fs::create_dir_all(state_dir).expect("mkdir");
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(
            "import fcntl, sys, time\n\
             f = open(sys.argv[1], 'a+')\n\
             fcntl.flock(f, fcntl.LOCK_EX)\n\
             print('held', flush=True)\n\
             time.sleep(120)\n",
        )
        .arg(&lock)
        .stdout(Stdio::piped())
        .spawn()
        .expect("the holder starts");
    // Wait for it to say it has the lock, so the test is not racing it.
    let mut line = String::new();
    let stdout = child.stdout.take().expect("stdout");
    let _ = BufReader::new(stdout).read_line(&mut line);
    assert!(line.starts_with("held"), "the holder never took the lock");
    child
}

#[test]
fn card_lease_is_exclusive_across_processes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("out/serve");
    let mut held = holder(&state);

    let refused = CardLease::try_acquire(&state, "j-mine", Some(8.0), Some("moss_sfx sfx"))
        .expect("the lock file opens");
    assert!(
        refused.is_none(),
        "another process holds the card; this one must not also have it"
    );

    held.kill().expect("kill");
    let _ = held.wait();
    let mine = CardLease::try_acquire(&state, "j-mine", Some(8.0), Some("moss_sfx sfx"))
        .expect("the lock file opens")
        .expect("the card is free once the holder is gone");
    drop(mine);
}

/// The kernel drops a `flock` when the holder dies, which is the whole
/// reason this is not a pidfile: a SIGKILL, an OOM kill or a closed lid
/// cannot leave the card claimed by a process that is not there.
#[test]
fn a_killed_holder_releases_the_card() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("out/serve");
    let mut held = holder(&state);
    // A stale projection from the dead holder, as a crash would leave.
    forge_serve::withhold(&state, "pretend this was written by the dead holder").expect("write");
    forge_serve::release_withhold(&state);
    std::fs::write(
        state.join("card.json"),
        r#"{"holder":"j-dead","pid":424242,"since":"2026-08-30T00:00:00Z","need_gb":8.0,"what":"moss_sfx sfx","note":null}"#,
    )
    .expect("write a stale projection");

    let killed = held.id();
    held.kill().expect("kill");
    let _ = held.wait();

    let deadline = Instant::now() + Duration::from_secs(5);
    let lease = loop {
        if let Some(lease) = CardLease::try_acquire(&state, "j-next", None, Some("ardy sweep"))
            .expect("the lock file opens")
        {
            break lease;
        }
        assert!(
            Instant::now() < deadline,
            "the kernel did not release the lock of pid {killed}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let projection = CardState::read(&state).expect("card.json");
    assert_eq!(
        projection.holder, "j-next",
        "the next acquirer overwrites the stale sidecar"
    );
    drop(lease);
}

/// Free VRAM under the backend's budget with a foreign pid holding the
/// difference is a wait with a name, never an OOM two minutes in.
#[test]
fn a_foreign_holder_blocks_rather_than_ooms() {
    let (dir, project) = common::project();
    // A backend that declares a budget and resolves an interpreter, so the
    // queue can plan a job through it without any of it being installed.
    let backends = dir.path().join("backends");
    let ardy = backends.join("ardy");
    std::fs::create_dir_all(&ardy).expect("mkdir");
    std::fs::write(
        ardy.join("backend.toml"),
        "name = \"ardy\"\nentry = \"motion\"\nvram_gb = 16\n",
    )
    .expect("backend.toml");
    let venv = dir.path().join("venv/bin");
    std::fs::create_dir_all(&venv).expect("mkdir");
    std::fs::write(venv.join("python"), "#!/bin/sh\n").expect("python");
    std::os::unix::fs::symlink(dir.path().join("venv"), ardy.join(".env")).expect("link");
    let mut project = project;
    project.backends_dir = Some(backends);

    // A card reader that says the card is nearly full and names who has it.
    let gpu = common::stub(
        dir.path(),
        "gpu.sh",
        "#!/bin/sh\n\
         echo '{\"ok\":false,\"name\":\"NVIDIA GeForce RTX 4090\",\"total_mb\":24564,\
         \"used_mb\":22000,\"free_mb\":2564,\"apps\":[{\"pid\":4123,\"name\":\"forge studio\",\
         \"used_mb\":8300,\"backend\":null}]}'\n",
    );
    std::fs::set_permissions(&gpu, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");

    let queue = forge_serve::LocalQueue::open(
        &project,
        forge_serve::LocalQueueOptions {
            forge: gpu.clone(),
            ..common::options(None)
        },
    )
    .expect("queue");

    let job = queue
        .submit(JobSpec {
            kind: String::from("generate_clips.sweep"),
            backend: Some(String::from("ardy")),
            argv: vec![String::from("motion"), String::from("sweep")],
            outputs_claimed: Vec::new(),
            record: None,
            created_by: String::from("agent:test"),
        })
        .expect("admitted");
    let blocked = common::until(queue.as_ref(), &job.id, "blocked", 15, |job| {
        job.state == JobState::Blocked
    });
    let who = blocked.blocked_by.unwrap_or_default();
    assert!(who.contains("pid 4123"), "{who}");
    assert!(who.contains("forge studio"), "{who}");
    assert!(who.contains("8.1 GB"), "the message says how much: {who}");
    assert_eq!(blocked.exit, None, "a blocked row has no exit code");

    let cancelled = queue.cancel(&job.id).expect("cancel");
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert_eq!(cancelled.exit, None);
    queue.stop();
}
