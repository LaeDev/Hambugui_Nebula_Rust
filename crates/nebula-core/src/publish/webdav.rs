//! WebDAV upload target, written for Synology DSM's WebDAV Server package but
//! plain enough for any RFC 4918 server.
//!
//! The public URLs are the ones already in the distribution — the static layout
//! under `<root>` is mirrored one to one — so this target never rewrites a
//! distribution. That is the whole point of hosting on a NAS: what Nebula
//! generates is what the launcher downloads.

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

use reqwest::blocking::{Body, Client};
use reqwest::{Method, StatusCode};
use url::Url;

use super::{Target, build_client, check, env_var, http, join_url};
use crate::error::{Error, Result, io};
use crate::hash::Digests;

pub struct WebDavTarget {
    upload_base: Url,
    public_base: Url,
    user: String,
    password: String,
    client: Client,
    /// Collections this run has already created, relative to `upload_base`.
    created: HashSet<String>,
}

impl WebDavTarget {
    pub fn new(upload_base: Url, user: String, password: String, public_base: Url) -> Result<Self> {
        Ok(Self {
            upload_base,
            public_base,
            user,
            password,
            client: build_client()?,
            created: HashSet::new(),
        })
    }

    /// `NEBULA_WEBDAV_URL`, `NEBULA_WEBDAV_USER`, `NEBULA_WEBDAV_PASSWORD` and
    /// the optional `NEBULA_WEBDAV_PUBLIC_URL`, which defaults to `base_url`
    /// because the served layout matches the uploaded one.
    pub fn from_env(base_url: &Url) -> Result<Self> {
        let upload_base = parse("NEBULA_WEBDAV_URL", &env_var("NEBULA_WEBDAV_URL")?)?;
        let public_base = match std::env::var("NEBULA_WEBDAV_PUBLIC_URL") {
            Ok(raw) if !raw.is_empty() => parse("NEBULA_WEBDAV_PUBLIC_URL", &raw)?,
            _ => base_url.clone(),
        };
        Self::new(
            upload_base,
            env_var("NEBULA_WEBDAV_USER")?,
            env_var("NEBULA_WEBDAV_PASSWORD")?,
            public_base,
        )
    }

    /// `MKCOL` every ancestor of `relative`, top down, once per run. The base
    /// itself is assumed to exist: it is the share the account was pointed at.
    fn ensure_collections(&mut self, relative: &str) -> Result<()> {
        let segments: Vec<&str> = relative
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty())
            .collect();

        for depth in 1..segments.len() {
            let dir = segments[..depth].join("/");
            if !self.created.insert(dir.clone()) {
                continue;
            }
            if self.mkcol(&dir)? {
                continue;
            }
            // 409: the collection above `dir` is missing. Walking top down
            // already created everything below the base, so retry once after
            // re-creating the parent and then give up with a useful message.
            if depth > 1 {
                self.mkcol(&segments[..depth - 1].join("/"))?;
            }
            if !self.mkcol(&dir)? {
                return Err(http(
                    &join_url(&self.upload_base, &dir),
                    format!(
                        "{} the parent collection is missing; create {} on the server first",
                        StatusCode::CONFLICT,
                        self.upload_base
                    ),
                ));
            }
        }
        Ok(())
    }

    /// `Ok(true)` when the collection is there afterwards, `Ok(false)` on 409.
    fn mkcol(&self, dir: &str) -> Result<bool> {
        let url = join_url(&self.upload_base, dir);
        let method = Method::from_bytes(b"MKCOL").expect("MKCOL is a valid method name");
        let response = self
            .client
            .request(method, &url)
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .map_err(|e| http(&url, e))?;

        match response.status() {
            // 405 is how a server says the collection is already there.
            StatusCode::METHOD_NOT_ALLOWED => Ok(true),
            StatusCode::CONFLICT => Ok(false),
            status if status.is_success() => Ok(true),
            _ => check(response, &url).map(|_| true),
        }
    }

    fn put(&self, local: &Path, relative: &str, digests: &Digests) -> Result<()> {
        let url = join_url(&self.upload_base, relative);
        let file = File::open(local).map_err(io(format!("opening {}", local.display())))?;
        // Sized so the request carries Content-Length: DSM rejects chunked PUT.
        let response = self
            .client
            .put(&url)
            .basic_auth(&self.user, Some(&self.password))
            .body(Body::sized(file, digests.size))
            .send()
            .map_err(|e| http(&url, e))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => check(response, &url).map(|_| ()),
        }
    }
}

impl Target for WebDavTarget {
    fn name(&self) -> &str {
        "webdav"
    }

    fn upload(&mut self, local: &Path, relative: &str, digests: &Digests) -> Result<String> {
        self.ensure_collections(relative)?;
        self.put(local, relative, digests)?;
        Ok(join_url(&self.public_base, relative))
    }
}

fn parse(key: &str, raw: &str) -> Result<Url> {
    Url::parse(raw).map_err(|e| Error::Config(format!("{key} {raw} is not a valid URL: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(upload: &str, public: &str) -> WebDavTarget {
        WebDavTarget::new(
            Url::parse(upload).unwrap(),
            "user".into(),
            "secret".into(),
            Url::parse(public).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn publishes_the_url_the_distribution_already_carries() {
        let target = target(
            "https://nas.invalid:5006/nebula-dist/",
            "https://cdn.invalid/",
        );
        assert_eq!(
            join_url(&target.public_base, "servers/S-1.20.1/files/a b.cfg"),
            "https://cdn.invalid/servers/S-1.20.1/files/a%20b.cfg"
        );
        assert_eq!(
            join_url(&target.upload_base, "servers/S-1.20.1/files/a b.cfg"),
            "https://nas.invalid:5006/nebula-dist/servers/S-1.20.1/files/a%20b.cfg"
        );
    }

    #[test]
    fn rejects_a_malformed_upload_url() {
        assert!(parse("NEBULA_WEBDAV_URL", "nas.invalid/nebula").is_err());
    }
}
