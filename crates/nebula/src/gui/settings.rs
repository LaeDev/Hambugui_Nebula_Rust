//! The GUI's own `settings.json`, which stands in for the CLI's `.env`.
//!
//! Every value is stored as a string exactly as typed so a half-finished
//! setting survives a restart; [`GuiSettings::to_config`] is the only place a
//! `Config` is made, and it fails loudly when the two required values are
//! missing.

use std::path::{Path, PathBuf};

use nebula_core::publish::gdrive::GoogleDriveConfig;
use nebula_core::publish::webdav::WebDavTarget;
use nebula_core::{Config, Error, Result};
use serde::{Deserialize, Serialize};

/// Where Nebula keeps per-user state: the GUI's `settings.json` and the Google
/// Drive refresh token, for the CLI as much as for the GUI.
/// `NEBULA_CONFIG_DIR` overrides it outright (tests, portable installs); it is
/// used as given, without the `nebula` suffix.
pub fn config_dir() -> PathBuf {
    if let Some(over) = std::env::var_os("NEBULA_CONFIG_DIR") {
        return PathBuf::from(over);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("nebula")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn gdrive_token_path() -> PathBuf {
    config_dir().join("gdrive-token.json")
}

/// Where `nebula keygen` writes the distribution signing key by default, and
/// where the CLI and the GUI look for it when no other key is named.
pub fn signing_key_path() -> PathBuf {
    config_dir().join(nebula_core::signing::DEFAULT_KEY_FILE_NAME)
}

/// The key to sign distributions with: `explicit` (a command-line key file),
/// then the environment, then [`signing_key_path`] if it exists. Logs where it
/// came from and the public key, never the key itself.
pub fn resolve_signing_key(explicit: Option<&Path>) -> Result<Option<nebula_core::ResolvedKey>> {
    let resolved = nebula_core::signing::resolve_key(explicit, Some(&signing_key_path()))?;
    if let Some(resolved) = &resolved {
        log::info!(
            "Signing with {} (public key {})",
            resolved.source,
            resolved.key.public_key_hex()
        );
    }
    Ok(resolved)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GuiSettings {
    pub root: String,
    pub base_url: String,
    pub java_executable: String,
    pub helios_data_folder: String,
    /// `system`, `light` or `dark`; anything else reads as `system`.
    pub theme: String,
    pub reduced_motion: bool,

    pub webdav_url: String,
    pub webdav_user: String,
    pub webdav_password: String,
    pub webdav_public_url: String,

    pub gdrive_client_id: String,
    pub gdrive_client_secret: String,
    pub gdrive_folder_id: String,

    /// The user's own CurseForge key (D2). A form field; the environment
    /// variable `NEBULA_CURSEFORGE_API_KEY` still wins over it, which is why
    /// `ModsData.has-cf-key` is the host's answer and not this field's length.
    pub curseforge_api_key: String,
    /// Which server the 모드 page was last looking at (DESIGN-v2 §8.2).
    /// **Not** a settings-form field: it is written when the pill strip
    /// changes, and `form_from_ui` carries it over so a 저장 cannot wipe it.
    pub last_mods_server: String,
}

impl GuiSettings {
    /// The stored settings, or a seed from the environment when there is no
    /// file yet. Never fails: a first run with neither is simply empty.
    pub fn load() -> Self {
        let path = settings_path();
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(settings) => settings,
                Err(error) => {
                    log::warn!(
                        "{} 을(를) 읽지 못해 환경 변수로 시작합니다: {error}",
                        path.display()
                    );
                    Self::from_env()
                }
            },
            Err(_) => Self::from_env(),
        }
    }

    /// Seeded from `Config::from_env()` (which also reads `.env`), so a user
    /// who already ran the CLI sees their root on the first launch.
    pub fn from_env() -> Self {
        let mut settings = Self {
            theme: "system".into(),
            ..Self::default()
        };
        if let Ok(cfg) = Config::from_env() {
            settings.root = cfg.root.display().to_string();
            settings.base_url = cfg.base_url.to_string();
            settings.java_executable = cfg
                .java_executable
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            settings.helios_data_folder = cfg
                .helios_data_folder
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }
        settings
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                context: format!("creating {}", parent.display()),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, body).map_err(|source| Error::Io {
            context: format!("writing {}", path.display()),
            source,
        })
    }

    /// The root and the base URL are the two values nothing works without.
    pub fn to_config(&self) -> Result<Config> {
        let root =
            trimmed(&self.root).ok_or_else(|| Error::Config("루트 폴더를 설정하세요".into()))?;
        let base_url =
            trimmed(&self.base_url).ok_or_else(|| Error::Config("기본 URL을 설정하세요".into()))?;
        Ok(Config {
            root: std::path::absolute(root).map_err(|source| Error::Io {
                context: format!("resolving {root}"),
                source,
            })?,
            base_url: Config::normalize_base_url(base_url)?,
            // Left verbatim so a bare `java` on PATH keeps working.
            java_executable: trimmed(&self.java_executable).map(PathBuf::from),
            helios_data_folder: trimmed(&self.helios_data_folder).map(PathBuf::from),
            // Only the stored field. `NEBULA_CURSEFORGE_API_KEY` wins over it,
            // but that rule lives in one place — `gui::mods::cf_key` — rather
            // than being applied twice with two chances to diverge.
            curseforge_api_key: trimmed(&self.curseforge_api_key).map(str::to_owned),
        })
    }

    /// Built from the stored fields rather than `from_env`, so the GUI never
    /// mutates the process environment while worker threads are running.
    /// `cfg.base_url` is the public base when no separate one is set.
    pub fn webdav_target(&self, cfg: &Config) -> Result<WebDavTarget> {
        let upload = trimmed(&self.webdav_url).ok_or_else(|| {
            Error::Config("WebDAV 업로드 URL이 비어 있습니다 — 설정에서 입력하세요".into())
        })?;
        // Both get the base URL's normalisation: a protocol is required and the
        // path always ends in `/`, because both are joined onto.
        let public = match trimmed(&self.webdav_public_url) {
            Some(raw) => Config::normalize_base_url(raw)
                .map_err(|error| Error::Config(format!("WebDAV 공개 URL {raw}: {error}")))?,
            None => cfg.base_url.clone(),
        };
        WebDavTarget::new(
            Config::normalize_base_url(upload)
                .map_err(|error| Error::Config(format!("WebDAV 업로드 URL {upload}: {error}")))?,
            trimmed(&self.webdav_user).unwrap_or_default().to_owned(),
            trimmed(&self.webdav_password)
                .unwrap_or_default()
                .to_owned(),
            public,
        )
    }

    pub fn gdrive_config(&self) -> Result<GoogleDriveConfig> {
        Ok(GoogleDriveConfig {
            client_id: required(&self.gdrive_client_id, "클라이언트 ID")?,
            client_secret: required(&self.gdrive_client_secret, "클라이언트 시크릿")?,
            folder_id: required(&self.gdrive_folder_id, "폴더 ID")?,
            token_path: gdrive_token_path(),
        })
    }
}

fn trimmed(value: &str) -> Option<&str> {
    Some(value.trim()).filter(|v| !v.is_empty())
}

fn required(value: &str, label: &str) -> Result<String> {
    trimmed(value).map(str::to_owned).ok_or_else(|| {
        Error::Config(format!(
            "Google Drive {label}이(가) 비어 있습니다 — 설정에서 입력하세요"
        ))
    })
}

/// `true` when the path is set and points at something usable.
pub fn is_file(value: &str) -> bool {
    trimmed(value).is_some_and(|v| Path::new(v).is_file())
}

pub fn is_dir(value: &str) -> bool {
    trimmed(value).is_some_and(|v| Path::new(v).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `NEBULA_CONFIG_DIR` is process-wide, so these run one at a time.
    fn with_config_dir<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("NEBULA_CONFIG_DIR", dir) };
        let out = body();
        unsafe { std::env::remove_var("NEBULA_CONFIG_DIR") };
        out
    }

    #[test]
    fn the_default_config_dir_ends_with_nebula() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::remove_var("NEBULA_CONFIG_DIR") };
        assert_eq!(config_dir().file_name().unwrap(), "nebula");
    }

    #[test]
    fn the_override_is_used_as_given() {
        let dir = crate::gui::TempDir::new("override");
        // Without the `nebula` suffix: a portable install points this at the
        // directory it actually wants written to.
        let path = with_config_dir(dir.path(), settings_path);
        assert_eq!(path, dir.path().join("settings.json"));
    }

    fn sample() -> GuiSettings {
        GuiSettings {
            root: "D:\\TestRoot".into(),
            base_url: "https://cdn.example.invalid/".into(),
            java_executable: "java".into(),
            helios_data_folder: String::new(),
            theme: "dark".into(),
            reduced_motion: true,
            webdav_url: "https://dav.example.invalid/nebula".into(),
            webdav_user: "hambugui".into(),
            webdav_password: "hunter2".into(),
            webdav_public_url: String::new(),
            gdrive_client_id: "id".into(),
            gdrive_client_secret: "secret".into(),
            gdrive_folder_id: "folder".into(),
            curseforge_api_key: "cf-key".into(),
            last_mods_server: "Test-1.20.1".into(),
        }
    }

    #[test]
    fn a_settings_file_written_before_the_mods_page_still_loads() {
        let dir = crate::gui::TempDir::new("legacy-settings");
        let settings = with_config_dir(dir.path(), || {
            // Exactly the shape shipped before §8: neither new key is there.
            std::fs::create_dir_all(dir.path()).unwrap();
            std::fs::write(
                settings_path(),
                r#"{ "root": "D:\\TestRoot", "base-url": "https://cdn.example.invalid/", "theme": "dark" }"#,
            )
            .unwrap();
            GuiSettings::load()
        });
        assert_eq!(settings.root, "D:\\TestRoot");
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.curseforge_api_key, "");
        assert_eq!(settings.last_mods_server, "");
    }

    #[test]
    fn the_two_mods_keys_persist_under_their_kebab_case_names() {
        let dir = crate::gui::TempDir::new("mods-settings");
        let (raw, read_back) = with_config_dir(dir.path(), || {
            sample().save().unwrap();
            (
                std::fs::read_to_string(settings_path()).unwrap(),
                GuiSettings::load(),
            )
        });
        assert!(raw.contains("\"curseforge-api-key\""), "{raw}");
        assert!(raw.contains("\"last-mods-server\""), "{raw}");
        assert_eq!(read_back.curseforge_api_key, "cf-key");
        assert_eq!(read_back.last_mods_server, "Test-1.20.1");
        // …and the key reaches the `Config` the catalog is built from.
        assert_eq!(
            read_back.to_config().unwrap().curseforge_api_key.as_deref(),
            Some("cf-key")
        );
    }

    #[test]
    fn round_trips_through_the_settings_file() {
        let dir = crate::gui::TempDir::new("settings");
        let (written, read_back) = with_config_dir(dir.path(), || {
            let settings = sample();
            settings.save().unwrap();
            (settings, GuiSettings::load())
        });
        assert_eq!(written, read_back);
        assert!(dir.path().join("settings.json").is_file());
    }

    #[test]
    fn seeds_from_the_environment_when_the_file_is_missing() {
        let dir = crate::gui::TempDir::new("seed");
        let settings = with_config_dir(&dir.path().join("absent"), GuiSettings::load);
        // Whatever the environment holds, an unwritten file never panics and
        // the theme falls back to the documented default.
        assert_eq!(settings.theme, "system");
    }

    #[test]
    fn to_config_requires_a_root_and_a_base_url() {
        let mut settings = sample();
        settings.root = "  ".into();
        assert_eq!(
            settings.to_config().unwrap_err().to_string(),
            "루트 폴더를 설정하세요"
        );

        let mut settings = sample();
        settings.base_url = String::new();
        assert_eq!(
            settings.to_config().unwrap_err().to_string(),
            "기본 URL을 설정하세요"
        );
    }

    #[test]
    fn to_config_normalises_the_base_url_and_keeps_java_verbatim() {
        let mut settings = sample();
        settings.base_url = "https://cdn.example.invalid/dist".into();
        let cfg = settings.to_config().unwrap();
        assert_eq!(cfg.base_url.as_str(), "https://cdn.example.invalid/dist/");
        assert_eq!(cfg.java_executable.unwrap(), Path::new("java"));
        assert!(cfg.helios_data_folder.is_none());
    }

    #[test]
    fn webdav_public_url_defaults_to_the_base_url() {
        let cfg = sample().to_config().unwrap();
        // Constructing the target proves the URLs parse; the fields are private.
        assert!(sample().webdav_target(&cfg).is_ok());

        // `WebDavTarget` is not `Debug`, so the error is read off `Result::err`.
        let mut missing = sample();
        missing.webdav_url = String::new();
        let error = missing.webdav_target(&cfg).err().unwrap().to_string();
        assert!(error.contains("업로드 URL"), "{error}");

        let mut unparsable = sample();
        unparsable.webdav_public_url = "dav.example.invalid".into();
        let error = unparsable.webdav_target(&cfg).err().unwrap().to_string();
        assert!(error.contains("공개 URL"), "{error}");
    }

    #[test]
    fn gdrive_config_names_the_empty_field() {
        let dir = crate::gui::TempDir::new("gdrive");
        with_config_dir(dir.path(), || {
            let mut settings = sample();
            settings.gdrive_folder_id = String::new();
            let message = settings.gdrive_config().unwrap_err().to_string();
            assert!(message.contains("폴더 ID"), "{message}");
            assert_eq!(
                sample().gdrive_config().unwrap().token_path,
                dir.path().join("gdrive-token.json")
            );
        });
    }
}
