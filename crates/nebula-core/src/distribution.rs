//! The root file structure and the `distribution.json` it produces.
//!
//! Port of `structure/spec_model/Distribution.struct.ts` and the `init root` /
//! `generate distro` handlers of upstream `index.ts`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::meta::{self, DistroMeta};
use crate::resolver::ResolveOptions;
use crate::scan::HashPolicy;
use crate::server;
use crate::signing::{self, DistributionKey};
use crate::spec::Distribution;

/// `Distribution.version` is a constant upstream, not the workspace version.
const DISTRIBUTION_VERSION: &str = "1.0.0";

/// Creates the empty standard file structure. Safe to re-run: only the schemas
/// are rewritten, and an existing `distrometa.json` is kept.
pub fn init_root(root: &Root) -> Result<()> {
    meta::generate_schemas(root)?;

    for dir in [
        root.path.clone(),
        root.meta_dir(),
        root.servers_dir(),
        root.modpacks_curseforge_dir(),
        root.modpacks_prism_dir(),
    ] {
        std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;
    }

    let distro_meta_file = root.distro_meta_file();
    if distro_meta_file.exists() {
        log::warn!(
            "Distro Meta file already exists at {}!",
            distro_meta_file.display()
        );
        log::warn!("If you wish to regenerate this file, you must delete the existing one!");
    } else {
        let mut distro_meta = meta::default_distro_meta();
        distro_meta.schema = Some(meta::schema_url(root, meta::DISTRO_META_SCHEMA));
        std::fs::write(
            &distro_meta_file,
            serde_json::to_string_pretty(&distro_meta)?,
        )
        .map_err(io(format!("writing {}", distro_meta_file.display())))?;
    }

    log::info!("Successfully created new root at {}", root.path.display());
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    /// File stem; the distribution is written to `<root>/<name>.json`.
    pub name: String,
    pub install_local: bool,
    pub discard_output: bool,
    pub invalidate_cache: bool,
    /// Emit untracked artifacts without digests, as upstream Nebula did. The
    /// resulting file only loads in Helios.
    pub helios_untracked: bool,
    /// Let a failed headless Forge/NeoForge install re-run the installer with
    /// its window open. Only the caller knows whether anybody can see it, so it
    /// defaults to off.
    pub gui_fallback: bool,
    /// Signs the written file into `<name>.json.sig`. Without a key the
    /// distribution is written unsigned, with a warning: fine for a local
    /// test, but the Hambugui launcher's release build refuses it and
    /// [`crate::publish::publish`] will not upload it.
    pub signing_key: Option<DistributionKey>,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            name: "distribution".into(),
            install_local: false,
            discard_output: false,
            invalidate_cache: false,
            helios_untracked: false,
            gui_fallback: false,
            signing_key: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateResult {
    pub path: PathBuf,
    pub installed_to: Option<PathBuf>,
    pub distribution: Distribution,
    /// SHA-256 of the bytes written to `path`.
    pub sha256: String,
    /// `<path>.sig`, when a signing key was given.
    pub signature: Option<PathBuf>,
}

/// The `serial` of the distribution about to be written: the current UNIX time
/// in seconds, or one more than `previous` when the clock is not past it (two
/// runs in one second, a clock set back). Strictly greater than `previous`
/// either way, so a launcher holding the older copy accepts the newer one and
/// refuses the older one afterwards.
pub fn next_serial(previous: Option<u64>, now: u64) -> Result<u64> {
    match previous {
        Some(previous) if now <= previous => previous.checked_add(1).ok_or_else(|| {
            Error::Invalid("the previous distribution serial is already the largest one".into())
        }),
        _ => Ok(now),
    }
}

/// The `serial` of the distribution already at `path`, the last one this root
/// produced and therefore the one that may have been published. A missing file
/// or one without a serial (written before serials existed, or by hand) has
/// none.
pub fn previous_serial(path: &Path) -> Option<u64> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("Could not read {} for its serial: {error}", path.display());
            return None;
        }
    };
    match serde_json::from_slice::<serde_json::Value>(&raw) {
        Ok(value) => value.get("serial").and_then(serde_json::Value::as_u64),
        Err(error) => {
            log::warn!(
                "{} is not JSON, so its serial is unknown: {error}",
                path.display()
            );
            None
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// Writes `body` to `path` and, with a key, its signature next to it. Without
/// one, an older `.sig` beside `path` is removed: it signs other bytes.
fn write_document(
    path: &Path,
    body: &str,
    key: Option<&DistributionKey>,
) -> Result<Option<PathBuf>> {
    std::fs::write(path, body).map_err(io(format!("writing {}", path.display())))?;
    match key {
        Some(key) => {
            let signature = signing::write_signature(key, path, body.as_bytes())?;
            log::info!("Signed {}", signature.display());
            Ok(Some(signature))
        }
        None => {
            signing::remove_stale_signature(path)?;
            Ok(None)
        }
    }
}

/// Resolves every server under `<root>/servers` and writes the distribution.
///
/// `JAVA_EXECUTABLE` is only consulted when a server actually declares Forge or
/// NeoForge; Fabric-only and loader-less roots generate without a JDK.
pub fn generate_distribution(cfg: &Config, opts: &GenerateOptions) -> Result<GenerateResult> {
    let file_name = format!("{}.json", opts.name);
    let install_dir = match opts.install_local {
        true => Some(cfg.helios_data_folder.clone().ok_or_else(|| {
            Error::Config(
                "Launcher 데이터 폴더가 설정되지 않았습니다 — 설정 화면에서 폴더를 지정하거나 \
                 '생성 후 Launcher 데이터 폴더에 설치'를 끄세요"
                    .into(),
            )
        })?),
        false => None,
    };

    let root = Root::new(&cfg.root, cfg.base_url.clone());
    let distro_meta_file = root.distro_meta_file();
    let raw = std::fs::read_to_string(&distro_meta_file)
        .map_err(io(format!("reading {}", distro_meta_file.display())))?;
    let distro_meta: DistroMeta = serde_json::from_str(&raw).map_err(|error| {
        Error::Invalid(format!(
            "{} is not a valid distrometa.json: {error}",
            distro_meta_file.display()
        ))
    })?;

    let java = cfg
        .java_executable
        .as_deref()
        .map(|exe| Java::new(exe, &root));
    let resolve = ResolveOptions {
        discard_output: opts.discard_output,
        invalidate_cache: opts.invalidate_cache,
        gui_fallback: opts.gui_fallback,
    };
    let policy = HashPolicy {
        helios_untracked: opts.helios_untracked,
    };

    let servers = server::build_all_server_specs(&root, java.as_ref(), &resolve, policy)?;

    let path = root.path.join(&file_name);
    let serial = next_serial(previous_serial(&path), unix_now())?;
    let distribution = Distribution {
        version: DISTRIBUTION_VERSION.into(),
        serial: Some(serial),
        rss: distro_meta.meta.rss,
        discord: distro_meta.meta.discord,
        servers,
    };

    // `JSON.stringify(distro, null, 2)` upstream: two spaces, no trailing
    // newline. The golden fixture is byte-compared against this.
    let body = serde_json::to_string_pretty(&distribution)?;
    let key = opts.signing_key.as_ref();
    let signature = write_document(&path, &body, key)?;
    log::info!("Successfully generated {file_name} (serial {serial})");
    log::info!("Saved to {}", path.display());
    match key {
        Some(key) => log::info!("Signed with public key {}", key.public_key_hex()),
        None => warn_unsigned(&file_name),
    }

    let installed_to = match install_dir {
        Some(dir) => {
            std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;
            let destination = dir.join(&file_name);
            log::info!("Installing distribution to {}", destination.display());
            write_document(&destination, &body, key)?;
            Some(destination)
        }
        None => None,
    };

    Ok(GenerateResult {
        path,
        installed_to,
        distribution,
        sha256: hash::digests_of_bytes(body.as_bytes()).sha256,
        signature,
    })
}

fn warn_unsigned(file_name: &str) {
    log::warn!("================================================================");
    log::warn!("{file_name} IS UNSIGNED: no distribution signing key was found.");
    log::warn!("A release Hambugui launcher refuses it, and `nebula publish` will");
    log::warn!("not upload it. Create a key with `nebula keygen`, or set");
    log::warn!("{} / {}.", signing::KEY_ENV, signing::KEY_FILE_ENV);
    log::warn!("================================================================");
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn config(dir: &std::path::Path) -> Config {
        Config {
            root: dir.to_path_buf(),
            base_url: Url::parse("https://cdn.example.invalid/").unwrap(),
            java_executable: None,
            helios_data_folder: None,
            curseforge_api_key: None,
        }
    }

    #[test]
    fn keeps_an_existing_distro_meta() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(
            dir.path(),
            Url::parse("https://cdn.example.invalid/").unwrap(),
        );
        init_root(&root).unwrap();

        std::fs::write(root.distro_meta_file(), r#"{"meta":{"rss":"kept"}}"#).unwrap();
        init_root(&root).unwrap();

        let raw = std::fs::read_to_string(root.distro_meta_file()).unwrap();
        assert_eq!(raw, r#"{"meta":{"rss":"kept"}}"#);
        assert!(root.modpacks_prism_dir().is_dir());
        assert!(root.modpacks_curseforge_dir().is_dir());
        assert!(
            root.schemas_dir()
                .join("ServerMetaSchema.schema.json")
                .is_file()
        );
    }

    #[test]
    fn refuses_a_local_install_without_a_helios_folder() {
        let dir = tempfile::tempdir().unwrap();
        let error = generate_distribution(
            &config(dir.path()),
            &GenerateOptions {
                install_local: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Launcher 데이터 폴더가 설정되지 않았습니다 — 설정 화면에서 폴더를 지정하거나 \
             '생성 후 Launcher 데이터 폴더에 설치'를 끄세요"
        );
    }
}
