//! Google Drive upload target.
//!
//! Drive is not a CDN and this is the fallback host, not the recommended one —
//! see `SYNOLOGY_DEPLOY_GUIDE.md` and the README caveats. Because the public
//! URLs are file ids rather than paths, this target only works together with
//! [`super::PublishOptions::rewrite_urls`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::{Body, Client};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{Target, build_client, check, env_var, excerpt, http, urlencode, with_query};
use crate::error::{Error, Result, io};
use crate::hash::Digests;

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
/// Per-file scope: Nebula only ever sees the files it created itself.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

const FILES_ENDPOINT: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_ENDPOINT: &str = "https://www.googleapis.com/upload/drive/v3/files";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const FILE_FIELDS: &str = "files(id,md5Checksum,permissions(type,role))";

const DONE_PAGE: &str = concat!(
    "<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><title>Nebula</title>",
    "<body style=\"font-family:sans-serif;padding:2rem\">",
    "<h1>인증이 완료되었습니다.</h1><p>이 창을 닫아도 됩니다.</p></body></html>"
);
const FAILED_PAGE: &str = concat!(
    "<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><title>Nebula</title>",
    "<body style=\"font-family:sans-serif;padding:2rem\">",
    "<h1>인증에 실패했습니다.</h1><p>터미널의 오류 메시지를 확인하세요.</p></body></html>"
);

#[derive(Debug, Clone)]
pub struct GoogleDriveConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Drive id of the folder the mirrored `<root>` tree is created under.
    pub folder_id: String,
    pub token_path: PathBuf,
}

/// `NEBULA_GDRIVE_CLIENT_ID`, `NEBULA_GDRIVE_CLIENT_SECRET` and
/// `NEBULA_GDRIVE_FOLDER_ID`; the refresh token lands in `token_dir`.
pub fn config_from_env(token_dir: &Path) -> Result<GoogleDriveConfig> {
    Ok(GoogleDriveConfig {
        client_id: env_var("NEBULA_GDRIVE_CLIENT_ID")?,
        client_secret: env_var("NEBULA_GDRIVE_CLIENT_SECRET")?,
        folder_id: env_var("NEBULA_GDRIVE_FOLDER_ID")?,
        token_path: token_dir.join("gdrive-token.json"),
    })
}

/// The consent URL for the installed-app flow.
pub fn consent_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    with_query(
        AUTH_ENDPOINT,
        &[
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", SCOPE),
            // Without both of these Google hands back an access token only, and
            // the next publish would need another browser round trip.
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state),
        ],
    )
}

/// Drive's query language quotes values in single quotes and escapes with a
/// backslash. A mod named `Tinkers' Construct.jar` is not hypothetical.
pub fn escape_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Splits a relative path into the folders to create and the file name.
pub fn folder_chain(relative: &str) -> (Vec<&str>, &str) {
    let mut segments: Vec<&str> = relative
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect();
    let name = segments.pop().unwrap_or("");
    (segments, name)
}

/// The direct-download form. `drive.google.com/uc?...` answers with an HTML
/// virus-scan page above 100 MB and Hambugui's download client follows
/// redirects but cannot tell HTML from a jar, so `confirm=t` on
/// `drive.usercontent.google.com` is the only form that survives.
pub fn public_url(file_id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm=t")
}

/// Runs the OAuth 2.0 installed-app flow and stores the refresh token.
///
/// `open_browser` is injected so the GUI can use its own opener; the URL is
/// also logged so a headless user can paste it.
pub fn authorize(
    config: &GoogleDriveConfig,
    open_browser: &dyn Fn(&str) -> Result<()>,
) -> Result<()> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(io("binding the OAuth loopback listener"))?;
    let port = listener
        .local_addr()
        .map_err(io("reading the OAuth loopback port"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let state = random_state();
    let url = consent_url(&config.client_id, &redirect_uri, &state);

    log::info!("Google 계정 연결: 브라우저에서 아래 주소를 열어 승인하세요.");
    log::info!("{url}");
    open_browser(&url)?;

    let code = accept_code(&listener, &state)?;
    let client = build_client()?;
    let response: TokenResponse = post_form(
        &client,
        TOKEN_ENDPOINT,
        &[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ],
    )?;

    let refresh_token = response.refresh_token.ok_or_else(|| {
        Error::Invalid(
            "Google returned no refresh token. Remove Nebula at \
             https://myaccount.google.com/permissions and authorise again."
                .into(),
        )
    })?;
    store_token(
        &config.token_path,
        &StoredToken {
            refresh_token,
            access_token: response.access_token,
            expires_at: expires_at(response.expires_in),
        },
    )
}

/// Opens `url` in the platform browser.
#[cfg(windows)]
pub fn default_open_browser(url: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;

    check_url(url)?;
    // `cmd` does not parse the quoting `Command` applies and a consent URL is
    // full of `&`, so the command line is written verbatim.
    run(Command::new("cmd").raw_arg(format!("/C start \"\" \"{url}\"")))
}

/// Opens `url` in the platform browser.
#[cfg(not(windows))]
pub fn default_open_browser(url: &str) -> Result<()> {
    check_url(url)?;
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    run(Command::new(opener).arg(url))
}

fn check_url(url: &str) -> Result<()> {
    if url.starts_with("https://") && !url.contains(['"', '\n', '\r']) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("refusing to open {url}")))
    }
}

fn run(command: &mut Command) -> Result<()> {
    let status = command.status().map_err(io("starting the browser"))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Tool {
            tool: "browser".into(),
            code: status.code(),
        })
    }
}

pub struct GoogleDriveTarget {
    config: GoogleDriveConfig,
    client: Client,
    token: Option<StoredToken>,
    /// Relative folder path -> Drive id, for this run.
    folders: HashMap<String, String>,
}

impl GoogleDriveTarget {
    pub fn new(config: GoogleDriveConfig) -> Result<Self> {
        Ok(Self {
            config,
            client: build_client()?,
            token: None,
            folders: HashMap::new(),
        })
    }

    /// A valid access token, refreshed when it expires within a minute.
    fn access_token(&mut self) -> Result<String> {
        if self.token.is_none() {
            self.token = Some(load_token(&self.config.token_path)?);
        }
        let current = self.token.as_ref().expect("just loaded").clone();
        if current.expires_at > unix_now() + 60 {
            return Ok(current.access_token);
        }

        log::debug!("Refreshing the Google Drive access token");
        let response: TokenResponse = post_form(
            &self.client,
            TOKEN_ENDPOINT,
            &[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("refresh_token", current.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ],
        )?;
        let refreshed = StoredToken {
            // A refresh response usually omits the refresh token; keep ours.
            refresh_token: response.refresh_token.unwrap_or(current.refresh_token),
            access_token: response.access_token,
            expires_at: expires_at(response.expires_in),
        };
        store_token(&self.config.token_path, &refreshed)?;
        let access_token = refreshed.access_token.clone();
        self.token = Some(refreshed);
        Ok(access_token)
    }

    fn folder(&mut self, dirs: &[&str]) -> Result<String> {
        let mut parent = self.config.folder_id.clone();
        let mut walked = String::new();
        for dir in dirs {
            if !walked.is_empty() {
                walked.push('/');
            }
            walked.push_str(dir);
            if let Some(known) = self.folders.get(&walked) {
                parent = known.clone();
                continue;
            }
            let id = match self.find(&parent, dir, true)? {
                Some(found) => found.id,
                None => self.create_folder(&parent, dir)?,
            };
            self.folders.insert(walked.clone(), id.clone());
            parent = id;
        }
        Ok(parent)
    }

    fn find(&mut self, parent: &str, name: &str, folder: bool) -> Result<Option<DriveFile>> {
        let token = self.access_token()?;
        let mut query = format!(
            "name='{}' and '{}' in parents and trashed=false",
            escape_query_value(name),
            escape_query_value(parent)
        );
        if folder {
            query.push_str(&format!(" and mimeType='{FOLDER_MIME}'"));
        }
        let url = with_query(
            FILES_ENDPOINT,
            &[
                ("q", query.as_str()),
                ("fields", FILE_FIELDS),
                ("pageSize", "10"),
            ],
        );

        let response = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .map_err(|e| http(&url, e))?;
        let list: FileList = check(response, &url)?.json().map_err(|e| http(&url, e))?;
        Ok(list.files.into_iter().next())
    }

    fn create_folder(&mut self, parent: &str, name: &str) -> Result<String> {
        let token = self.access_token()?;
        let body = serde_json::json!({
            "name": name,
            "mimeType": FOLDER_MIME,
            "parents": [parent],
        });
        let response = self
            .client
            .post(FILES_ENDPOINT)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .map_err(|e| http(FILES_ENDPOINT, e))?;
        let created: DriveFile = check(response, FILES_ENDPOINT)?
            .json()
            .map_err(|e| http(FILES_ENDPOINT, e))?;
        Ok(created.id)
    }

    /// Resumable upload: initiate the session, then send the body in one PUT.
    fn upload_body(
        &mut self,
        local: &Path,
        name: &str,
        parent: &str,
        existing: Option<&str>,
        digests: &Digests,
    ) -> Result<String> {
        let token = self.access_token()?;
        let (method, url, metadata) = match existing {
            // A patch keeps the file id, so published URLs stay valid.
            Some(id) => (
                Method::PATCH,
                format!("{UPLOAD_ENDPOINT}/{id}?uploadType=resumable"),
                serde_json::json!({ "name": name }),
            ),
            None => (
                Method::POST,
                format!("{UPLOAD_ENDPOINT}?uploadType=resumable"),
                serde_json::json!({ "name": name, "parents": [parent] }),
            ),
        };

        let response = self
            .client
            .request(method, &url)
            .bearer_auth(&token)
            .header("X-Upload-Content-Length", digests.size.to_string())
            .json(&metadata)
            .send()
            .map_err(|e| http(&url, e))?;
        let response = check(response, &url)?;
        let session = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| http(&url, "the resumable upload response carried no Location"))?
            .to_owned();

        let file = File::open(local).map_err(io(format!("opening {}", local.display())))?;
        let response = self
            .client
            .put(&session)
            .bearer_auth(&token)
            .body(Body::sized(file, digests.size))
            .send()
            .map_err(|e| http(&session, e))?;
        let uploaded: DriveFile = check(response, &session)?
            .json()
            .map_err(|e| http(&session, e))?;
        Ok(uploaded.id)
    }

    fn share(&mut self, id: &str) -> Result<()> {
        let token = self.access_token()?;
        let url = format!("{FILES_ENDPOINT}/{id}/permissions");
        let response = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "role": "reader", "type": "anyone" }))
            .send()
            .map_err(|e| http(&url, e))?;

        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().unwrap_or_default();
        // Drive answers 400/403 when the permission is already there or when
        // the account's domain policy forbids changing it; neither is fatal.
        if status == StatusCode::BAD_REQUEST || status == StatusCode::FORBIDDEN {
            log::warn!("Could not share {id} ({status}): {}", excerpt(&body));
            return Ok(());
        }
        Err(http(&url, format!("{status} {}", excerpt(&body))))
    }
}

impl Target for GoogleDriveTarget {
    fn name(&self) -> &str {
        "gdrive"
    }

    fn upload(&mut self, local: &Path, relative: &str, digests: &Digests) -> Result<String> {
        let (dirs, name) = folder_chain(relative);
        if name.is_empty() {
            return Err(Error::Invalid(format!("{relative} has no file name")));
        }
        let parent = self.folder(&dirs)?;
        let existing = self.find(&parent, name, false)?;

        if let Some(found) = &existing
            && found.md5_checksum.as_deref() == Some(digests.md5.as_str())
        {
            log::debug!("Drive already holds {relative}");
            // A run that died between the upload and the permission would
            // otherwise leave a file nobody can download. Sharing is idempotent
            // and cheap; never let this decide whether to re-send the bytes.
            let id = found.id.clone();
            if !found.is_public() {
                self.share(&id)?;
            }
            return Ok(public_url(&id));
        }

        let id = self.upload_body(
            local,
            name,
            &parent,
            existing.as_ref().map(|found| found.id.as_str()),
            digests,
        )?;
        self.share(&id)?;
        Ok(public_url(&id))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StoredToken {
    refresh_token: String,
    access_token: String,
    /// Unix seconds.
    expires_at: u64,
}

#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
struct DrivePermission {
    #[serde(default, rename = "type")]
    permission_type: String,
    #[serde(default)]
    role: String,
}

#[derive(Deserialize, Debug, Clone)]
struct DriveFile {
    id: String,
    #[serde(default, rename = "md5Checksum")]
    md5_checksum: Option<String>,
    /// Asked for in the same lookup, so an unchanged file costs no extra call
    /// to find out whether an interrupted run left it unshared.
    #[serde(default)]
    permissions: Vec<DrivePermission>,
}

impl DriveFile {
    fn is_public(&self) -> bool {
        self.permissions
            .iter()
            .any(|permission| permission.permission_type == "anyone" && permission.role == "reader")
    }
}

#[derive(Deserialize, Debug)]
struct FileList {
    #[serde(default)]
    files: Vec<DriveFile>,
}

fn post_form<T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
    pairs: &[(&str, &str)],
) -> Result<T> {
    let response = client
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(urlencode(pairs))
        .send()
        .map_err(|e| http(url, e))?;
    check(response, url)?.json().map_err(|e| http(url, e))
}

/// Serves exactly one redirect, then hands back the authorisation code.
fn accept_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    let (stream, _) = listener
        .accept()
        .map_err(io("accepting the OAuth redirect"))?;
    let mut writer = stream
        .try_clone()
        .map_err(io("reading the OAuth redirect"))?;
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(io("reading the OAuth redirect"))?;
    // Drain the headers so the browser gets a clean response, not a reset.
    loop {
        let mut header = String::new();
        let read = reader
            .read_line(&mut header)
            .map_err(io("reading the OAuth redirect"))?;
        if read == 0 || header.trim().is_empty() {
            break;
        }
    }

    let outcome = read_code(&request_line, expected_state);
    let page = if outcome.is_ok() {
        DONE_PAGE
    } else {
        FAILED_PAGE
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();
    outcome
}

fn read_code(request_line: &str, expected_state: &str) -> Result<String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let parsed = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|e| Error::Invalid(format!("unreadable OAuth redirect {target}: {e}")))?;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }

    if let Some(error) = error {
        return Err(Error::Invalid(format!(
            "Google refused the request: {error}"
        )));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(Error::Invalid(
            "the OAuth redirect carried the wrong state; the request did not come from this run"
                .into(),
        ));
    }
    code.ok_or_else(|| Error::Invalid("the OAuth redirect carried no code".into()))
}

/// Loopback CSRF nonce. `RandomState` is seeded by the OS, which is all this
/// needs: the value lives for one browser round trip and guards nothing else.
fn random_state() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(unix_now());
    let high = hasher.finish();
    let low = RandomState::new().build_hasher().finish();
    format!("{high:016x}{low:016x}")
}

fn load_token(path: &Path) -> Result<StoredToken> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Config(format!(
                "no Google Drive token at {}; connect the account first",
                path.display()
            ))
        } else {
            Error::Io {
                context: format!("reading {}", path.display()),
                source: e,
            }
        }
    })?;
    Ok(serde_json::from_str(&raw)?)
}

fn store_token(path: &Path, token: &StoredToken) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let body = serde_json::to_string_pretty(token)?;
    std::fs::write(path, body).map_err(io(format!("writing {}", path.display())))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(io(format!("restricting {}", path.display())))?;
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

fn expires_at(expires_in: Option<u64>) -> u64 {
    unix_now() + expires_in.unwrap_or(3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_code_from_the_redirect() {
        let code = read_code("GET /?state=abc&code=4%2F0Ax7 HTTP/1.1", "abc").unwrap();
        assert_eq!(code, "4/0Ax7");
    }

    #[test]
    fn refuses_a_redirect_that_is_not_ours() {
        assert!(read_code("GET /?state=other&code=x HTTP/1.1", "abc").is_err());
        assert!(read_code("GET /?state=abc HTTP/1.1", "abc").is_err());
        let denied = read_code("GET /?state=abc&error=access_denied HTTP/1.1", "abc").unwrap_err();
        assert!(denied.to_string().contains("access_denied"));
    }

    #[test]
    fn produces_a_fresh_state_each_time() {
        let first = random_state();
        assert_eq!(first.len(), 32);
        assert_ne!(first, random_state());
    }

    #[test]
    fn only_opens_https_urls() {
        assert!(check_url("https://accounts.google.com/o/oauth2/v2/auth?a=b&c=d").is_ok());
        assert!(check_url("file:///etc/passwd").is_err());
        assert!(check_url("https://host/\" & calc.exe").is_err());
    }

    #[test]
    fn treats_a_file_as_public_only_when_anyone_can_read_it() {
        let shared: DriveFile =
            serde_json::from_str(r#"{"id":"a","permissions":[{"type":"anyone","role":"reader"}]}"#)
                .unwrap();
        assert!(shared.is_public());
        let private: DriveFile =
            serde_json::from_str(r#"{"id":"a","permissions":[{"type":"user","role":"owner"}]}"#)
                .unwrap();
        assert!(!private.is_public());
        let unknown: DriveFile = serde_json::from_str(r#"{"id":"a"}"#).unwrap();
        assert!(!unknown.is_public());
    }
}
