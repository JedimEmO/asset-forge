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
