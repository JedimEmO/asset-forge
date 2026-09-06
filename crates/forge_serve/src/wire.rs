//! Thirty lines of HTTP/1.1 over a `TcpStream`, because every request this
//! crate makes is to a server on this machine that it also wrote or pinned.
//!
//! Two callers: [`crate::client::RemoteQueue`] talking to a `forge serve`
//! daemon, and [`crate::card`] asking `ComfyUI` for `/system_stats` and
//! `/free`. Both are loopback, neither has TLS, both answer in bytes a
//! `Content-Length` describes, and a full client stack would add a runtime
//! to a call the CLI makes synchronously between two prints.
//!
//! Every request says `Connection: close`, so the body is "read to EOF" and
//! there is no keep-alive state machine here, no chunked decoder, and no
//! second thing to get wrong.

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

/// One answer.
#[derive(Debug, Clone)]
pub(crate) struct Response {
    /// The status line's code.
    pub(crate) status: u16,
    /// The headers, lower-cased names.
    pub(crate) headers: Vec<(String, String)>,
    /// The body.
    pub(crate) body: String,
}

impl Response {
    /// One header by name.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON, when it is.
    pub(crate) fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.body).ok()
    }
}

/// A request to a loopback URL.
///
/// `url` is `http://host:port/path`; anything else is a refusal, because
/// this client is deliberately not a general one.
pub(crate) fn request(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: Option<&str>,
    timeout: Duration,
) -> Result<Response, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{url} is not an http:// url — this client is loopback only"))?;
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, String::from("/")), |(a, p)| (a, format!("/{p}")));
    let address = authority
        .to_socket_addrs()
        .map_err(|e| format!("{authority}: {e}"))?
        .next()
        .ok_or_else(|| format!("{authority} resolves to nothing"))?;
    let mut stream =
        TcpStream::connect_timeout(&address, timeout).map_err(|e| format!("{authority}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|e| e.to_string())?;

    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: */*\r\n"
    );
    if let Some(token) = token {
        let _ = write!(head, "Authorization: Bearer {token}\r\n");
    }
    if let Some(body) = body {
        let _ = write!(
            head,
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        );
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body.unwrap_or("").as_bytes()))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("writing to {authority}: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("reading from {authority}: {e}"))?;
    parse(&raw)
}

/// Split a response into status, headers and body.
fn parse(raw: &[u8]) -> Result<Response, String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| String::from("the answer had no header block"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("no status code in {status_line:?}"))?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok(Response {
        status,
        headers,
        body: body.to_owned(),
    })
}

/// `GET url`.
pub(crate) fn get(url: &str, token: Option<&str>, timeout: Duration) -> Result<Response, String> {
    request("GET", url, token, None, timeout)
}

/// `POST url` with a JSON body.
pub(crate) fn post(
    url: &str,
    token: Option<&str>,
    body: &str,
    timeout: Duration,
) -> Result<Response, String> {
    request("POST", url, token, Some(body), timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_splits_into_status_headers_and_body() {
        let raw = b"HTTP/1.1 201 Created\r\nX-Forge-Log-Next: 42\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        let response = parse(raw).expect("parse");
        assert_eq!(response.status, 201);
        assert_eq!(response.header("x-forge-log-next"), Some("42"));
        assert_eq!(
            response
                .json()
                .and_then(|v| v.get("ok").and_then(serde_json::Value::as_bool)),
            Some(true)
        );
    }

    #[test]
    fn a_url_that_is_not_loopback_http_is_refused_before_a_socket() {
        let err = get("https://example.com/", None, Duration::from_millis(1)).expect_err("refused");
        assert!(err.contains("loopback only"), "{err}");
    }
}
