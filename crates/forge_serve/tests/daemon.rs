//! One daemon per project, and a stale endpoint file that names a stranger
//! is not a daemon.

mod common;

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};

use forge_serve::{Daemon, JobStore, daemon};

#[test]
fn a_second_daemon_refuses_and_names_the_first() {
    let (_dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    // What a daemon writes when it comes up: the endpoint file, then the
    // lock held for as long as it lives. Here the "first daemon" is another
    // process holding the same flock, which is the only way this can be
    // tested honestly.
    let lock = store.daemon_lock_path();
    let mut first = Command::new("python3")
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
        .expect("the first daemon starts");
    let mut line = String::new();
    let stdout = first.stdout.take().expect("stdout");
    let _ = BufReader::new(stdout).read_line(&mut line);
    assert!(line.starts_with("held"));

    store
        .write_daemon(&Daemon {
            forge_serve: 1,
            pid: first.id(),
            start_ticks: 918_273,
            port: 41773,
            token: daemon::token(),
            url: String::from("http://127.0.0.1:41773"),
            version: String::from("0.1.0"),
            project: project.root.display().to_string(),
            started: forge_library::clock::now_iso(),
        })
        .expect("write daemon.json");

    let refusal = daemon::DaemonLock::take(&store).expect_err("a second daemon is refused");
    let text = refusal.to_string();
    assert!(
        text.contains(&format!("pid {}", first.id())),
        "the refusal names the first: {text}"
    );
    assert!(text.contains("http://127.0.0.1:41773"), "{text}");
    assert!(text.contains("forge stop"), "it says how to end it: {text}");

    first.kill().expect("kill");
    let _ = first.wait();
}

/// `daemon.json` is mode 0600, because it carries a bearer token and the
/// thing behind that port drives a GPU.
#[test]
fn the_endpoint_file_is_not_world_readable() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    store
        .write_daemon(&Daemon {
            forge_serve: 1,
            pid: std::process::id(),
            start_ticks: 1,
            port: 1,
            token: daemon::token(),
            url: String::from("http://127.0.0.1:1"),
            version: String::from("0.1.0"),
            project: project.root.display().to_string(),
            started: forge_library::clock::now_iso(),
        })
        .expect("write");
    let mode = std::fs::metadata(store.daemon_path())
        .expect("stat")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "daemon.json is {mode:o}");
}

/// A pid that is alive but was born at another moment is a stranger who
/// inherited the number, and discovery must not hand it a job.
#[test]
fn a_daemon_file_whose_start_time_disagrees_is_dropped() {
    let (_dir, project) = common::project();
    let store = JobStore::open(&project.root).expect("store");
    store
        .write_daemon(&Daemon {
            forge_serve: 1,
            // This process is certainly alive, and certainly did not start
            // at tick 1.
            pid: std::process::id(),
            start_ticks: 1,
            port: 41773,
            token: String::from("t0ken"),
            url: String::from("http://127.0.0.1:41773"),
            version: String::from("0.1.0"),
            project: project.root.display().to_string(),
            started: forge_library::clock::now_iso(),
        })
        .expect("write");
    assert!(
        forge_serve::discovery::find(&project).is_none(),
        "a start time that disagrees is a stranger"
    );
    assert!(
        store.daemon().is_none(),
        "and the stale file is removed rather than left to mislead the next call"
    );
}
