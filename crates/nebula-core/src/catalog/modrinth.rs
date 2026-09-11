//! Modrinth API v2 (facts §A). Every response field is optional or defaulted
//! and unknown fields are ignored, so a v2 field being added or retired never
//! breaks a lookup (R5).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::http;
use super::{
    CatalogError, Dependency, DependencyKind, HashAlgo, Loader, ProjectHit, Provider, ProviderId,
    ReleaseType, SearchPage, SearchQuery, SortKey, Version, VersionFile, preferred_version,
};
use crate::publish::with_query;
use crate::version::MinecraftVersion;

/// The API version prefix, kept as one constant for the eventual v3 move.
const API_VERSION: &str = "v2";
/// The documented successor is `update_many`; keep the path in one place.
const UPDATE_PATH: &str = "version_files/update";
/// The bulk identify endpoint.
const VERSION_FILES_PATH: &str = "version_files";
/// Where a project's web page lives, for the 프로젝트 열기 link.
const SITE: &str = "https://modrinth.com/mod/";

/// The Modrinth provider. Stateless apart from its base URL.
pub struct Modrinth {
    base: String,
}

impl Modrinth {
    /// `base` is the host only, e.g. `https://api.modrinth.com`.
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// `<base>/v2/<segments…>`, percent-encoded: a slug may legally contain
    /// `+`, `"` and a backtick (§E.3.6).
    fn url(&self, segments: &[&str]) -> Result<String, CatalogError> {
        let mut url = url::Url::parse(&self.base)
            .map_err(|error| CatalogError::Network(format!("{}: {error}", self.base)))?;
        {
            let mut path = url.path_segments_mut().map_err(|()| {
                CatalogError::Network(format!("{} cannot be a base URL", self.base))
            })?;
            path.pop_if_empty().push(API_VERSION).extend(segments);
        }
        Ok(url.to_string())
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, CatalogError> {
        http::get_json(ProviderId::Modrinth, url, &[])
    }

    /// The `facets` value: project type, loader and Minecraft version, ANDed.
    /// Loaders go under `categories:` (project-level, documented); `loaders:`
    /// is the stricter version-level filter and must never be mixed in (§A.2).
    fn facets(mc: &MinecraftVersion, loader: Loader) -> String {
        json!([
            ["project_type:mod"],
            [format!("categories:{}", loader.as_str())],
            [format!("versions:{mc}")],
        ])
        .to_string()
    }

    fn index(sort: SortKey) -> &'static str {
        match sort {
            SortKey::Relevance => "relevance",
            SortKey::Downloads => "downloads",
            SortKey::Newest => "updated",
        }
    }

    /// Bulk hash lookups share one body shape; only the filters differ.
    fn hash_body(
        hashes: &[String],
        algo: HashAlgo,
        filters: Option<(&MinecraftVersion, Loader)>,
    ) -> serde_json::Value {
        // Matching is case sensitive against lowercase hex in the database:
        // an uppercase digest looks exactly like "not installed" (§A.5).
        let lowered: Vec<String> = hashes.iter().map(|hash| hash.to_lowercase()).collect();
        // The server does not validate `algorithm`; a wrong one is
        // indistinguishable from no matches, so it comes off the hash itself.
        let algorithm = lowered
            .first()
            .map_or(algo, |hash| HashAlgo::for_hex(hash))
            .as_str();
        let mut body = json!({ "hashes": lowered, "algorithm": algorithm });
        if let Some((mc, loader)) = filters {
            // Omitting either filter silently widens the check rather than
            // erroring, so both are always sent (§A.5).
            body["loaders"] = json!([loader.as_str()]);
            body["game_versions"] = json!([mc.to_string()]);
        }
        body
    }

    fn post_hashes(
        &self,
        path: &str,
        hashes: &[String],
        algo: HashAlgo,
        filters: Option<(&MinecraftVersion, Loader)>,
    ) -> Result<BTreeMap<String, Version>, CatalogError> {
        // An empty list returns `{}` today, but Prism learned not to rely on
        // that tolerance (§A.5).
        if hashes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let body = Self::hash_body(hashes, algo, filters);
        let url = self.url(&path.split('/').collect::<Vec<_>>())?;
        let raw: BTreeMap<String, RawVersion> =
            http::post_json(ProviderId::Modrinth, &url, &[], &body)?;
        Ok(raw
            .into_iter()
            .map(|(hash, version)| (hash.to_lowercase(), version.into()))
            .collect())
    }
}

impl Provider for Modrinth {
    fn id(&self) -> ProviderId {
        ProviderId::Modrinth
    }

    fn search(&self, q: &SearchQuery) -> Result<SearchPage, CatalogError> {
        // `offset = n * limit`: the echoed offset is rounded down to a
        // multiple of the limit and must never drive paging (§A.2).
        let url = with_query(
            &self.url(&["search"])?,
            &[
                ("query", q.query.as_str()),
                ("facets", &Self::facets(&q.mc, q.loader)),
                ("index", Self::index(q.sort)),
                ("limit", &q.limit.to_string()),
                ("offset", &q.offset.to_string()),
            ],
        );
        let response: SearchResponse = self.get(&url)?;
        let mut hits = Vec::with_capacity(response.hits.len());
        for hit in response.hits {
            // `project_type:` is rewritten server-side to the plural
            // all-versions field, so a modpack that once shipped a mod version
            // matches the facet. Trust the hit's own type (§E.1.4/§E.1.5).
            if hit.project_type.as_deref() != Some("mod") {
                log::debug!(
                    "Dropping Modrinth hit {:?}: project_type {:?}",
                    hit.slug,
                    hit.project_type
                );
                continue;
            }
            let Some(project_id) = hit.project_id.clone() else {
                continue;
            };
            let slug = hit.slug.clone().unwrap_or_else(|| project_id.clone());
            hits.push(ProjectHit {
                provider: ProviderId::Modrinth,
                page_url: Some(format!("{SITE}{slug}")),
                project_id,
                title: hit.title.unwrap_or_else(|| slug.clone()),
                slug,
                author: hit.author,
                description: hit.description.unwrap_or_default(),
                downloads: hit.downloads.unwrap_or_default(),
                updated: hit.date_modified,
                icon_url: hit.icon_url,
            });
        }
        Ok(SearchPage {
            hits,
            total: response.total_hits,
        })
    }

    fn project(&self, id: &str) -> Result<ProjectHit, CatalogError> {
        let raw: RawProject = self.get(&self.url(&["project", id])?)?;
        let project_id = raw.id.unwrap_or_else(|| id.to_owned());
        let slug = raw.slug.unwrap_or_else(|| project_id.clone());
        Ok(ProjectHit {
            provider: ProviderId::Modrinth,
            page_url: Some(format!("{SITE}{slug}")),
            project_id,
            title: raw.title.unwrap_or_else(|| slug.clone()),
            slug,
            author: None,
            description: raw.description.unwrap_or_default(),
            downloads: raw.downloads.unwrap_or_default(),
            updated: raw.updated,
            icon_url: raw.icon_url,
        })
    }

    fn versions(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Vec<Version>, CatalogError> {
        let url = with_query(
            &self.url(&["project", project_id, "version"])?,
            &[
                ("loaders", &json!([loader.as_str()]).to_string()),
                ("game_versions", &json!([mc.to_string()]).to_string()),
                // Changelogs are markdown blobs a picker never shows.
                ("include_changelog", "false"),
            ],
        );
        let raw: Vec<RawVersion> = self.get(&url)?;
        let mut versions: Vec<Version> = raw.into_iter().map(Version::from).collect();
        // The server sorts by date_published desc, but D3 is ours to apply,
        // so the order is re-established here rather than assumed.
        versions.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(versions)
    }

    fn versions_by_hashes(
        &self,
        algo: HashAlgo,
        hashes: &[String],
    ) -> Result<BTreeMap<String, Version>, CatalogError> {
        self.post_hashes(VERSION_FILES_PATH, hashes, algo, None)
    }

    fn latest_file(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Option<Version>, CatalogError> {
        let versions = self.versions(project_id, mc, loader)?;
        Ok(preferred_version(&versions).cloned())
    }

    fn updates_by_hashes(
        &self,
        algo: HashAlgo,
        hashes: &[String],
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<BTreeMap<String, Version>, CatalogError> {
        self.post_hashes(UPDATE_PATH, hashes, algo, Some((mc, loader)))
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    hits: Vec<SearchHit>,
    #[serde(default)]
    total_hits: u64,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    project_type: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    date_modified: Option<String>,
    #[serde(default)]
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    updated: Option<String>,
    #[serde(default)]
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version_number: Option<String>,
    #[serde(default)]
    version_type: Option<String>,
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default)]
    hashes: RawHashes,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Default, Deserialize)]
struct RawHashes {
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    sha512: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDependency {
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: Option<String>,
}

impl From<RawVersion> for Version {
    fn from(raw: RawVersion) -> Self {
        let version_id = raw.id.unwrap_or_default();
        Version {
            project_id: raw.project_id.unwrap_or_default(),
            name: raw.name.clone().unwrap_or_else(|| version_id.clone()),
            number: raw.version_number.unwrap_or_else(|| version_id.clone()),
            version_id,
            release: release_type(raw.version_type.as_deref()),
            published: raw.date_published.unwrap_or_default(),
            game_versions: raw.game_versions,
            loaders: raw.loaders.iter().filter_map(|l| loader(l)).collect(),
            files: raw.files.into_iter().map(VersionFile::from).collect(),
            dependencies: raw.dependencies.into_iter().map(Dependency::from).collect(),
        }
    }
}

impl From<RawFile> for VersionFile {
    fn from(raw: RawFile) -> Self {
        VersionFile {
            // Written verbatim: legacy uploads carry a literal `%2B` in
            // `filename`, and decoding it would break the index key (§A.4).
            file_name: raw.filename.unwrap_or_default(),
            // Requested verbatim: the CDN path is opaque and is not the
            // filename percent-encoded (§A.4).
            url: raw.url,
            size: raw.size,
            primary: raw.primary,
            sha1: raw.hashes.sha1,
            sha512: raw.hashes.sha512,
            md5: None,
            fingerprint: None,
            file_id: None,
        }
    }
}

impl From<RawDependency> for Dependency {
    fn from(raw: RawDependency) -> Self {
        Dependency {
            project_id: raw.project_id,
            version_id: raw.version_id,
            kind: match raw.dependency_type.as_deref() {
                Some("required") => DependencyKind::Required,
                Some("incompatible") => DependencyKind::Incompatible,
                Some("embedded") => DependencyKind::Embedded,
                _ => DependencyKind::Optional,
            },
        }
    }
}

/// An unrecognised type is treated as an alpha: D3 then only reaches it when
/// there is nothing else, which is the safe direction for a provisioning tool.
fn release_type(value: Option<&str>) -> ReleaseType {
    match value {
        Some("release") => ReleaseType::Release,
        Some("beta") => ReleaseType::Beta,
        Some("alpha") => ReleaseType::Alpha,
        other => {
            log::debug!("Unknown Modrinth version_type {other:?}; treating it as alpha");
            ReleaseType::Alpha
        }
    }
}

fn loader(value: &str) -> Option<Loader> {
    match value {
        "forge" => Some(Loader::Forge),
        "neoforge" => Some(Loader::NeoForge),
        "fabric" => Some(Loader::Fabric),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc() -> MinecraftVersion {
        "1.20.1".parse().unwrap()
    }

    #[test]
    fn builds_the_documented_facet_string() {
        assert_eq!(
            Modrinth::facets(&mc(), Loader::NeoForge),
            r#"[["project_type:mod"],["categories:neoforge"],["versions:1.20.1"]]"#
        );
    }

    #[test]
    fn maps_every_sort_key() {
        assert_eq!(Modrinth::index(SortKey::Relevance), "relevance");
        assert_eq!(Modrinth::index(SortKey::Downloads), "downloads");
        assert_eq!(Modrinth::index(SortKey::Newest), "updated");
    }

    #[test]
    fn picks_the_algorithm_off_the_hashes_and_lowercases_them() {
        let body = Modrinth::hash_body(
            &["AB".repeat(64)],
            HashAlgo::Sha1,
            Some((&mc(), Loader::Fabric)),
        );
        assert_eq!(body["algorithm"], "sha512");
        assert_eq!(body["hashes"][0], "ab".repeat(64));
        assert_eq!(body["loaders"][0], "fabric");
        assert_eq!(body["game_versions"][0], "1.20.1");

        let short = Modrinth::hash_body(&["A".repeat(40)], HashAlgo::Sha512, None);
        assert_eq!(short["algorithm"], "sha1");
        assert!(short.get("loaders").is_none());
    }

    #[test]
    fn percent_encodes_a_slug_in_the_path() {
        let modrinth = Modrinth::new("https://api.modrinth.invalid/");
        assert_eq!(
            modrinth.url(&["project", "a+b", "version"]).unwrap(),
            "https://api.modrinth.invalid/v2/project/a+b/version"
        );
        assert_eq!(
            modrinth.url(&["project", "a b"]).unwrap(),
            "https://api.modrinth.invalid/v2/project/a%20b"
        );
    }
}
