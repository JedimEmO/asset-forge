//! The daemon's door, driven the way a client drives it: submit, poll,
//! read the log, cancel — against a stub generator, with no socket and no
//! runtime beyond the test's own.

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use forge_serve::http::{ServeState, router};
use forge_serve::{JobState, LocalQueue};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const TOKEN: &str = "t0ken-for-tests";

/// A router over a queue whose generator is a stub.
fn app(project: &forge_library::Project, script: &std::path::Path) -> axum::Router {
    let queue = LocalQueue::open(project, common::options(Some(script))).expect("queue");
    router(Arc::new(ServeState {
        queue,
        token: String::from(TOKEN),
        started: forge_library::clock::now_iso(),
        version: String::from("0.1.0"),
    }))
}

/// One request, with the two things every route insists on.
async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Vec<(String, String)>, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:41773")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json");
    let request = match body {
        Some(value) => request.body(Body::from(value.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn a_job_goes_in_comes_back_and_can_be_read_and_cancelled() {
    let (dir, project) = common::project();
    let script = common::stub(
        dir.path(),
        "http_gen.py",
        r#"
import json, os, sys, time
print("[sfx] loading the model")
sys.stdout.flush()
time.sleep(0.3)
print(json.dumps({"ok": True, "outputs": ["out/audio/sfx/door.wav"],
                  "record": "out/audio/sfx/door.json", "seed": 815273}))
"#,
    );
    let app = app(&project, &script);

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/jobs",
        Some(json!({
            "kind": "generate_audio.sfx",
            "backend": null,
            "argv": ["sfx", "--prompt", "a heavy iron door"],
            "outputs_claimed": ["out/audio/sfx/door.wav"],
            "record": "out/audio/sfx/door.json",
            "created_by": "agent:claude"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let job: Value = serde_json::from_str(&body).expect("the row");
    let id = job["id"].as_str().expect("an id").to_owned();
    assert_eq!(job["forge_job"], 1);
    assert_eq!(job["state"], "queued");
    assert!(job["exit"].is_null(), "a queued row has no exit code");

    // The same claim again, from the other door: refused, naming the row.
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/jobs",
        Some(json!({
            "kind": "generate_audio.sfx",
            "argv": ["sfx"],
            "outputs_claimed": ["out/audio/sfx/door.wav"],
            "created_by": "human"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains(&id), "{body}");

    // Poll until it is done, the way a client with no socket does.
    let mut row = Value::Null;
    for _ in 0..200 {
        let (status, _, body) = call(&app, "GET", &format!("/v1/jobs/{id}"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        row = serde_json::from_str(&body).expect("the row");
        if row["state"] == "done" || row["state"] == "failed" || row["state"] == "refused" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(row["state"], "done", "{row}");
    assert_eq!(row["exit"], 0);
    assert_eq!(row["record"], "out/audio/sfx/door.json");
    assert_eq!(
        row["payload"]["seed"], 815_273,
        "the child's own object survives"
    );

    // The log, by byte offset, with the header that says where to ask next.
    let (status, headers, text) =
        call(&app, "GET", &format!("/v1/jobs/{id}/log?from=0"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("loading the model"), "{text}");
    assert!(
        !text.contains("\"ok\": true") && !text.contains("\"ok\":true"),
        "the last line is the payload and is held back from the log: {text}"
    );
    let next: u64 = headers
        .iter()
        .find(|(name, _)| name == "x-forge-log-next")
        .map(|(_, value)| value.parse().expect("a number"))
        .expect("the next-offset header");
    assert_eq!(next as usize, text.len());
    let (_, _, more) = call(&app, "GET", &format!("/v1/jobs/{id}/log?from={next}"), None).await;
    assert!(more.is_empty(), "there is nothing after the end: {more:?}");

    // Cancelling something that is over is a refusal naming its state.
    let (status, _, body) = call(&app, "POST", &format!("/v1/jobs/{id}/cancel"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("done"), "{body}");

    // A job nobody has is a 404 that lists the ones that do exist.
    let (status, _, body) = call(&app, "GET", "/v1/jobs/j-does-not-exist", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body.contains(&id),
        "the refusal names what does exist: {body}"
    );

    // And the listing and the status object answer.
    let (status, _, body) = call(&app, "GET", "/v1/jobs?limit=10", None).await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<Value> = serde_json::from_str(&body).expect("rows");
    assert_eq!(
        rows.len(),
        1,
        "the refused submit left no row behind: {body}"
    );

    let (status, _, body) = call(&app, "GET", "/v1/status", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let status_object: Value = serde_json::from_str(&body).expect("status");
    assert_eq!(status_object["queue"]["running"], 0);
    assert_eq!(status_object["project"], project.root.display().to_string());

    let (status, _, body) = call(&app, "GET", "/v1/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<Value>(&body).expect("health")["forge_serve"],
        1
    );
}

#[tokio::test]
async fn a_running_job_is_cancelled_through_the_door() {
    let (dir, project) = common::project();
    let script = common::stub(
        dir.path(),
        "slow_gen.py",
        "import time\nprint('working', flush=True)\ntime.sleep(120)\n",
    );
    let app = app(&project, &script);
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/jobs",
        Some(json!({
            "kind": "generate_audio.music",
            "argv": ["music"],
            "outputs_claimed": [],
            "created_by": "human"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = serde_json::from_str::<Value>(&body).expect("row")["id"]
        .as_str()
        .expect("id")
        .to_owned();

    for _ in 0..200 {
        let (_, _, body) = call(&app, "GET", &format!("/v1/jobs/{id}"), None).await;
        if serde_json::from_str::<Value>(&body).expect("row")["state"] == "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (status, _, body) = call(&app, "POST", &format!("/v1/jobs/{id}/cancel"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let answer: Value = serde_json::from_str(&body).expect("the cancel frame");
    assert_eq!(answer["cancelled"], true);
    assert_eq!(answer["was"], "running");
    assert_eq!(answer["job"]["state"], JobState::Cancelled.as_str());
    assert!(answer["job"]["exit"].is_null());
    assert!(
        answer["note"]
            .as_str()
            .unwrap_or_default()
            .contains("nothing under assets/"),
        "{body}"
    );
}
