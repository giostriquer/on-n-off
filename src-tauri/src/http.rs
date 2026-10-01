use std::time::Duration;

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitReset {
    RetryAfter(u64),
    At(i64),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Unauthorized,
    RateLimited(RateLimitReset),
    Status(u16),
    Network(String),
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

type Reply = ureq::http::Response<ureq::Body>;

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(agent_config(ureq::Agent::config_builder(), !cfg!(test)))
}

fn agent_config(
    builder: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>,
    honour_env_proxy: bool,
) -> ureq::config::Config {
    let builder = builder
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("on-n-off/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false);
    if honour_env_proxy {
        builder.build()
    } else {
        builder.proxy(None).build()
    }
}

fn parse_body(mut response: Reply) -> Result<Value, HttpError> {
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| HttpError::Network(error.to_string()))?;
    serde_json::from_str::<Value>(&body).map_err(|error| HttpError::Parse(error.to_string()))
}

pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, HttpError> {
    let mut request = agent().get(url);
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
    {
        request = request.header("Accept", "application/json");
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .call()
        .map_err(|error| HttpError::Network(error.to_string()))?;
    finish(response, Forbidden::RejectedToken)
}

pub fn post_json(url: &str, bearer: &str, body: &Value) -> Result<Value, HttpError> {
    let payload =
        serde_json::to_string(body).map_err(|error| HttpError::Parse(error.to_string()))?;
    let response = agent()
        .post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Authorization", &format!("Bearer {bearer}"))
        .send(&payload)
        .map_err(|error| HttpError::Network(error.to_string()))?;
    finish(response, Forbidden::RateLimitWhenSaid)
}

pub fn post_grant(url: &str, body: &Value) -> Result<Value, HttpError> {
    let payload =
        serde_json::to_string(body).map_err(|error| HttpError::Parse(error.to_string()))?;
    let response = agent()
        .post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send(&payload)
        .map_err(|error| HttpError::Network(error.to_string()))?;
    finish(response, Forbidden::PlainStatus)
}

enum Forbidden {
    RejectedToken,
    RateLimitWhenSaid,
    PlainStatus,
}

fn finish(response: Reply, forbidden: Forbidden) -> Result<Value, HttpError> {
    let code = response.status().as_u16();
    match (code, &forbidden) {
        (200..=299, _) => parse_body(response),
        (401, _) => Err(HttpError::Unauthorized),
        (403, Forbidden::RejectedToken) => Err(HttpError::Unauthorized),
        (403, Forbidden::RateLimitWhenSaid) => {
            Err(rate_limit_reset(&response).map_or(HttpError::Status(403), HttpError::RateLimited))
        }
        (429, Forbidden::RejectedToken | Forbidden::RateLimitWhenSaid) => Err(
            HttpError::RateLimited(rate_limit_reset(&response).unwrap_or(RateLimitReset::Unknown)),
        ),
        _ => Err(HttpError::Status(code)),
    }
}

fn rate_limit_reset(response: &Reply) -> Option<RateLimitReset> {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
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

#[cfg(test)]
const ACCEPT_DEADLINE: Duration = Duration::from_secs(10);

#[cfg(test)]
fn accept_within(listener: &std::net::TcpListener, deadline: Duration) -> std::net::TcpStream {
    listener.set_nonblocking(true).unwrap();
    let started = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < deadline,
                    "no request reached the loopback server within {deadline:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("loopback accept failed: {error}"),
        }
    }
}

#[cfg(test)]
pub(crate) fn accept_in_time(listener: &std::net::TcpListener) -> std::net::TcpStream {
    accept_within(listener, ACCEPT_DEADLINE)
}

#[cfg(test)]
pub(crate) fn serve_once(
    status_line: &str,
    body: &str,
) -> (String, std::thread::JoinHandle<String>) {
    serve("/usage", &[(status_line, &[], body)], |mut requests| {
        requests.remove(0).head
    })
}

#[cfg(test)]
pub(crate) struct CapturedRequest {
    pub(crate) head: String,
    pub(crate) body: String,
}

#[cfg(test)]
pub(crate) fn serve_once_capturing(
    status_line: &str,
    response_headers: &[&str],
    body: &str,
) -> (String, std::thread::JoinHandle<CapturedRequest>) {
    serve(
        "/graphql",
        &[(status_line, response_headers, body)],
        |mut requests| requests.remove(0),
    )
}

#[cfg(test)]
pub(crate) fn serve_sequence(
    responses: &[(&str, &[&str], &str)],
) -> (String, std::thread::JoinHandle<Vec<CapturedRequest>>) {
    serve("/graphql", responses, |requests| requests)
}

#[cfg(test)]
fn serve<T: Send + 'static>(
    path: &str,
    responses: &[(&str, &[&str], &str)],
    finish: fn(Vec<CapturedRequest>) -> T,
) -> (String, std::thread::JoinHandle<T>) {
    serve_with(path, responses, || {}, finish)
}

#[cfg(test)]
fn serve_with<T: Send + 'static>(
    path: &str,
    responses: &[(&str, &[&str], &str)],
    mut on_request: impl FnMut() + Send + 'static,
    finish: impl FnOnce(Vec<CapturedRequest>) -> T + Send + 'static,
) -> (String, std::thread::JoinHandle<T>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}{path}", listener.local_addr().unwrap());
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
    let handle = std::thread::spawn(move || {
        let mut captured = Vec::new();
        for (status_line, extra_headers, body) in responses {
            let mut stream = accept_in_time(&listener);
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
            on_request();
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
        finish(captured)
    });
    (url, handle)
}

#[cfg(test)]
pub(crate) fn head_header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

#[cfg(test)]
pub(crate) fn refused_url() -> String {
    "http://127.0.0.1:0/usage".to_string()
}

#[cfg(test)]
pub(crate) fn never_asked() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/never-asked", listener.local_addr().unwrap());
    (listener, url)
}

#[cfg(test)]
pub(crate) fn was_asked(listener: &std::net::TcpListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    match listener.accept() {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("loopback accept failed: {error}"),
    }
}

#[cfg(test)]
mod tests;
