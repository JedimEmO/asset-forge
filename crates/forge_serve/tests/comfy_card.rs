//! The VRAM ladder, when the card does not come back.
//!
//! `hosting.md` records `POST /free` returning the card on both spike runs,
//! so this path is a safety net that must be **loud when it fires and never
//! routine**. What has to be proved is that the restart happens once and
//! that a card nobody can prove is free is withheld rather than handed to
//! the next job — a daemon that hands out a card it cannot prove is free
//! produces an OOM three jobs later with nothing naming the cause.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use forge_serve::{CardState, release_comfy_with};

/// A stub `ComfyUI` that answers `/system_stats` with a card that stays
/// full, and `/free` with a cheerful nothing.
fn stub_comfy(free_gb: f64) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0_u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let body = if request.starts_with("POST /free") {
                String::from("{}")
            } else {
                format!(
                    "{{\"devices\":[{{\"name\":\"stub\",\"vram_total\":25757220864,\
                     \"vram_free\":{}}}]}}",
                    (free_gb * 1_073_741_824.0) as u64
                )
            };
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn vram_that_does_not_return_restarts_the_unit_then_withholds() {
    // Half a gigabyte free where the job started with twenty-two: the host
    // is still holding a model.
    let url = stub_comfy(0.5);
    let restarts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&restarts);
    let mut restart = move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    };
    let mut said = Vec::new();
    let release = release_comfy_with(
        &url,
        "forge-comfy.service",
        &mut restart,
        Some(22.4),
        1,
        |line| said.push(line.to_owned()),
    );

    assert_eq!(
        restarts.load(Ordering::SeqCst),
        1,
        "the unit is restarted once, not in a loop"
    );
    assert!(release.restarted, "the row records that it happened");
    assert!(
        !release.returned,
        "the card did not come back, and the ladder must not pretend it did"
    );
    let note = release.note.expect("a withheld card says why");
    assert!(note.contains("forge-comfy.service"), "{note}");
    assert!(note.contains("forge gpu --free"), "{note}");
    let spoken = said.join("\n");
    assert!(
        spoken.contains("restarting"),
        "the log says it out loud when this fires:\n{spoken}"
    );

    // And what the worker does with that note: no card job starts until
    // something proves the card is free.
    let dir = tempfile::tempdir().expect("tempdir");
    forge_serve::withhold(dir.path(), &note).expect("withhold");
    assert!(
        CardState::withheld(dir.path()).is_some(),
        "the next card job sits blocked, naming comfy"
    );
    let projection = CardState::read(dir.path()).expect("card.json");
    assert_eq!(projection.holder, "foreign");
    forge_serve::release_withhold(dir.path());
    assert!(CardState::withheld(dir.path()).is_none());
}

/// The one the old test could not see: nothing this job left, and 9 GB of
/// somebody else's model still on the card.
///
/// 15 GB free before and 15 GB free after, on a 24 GB card. Judged against
/// the job's own `before` this is "the card came back" — the two numbers
/// are the same — and that is exactly what happened on 2026-08-30: an MCP
/// speech job went 16.44 → 16.38 GB and was released clean with 7.3 GB of
/// MOSS resident, and `forge gpu --free` printed "the card is back" one
/// line above "holding pid 693788 8.1 GB". A test that compares a number
/// against itself passes for the wrong reason, so the ladder is judged
/// against the card's idle floor.
#[test]
fn a_model_an_earlier_job_left_is_seen_and_the_card_is_withheld() {
    let url = stub_comfy(15.0);
    let restarts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&restarts);
    let mut restart = move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    };
    let release = release_comfy_with(
        &url,
        "forge-comfy.service",
        &mut restart,
        Some(15.0),
        1,
        |_| {},
    );
    assert_eq!(
        restarts.load(Ordering::SeqCst),
        1,
        "the ladder tries the one lever that works on the MOSS pack"
    );
    assert!(
        !release.returned,
        "15 GB free on a 24 GB card is not a card that came back, whatever this job started with"
    );
    let floor = release.floor_gb.expect("the stub says how big the card is");
    assert!(
        (21.0..23.0).contains(&floor),
        "the floor is the card less its idle context, not a constant: {floor}"
    );
    let note = release.note.expect("a withheld card says why");
    assert!(
        note.contains("15.0"),
        "the note quotes what it measured: {note}"
    );
}

/// A host that does not answer is not evidence that the card is held.
///
/// Withholding on an unreachable service would block every card job — an
/// `env` lift that never touches the host included — until a human ran
/// `forge gpu --free`, which cannot answer either while the service is
/// down. Nothing is measured, nothing is claimed, nothing is restarted.
#[test]
fn a_host_that_does_not_answer_withholds_nothing() {
    let restarts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&restarts);
    let mut restart = move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    };
    let mut said = Vec::new();
    let release = release_comfy_with(
        // A port nothing is listening on.
        "http://127.0.0.1:9",
        "forge-comfy.service",
        &mut restart,
        Some(22.4),
        1,
        |line| said.push(line.to_owned()),
    );
    assert_eq!(restarts.load(Ordering::SeqCst), 0, "nothing to restart");
    assert!(release.after_gb.is_none(), "null means unknown");
    assert!(release.floor_gb.is_none());
    assert!(release.note.is_none(), "no note means no withholding");
    assert!(release.returned, "and no claim that the card is held");
    assert!(
        said.join("\n").contains("did not answer"),
        "it says what happened: {said:?}"
    );
}

/// The happy path: `/free` gives the card back, and nothing is restarted.
/// This is what both spike runs measured, and it is the one that must not
/// become noisy.
#[test]
fn a_card_that_comes_back_restarts_nothing() {
    let url = stub_comfy(22.3);
    let restarts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&restarts);
    let mut restart = move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    };
    let release = release_comfy_with(
        &url,
        "forge-comfy.service",
        &mut restart,
        Some(22.4),
        1,
        |_| {},
    );
    assert!(release.returned, "22.3 GB back out of 22.4 is back");
    assert!(!release.restarted);
    assert_eq!(restarts.load(Ordering::SeqCst), 0);
    assert!(release.note.is_none());
}
