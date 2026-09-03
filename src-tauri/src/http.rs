//! Thin JSON GET/POST over `ureq` with a hard timeout; status codes map to a small error taxonomy
//! shared by the Limits and GitHub readers.

use std::time::Duration;

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

/// When an exhausted rate limit opens again, as the service stated it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitReset {
    /// `retry-after`: seconds from the moment the reply arrived (secondary limits).
    RetryAfter(u64),
    /// `x-ratelimit-reset`: epoch seconds (the primary limit).
    At(i64),
    /// Exhausted without a usable instant.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The stored token was rejected (401; also 403 for the Limits GET, whose services use it).
    Unauthorized,
    /// The service's rate limit is exhausted (403/429 carrying `x-ratelimit-remaining: 0` or
    /// `retry-after`).
    RateLimited(RateLimitReset),
    /// Any other non-success status.
    Status(u16),
    /// DNS, connect, TLS, or timeout failure.
    Network(String),
    /// 2xx with a body that is not JSON.
    Parse(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "token rejected"),
            Self::RateLimited(_) => write!(f, "rate limited"),
            Self::Status(code) => write!(f, "HTTP {code}"),
            Self::Network(why) => write!(f, "network error: {why}"),
            Self::Parse(why) => write!(f, "unreadable response: {why}"),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .user_agent(concat!("on-n-off/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn parse_body(response: ureq::Response) -> Result<Value, HttpError> {
    let body = response
        .into_string()
        .map_err(|error| HttpError::Network(error.to_string()))?;
    serde_json::from_str::<Value>(&body).map_err(|error| HttpError::Parse(error.to_string()))
}

/// GET `url` with the given headers (a `User-Agent` is always added) and parse the JSON body.
/// The request never logs or echoes its headers.
pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, HttpError> {
    let mut request = agent().get(url).set("Accept", "application/json");
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(401 | 403, _)) => return Err(HttpError::Unauthorized),
        Err(ureq::Error::Status(code, _)) => return Err(HttpError::Status(code)),
        Err(ureq::Error::Transport(transport)) => {
            return Err(HttpError::Network(transport.kind().to_string()))
        }
    };
    parse_body(response)
}

/// POST `body` as JSON to `url` with a bearer token and parse the JSON reply. Unlike `get_json`,
/// a 403 is only `Unauthorized` when it is not a rate limit: GitHub answers 403/429 for exhausted
/// limits and 401 for a bad token. The token is never logged or echoed.
pub fn post_json(url: &str, bearer: &str, body: &Value) -> Result<Value, HttpError> {
    let payload =
        serde_json::to_string(body).map_err(|error| HttpError::Parse(error.to_string()))?;
    let response = match agent()
        .post(url)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {bearer}"))
        .send_string(&payload)
    {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => return Err(HttpError::Unauthorized),
        Err(ureq::Error::Status(code @ (403 | 429), response)) => {
            return Err(match rate_limit_reset(&response) {
                Some(reset) => HttpError::RateLimited(reset),
                None if code == 429 => HttpError::RateLimited(RateLimitReset::Unknown),
                None => HttpError::Status(code),
            })
        }
        Err(ureq::Error::Status(code, _)) => return Err(HttpError::Status(code)),
        Err(ureq::Error::Transport(transport)) => {
            return Err(HttpError::Network(transport.kind().to_string()))
        }
    };
    parse_body(response)
}

/// `Some` when the response says the rate limit is exhausted: `retry-after` (secondary limits)
/// wins over `x-ratelimit-reset` (the primary limit). GitHub sends the `x-ratelimit-*` headers
/// on every reply, so only a zero remaining count marks a 403 as a rate limit.
fn rate_limit_reset(response: &ureq::Response) -> Option<RateLimitReset> {
    let header = |name: &str| response.header(name).map(str::trim);
    if let Some(seconds) = header("retry-after").and_then(|value| value.parse::<u64>().ok()) {
        return Some(RateLimitReset::RetryAfter(seconds));
    }
    if header("x-ratelimit-remaining") != Some("0") {
        return None;
    }
    Some(
        header("x-ratelimit-reset")
            .and_then(|value| value.parse::<i64>().ok())
            .map_or(RateLimitReset::Unknown, RateLimitReset::At),
    )
}

/// One-shot HTTP server on a loopback port; returns the URL and the captured request head.
/// Shared by the http and pipeline tests so no test ever touches the network.
#[cfg(test)]
pub(crate) fn serve_once(
    status_line: &str,
    body: &str,
) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    let body = body.to_string();
    let status_line = status_line.to_string();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            request.extend_from_slice(&buf[..n]);
            if n == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        String::from_utf8_lossy(&request).to_string()
    });
    (url, handle)
}

/// What `serve_once_capturing` saw: the request head (request line + headers) and the body.
#[cfg(test)]
pub(crate) struct CapturedRequest {
    pub(crate) head: String,
    pub(crate) body: String,
}

/// Like `serve_once`, but honours `Content-Length` so a POST body is captured in full, and lets
/// the test add response headers (rate-limit headers, for instance).
#[cfg(test)]
pub(crate) fn serve_once_capturing(
    status_line: &str,
    response_headers: &[&str],
    body: &str,
) -> (String, std::thread::JoinHandle<CapturedRequest>) {
    let (url, handle) = serve_sequence(&[(status_line, response_headers, body)]);
    (
        url,
        std::thread::spawn(move || handle.join().unwrap().remove(0)),
    )
}

/// A loopback server answering one connection per entry, in order, capturing each request's
/// head and body. Lets a test script "401, then 200" or "429, then 200" against one URL.
#[cfg(test)]
pub(crate) fn serve_sequence(
    responses: &[(&str, &[&str], &str)],
) -> (String, std::thread::JoinHandle<Vec<CapturedRequest>>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/graphql", listener.local_addr().unwrap());
    let responses: Vec<(String, String, String)> = responses
        .iter()
        .map(|(status_line, headers, body)| {
            (
                status_line.to_string(),
                headers
                    .iter()
                    .map(|header| format!("{header}\r\n"))
                    .collect::<String>(),
                body.to_string(),
            )
        })
        .collect();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut captured = Vec::new();
        for (status_line, extra_headers, body) in responses {
            // A test whose code under test never connects must fail, not hang the whole run.
            let started = std::time::Instant::now();
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < Duration::from_secs(10),
                            "no request reached the loopback server within 10 s"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            let head_end = loop {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
                if n == 0 {
                    break request.len();
                }
            };
            let head = String::from_utf8_lossy(&request[..head_end]).to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while request.len() - head_end < content_length {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let body_end = (head_end + content_length).min(request.len());
            captured.push(CapturedRequest {
                head,
                body: String::from_utf8_lossy(&request[head_end..body_end]).to_string(),
            });
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
        captured
    });
    (url, handle)
}

/// A loopback URL nothing listens on: any request to it fails with a connection error.
#[cfg(test)]
pub(crate) fn refused_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    drop(listener);
    url
}

#[cfg(test)]
mod tests;
