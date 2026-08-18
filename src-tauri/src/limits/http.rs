//! Thin JSON GET over `ureq` with a hard timeout; status codes map to limits-specific errors.

use std::time::Duration;

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// 401 / 403: the stored token was rejected.
    Unauthorized,
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
            Self::Status(code) => write!(f, "HTTP {code}"),
            Self::Network(why) => write!(f, "network error: {why}"),
            Self::Parse(why) => write!(f, "unreadable response: {why}"),
        }
    }
}

/// GET `url` with the given headers (a `User-Agent` is always added) and parse the JSON body.
/// The request never logs or echoes its headers.
pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, HttpError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .user_agent(concat!("on-n-off/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut request = agent.get(url).set("Accept", "application/json");
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
    let body = response
        .into_string()
        .map_err(|error| HttpError::Network(error.to_string()))?;
    serde_json::from_str::<Value>(&body).map_err(|error| HttpError::Parse(error.to_string()))
}

/// One-shot HTTP server on a loopback port; returns the URL and the captured request head.
/// Shared by the http and pipeline tests so no test ever touches the network.
#[cfg(test)]
pub(super) fn serve_once(
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

/// A loopback URL nothing listens on: any request to it fails with a connection error.
#[cfg(test)]
pub(super) fn refused_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    drop(listener);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_parsed_json_and_sends_the_given_headers() {
        let (url, request) = serve_once("200 OK", r#"{"ok":true}"#);
        let value = get_json(&url, &[("Authorization", "Bearer t0k"), ("X-Probe", "1")]).unwrap();
        assert_eq!(value["ok"], serde_json::Value::Bool(true));
        let head = request.join().unwrap();
        assert!(head.contains("Authorization: Bearer t0k"), "{head}");
        assert!(head.contains("X-Probe: 1"), "{head}");
        assert!(head.contains("User-Agent: on-n-off/"), "{head}");
        assert!(head.contains("Accept: application/json"), "{head}");
    }

    #[test]
    fn unauthorized_and_forbidden_are_unauthorized() {
        for status in ["401 Unauthorized", "403 Forbidden"] {
            let (url, request) = serve_once(status, r#"{"error":"nope"}"#);
            assert_eq!(
                get_json(&url, &[]),
                Err(HttpError::Unauthorized),
                "{status}"
            );
            request.join().unwrap();
        }
    }

    #[test]
    fn other_error_statuses_keep_their_code() {
        let (url, request) = serve_once("503 Service Unavailable", "");
        assert_eq!(get_json(&url, &[]), Err(HttpError::Status(503)));
        request.join().unwrap();
    }

    #[test]
    fn a_non_json_body_is_a_parse_error() {
        let (url, request) = serve_once("200 OK", "<html>");
        assert!(matches!(get_json(&url, &[]), Err(HttpError::Parse(_))));
        request.join().unwrap();
    }

    #[test]
    fn a_refused_connection_is_a_network_error() {
        assert!(matches!(
            get_json(&refused_url(), &[]),
            Err(HttpError::Network(_))
        ));
    }
}
