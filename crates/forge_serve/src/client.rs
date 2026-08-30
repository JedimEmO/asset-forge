//! `RemoteQueue`: the same eight methods, over the loopback HTTP door.
//!
//! Nothing above this line knows which impl it holds. That is the point of
//! the trait, and it is what makes `mcp-session` a real gate: the same
//! script runs against a daemon's queue and against a queue of one inside
//! the MCP process, and neither leg is a special case of the other.

use std::time::{Duration, Instant};

use crate::job::{Job, JobFilter, JobId, JobSpec, LogChunk, Run, RunFilter, Status};
use crate::wire;
use crate::{Queue, ServeError};

/// How long a call to the daemon may take before it is unreachable.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the health check at discovery may take.
pub(crate) const HEALTH_TIMEOUT: Duration = Duration::from_millis(300);

/// How often [`Queue::wait`] asks the daemon again.
const WAIT_POLL: Duration = Duration::from_millis(300);

/// A queue that lives in another process.
#[derive(Debug, Clone)]
pub struct RemoteQueue {
    base: String,
    token: String,
}

impl RemoteQueue {
    /// A client of the daemon at `url` holding `token`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            base: url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        }
    }

    /// The daemon's `/v1/health`, or why it did not answer.
    ///
    /// # Errors
    ///
    /// [`ServeError::Unreachable`] when nothing answered in `timeout`.
    pub fn health(&self, timeout: Duration) -> Result<serde_json::Value, ServeError> {
        let response = wire::get(
            &format!("{}/v1/health", self.base),
            Some(&self.token),
            timeout,
        )
        .map_err(ServeError::Unreachable)?;
        response
            .json()
            .ok_or_else(|| ServeError::Protocol(String::from("health was not JSON")))
    }

    /// Ask the daemon to stop.
    ///
    /// # Errors
    ///
    /// [`ServeError::Unreachable`] when it does not answer.
    pub fn stop(&self) -> Result<(), ServeError> {
        wire::post(
            &format!("{}/v1/stop", self.base),
            Some(&self.token),
            "{}",
            CALL_TIMEOUT,
        )
        .map_err(ServeError::Unreachable)?;
        Ok(())
    }

    /// The URL this client talks to — what `forge serve --status` prints.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.base
    }

    /// A GET whose body is JSON.
    fn get_json(&self, path: &str) -> Result<serde_json::Value, ServeError> {
        let response = wire::get(
            &format!("{}{path}", self.base),
            Some(&self.token),
            CALL_TIMEOUT,
        )
        .map_err(ServeError::Unreachable)?;
        decode(&response)
    }
}

/// Turn an answer into a value, or into the refusal it carries.
fn decode(response: &wire::Response) -> Result<serde_json::Value, ServeError> {
    let value = response
        .json()
        .ok_or_else(|| ServeError::Protocol(format!("not JSON: {}", first_line(&response.body))))?;
    match response.status {
        200..=299 => Ok(value),
        // A row that is not there and a path another row holds are both
        // the caller's to fix, and both name what does exist.
        404 | 409 => Err(ServeError::Refused(message_of(&value))),
        other => Err(ServeError::Protocol(format!(
            "{other}: {}",
            message_of(&value)
        ))),
    }
}

/// The `message` of a refusal body, or the whole of it.
fn message_of(value: &serde_json::Value) -> String {
    value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// The first line of a body, for an error that should not paste a page.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

impl Queue for RemoteQueue {
    fn submit(&self, spec: JobSpec) -> Result<Job, ServeError> {
        let body = serde_json::to_string(&spec)
            .map_err(|e| ServeError::Protocol(format!("the spec would not serialise: {e}")))?;
        let response = wire::post(
            &format!("{}/v1/jobs", self.base),
            Some(&self.token),
            &body,
            CALL_TIMEOUT,
        )
        .map_err(ServeError::Unreachable)?;
        let value = decode(&response)?;
        serde_json::from_value(value)
            .map_err(|e| ServeError::Protocol(format!("the job row did not parse: {e}")))
    }

    fn get(&self, id: &JobId) -> Result<Option<Job>, ServeError> {
        match self.get_json(&format!("/v1/jobs/{id}")) {
            Ok(value) => serde_json::from_value(value)
                .map(Some)
                .map_err(|e| ServeError::Protocol(format!("the job row did not parse: {e}"))),
            Err(ServeError::Refused(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, ServeError> {
        let mut query = Vec::new();
        if let Some(state) = filter.state {
            query.push(format!("state={}", state.as_str()));
        }
        if let Some(kind) = &filter.kind {
            query.push(format!("kind={kind}"));
        }
        if let Some(limit) = filter.limit {
            query.push(format!("limit={limit}"));
        }
        let value = self.get_json(&format!("/v1/jobs?{}", query.join("&")))?;
        serde_json::from_value(value)
            .map_err(|e| ServeError::Protocol(format!("the job list did not parse: {e}")))
    }

    fn cancel(&self, id: &JobId) -> Result<Job, ServeError> {
        let response = wire::post(
            &format!("{}/v1/jobs/{id}/cancel", self.base),
            Some(&self.token),
            "{}",
            CALL_TIMEOUT,
        )
        .map_err(ServeError::Unreachable)?;
        let value = decode(&response)?;
        serde_json::from_value(value.get("job").cloned().unwrap_or(value))
            .map_err(|e| ServeError::Protocol(format!("the cancelled row did not parse: {e}")))
    }

    fn log(&self, id: &JobId, from: u64) -> Result<LogChunk, ServeError> {
        let response = wire::get(
            &format!("{}/v1/jobs/{id}/log?from={from}", self.base),
            Some(&self.token),
            CALL_TIMEOUT,
        )
        .map_err(ServeError::Unreachable)?;
        if response.status == 404 {
            return Err(ServeError::Refused(format!("no job {id}")));
        }
        let next = response
            .header("x-forge-log-next")
            .and_then(|value| value.parse().ok())
            .unwrap_or(from);
        let done = response.header("x-forge-log-done") == Some("true");
        Ok(LogChunk {
            text: response.body,
            next,
            done,
        })
    }

    fn status(&self) -> Result<Status, ServeError> {
        let value = self.get_json("/v1/status")?;
        serde_json::from_value(value)
            .map_err(|e| ServeError::Protocol(format!("the status did not parse: {e}")))
    }

    fn runs(&self, filter: &RunFilter) -> Result<Vec<Run>, ServeError> {
        let mut query = Vec::new();
        if let Some(kind) = &filter.kind {
            query.push(format!("kind={kind}"));
        }
        if let Some(since) = &filter.since {
            query.push(format!("since={since}"));
        }
        if let Some(limit) = filter.limit {
            query.push(format!("limit={limit}"));
        }
        let value = self.get_json(&format!("/v1/runs?{}", query.join("&")))?;
        serde_json::from_value(value)
            .map_err(|e| ServeError::Protocol(format!("the runs list did not parse: {e}")))
    }

    fn wait(&self, id: &JobId, max: Duration) -> Result<Job, ServeError> {
        // Polling, not a long-lived socket: a wait that survives a daemon
        // restart is worth more than one that saves a round trip a second.
        let deadline = Instant::now() + max;
        loop {
            let Some(job) = self.get(id)? else {
                return Err(ServeError::NoSuchJob {
                    id: id.clone(),
                    existing: self
                        .list(&JobFilter {
                            limit: Some(10),
                            ..JobFilter::default()
                        })
                        .unwrap_or_default()
                        .into_iter()
                        .map(|job| job.id)
                        .collect(),
                });
            };
            if job.state.is_terminal() || Instant::now() >= deadline {
                return Ok(job);
            }
            std::thread::sleep(WAIT_POLL);
        }
    }
}
