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
    /// **Call this once on the main thread before starting any worker.**
    /// The first call runs `dotenvy::dotenv()`, which writes to the process
    /// environment; in edition 2024 that is `unsafe` precisely because another
    /// thread may be reading the environment at the same time. The `Once` makes
    /// later calls cheap and thread safe, not the first one.
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
                // This one reaches a settings field, so it speaks the language
                // the rest of the form does (DESIGN-v2 §5).
                log::debug!("BASE_URL {raw} has no URL protocol (ex. http:// or https://)");
                return Err(Error::Config(
                    "기본 URL에 프로토콜이 없습니다. https:// 로 시작하는 주소를 입력하세요".into(),
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
            .ok_or_else(|| {
                Error::Config(
                    "JAVA_EXECUTABLE이 설정되지 않았습니다 — Forge/NeoForge 서버를 처리하려면 \
                     설정 화면에서 Java 실행 파일 경로를 지정하세요 (예: C:\\Program Files\\Java\\jdk-21\\bin\\java.exe)"
                        .into(),
                )
            })
    }
}

fn required(key: &str) -> Result<String> {
    optional(key).ok_or_else(|| Error::Config(missing(key)))
}

/// The two values nothing works without reach the GUI as a failed `to_config`,
/// so they name the fix as well as the cause.
fn missing(key: &str) -> String {
    match key {
        "ROOT" => "ROOT가 비어 있습니다 — 배포본을 만들 루트 폴더를 설정 화면의 루트에 입력하세요 \
                   (.env를 쓴다면 ROOT=D:\\NebulaRoot)"
            .to_owned(),
        "BASE_URL" => "BASE_URL이 비어 있습니다 — 배포본이 올라갈 주소를 설정 화면의 기본 URL에 \
                       입력하세요 (예: https://cdn.example.com/)"
            .to_owned(),
        other => format!("{other} 값이 비어 있습니다 — .env 또는 설정 화면에서 값을 입력하세요"),
    }
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
            "기본 URL에 프로토콜이 없습니다. https:// 로 시작하는 주소를 입력하세요"
        );
    }

    #[test]
    fn rejects_a_non_hierarchical_url() {
        assert!(Config::normalize_base_url("data:text//plain,hello").is_err());
    }
}
