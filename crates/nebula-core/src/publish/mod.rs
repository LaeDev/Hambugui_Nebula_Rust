//! Uploading a generated distribution and exactly the files it references.
//!
//! A publish never walks `<root>` looking for content: caches, installer output
//! and schemas are not part of a distribution. The plan is read back out of the
//! generated JSON, so what ships is what the launcher will ask for.
//!
//! The only target is WebDAV, which mirrors `<root>` one to one: the uploaded
//! distribution is the generated one, and its signature lands at the
//! distribution's own path plus `.sig`, which is where the Hambugui launcher
//! looks. A host that serves files by id instead of by path (Google Drive, the
//! target Nebula once had) cannot serve the signature there, so a release
//! launcher would never load what it published.

pub mod webdav;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io};
use crate::hash::{self, Digests};
use crate::layout::Root;
use crate::paths;
use crate::signing::{self, DistributionKey};

/// Somewhere a published file can go. Implementations are stateful so they can
/// cache created directories and connections across a run.
pub trait Target {
    /// Stable, filesystem safe: it keys [`PublishState`].
    fn name(&self) -> &str;

    /// Uploads `local` to `relative` and returns the public URL it can be
    /// downloaded from. `digests` is already computed; a target may use it to
    /// skip a transfer it knows is redundant.
    fn upload(&mut self, local: &Path, relative: &str, digests: &Digests) -> Result<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub local: PathBuf,
    /// Path below `<root>`, forward slashes, already percent-decoded.
    pub relative: String,
}

#[derive(Debug, Clone)]
pub struct PublishPlan {
    pub files: Vec<PlannedFile>,
    pub distribution_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Skip files whose SHA-256 already matches [`PublishState`].
    pub only_changed: bool,
    /// Signs the uploaded distribution into `<file>.sig`. Required: without a
    /// key [`publish`] refuses before uploading anything, so an unsigned
    /// distribution never reaches a launcher.
    pub signing_key: Option<DistributionKey>,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            only_changed: true,
            signing_key: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReport {
    /// Files handed to [`Target::upload`], the distribution and its signature
    /// included.
    pub uploaded: usize,
    /// Files skipped because [`PublishState`] already had their SHA-256.
    pub skipped: usize,
    /// What the launcher should be pointed at.
    pub distribution_url: String,
    /// Where the target serves the distribution's signature.
    pub signature_url: String,
}

/// What [`publish`] says when it has no key. The GUI matches on
/// [`NO_SIGNING_KEY`] to offer the fix.
pub const NO_SIGNING_KEY: &str = "no distribution signing key";

/// Every file the distribution references, in the order it references them.
///
/// URLs outside `root.base_url` are somebody else's to host and are only
/// logged. Referenced files that are missing on disk are fatal.
pub fn plan_from_distribution(root: &Root, distribution_path: &Path) -> Result<PublishPlan> {
    let raw = std::fs::read_to_string(distribution_path)
        .map_err(io(format!("reading {}", distribution_path.display())))?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;

    let mut urls = Vec::new();
    for server in value
        .get("servers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(icon) = server.get("icon").and_then(serde_json::Value::as_str) {
            urls.push(icon.to_owned());
        }
        collect_artifacts(server.get("modules"), &mut urls);
    }

    let base = base_prefix(root);
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut missing = Vec::new();
    for url in urls {
        let Some(rest) = url.strip_prefix(&base) else {
            log::warn!("Not published, outside {base}: {url}");
            continue;
        };
        // A distribution is generated output, but it is still a file on disk
        // that decides what `<root>` path gets opened, so it goes through the
        // same check as a scanned artifact path.
        let relative = paths::clean_relative(&percent_decode(rest))?;
        if !seen.insert(relative.clone()) {
            continue;
        }
        let local = root.path.join(&relative);
        if !local.is_file() {
            missing.push(relative.clone());
        }
        files.push(PlannedFile { local, relative });
    }

    if !missing.is_empty() {
        return Err(Error::Invalid(format!(
            "{} file(s) referenced by {} are missing under {}: {}",
            missing.len(),
            distribution_path.display(),
            root.path.display(),
            missing.join(", ")
        )));
    }

    Ok(PublishPlan {
        files,
        distribution_path: distribution_path.to_path_buf(),
    })
}

fn collect_artifacts(modules: Option<&serde_json::Value>, out: &mut Vec<String>) {
    for module in modules
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(url) = module
            .pointer("/artifact/url")
            .and_then(serde_json::Value::as_str)
        {
            out.push(url.to_owned());
        }
        // Helios spells it `subModules`; hand written distributions in the wild
        // use the all lowercase form and the launcher accepts both.
        collect_artifacts(
            module
                .get("subModules")
                .or_else(|| module.get("submodules")),
            out,
        );
    }
}

/// `root.base_url` as a string prefix. `Root` does not force the trailing slash
/// that `Config` does, and `https://host/dist` must still match
/// `https://host/dist/servers/...`.
fn base_prefix(root: &Root) -> String {
    let mut base = root.base_url.to_string();
    if !base.ends_with('/') {
        base.push('/');
    }
    base
}

/// Reverses the percent-encoding [`Root::url`] applies. A malformed escape is
/// kept verbatim rather than rejected, which is what browsers do, and invalid
/// UTF-8 becomes the replacement character (such a name cannot exist under
/// `<root>` anyway and is caught as a missing file).
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| hex_byte(*bytes.get(index + 1)?, *bytes.get(index + 2)?))
            .flatten();
        match escape {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    let digit = |c: u8| char::from(c).to_digit(16).map(|d| d as u8);
    Some((digit(high)? << 4) | digit(low)?)
}

/// `<root>/publish-state.json`: `{ "<target>": { "<relative>": "<sha256>" } }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublishState {
    targets: BTreeMap<String, BTreeMap<String, String>>,
}

impl PublishState {
    pub fn path(root: &Root) -> PathBuf {
        root.path.join("publish-state.json")
    }

    /// A missing or unreadable state file is not an error: the worst it costs
    /// is a full re-upload.
    pub fn load(root: &Root) -> Self {
        let path = Self::path(root);
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                log::warn!("Ignoring unreadable {}: {e}", path.display());
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                log::warn!("Ignoring unreadable {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, root: &Root) -> Result<()> {
        let path = Self::path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(io(format!("creating {}", parent.display())))?;
        }
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, body).map_err(io(format!("writing {}", path.display())))
    }

    pub fn matches(&self, target: &str, relative: &str, sha256: &str) -> bool {
        self.targets
            .get(target)
            .and_then(|files| files.get(relative))
            .is_some_and(|known| known == sha256)
    }

    pub fn record(&mut self, target: &str, relative: &str, sha256: &str) {
        self.targets
            .entry(target.to_owned())
            .or_default()
            .insert(relative.to_owned(), sha256.to_owned());
    }
}

/// Uploads the plan, then the distribution, then its signature. The
/// distribution goes after its files so a launcher never sees a manifest
/// pointing at files that are not there yet; the signature goes right after the
/// distribution, and a launcher that fetches between the two sees a mismatch
/// and fetches both once more.
///
/// Refuses, before uploading a byte, without a signing key or when the
/// distribution has no `serial` (written by hand or by a Nebula without
/// serials: regenerate it). The bytes uploaded are the bytes signed.
pub fn publish(
    root: &Root,
    plan: &PublishPlan,
    target: &mut dyn Target,
    opts: &PublishOptions,
) -> Result<PublishReport> {
    let key = require_signing_key(opts.signing_key.as_ref())?;
    let serial = require_serial(&plan.distribution_path)?;
    let name = target.name().to_owned();
    let mut state = PublishState::load(root);
    let mut uploaded = 0;
    let mut skipped = 0;

    for file in &plan.files {
        let digests = hash::digests_of_file(&file.local)?;
        if opts.only_changed && state.matches(&name, &file.relative, &digests.sha256) {
            log::debug!("Unchanged, skipping {}", file.relative);
            skipped += 1;
            continue;
        }

        log::info!("Uploading {} to {name}", file.relative);
        target.upload(&file.local, &file.relative, &digests)?;
        uploaded += 1;
        state.record(&name, &file.relative, &digests.sha256);
        // Written after every file so an interrupted run keeps its progress.
        state.save(root)?;
    }

    let local = plan.distribution_path.as_path();
    let relative = distribution_relative(root, local);

    // Signed from the bytes on disk right before they are uploaded.
    let (signature, body) = signing::sign_file(key, local)?;
    let digests = hash::digests_of_bytes(&body);
    log::info!("Uploading {relative} (serial {serial}) to {name}");
    let distribution_url = target.upload(local, &relative, &digests)?;
    uploaded += 1;
    state.record(&name, &relative, &digests.sha256);
    state.save(root)?;

    let signature_relative = format!("{relative}.sig");
    let signature_digests = hash::digests_of_file(&signature)?;
    log::info!("Uploading {signature_relative} to {name}");
    let signature_url = target.upload(&signature, &signature_relative, &signature_digests)?;
    uploaded += 1;
    state.record(&name, &signature_relative, &signature_digests.sha256);
    state.save(root)?;
    log::info!("Signed with public key {}", key.public_key_hex());

    Ok(PublishReport {
        uploaded,
        skipped,
        distribution_url,
        signature_url,
    })
}

/// The key a publish signs with, or the refusal [`publish`] gives without one.
/// Callers may run it first, before they set up a target, so the missing key
/// is the error a user sees rather than a missing upload setting.
pub fn require_signing_key(key: Option<&DistributionKey>) -> Result<&DistributionKey> {
    key.ok_or_else(|| {
        Error::Config(format!(
            "publish refused: {NO_SIGNING_KEY}, and the Hambugui launcher refuses an unsigned \
             distribution. Create one with `nebula keygen`, or set {} or {}",
            signing::KEY_ENV,
            signing::KEY_FILE_ENV
        ))
    })
}

/// The distribution's `serial`, which must be there: a launcher compares it to
/// refuse an older copy, and only a file Nebula generated carries one.
fn require_serial(path: &Path) -> Result<u64> {
    let raw = std::fs::read(path).map_err(io(format!("reading {}", path.display())))?;
    let value: serde_json::Value = serde_json::from_slice(&raw)?;
    value
        .get("serial")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "publish refused: {} has no serial (an unsigned 64-bit integer), so a launcher \
                 could not tell it from an older copy. Regenerate it with `nebula generate distro`",
                path.display()
            ))
        })
}

/// Where the distribution lands on the target: its path below `<root>` when it
/// lives there, otherwise the bare file name at the upload root.
fn distribution_relative(root: &Root, path: &Path) -> String {
    path.strip_prefix(&root.path)
        .ok()
        .and_then(Path::to_str)
        .map(|relative| relative.replace('\\', "/"))
        .unwrap_or_else(|| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

pub(crate) fn env_var(key: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("{key} is not set")))
}

/// No overall timeout: an upload is as long as the file is. `reqwest`'s
/// blocking default of 30 s would kill every mod pack.
pub(crate) fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("nebula-rust/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .timeout(None::<Duration>)
        .build()
        .map_err(|e| Error::Config(format!("could not build the HTTP client: {e}")))
}

/// Percent-encodes exactly like [`Root::url`]. Reused rather than
/// reimplemented: an upload path that drifts from the generated URL is a 404
/// nobody notices until a player launches the game.
pub(crate) fn join_url(base: &url::Url, relative: &str) -> String {
    Root::new(PathBuf::new(), base.clone()).url(relative)
}

/// Enough of an error body to identify it, not enough to flood a log.
pub(crate) fn excerpt(body: &str) -> String {
    let trimmed = body.trim();
    let mut short: String = trimmed.chars().take(200).collect();
    if short.len() < trimmed.len() {
        short.push('…');
    }
    short
}

pub(crate) fn http(url: &str, message: impl std::fmt::Display) -> Error {
    Error::Http {
        url: url.to_owned(),
        message: message.to_string(),
    }
}

/// Turns a non-2xx response into `Error::Http` carrying status and body.
pub(crate) fn check(
    response: reqwest::blocking::Response,
    url: &str,
) -> Result<reqwest::blocking::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().unwrap_or_default();
    Err(http(url, format!("{status} {}", excerpt(&body))))
}

/// `application/x-www-form-urlencoded`. Built through the `url` crate's
/// serializer because reqwest's `form` and `query` helpers are behind features
/// this crate does not enable.
pub(crate) fn urlencode(pairs: &[(&str, &str)]) -> String {
    let mut url = url::Url::parse("http://form.invalid/").expect("constant is a valid URL");
    url.query_pairs_mut().extend_pairs(pairs.iter().copied());
    url.query().unwrap_or_default().to_owned()
}

pub(crate) fn with_query(endpoint: &str, pairs: &[(&str, &str)]) -> String {
    format!("{endpoint}?{}", urlencode(pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_root_url_encodes() {
        assert_eq!(percent_decode("a%20b%231.cfg"), "a b#1.cfg");
        assert_eq!(percent_decode("%ED%95%9C%EA%B8%80.cfg"), "한글.cfg");
        assert_eq!(percent_decode("plain.cfg"), "plain.cfg");
        assert_eq!(percent_decode("a%2fb"), "a/b");
    }

    #[test]
    fn keeps_malformed_escapes_verbatim() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("50%zz"), "50%zz");
        assert_eq!(percent_decode("%2"), "%2");
    }

    #[test]
    fn shortens_long_error_bodies() {
        assert_eq!(excerpt("  boom  "), "boom");
        let long = "x".repeat(500);
        let short = excerpt(&long);
        assert_eq!(short.chars().count(), 201);
        assert!(short.ends_with('…'));
    }

    #[test]
    fn encodes_form_bodies() {
        assert_eq!(
            urlencode(&[("grant_type", "authorization_code"), ("code", "a b&c")]),
            "grant_type=authorization_code&code=a+b%26c"
        );
    }
}
