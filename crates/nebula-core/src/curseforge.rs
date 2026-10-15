//! The CurseForge branch of [`crate::import`].
//!
//! Port of `parser/CurseForgeParser.ts`, keeping upstream's API key, endpoints
//! and file routing, extended with the `neoforge-` and `fabric-` loader ids.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::import::{self, Counters, DownloadFn, ManualDownload};
use crate::layout::Root;
use crate::remote;
use crate::scan::Namespace;
use crate::server::{CreatedServer, LoaderKind, create_server};
use crate::version::MinecraftVersion;

pub const API_BASE: &str = "https://api.curseforge.com/v1";
/// Upstream Nebula's key, kept so imports keep working out of the box.
pub const API_KEY: &str = "$2a$10$JL4kTO/N/oXIM6o3uTYC3eLxGrOI4BIAqpX4vAFeIPoXiTtagidkK";
const DEFAULT_OVERRIDES: &str = "overrides";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeManifest {
    pub minecraft: CurseForgeMinecraft,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub files: Vec<CurseForgeFile>,
    #[serde(default)]
    pub overrides: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeMinecraft {
    pub version: String,
    #[serde(rename = "modLoaders", default)]
    pub mod_loaders: Vec<CurseForgeModLoader>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeModLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default)]
    pub required: bool,
}

pub fn read_manifest(zip_path: &Path) -> Result<CurseForgeManifest> {
    import::read_json(&mut import::open(zip_path)?, "manifest.json")
}

/// Reads `minecraft.modLoaders[].id`, which upstream only ever parsed as
/// `forge-<v>`.
///
/// Prism writes the Minecraft version into the legacy NeoForge id
/// (`neoforge-1.20.1-47.1.106`); that build is `net.neoforged:forge`, which the
/// NeoForge resolver does not handle.
pub fn loader_for(manifest: &CurseForgeManifest) -> Result<Option<(LoaderKind, String)>> {
    let mc = &manifest.minecraft.version;
    for loader in &manifest.minecraft.mod_loaders {
        let lower = loader.id.to_lowercase();
        for (prefix, kind) in [
            ("forge-", LoaderKind::Forge),
            ("neoforge-", LoaderKind::NeoForge),
            ("fabric-", LoaderKind::Fabric),
        ] {
            // NeoForge ids also start with `forge-`'s letters only after the
            // `neo`, so the lowercase prefix test is unambiguous.
            if !lower.starts_with(prefix) {
                continue;
            }
            let version = loader.id[prefix.len()..].to_owned();
            if kind == LoaderKind::NeoForge && version.starts_with(&format!("{mc}-")) {
                return Err(Error::Invalid(format!(
                    "Legacy NeoForge({})는 지원하지 않습니다 — Minecraft {mc} 모드팩은 Forge를 쓰세요",
                    loader.id
                )));
            }
            return Ok(Some((kind, version)));
        }
        if lower.starts_with("quilt-") {
            return Err(Error::Invalid(
                "Quilt 모드팩은 지원하지 않습니다 — Helios도 Hambugui도 Quilt를 실행할 수 없습니다. \
                 Forge, NeoForge 또는 Fabric 모드팩을 쓰세요"
                    .into(),
            ));
        }
        log::warn!("Ignoring unknown CurseForge mod loader {}.", loader.id);
    }
    Ok(None)
}

pub(crate) fn import(
    root: &Root,
    id: &str,
    path: &Path,
    download: &DownloadFn,
    counters: &mut Counters,
) -> Result<CreatedServer> {
    let mut zip = import::open(path)?;
    let manifest: CurseForgeManifest = import::read_json(&mut zip, "manifest.json")?;
    let mc = manifest.minecraft.version.parse::<MinecraftVersion>()?;
    let loader = loader_for(&manifest)?;

    let created = create_server(
        root,
        id,
        &mc,
        &import::options(manifest.version.clone(), loader.as_ref()),
    )?;
    import::rename_server(&created, manifest.name.as_deref())?;

    let overrides = manifest.overrides.as_deref().unwrap_or(DEFAULT_OVERRIDES);
    import::extract_roots(
        &mut zip,
        &[format!("{}/", overrides.trim_end_matches('/'))],
        &["manifest.json", "modlist.html"],
        &created,
        |_| false,
        counters,
    )?;

    download_files(&manifest, &created, download, counters)?;
    Ok(created)
}

fn download_files(
    manifest: &CurseForgeManifest,
    created: &CreatedServer,
    download: &DownloadFn,
    counters: &mut Counters,
) -> Result<()> {
    if manifest.files.is_empty() {
        return Ok(());
    }
    // Upstream skipped the whole download loop without a mod container.
    let Some(container) = &created.mod_container else {
        log::warn!(
            "The modpack declares {} files but no mod loader; nothing was downloaded.",
            manifest.files.len()
        );
        return Ok(());
    };

    for file in &manifest.files {
        log::debug!(
            "Processing - Mod: {}, File: {}",
            file.project_id,
            file.file_id
        );
        let info: ModFileResponse = get(&format!(
            "{API_BASE}/mods/{}/files/{}",
            file.project_id, file.file_id
        ))?;
        let file_name = info.data.file_name;
        let lower = file_name.to_lowercase();

        let Some(url) = info.data.download_url else {
            log::warn!(
                "{file_name} is not available for 3rd-party download through the curseforge API!"
            );
            let modinfo: ModResponse = get(&format!("{API_BASE}/mods/{}", file.project_id))?;
            counters.manual.push(ManualDownload {
                name: modinfo.data.name,
                url: format!(
                    "https://www.curseforge.com/minecraft/mc-mods/{}/download/{}",
                    modinfo.data.slug, file.file_id
                ),
                file_name,
            });
            continue;
        };

        let destination = if lower.ends_with("jar") {
            let namespace = match file.required {
                true => Namespace::Required,
                false => Namespace::OptionalOn,
            };
            container.join(namespace.dir_name()).join(&file_name)
        } else if lower.ends_with("zip") {
            // Assume it is a resource pack, as upstream did.
            created.files_dir.join("resourcepacks").join(&file_name)
        } else {
            created.files_dir.join(&file_name)
        };

        log::debug!("Downloading {file_name}");
        import::create_parent(&destination)?;
        download(&url, &destination, None)?;
        counters.downloaded += 1;
    }
    Ok(())
}

fn get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    remote::get_json_with_headers(url, &[("X-API-KEY", API_KEY)])
}

#[derive(Debug, Deserialize)]
struct ModFileResponse {
    data: ModFileData,
}

#[derive(Debug, Deserialize)]
struct ModFileData {
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModResponse {
    data: ModData,
}

#[derive(Debug, Deserialize)]
struct ModData {
    name: String,
    slug: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(mc: &str, loaders: &[&str]) -> CurseForgeManifest {
        CurseForgeManifest {
            minecraft: CurseForgeMinecraft {
                version: mc.to_owned(),
                mod_loaders: loaders
                    .iter()
                    .map(|id| CurseForgeModLoader {
                        id: (*id).to_owned(),
                        primary: true,
                    })
                    .collect(),
            },
            name: None,
            version: None,
            author: None,
            files: Vec::new(),
            overrides: None,
        }
    }

    #[test]
    fn reads_every_supported_loader_id() {
        assert_eq!(
            loader_for(&manifest("1.20.1", &["forge-47.2.0"])).unwrap(),
            Some((LoaderKind::Forge, "47.2.0".into()))
        );
        assert_eq!(
            loader_for(&manifest("1.21.1", &["neoforge-21.1.77"])).unwrap(),
            Some((LoaderKind::NeoForge, "21.1.77".into()))
        );
        assert_eq!(
            loader_for(&manifest("1.20.1", &["fabric-0.14.18"])).unwrap(),
            Some((LoaderKind::Fabric, "0.14.18".into()))
        );
        assert_eq!(loader_for(&manifest("1.20.1", &[])).unwrap(), None);
    }

    #[test]
    fn rejects_legacy_neoforge_and_quilt() {
        let error = loader_for(&manifest("1.20.1", &["neoforge-1.20.1-47.1.106"])).unwrap_err();
        assert!(error.to_string().contains("Legacy NeoForge"), "{error}");

        let error = loader_for(&manifest("1.20.1", &["quilt-0.20.0"])).unwrap_err();
        assert!(error.to_string().contains("Quilt"), "{error}");
    }

    #[test]
    fn keeps_a_neoforge_version_that_merely_looks_like_a_range() {
        // `21.1.78-beta` must not be mistaken for `<mc>-<version>`.
        assert_eq!(
            loader_for(&manifest("1.21.1", &["neoforge-21.1.78-beta"])).unwrap(),
            Some((LoaderKind::NeoForge, "21.1.78-beta".into()))
        );
    }
}
