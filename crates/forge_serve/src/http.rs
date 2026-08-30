//! The daemon's door: axum on `127.0.0.1`, an ephemeral port, one listener.
//!
//! A unix socket beside it would mean two auth stories and two client paths
//! for one loopback queue, and the MCP-over-HTTP client needs the port
//! anyway. Every route requires `Authorization: Bearer <token>` from
//! `daemon.json` **and** a `Host` of `127.0.0.1`/`localhost`: loopback alone
//! is not access control on a shared box, and the thing behind this port
//! drives a GPU.
//!
//! | method | route | body / result |
//! |---|---|---|
//! | `GET` | `/v1/health` | `{"ok":true,"forge_serve":1,"version":…,"project":…,"pid":…,"started":…,"uptime_s":…}` |
//! | `GET` | `/v1/status` | the status object |
//! | `POST` | `/v1/jobs` | a [`JobSpec`] → `201` with the job row |
//! | `GET` | `/v1/jobs?state=&kind=&limit=` | rows, newest first |
//! | `GET` | `/v1/jobs/{id}` | the row; 404 is a refusal naming the ids that exist |
//! | `GET` | `/v1/jobs/{id}/log?from=<byte>` | `text/plain`, header `X-Forge-Log-Next` |
//! | `POST` | `/v1/jobs/{id}/cancel` | `{"cancelled":true,"was":"running","note":…}` |
//! | `GET` | `/v1/runs?kind=&since=&limit=` | the `out/` walk |
//! | `GET` | `/v1/backends` | the backend table, the cheap half |
//! | `POST` | `/v1/stop` | what `forge stop` calls |
//!
//! `/mcp` is **not** here: the binary nests `StreamableHttpService` onto
//! this router, because this crate must not depend on rmcp.
//!
//! # Two shapes that differ from the sketch, and why
//!
//! `POST /v1/jobs` takes a [`JobSpec`] — `argv` and `outputs_claimed` — and
//! not an `args {}` map. Turning a map into a generator's flags would put a
//! generator's flag table inside the daemon, and the one thing this daemon
//! is not allowed to know is what a generator's arguments mean.
//!
//! `?follow=1` on the log route long-polls for up to a second rather than
//! opening an event stream: the CLI follows a job by asking again with the
//! offset the last answer gave it, which is one code path for "read the log"
//! and "watch the log", and the byte offset is the whole protocol.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use crate::job::{JobFilter, JobId, JobSpec, JobState, RunFilter};
use crate::queue::LocalQueue;
use crate::{Queue, ServeError};

/// How long a `follow=1` request waits for new bytes before answering with
/// none.
const FOLLOW_WAIT: Duration = Duration::from_secs(1);

/// What every handler shares.
#[derive(Debug)]
pub struct ServeState {
    /// The queue this daemon owns.
    pub queue: Arc<LocalQueue>,
    /// The bearer token from `daemon.json`.
    pub token: String,
    /// When the daemon started.
    pub started: String,
    /// Its build version.
    pub version: String,
}

impl ServeState {
    /// Seconds since the daemon started.
    fn uptime_s(&self) -> u64 {
        forge_library::clock::parse_stamp(&self.started).map_or(0, |then| {
            forge_library::clock::unix_seconds().saturating_sub(then)
        })
    }
}

/// The daemon's routes, without `/mcp`.
pub fn router(state: Arc<ServeState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/jobs", get(list_jobs).post(submit_job))
        .route("/v1/jobs/{id}", get(get_job))
        .route("/v1/jobs/{id}/log", get(job_log))
        .route("/v1/jobs/{id}/cancel", post(cancel_job))
        .route("/v1/runs", get(list_runs))
        .route("/v1/backends", get(backends))
        .route("/v1/stop", post(stop))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            guard,
        ))
        .with_state(state)
}

/// The bearer token and the loopback `Host`, on every route.
async fn guard(State(state): State<Arc<ServeState>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !host_is_loopback(headers) {
        return refusal(
            StatusCode::FORBIDDEN,
            "this daemon answers only to 127.0.0.1 and localhost",
        );
    }
    let offered = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default()
        .trim();
    if offered.is_empty() || offered != state.token {
        return refusal(
            StatusCode::UNAUTHORIZED,
            "every route needs the bearer token from out/serve/daemon.json",
        );
    }
    next.run(request).await
}

/// Whether the `Host` header names this machine's loopback.
fn host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        // HTTP/1.1 requires one; a request without it is not one this
        // daemon has to guess about.
        return false;
    };
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// An error body that reads like every other refusal here.
fn refusal(code: StatusCode, message: &str) -> Response {
    (code, axum::Json(json!({"ok": false, "message": message}))).into_response()
}

/// A [`ServeError`] as a status and a body.
fn from_error(err: &ServeError) -> Response {
    let code = match err {
        ServeError::NoSuchJob { .. } => StatusCode::NOT_FOUND,
        ServeError::Refused(_) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    refusal(code, &err.to_string())
}

async fn health(State(state): State<Arc<ServeState>>) -> Response {
    axum::Json(json!({
        "ok": true,
        "forge_serve": 1,
        "version": state.version,
        "project": state.queue.project().root.display().to_string(),
        "pid": std::process::id(),
        "started": state.started,
        "uptime_s": state.uptime_s(),
    }))
    .into_response()
}

async fn status(State(state): State<Arc<ServeState>>) -> Response {
    match state.queue.status() {
        Ok(status) => axum::Json(status).into_response(),
        Err(err) => from_error(&err),
    }
}

/// `?state=&kind=&limit=` on the job listing.
#[derive(Debug, Deserialize)]
struct JobQuery {
    state: Option<String>,
    kind: Option<String>,
    limit: Option<usize>,
}

async fn list_jobs(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<JobQuery>,
) -> Response {
    let filter = JobFilter {
        state: query.state.as_deref().and_then(JobState::parse),
        kind: query.kind,
        limit: query.limit,
    };
    match state.queue.list(&filter) {
        Ok(jobs) => axum::Json(jobs).into_response(),
        Err(err) => from_error(&err),
    }
}

async fn submit_job(
    State(state): State<Arc<ServeState>>,
    axum::Json(spec): axum::Json<JobSpec>,
) -> Response {
    match state.queue.submit(spec) {
        Ok(job) => (StatusCode::CREATED, axum::Json(job)).into_response(),
        Err(err) => from_error(&err),
    }
}

async fn get_job(State(state): State<Arc<ServeState>>, Path(id): Path<String>) -> Response {
    let id = JobId::from(id);
    match state.queue.get(&id) {
        Ok(Some(job)) => axum::Json(job).into_response(),
        Ok(None) => from_error(&state.queue.store().no_such_job(&id)),
        Err(err) => from_error(&err),
    }
}

/// `?from=<byte>&follow=1` on the log route.
#[derive(Debug, Deserialize)]
struct LogQuery {
    from: Option<u64>,
    follow: Option<u8>,
}

async fn job_log(
    State(state): State<Arc<ServeState>>,
    Path(id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Response {
    let id = JobId::from(id);
    let from = query.from.unwrap_or(0);
    let follow = query.follow.unwrap_or(0) == 1;
    let deadline = std::time::Instant::now() + FOLLOW_WAIT;
    loop {
        match state.queue.log(&id, from) {
            Ok(chunk) => {
                if follow
                    && chunk.text.is_empty()
                    && !chunk.done
                    && std::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                let mut headers = HeaderMap::new();
                headers.insert(
                    "x-forge-log-next",
                    chunk
                        .next
                        .to_string()
                        .parse()
                        .unwrap_or(header::HeaderValue::from_static("0")),
                );
                headers.insert(
                    "x-forge-log-done",
                    header::HeaderValue::from_static("").clone(),
                );
                if let Ok(value) = chunk.done.to_string().parse() {
                    headers.insert("x-forge-log-done", value);
                }
                headers.insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/plain; charset=utf-8"),
                );
                return (headers, chunk.text).into_response();
            }
            Err(err) => return from_error(&err),
        }
    }
}

async fn cancel_job(State(state): State<Arc<ServeState>>, Path(id): Path<String>) -> Response {
    let id = JobId::from(id);
    let queue = Arc::clone(&state.queue);
    let was = queue.get(&id).ok().flatten().map(|job| job.state);
    // A cancel waits for the worker to write the terminal row, so it does
    // not belong on the runtime's thread.
    let cancelled = tokio::task::spawn_blocking(move || queue.cancel(&id)).await;
    match cancelled {
        Ok(Ok(job)) => axum::Json(json!({
            "cancelled": true,
            "was": was.map(JobState::as_str),
            "job": job,
            "note": job_cancel_note(&job),
        }))
        .into_response(),
        Ok(Err(err)) => from_error(&err),
        Err(err) => refusal(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the cancel task failed: {err}"),
        ),
    }
}

/// The sentence a cancel answers with.
fn job_cancel_note(job: &crate::job::Job) -> String {
    job.message.clone().unwrap_or_else(|| {
        String::from("partial outputs under out/ were left, nothing under assets/ was touched")
    })
}

/// `?kind=&since=&limit=` on the runs walk.
#[derive(Debug, Deserialize)]
struct RunQuery {
    kind: Option<String>,
    since: Option<String>,
    limit: Option<usize>,
}

async fn list_runs(
    State(state): State<Arc<ServeState>>,
    Query(query): Query<RunQuery>,
) -> Response {
    let filter = RunFilter {
        kind: query.kind,
        since: query.since,
        limit: query.limit,
    };
    match state.queue.runs(&filter) {
        Ok(runs) => axum::Json(runs).into_response(),
        Err(err) => from_error(&err),
    }
}

async fn backends(State(state): State<Arc<ServeState>>) -> Response {
    let found = forge_library::backends::Backends::discover(state.queue.project());
    let rows: Vec<_> = found
        .backends
        .iter()
        .map(|backend| {
            json!({
                "name": backend.name,
                "state": backend.state.as_str(),
                "role": backend.role,
                "vram_gb": backend.vram_gb,
                "entry": backend.entry,
            })
        })
        .collect();
    axum::Json(json!({"origin": found.origin, "backends": rows})).into_response()
}

async fn stop(State(state): State<Arc<ServeState>>) -> Response {
    state.queue.stop();
    // The process exits after the answer is on the wire; a caller that got
    // a connection reset instead of a body would have to guess.
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        std::process::exit(0);
    });
    axum::Json(json!({"ok": true, "stopping": true})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt as _;

    fn project() -> (tempfile::TempDir, forge_library::Project) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = forge_library::Project::init(dir.path(), "serve_test").expect("init");
        (dir, project)
    }

    fn state() -> (tempfile::TempDir, Arc<ServeState>) {
        let (dir, project) = project();
        let queue = LocalQueue::open(
            &project,
            crate::LocalQueueOptions {
                run_worker: false,
                ..crate::LocalQueueOptions::default()
            },
        )
        .expect("queue");
        (
            dir,
            Arc::new(ServeState {
                queue,
                token: String::from("t0ken"),
                started: forge_library::clock::now_iso(),
                version: String::from("0.1.0"),
            }),
        )
    }

    #[tokio::test]
    async fn health_needs_the_token_and_a_loopback_host() {
        let (_dir, state) = state();
        let app = router(Arc::clone(&state));
        let no_token = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/health")
                    .header("host", "127.0.0.1:41773")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(no_token.status(), StatusCode::UNAUTHORIZED);

        let foreign_host = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/health")
                    .header("host", "example.com")
                    .header("authorization", "Bearer t0ken")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(foreign_host.status(), StatusCode::FORBIDDEN);

        let ok = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/health")
                    .header("host", "localhost:41773")
                    .header("authorization", "Bearer t0ken")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(ok.status(), StatusCode::OK);
    }
}
