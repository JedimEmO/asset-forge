//! Recovery and GPU exclusion across independent game projects.
mod common;
use forge_serve::{JobSpec, JobState, LocalQueue, LocalQueueOptions, Queue};

#[test]
fn production_options_share_one_card_even_for_different_projects() {
    let (_a, a) = common::project();
    let (_b, b) = common::project();
    let first = LocalQueueOptions::for_project(&a, "forge".into());
    let second = LocalQueueOptions::for_daemon(&b, "forge".into());
    assert_eq!(first.card_state_dir, second.card_state_dir);
    assert_eq!(first.card_state_dir, Some(forge_serve::shared_card_dir()));
    assert_ne!(first.card_state_dir, Some(forge_serve::state_dir(&a.root)));
}

#[test]
fn cancelling_one_game_releases_the_gpu_for_another_without_sharing_outputs() {
    let (a_dir, a) = common::project();
    let (b_dir, b) = common::project();
    let shared = tempfile::tempdir().expect("shared card state");
    let stub = common::stub(
        a_dir.path(),
        "generator.py",
        r"
import json, pathlib, sys, time
path = pathlib.Path('out/same.wav')
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(sys.argv[1])
time.sleep(float(sys.argv[2]))
print(json.dumps({'ok': True, 'fake': True, 'outputs': []}))
",
    );
    let options = || {
        let mut options = common::options(Some(&stub));
        options.card_state_dir = Some(shared.path().to_path_buf());
        options.tier = String::from("fake");
        options
    };
    let first = LocalQueue::open(&a, options()).expect("first queue");
    let second = LocalQueue::open(&b, options()).expect("second queue");
    let spec = |label: &str, seconds: &str| JobSpec {
        kind: String::from("generate_audio.sfx"),
        backend: Some(String::from("moss_sfx")),
        argv: vec![label.to_owned(), seconds.to_owned()],
        outputs_claimed: vec![String::from("out/same.wav")],
        record: None,
        created_by: String::from("test"),
        fake: Some(true),
    };
    let running = first
        .submit(spec("first-partial", "30"))
        .expect("first admitted");
    common::until(
        first.as_ref(),
        &running.id,
        "wrote partial output",
        20,
        |_| a_dir.path().join("out/same.wav").is_file(),
    );
    let waiting = second
        .submit(spec("second-complete", "0"))
        .expect("same relative path in another game is independent");
    let blocked = common::until(
        second.as_ref(),
        &waiting.id,
        "blocked on shared GPU",
        20,
        |job| job.state == JobState::Blocked,
    );
    assert!(
        blocked
            .blocked_by
            .unwrap_or_default()
            .contains(running.id.as_str())
    );
    assert!(!b_dir.path().join("out/same.wav").exists());
    assert_eq!(
        first.cancel(&running.id).expect("cancel").state,
        JobState::Cancelled
    );
    assert_eq!(
        common::finished(second.as_ref(), &waiting.id, 20).state,
        JobState::Done
    );
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("out/same.wav")).expect("partial retained"),
        "first-partial"
    );
    assert_eq!(
        std::fs::read_to_string(b_dir.path().join("out/same.wav")).expect("second output"),
        "second-complete"
    );
    assert!(first.get(&waiting.id).expect("first store").is_none());
    assert!(second.get(&running.id).expect("second store").is_none());
    first.stop();
    second.stop();
}

#[test]
fn a_native_generator_crash_releases_the_card_for_another_project() {
    let (a_dir, a) = common::project();
    let (b_dir, b) = common::project();
    let shared = tempfile::tempdir().expect("shared card state");
    let stub = common::stub(
        a_dir.path(),
        "native_crash.py",
        r"
import json, os, pathlib, resource, signal, sys
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
path = pathlib.Path('out/model.glb')
path.parent.mkdir(parents=True, exist_ok=True)
if sys.argv[1] == 'crash':
    path.write_text('partial, never a successful asset')
    print('native import failed before a record was written', flush=True)
    os.kill(os.getpid(), signal.SIGSEGV)
path.write_text('next project completed')
print(json.dumps({'ok': True, 'fake': True, 'outputs': ['out/model.glb']}))
",
    );
    let options = || {
        let mut options = common::options(Some(&stub));
        options.card_state_dir = Some(shared.path().to_path_buf());
        options.tier = String::from("fake");
        options
    };
    let first = LocalQueue::open(&a, options()).expect("first queue");
    let second = LocalQueue::open(&b, options()).expect("second queue");
    let spec = |mode: &str| JobSpec {
        kind: String::from("generate_mesh.lift"),
        backend: Some(String::from("trellis2")),
        argv: vec![mode.to_owned()],
        outputs_claimed: vec![String::from("out/model.glb")],
        record: Some(String::from("out/model.json")),
        created_by: String::from("test"),
        fake: Some(true),
    };
    let crashed = first.submit(spec("crash")).expect("admitted");
    let failed = common::finished(first.as_ref(), &crashed.id, 20);
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.exit, None);
    assert!(
        failed
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("signal 11")
    );
    assert!(!a_dir.path().join("out/model.json").exists());
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("out/model.glb")).expect("diagnostic retained"),
        "partial, never a successful asset"
    );
    let next = second
        .submit(spec("complete"))
        .expect("next project admitted");
    assert_eq!(
        common::finished(second.as_ref(), &next.id, 20).state,
        JobState::Done
    );
    assert_eq!(
        std::fs::read_to_string(b_dir.path().join("out/model.glb")).expect("independent output"),
        "next project completed"
    );
    assert_eq!(
        first
            .get(&crashed.id)
            .expect("failed row retained")
            .expect("row")
            .state,
        JobState::Failed
    );
    first.stop();
    second.stop();
}
