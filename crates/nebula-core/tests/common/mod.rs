//! The in-process HTTP stub every offline network test is served by.
//!
//! Nothing here reaches the real Modrinth or CurseForge APIs: a test passes a
//! route table, the stub answers it and records every request — method,
//! target, headers, body and the instant it arrived — so client-side rules
//! such as the 250 ms CurseForge spacing and `x-api-key` surviving a redirect
//! are assertable (plan reliability rule 4).
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// One request as the stub saw it.
#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    /// The raw request target, still percent-encoded.
    pub target: String,
    /// Header names are lowercased; values are trimmed.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    /// The path with the query removed.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }

    /// The decoded query parameters. Routes must read these rather than
    /// string-matching the target: `facets` and `searchFilter` arrive
    /// percent-encoded.
    pub fn query(&self) -> HashMap<String, String> {
        let url = format!("http://stub.invalid{}", self.target);
        url::Url::parse(&url)
            .map(|url| {
                url.query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One header value, or `None`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The body parsed as JSON, for the POST endpoints.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// Every request the stub has answered, in order, with its arrival instant.
pub type Calls = Arc<Mutex<Vec<(Instant, Req)>>>;

/// What a route answers with.
///
/// The second element is normally the `Content-Type` value. A value that
/// already contains a colon is written out as raw header lines instead, which
/// is how a route sets `Location` on a redirect or `Retry-After` on a 429;
/// separate several with `\r\n`.
pub type Response = (u16, &'static str, Vec<u8>);

/// Starts a stub on an ephemeral port and returns its address and call log.
pub fn stub(routes: impl Fn(&Req) -> Response + Send + Sync + 'static) -> (SocketAddr, Calls) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));

    let recorded = Arc::clone(&calls);
    let routes = Arc::new(routes);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let recorded = Arc::clone(&recorded);
            let routes = Arc::clone(&routes);
            std::thread::spawn(move || serve(stream, &recorded, routes.as_ref()));
        }
    });
    (addr, calls)
}

/// A stub that answers `201 Created` with an empty body to everything, which
/// is what the WebDAV tests need.
pub fn stub_created() -> (SocketAddr, Calls) {
    stub(|_| (201, "text/plain", Vec::new()))
}

/// The requests recorded so far, without their instants.
pub fn requests(calls: &Calls) -> Vec<Req> {
    calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, req)| req.clone())
        .collect()
}

/// `(method, target, body length)` for each call, the shape the publishing
/// tests assert on.
pub fn summaries(calls: &Calls) -> Vec<(String, String, usize)> {
    calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, req)| (req.method.clone(), req.target.clone(), req.body.len()))
        .collect()
}

fn serve(stream: TcpStream, calls: &Mutex<Vec<(Instant, Req)>>, routes: &dyn Fn(&Req) -> Response) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);

    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_owned();
        let target = parts.next().unwrap_or_default().to_owned();

        let mut headers = Vec::new();
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let Some((name, value)) = header.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }

        let mut body = vec![0u8; length];
        if length > 0 && reader.read_exact(&mut body).is_err() {
            return;
        }

        let request = Req {
            method,
            target,
            headers,
            body,
        };
        let (status, header, payload) = routes(&request);
        calls.lock().unwrap().push((Instant::now(), request));

        let header_lines = match header.contains(':') {
            true => header.to_owned(),
            false => format!("Content-Type: {header}"),
        };
        let head = format!(
            "HTTP/1.1 {status} {}\r\n{header_lines}\r\nContent-Length: {}\r\n\r\n",
            reason(status),
            payload.len()
        );
        if writer.write_all(head.as_bytes()).is_err() || writer.write_all(&payload).is_err() {
            return;
        }
        let _ = writer.flush();
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        201 => "Created",
        302 => "Found",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    }
}
