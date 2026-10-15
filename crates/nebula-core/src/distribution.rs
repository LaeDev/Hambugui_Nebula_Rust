//! The root file structure and the `distribution.json` it produces.
//!
//! Port of `structure/spec_model/Distribution.struct.ts` and the `init root` /
//! `generate distro` handlers of upstream `index.ts`.

use std::path::PathBuf;

use crate::config::Config;
use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::meta::{self, DistroMeta};
use crate::resolver::ResolveOptions;
use crate::scan::HashPolicy;
use crate::server;
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

    let distribution = Distribution {
        version: DISTRIBUTION_VERSION.into(),
        rss: distro_meta.meta.rss,
        discord: distro_meta.meta.discord,
        servers,
    };

    // `JSON.stringify(distro, null, 2)` upstream: two spaces, no trailing
    // newline. The golden fixture is byte-compared against this.
    let body = serde_json::to_string_pretty(&distribution)?;
    let path = root.path.join(&file_name);
    std::fs::write(&path, &body).map_err(io(format!("writing {}", path.display())))?;
    log::info!("Successfully generated {file_name}");
    log::info!("Saved to {}", path.display());

    let installed_to = match install_dir {
        Some(dir) => {
            std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;
            let destination = dir.join(&file_name);
            log::info!("Installing distribution to {}", destination.display());
            std::fs::write(&destination, &body)
                .map_err(io(format!("writing {}", destination.display())))?;
            Some(destination)
        }
        None => None,
    };

    Ok(GenerateResult {
        path,
        installed_to,
        distribution,
        sha256: hash::digests_of_bytes(body.as_bytes()).sha256,
    })
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
