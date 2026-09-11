//! The one retrying, rate-limit-aware JSON path the providers share.
//!
//! It sends through [`crate::remote::client`] — the crate's only connection
//! pool — with a per-request timeout, so nothing else in the crate changes
//! behaviour. Every number below is **our** policy, not provider guidance:
//! Modrinth documents no `Retry-After` and CurseForge publishes no limit at
//! all (facts §E.3.2), so they are named constants and nothing else.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{CatalogError, ProviderId, USER_AGENT};
use crate::publish::excerpt;

/// Whole-request budget, overriding the shared client's download-friendly one.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Retries after the first attempt.
const RETRIES: u32 = 3;
/// First backoff; each retry doubles it (0.5 s, 1 s, 2 s).
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
/// Longest we will sit on a 429 before handing the caller `RateLimited`.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(30);
/// `X-Ratelimit-Reset` reads `0` on a fresh window, so it is clamped (§A.1).
const MIN_RESET_WAIT: Duration = Duration::from_secs(1);
const MAX_RESET_WAIT: Duration = Duration::from_secs(60);
/// CurseForge publishes no budget, so we space its requests ourselves (R7).
const CURSEFORGE_SPACING: Duration = Duration::from_millis(250);

/// When the next CurseForge request may start, process-wide. A reservation
/// rather than "the last request's instant", so a thread that is sleeping out
/// its spacing has already claimed its slot and the one behind it queues
/// after that instead of racing it.
///
/// ponytail: process-local by design (R7). Two `nebula mods … search` runs
/// started back to back can hit CurseForge 119 ms apart, because a `static`
/// does not cross a process boundary. A file-based reservation next to
/// `.nebula-lock` is the upgrade if a scripted loop ever draws a 429; until
/// then `RUST_LOG=trace` prints one line per request (`trace_request`) so the
/// real spacing is measurable instead of assumed (CF-08).
static CURSEFORGE_NEXT: Mutex<Option<Instant>> = Mutex::new(None);
/// Modrinth's advertised budget: requests left and when the window resets.
static MODRINTH_BUDGET: Mutex<Option<(u32, Instant)>> = Mutex::new(None);

/// GET a JSON document.
pub(crate) fn get_json<T: DeserializeOwned>(
    provider: ProviderId,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<T, CatalogError> {
    parse(url, &send(provider, Method::Get, url, headers, None)?)
}

/// POST a JSON body and read a JSON document back. Both bulk endpoints we use
/// (`version_files`, `fingerprints`) are reads, so retrying a POST is safe.
pub(crate) fn post_json<B: Serialize, T: DeserializeOwned>(
    provider: ProviderId,
    url: &str,
    headers: &[(&str, &str)],
    body: &B,
) -> Result<T, CatalogError> {
    let body = serde_json::to_vec(body).map_err(|error| network(url, error))?;
    parse(
        url,
        &send(provider, Method::Post, url, headers, Some(&body))?,
    )
}

/// GET raw bytes, for the icon fetcher. Same throttle and retry policy as the
/// JSON path; no per-request timeout is imposed because an icon is tiny and
/// the shared client already caps the connect phase.
pub(crate) fn get_bytes(
    provider: ProviderId,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, CatalogError> {
    raw_get(provider, url, headers)?
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|error| network(url, error))
}

/// Streams one file to `dest`. The provider headers travel with it because
/// CurseForge wants `x-api-key` on the CDN too, and it must survive the
/// `edge.forgecdn.net` → `mediafilez.forgecdn.net` redirect (R2).
pub(crate) fn download(
    provider: ProviderId,
    url: &str,
    headers: &[(&str, &str)],
    dest: &std::path::Path,
) -> Result<(), CatalogError> {
    let mut response = raw_get(provider, url, headers)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(dest)?;
    response
        .copy_to(&mut file)
        .map_err(|error| network(url, error))?;
    Ok(())
}

/// One GET with the throttle, retries and 429 policy, handing the body back
/// unread so a caller can stream it.
fn raw_get(
    provider: ProviderId,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<reqwest::blocking::Response, CatalogError> {
    let response = request(provider, Method::Get, url, headers, None, None, false)?;
    match ok(&response) {
        // The body is the file being streamed, so a failure names the status
        // alone rather than eating the bytes to quote them.
        false => Err(network(url, format!("HTTP {}", response.status().as_u16()))),
        true => Ok(response),
    }
}

/// Whether a response is usable or the request has to be repeated.
enum Retry {
    Wait(Duration),
    Done,
}

/// Whether the status is a 2xx, which is the only thing either caller treats
/// as an answer.
fn ok(response: &reqwest::blocking::Response) -> bool {
    (200..300).contains(&response.status().as_u16())
}

/// The retry decision for one response. A status this cannot act on — any
/// non-2xx that is neither a 429, a retryable 5xx, a 404 nor a rejected key —
/// comes back as [`Retry::Done`], because what such a body is worth saying
/// about is the caller's to decide.
fn classify(
    provider: ProviderId,
    url: &str,
    response: &reqwest::blocking::Response,
    backoff: Duration,
    last: bool,
) -> Result<Retry, CatalogError> {
    let status = response.status().as_u16();
    if status == 429 {
        let wait = rate_limit_wait(response).unwrap_or(backoff);
        if wait > MAX_RATE_LIMIT_WAIT || last {
            return Err(CatalogError::RateLimited {
                retry_after_secs: wait.as_secs().max(1) as u32,
            });
        }
        log::debug!("Rate limited by {url}; waiting {wait:?}");
        return Ok(Retry::Wait(wait));
    }
    if (500..600).contains(&status) && !last {
        log::debug!("Retrying {url} after HTTP {status}");
        return Ok(Retry::Wait(backoff));
    }
    if status == 404 {
        return Err(CatalogError::NotFound);
    }
    if let Some(rejected) = key_rejected(provider, status) {
        return Err(rejected);
    }
    Ok(Retry::Done)
}

/// CurseForge answers 401/403 when the user's key is missing, wrong or
/// revoked. That is not a network failure and it is not retried: the key will
/// not fix itself in 3.5 s, and the user needs the prescription, not a status
/// code (CF-04). Modrinth needs no key, so its 403 stays a plain failure.
fn key_rejected(provider: ProviderId, status: u16) -> Option<CatalogError> {
    match provider == ProviderId::CurseForge && matches!(status, 401 | 403) {
        true => Some(CatalogError::Unauthorized),
        false => None,
    }
}

/// One line per outgoing catalog request, logged **after** the throttle so the
/// spacing R7 promises can be measured from `RUST_LOG=trace`. Headers are never
/// logged: `x-api-key` travels in one (CF-08).
fn trace_request(provider: ProviderId, method: &str, url: &str) {
    log::trace!("{provider} {method} {url}");
}

fn parse<T: DeserializeOwned>(url: &str, body: &str) -> Result<T, CatalogError> {
    serde_json::from_str(body)
        .map_err(|error| CatalogError::Network(format!("{url}: {error} — {}", excerpt(body))))
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

fn network(url: &str, message: impl std::fmt::Display) -> CatalogError {
    CatalogError::Network(format!("{url}: {message}"))
}

/// One JSON request with the retry, backoff and 429 policy applied. Error
/// bodies may be empty (§E.1.1), so nothing here requires one to build an
/// error — but a body that is there is quoted, because on this path it is a
/// provider's explanation rather than a file.
fn send(
    provider: ProviderId,
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<String, CatalogError> {
    let response = request(
        provider,
        method,
        url,
        headers,
        body,
        Some(REQUEST_TIMEOUT),
        true,
    )?;
    if !ok(&response) {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        return Err(network(url, format!("{status} {}", excerpt(&body))));
    }
    response.text().map_err(|error| network(url, error))
}

/// The one attempt loop both paths share: throttle, send, [`classify`], sleep,
/// repeat. The response comes back unread so a caller can stream it.
///
/// `timeout` overrides the shared client's download-friendly budget, which only
/// the JSON path wants; `budget` is Modrinth's advertised-quota bookkeeping,
/// which only its API host sends — a CDN response carries no such headers and
/// must not be asked about them.
fn request(
    provider: ProviderId,
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Option<Duration>,
    budget: bool,
) -> Result<reqwest::blocking::Response, CatalogError> {
    let client = crate::remote::client()?;
    let mut backoff = FIRST_BACKOFF;

    for attempt in 0..=RETRIES {
        let last = attempt == RETRIES;
        throttle(provider);
        trace_request(provider, method.as_str(), url);

        let mut request = match method {
            Method::Get => client.get(url),
            Method::Post => client.post(url),
        };
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        request = request.header("user-agent", USER_AGENT);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_vec());
        }

        let response = match request.send() {
            Ok(response) => response,
            Err(error) => {
                if last || error.is_builder() || error.is_redirect() {
                    return Err(network(url, error));
                }
                log::debug!("Retrying {url} after {error}");
                std::thread::sleep(backoff);
                backoff *= 2;
                continue;
            }
        };
        if budget {
            record_budget(provider, &response);
        }
        match classify(provider, url, &response, backoff, last)? {
            Retry::Wait(wait) => {
                std::thread::sleep(wait);
                backoff *= 2;
            }
            Retry::Done => return Ok(response),
        }
    }
    unreachable!("the loop returns on its last attempt")
}

/// A numeric response header, or `None` when it is absent or unreadable.
fn header_u64(response: &reqwest::blocking::Response, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// `Retry-After` first, then `X-Ratelimit-Reset` clamped to 1-60 s because it
/// reads `0` on a fresh window; `None` falls back to the backoff schedule.
fn rate_limit_wait(response: &reqwest::blocking::Response) -> Option<Duration> {
    if let Some(seconds) = header_u64(response, "retry-after") {
        return Some(Duration::from_secs(seconds));
    }
    let reset = header_u64(response, "x-ratelimit-reset")?;
    Some(Duration::from_secs(reset).clamp(MIN_RESET_WAIT, MAX_RESET_WAIT))
}

/// Modrinth advertises its budget on every response; CurseForge advertises
/// nothing, so only the spacing clock is kept for it.
fn record_budget(provider: ProviderId, response: &reqwest::blocking::Response) {
    if provider != ProviderId::Modrinth {
        return;
    }
    let (Some(remaining), Some(reset)) = (
        header_u64(response, "x-ratelimit-remaining"),
        header_u64(response, "x-ratelimit-reset"),
    ) else {
        return;
    };
    let reset = Duration::from_secs(reset).clamp(MIN_RESET_WAIT, MAX_RESET_WAIT);
    *lock(&MODRINTH_BUDGET) = Some((remaining as u32, Instant::now() + reset));
}

/// Blocks until this provider may be called again. Process-global and shared
/// by quiet and mutating jobs alike (reliability rule 2).
///
/// The wait is decided under the limiter's mutex and slept **after** the guard
/// is dropped: a mutating job holds the cross-process `ServerLock` while it
/// downloads, so parking inside the mutex would let one exhausted rate-limit
/// window make every other Nebula process see `Locked` for up to a minute.
/// Each arm still reserves its slot before letting go, so two threads cannot
/// both decide they may go now.
fn throttle(provider: ProviderId) {
    let wait = match provider {
        ProviderId::CurseForge => {
            let mut next = lock(&CURSEFORGE_NEXT);
            let now = Instant::now();
            let start = next.unwrap_or(now).max(now);
            *next = Some(start + CURSEFORGE_SPACING);
            start.saturating_duration_since(now)
        }
        ProviderId::Modrinth => {
            let mut budget = lock(&MODRINTH_BUDGET);
            match *budget {
                Some((0, reset_at)) => {
                    *budget = None;
                    reset_at
                        .saturating_duration_since(Instant::now())
                        .min(MAX_RESET_WAIT)
                }
                _ => Duration::ZERO,
            }
        }
    };
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

/// A poisoned limiter must not take the whole process down with it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mutating job holds the cross-process server lock while it downloads,
    /// so the limiter must be free while a thread sits out its spacing —
    /// otherwise one rate-limited window makes every other Nebula process see
    /// `Locked`.
    #[test]
    fn the_curseforge_spacing_is_slept_outside_the_limiter() {
        let start = Instant::now();
        throttle(ProviderId::CurseForge);
        let waiter = std::thread::spawn(|| throttle(ProviderId::CurseForge));

        std::thread::sleep(Duration::from_millis(60));
        assert!(
            CURSEFORGE_NEXT.try_lock().is_ok(),
            "the limiter was held while a request waited its turn"
        );
        waiter.join().unwrap();
        // The slot is still reserved, so the spacing itself survives.
        assert!(
            start.elapsed() >= CURSEFORGE_SPACING,
            "{:?}",
            start.elapsed()
        );
    }
}
