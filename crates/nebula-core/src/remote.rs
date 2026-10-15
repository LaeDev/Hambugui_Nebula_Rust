use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io};
use crate::version::{MinecraftVersion, Promotion, neoforge_prefix};

pub const FORGE_MAVEN: &str = "https://files.minecraftforge.net/maven/";
pub const MOJANG_MAVEN: &str = "https://libraries.minecraft.net/";
pub const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/maven/net/minecraftforge/forge/promotions_slim.json";
pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions/";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/";
pub const NEOFORGE_VERSIONS_API: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

static CLIENT: OnceLock<std::result::Result<reqwest::blocking::Client, String>> = OnceLock::new();

/// The one blocking client in the crate. `catalog::http` sends its own
/// requests through it with a per-request timeout, so there is no second
/// connection pool and no second User-Agent policy.
pub(crate) fn client() -> Result<&'static reqwest::blocking::Client> {
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent(concat!("nebula-rust/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(30))
                // No read timeout: `read_timeout` exists only on reqwest's async
                // builder, and the blocking `timeout` covers the whole request,
                // which would kill a legitimate 300 MB installer download.
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| Error::Config(format!("could not build the HTTP client: {e}")))
}

fn http(url: &str, message: impl std::fmt::Display) -> Error {
    Error::Http {
        url: url.to_owned(),
        message: message.to_string(),
    }
}

pub fn get_json<T: DeserializeOwned>(url: &str) -> Result<T> {
    get_json_with_headers(url, &[])
}

/// [`get_json`] plus request headers, for APIs that demand a key.
pub fn get_json_with_headers<T: DeserializeOwned>(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<T> {
    log::debug!("Requesting {url}");
    let mut request = client()?.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request
        .send()
        .map_err(|e| http(url, e))?
        .error_for_status()
        .map_err(|e| http(url, e))?
        .json()
        .map_err(|e| http(url, e))
}

pub fn download_to(url: &str, dest: &Path) -> Result<()> {
    log::debug!("Downloading {url}");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }

    let mut response = client()?
        .get(url)
        .send()
        .map_err(|e| http(url, e))?
        .error_for_status()
        .map_err(|e| http(url, e))?;

    let partial = dest.with_extension("part");
    let mut file =
        std::fs::File::create(&partial).map_err(io(format!("creating {}", partial.display())))?;
    response.copy_to(&mut file).map_err(|e| http(url, e))?;
    drop(file);

    std::fs::rename(&partial, dest).map_err(io(format!("writing {}", dest.display())))?;
    Ok(())
}

/// [`download_to`] with an optional SHA-1 check. A mismatch deletes the file so
/// a resumed import can never pick up bad bytes it thinks are verified.
pub fn download_to_verified(url: &str, dest: &Path, expected_sha1: Option<&str>) -> Result<()> {
    download_to(url, dest)?;
    let Some(expected) = expected_sha1 else {
        return Ok(());
    };
    let actual = crate::hash::sha1_hex_of_file(dest)?;
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    let _ = std::fs::remove_file(dest);
    Err(http(
        url,
        format!("SHA-1 mismatch: expected {expected}, downloaded {actual}"),
    ))
}

pub fn head_exists(url: &str) -> bool {
    client()
        .and_then(|c| c.head(url).send().map_err(|e| http(url, e)))
        .is_ok_and(|r| r.status() == reqwest::StatusCode::OK)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromotionsSlim {
    pub homepage: String,
    pub promos: BTreeMap<String, String>,
}

pub fn get_promotion_index() -> Result<PromotionsSlim> {
    get_json(FORGE_PROMOTIONS)
}

pub fn get_promoted_forge_version(mc: &MinecraftVersion, promotion: Promotion) -> Result<String> {
    let index = get_promotion_index()?;
    if let Some(version) = index.promos.get(&format!("{mc}-{promotion}")) {
        return Ok(version.clone());
    }
    log::warn!("No {promotion} version found for Forge {mc}.");
    log::warn!("Attempting to pull latest version instead.");
    index
        .promos
        .get(&format!("{mc}-latest"))
        .cloned()
        .ok_or_else(|| Error::Invalid(format!("No latest version found for Forge {mc}.")))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NeoForgeVersions {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: bool,
    /// Ascending per the API docs, but the promotion pick never relies on it.
    pub versions: Vec<String>,
}

pub fn get_neoforge_versions() -> Result<Vec<String>> {
    Ok(get_json::<NeoForgeVersions>(NEOFORGE_VERSIONS_API)?.versions)
}

pub fn neoforge_installer_url(version: &str) -> String {
    format!("{NEOFORGE_MAVEN}net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar")
}

/// Numeric sort key: the dot separated components, with the optional `-beta`
/// suffix ignored. `None` for anything that is not digits and dots, which is
/// how entries such as `0.25w14craftmine.3-beta` are dropped.
fn neoforge_sort_key(version: &str) -> Option<Vec<u64>> {
    version
        .strip_suffix("-beta")
        .unwrap_or(version)
        .split('.')
        .map(|component| component.parse().ok())
        .collect()
}

/// NeoForge publishes no promotion index, only a flat version list, so the
/// promotions are derived: keep the builds for this minecraft version's prefix,
/// then `latest` is the numerically highest and `recommended` the highest
/// without a `-beta` suffix (falling back to `latest`).
pub fn pick_neoforge_version(
    versions: &[String],
    mc: &MinecraftVersion,
    promotion: Promotion,
) -> Option<String> {
    let prefix = neoforge_prefix(mc);
    // Sorted ascending; `stable` breaks a tie in favour of the non-beta build.
    let mut candidates: Vec<(Vec<u64>, bool, &String)> = versions
        .iter()
        .filter(|version| version.starts_with(&prefix))
        .filter_map(|version| {
            Some((
                neoforge_sort_key(version)?,
                !version.ends_with("-beta"),
                version,
            ))
        })
        .collect();
    candidates.sort();

    let latest = candidates.last()?.2;
    let picked = match promotion {
        Promotion::Latest => latest,
        Promotion::Recommended => candidates
            .iter()
            .rfind(|(_, stable, _)| *stable)
            .map_or_else(
                || {
                    log::warn!("No recommended version found for NeoForge {mc}.");
                    log::warn!("Attempting to pull latest version instead.");
                    latest
                },
                |candidate| candidate.2,
            ),
    };
    Some(picked.clone())
}

pub fn get_promoted_neoforge_version(
    mc: &MinecraftVersion,
    promotion: Promotion,
) -> Result<String> {
    let versions = get_neoforge_versions()?;
    pick_neoforge_version(&versions, mc, promotion)
        .ok_or_else(|| Error::Invalid(format!("No NeoForge build found for Minecraft {mc}")))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FabricLoaderMeta {
    pub version: String,
    pub stable: bool,
    pub separator: Option<String>,
    pub build: Option<u64>,
    pub maven: Option<String>,
}

pub fn get_fabric_loader_meta() -> Result<Vec<FabricLoaderMeta>> {
    get_json(&format!("{FABRIC_META}loader"))
}

pub fn get_promoted_fabric_version(promotion: Promotion) -> Result<String> {
    let meta = get_fabric_loader_meta()?;
    let selected = match promotion {
        Promotion::Latest => meta.first(),
        Promotion::Recommended => meta.iter().find(|entry| entry.stable),
    };
    selected
        .map(|entry| entry.version.clone())
        .ok_or_else(|| Error::Invalid(format!("No {promotion} Fabric loader version found.")))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FabricLibrary {
    /// Maven identifier.
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfileJson {
    pub id: String,
    pub inherits_from: String,
    pub libraries: Vec<FabricLibrary>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

pub fn get_fabric_profile_json(mc: &MinecraftVersion, loader: &str) -> Result<FabricProfileJson> {
    get_json(&format!("{FABRIC_META}loader/{mc}/{loader}/profile/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_fabric_profile() {
        let raw = r#"{
            "id": "1.20.1-fabric-0.14.21",
            "inheritsFrom": "1.20.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [{ "name": "net.fabricmc:fabric-loader:0.14.21", "url": "https://maven.fabricmc.net/" }]
        }"#;
        let profile: FabricProfileJson = serde_json::from_str(raw).unwrap();
        assert_eq!(profile.inherits_from, "1.20.1");
        assert_eq!(
            profile.libraries[0].name,
            "net.fabricmc:fabric-loader:0.14.21"
        );
        assert_eq!(
            profile.rest["mainClass"],
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );

        let round_tripped = serde_json::to_value(&profile).unwrap();
        assert_eq!(round_tripped["inheritsFrom"], "1.20.1");
        assert_eq!(
            round_tripped["mainClass"],
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
    }

    #[test]
    fn reads_the_forge_promotion_index() {
        let index: PromotionsSlim = serde_json::from_str(
            r#"{"homepage":"https://files.minecraftforge.net/","promos":{"1.20.1-latest":"47.4.0","1.20.1-recommended":"47.2.0"}}"#,
        )
        .unwrap();
        assert_eq!(index.promos["1.20.1-recommended"], "47.2.0");
    }

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    fn neoforge_list() -> Vec<String> {
        [
            "0.25w14craftmine.3-beta",
            "20.2.3-beta",
            "21.1.77",
            "21.1.78-beta",
            "26.2.0.84",
            "21.1.9",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn picks_neoforge_promotions() {
        let versions = neoforge_list();
        assert_eq!(
            pick_neoforge_version(&versions, &mc("1.21.1"), Promotion::Latest),
            Some("21.1.78-beta".into())
        );
        // 21.1.9 loses to 21.1.77: components are compared as numbers, not text.
        assert_eq!(
            pick_neoforge_version(&versions, &mc("1.21.1"), Promotion::Recommended),
            Some("21.1.77".into())
        );
        for promotion in [Promotion::Latest, Promotion::Recommended] {
            assert_eq!(
                pick_neoforge_version(&versions, &mc("26.2"), promotion),
                Some("26.2.0.84".into())
            );
        }
    }

    #[test]
    fn falls_back_to_the_latest_neoforge_beta() {
        let versions = neoforge_list();
        assert_eq!(
            pick_neoforge_version(&versions, &mc("1.20.2"), Promotion::Recommended),
            Some("20.2.3-beta".into())
        );
        assert_eq!(
            pick_neoforge_version(&versions, &mc("1.19.4"), Promotion::Latest),
            None
        );
        // Right prefix, but the remainder is not digits, dots and `-beta`.
        assert_eq!(
            pick_neoforge_version(
                &["21.1.80-rc1".into(), "21.1.x".into()],
                &mc("1.21.1"),
                Promotion::Latest
            ),
            None
        );
    }

    #[test]
    fn reads_the_neoforge_version_list() {
        let list: NeoForgeVersions =
            serde_json::from_str(r#"{"isSnapshot":false,"versions":["21.1.77","21.1.78-beta"]}"#)
                .unwrap();
        assert!(!list.is_snapshot);
        assert_eq!(list.versions, ["21.1.77", "21.1.78-beta"]);
        assert_eq!(
            neoforge_installer_url("21.1.77"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-installer.jar"
        );
    }
}
