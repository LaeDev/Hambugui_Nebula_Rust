use std::env;
use std::path::{Path, PathBuf};
use std::sync::Once;

use url::Url;

use crate::error::{Error, Result, io};

static DOTENV: Once = Once::new();

#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub base_url: Url,
    pub java_executable: Option<PathBuf>,
    pub helios_data_folder: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        DOTENV.call_once(|| {
            dotenvy::dotenv().ok();
        });

        Ok(Self {
            root: absolute(&required("ROOT")?)?,
            base_url: Self::normalize_base_url(&required("BASE_URL")?)?,
            // Left verbatim so a bare `java` on PATH keeps working.
            java_executable: optional("JAVA_EXECUTABLE").map(PathBuf::from),
            helios_data_folder: optional("HELIOS_DATA_FOLDER")
                .map(|v| absolute(&v))
                .transpose()?,
        })
    }

    pub fn normalize_base_url(raw: &str) -> Result<Url> {
        let mut candidate = raw.to_owned();
        if !candidate.contains("//") {
            if candidate.to_lowercase().starts_with("localhost") {
                candidate.insert_str(0, "http://");
            } else {
                return Err(Error::Config(
                    "Please provide a URL protocol (ex. http:// or https://)".into(),
                ));
            }
        }

        let mut url = Url::parse(&candidate)
            .map_err(|e| Error::Config(format!("BASE_URL {raw} is not a valid URL: {e}")))?;
        if url.cannot_be_a_base() {
            return Err(Error::Config(format!(
                "BASE_URL {raw} must be a hierarchical URL"
            )));
        }
        // Upstream let `https://host/dist` silently drop `dist` when joined.
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(url)
    }

    pub fn java(&self) -> Result<&Path> {
        self.java_executable
            .as_deref()
            .ok_or_else(|| Error::Config("JAVA_EXECUTABLE is not set".into()))
    }
}

fn required(key: &str) -> Result<String> {
    optional(key).ok_or_else(|| Error::Config(format!("{key} is not set")))
}

fn optional(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

fn absolute(value: &str) -> Result<PathBuf> {
    std::path::absolute(value).map_err(io(format!("resolving {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_localhost_without_protocol() {
        let url = Config::normalize_base_url("LocalHost:8080").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8080/");
    }

    #[test]
    fn forces_a_trailing_slash() {
        let url = Config::normalize_base_url("https://host.example/dist").unwrap();
        assert_eq!(url.as_str(), "https://host.example/dist/");
        let already = Config::normalize_base_url("https://host.example/dist/").unwrap();
        assert_eq!(already.as_str(), "https://host.example/dist/");
    }

    #[test]
    fn rejects_a_missing_protocol() {
        let err = Config::normalize_base_url("cdn.example.invalid").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Please provide a URL protocol (ex. http:// or https://)"
        );
    }

    #[test]
    fn rejects_a_non_hierarchical_url() {
        assert!(Config::normalize_base_url("data:text//plain,hello").is_err());
    }
}
