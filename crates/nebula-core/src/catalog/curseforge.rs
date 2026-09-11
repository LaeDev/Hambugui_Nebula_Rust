//! CurseForge Core API v1 (facts §B). The key comes from [`super::Catalog`],
//! which gets it from `Config.curseforge_api_key`; this module never reads the
//! environment (D2).
//!
//! **CDN URL reconstruction is forbidden (R4).** `downloadUrl` decomposes into
//! `edge.forgecdn.net/files/{id/1000}/{id%1000}/{name}` and rebuilding it does
//! work, including for files whose author opted out of third-party
//! distribution. Nebula never does it: a blocked file becomes
//! `VersionFile { url: None }` and reaches the user as a manual download link.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::json;

use super::http;
use super::{
    CatalogError, Dependency, DependencyKind, HashAlgo, Loader, ProjectHit, Provider, ProviderId,
    ReleaseType, SearchPage, SearchQuery, SortKey, Version, VersionFile, preferred_version,
};
use crate::publish::with_query;
use crate::version::MinecraftVersion;

/// Minecraft.
const GAME_ID: &str = "432";
/// The Mc-Mods class; without it a search is dominated by modpacks (§B.2).
const CLASS_ID: &str = "6";
/// `/mods/{id}/files` pages at the documented maximum.
const FILES_PAGE_SIZE: u32 = 50;
/// Search pages are hard-capped at 50 by the server.
const MAX_SEARCH_PAGE: u32 = 50;
/// Stop paging `/files` here; a project with more than 250 matching files for
/// one Minecraft version and loader does not exist in practice.
const MAX_FILE_PAGES: u32 = 5;
/// `sortableGameVersions[].gameVersionTypeId` for the loader rows (§B.4).
const MODLOADER_TYPE_ID: i64 = 68441;
/// `fileStatus` / `ModStatus` both use 4 for Approved, but they are different
/// enums; keeping two Rust types stops one papering over the other (§E.2.2).
const APPROVED: u32 = 4;

/// The CurseForge provider.
pub struct CurseForge {
    base: String,
    key: String,
}

impl CurseForge {
    /// `base` is the versioned root, `https://api.curseforge.com/v1`.
    pub fn new(base: &str, key: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            key: key.to_owned(),
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.base)
    }

    fn headers(&self) -> [(&str, &str); 1] {
        [("x-api-key", self.key.as_str())]
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, CatalogError> {
        http::get_json(ProviderId::CurseForge, url, &self.headers())
    }

    /// `modLoaderType` is **not** a wildcard at 0 — it filters literally and
    /// returns nothing, so "any loader" means omitting the parameter (§B.2).
    fn mod_loader_type(loader: Loader) -> u32 {
        match loader {
            Loader::Forge => 1,
            Loader::Fabric => 4,
            Loader::NeoForge => 6,
        }
    }

    fn sort_field(sort: SortKey) -> &'static str {
        match sort {
            // 2 Popularity, 6 TotalDownloads, 3 LastUpdated (§B.2).
            SortKey::Relevance => "2",
            SortKey::Downloads => "6",
            SortKey::Newest => "3",
        }
    }

    /// Built separately from [`Provider::search`] so the "omit rather than
    /// send 0" rule is testable without a server.
    fn search_url(&self, q: &SearchQuery, loader_type: Option<u32>) -> String {
        let page_size = q.limit.clamp(1, MAX_SEARCH_PAGE).to_string();
        let mc = q.mc.to_string();
        let index = q.offset.to_string();
        let mut pairs = vec![
            ("gameId", GAME_ID),
            ("classId", CLASS_ID),
            ("searchFilter", q.query.as_str()),
            ("gameVersion", mc.as_str()),
            ("sortField", Self::sort_field(q.sort)),
            ("sortOrder", "desc"),
            ("pageSize", page_size.as_str()),
            ("index", index.as_str()),
        ];
        let loader_type = loader_type.map(|value| value.to_string());
        if let Some(loader_type) = &loader_type {
            pairs.push(("modLoaderType", loader_type.as_str()));
        }
        with_query(&self.endpoint("mods/search"), &pairs)
    }

    /// One `/files` page filtered by Minecraft version and loader.
    fn files_page(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
        index: u32,
    ) -> Result<Vec<RawFile>, CatalogError> {
        let url = with_query(
            &self.endpoint(&format!("mods/{project_id}/files")),
            &[
                ("gameVersion", &mc.to_string()),
                ("modLoaderType", &Self::mod_loader_type(loader).to_string()),
                ("pageSize", &FILES_PAGE_SIZE.to_string()),
                ("index", &index.to_string()),
            ],
        );
        let response: ApiResponse<Vec<RawFile>> = self.get(&url)?;
        Ok(response.data)
    }

    /// Every acceptable file for this Minecraft version and loader.
    ///
    /// Pages to the end of the list rather than stopping at the first page
    /// that yields a match: nothing documents the order `/files` returns, and
    /// if it is oldest-first then a project with more than one page of
    /// matching files would hand D3 an ancient release as "the latest". The
    /// server already filters by version and loader, so this is one request
    /// for almost every project and never more than [`MAX_FILE_PAGES`].
    fn matching_files(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Vec<RawFile>, CatalogError> {
        let mut kept = Vec::new();
        for page in 0..MAX_FILE_PAGES {
            let files = self.files_page(project_id, mc, loader, page * FILES_PAGE_SIZE)?;
            let exhausted = (files.len() as u32) < FILES_PAGE_SIZE;
            for file in files {
                if file.is_acceptable(mc, loader) {
                    kept.push(file);
                }
            }
            if exhausted {
                break;
            }
        }
        Ok(kept)
    }

    /// Turns raw files into versions, recovering a blocked file's digests
    /// through `download-info` (§E.2.1), which answers 200 where
    /// `/download-url` is a bare 403.
    fn to_versions(&self, project_id: &str, files: Vec<RawFile>) -> Vec<Version> {
        files
            .into_iter()
            .map(|file| {
                let mut version = file.into_version(project_id);
                let blocked = version.files.first().is_some_and(|file| file.url.is_none());
                if blocked && let Some(file) = version.files.first_mut() {
                    self.fill_blocked(project_id, file);
                }
                version
            })
            .collect()
    }

    fn fill_blocked(&self, project_id: &str, file: &mut VersionFile) {
        let Some(file_id) = file.file_id.clone() else {
            return;
        };
        let url = self.endpoint(&format!("mods/{project_id}/files/{file_id}/download-info"));
        let response: Result<ApiResponse<Vec<RawDownloadInfo>>, _> = self.get(&url);
        match response {
            Ok(response) => {
                let Some(info) = response.data.into_iter().next() else {
                    return;
                };
                // `null`, `""` and absent all mean blocked; the URL stays
                // `None` and the caller offers a manual download (R3/R4).
                for hash in info.hashes {
                    hash.apply(file);
                }
                if file.file_name.is_empty()
                    && let Some(name) = info.file_name
                {
                    file.file_name = name;
                }
            }
            Err(error) => log::warn!("download-info for {project_id}/{file_id} failed: {error}"),
        }
    }
}

impl Provider for CurseForge {
    fn id(&self) -> ProviderId {
        ProviderId::CurseForge
    }

    fn search(&self, q: &SearchQuery) -> Result<SearchPage, CatalogError> {
        let url = self.search_url(q, Some(Self::mod_loader_type(q.loader)));
        let response: ApiResponse<Vec<RawMod>> = self.get(&url)?;
        let total = response
            .pagination
            .as_ref()
            .and_then(|pagination| pagination.total_count)
            .unwrap_or(response.data.len() as u64);
        Ok(SearchPage {
            hits: response.data.into_iter().map(ProjectHit::from).collect(),
            total,
        })
    }

    fn project(&self, id: &str) -> Result<ProjectHit, CatalogError> {
        let response: ApiResponse<RawMod> = self.get(&self.endpoint(&format!("mods/{id}")))?;
        Ok(ProjectHit::from(response.data))
    }

    fn versions(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Vec<Version>, CatalogError> {
        let files = self.matching_files(project_id, mc, loader)?;
        let mut versions = self.to_versions(project_id, files);
        versions.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(versions)
    }

    /// CurseForge identifies files by MurmurHash2 fingerprint, so the strings
    /// in `hashes` are decimal fingerprints and `algo` is ignored (D8). The
    /// matches are identification only: `latestFiles` on a match is unfiltered
    /// by Minecraft version and loader, so it can never answer "is this an
    /// update" (§E.3.1).
    fn versions_by_hashes(
        &self,
        _algo: HashAlgo,
        hashes: &[String],
    ) -> Result<BTreeMap<String, Version>, CatalogError> {
        let submitted: Vec<u32> = hashes
            .iter()
            .filter_map(|hash| hash.trim().parse().ok())
            .collect();
        if submitted.is_empty() {
            return Ok(BTreeMap::new());
        }
        let body = json!({ "fingerprints": submitted });
        let response: ApiResponse<RawFingerprints> = http::post_json(
            ProviderId::CurseForge,
            &self.endpoint("fingerprints"),
            &self.headers(),
            &body,
        )?;

        let mut matched = BTreeMap::new();
        for entry in response.data.exact_matches {
            let Some(file) = entry.file else { continue };
            let Some(fingerprint) = file.file_fingerprint else {
                continue;
            };
            let project_id = entry
                .id
                .map(|id| id.to_string())
                .unwrap_or_else(|| file.mod_id.map(|id| id.to_string()).unwrap_or_default());
            matched.insert(fingerprint.to_string(), file.into_version(&project_id));
        }
        // `unmatchedFingerprints` came back `null` even for a fingerprint that
        // matched nothing, so the unmatched set is computed here (§B.6).
        let exact: BTreeSet<u32> = response.data.exact_fingerprints.into_iter().collect();
        let unmatched: Vec<u32> = submitted
            .iter()
            .copied()
            .filter(|fingerprint| !exact.contains(fingerprint))
            .collect();
        if !unmatched.is_empty() {
            log::debug!("CurseForge matched none of {unmatched:?}");
        }
        Ok(matched)
    }

    fn latest_file(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Option<Version>, CatalogError> {
        // One filtered `/files` call per managed mod is the only correct
        // update source CurseForge offers (D7).
        let versions = self.versions(project_id, mc, loader)?;
        Ok(preferred_version(&versions).cloned())
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// Every Core API response is `{ "data": …, "pagination": … }`.
#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: T,
    #[serde(default)]
    pagination: Option<RawPagination>,
}

#[derive(Debug, Deserialize)]
struct RawPagination {
    /// Capped at 10000 and not the true total (§E.2.3).
    #[serde(rename = "totalCount", default)]
    total_count: Option<u64>,
}

/// A project's moderation state. Four means Approved here **and** on
/// `fileStatus`, but the two enums are otherwise unrelated (§E.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "u32")]
enum ModStatus {
    Approved,
    Other(u32),
}

impl From<u32> for ModStatus {
    fn from(value: u32) -> Self {
        match value {
            APPROVED => ModStatus::Approved,
            other => ModStatus::Other(other),
        }
    }
}

/// A file's publishing state: fifteen values, and not [`ModStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "u32")]
enum FileStatus {
    Approved,
    Other(u32),
}

impl From<u32> for FileStatus {
    fn from(value: u32) -> Self {
        match value {
            APPROVED => FileStatus::Approved,
            other => FileStatus::Other(other),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawMod {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(rename = "downloadCount", default)]
    download_count: Option<f64>,
    #[serde(rename = "dateModified", default)]
    date_modified: Option<String>,
    #[serde(default)]
    status: Option<ModStatus>,
    #[serde(default)]
    logo: Option<RawLogo>,
    #[serde(default)]
    links: Option<RawLinks>,
    #[serde(default)]
    authors: Vec<RawAuthor>,
}

#[derive(Debug, Deserialize)]
struct RawLogo {
    /// Already 256x256; always preferred over the full-size `url` (§B.7).
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// Absent links are `null`, `""` or missing, sometimes mixed in one object.
#[derive(Debug, Deserialize)]
struct RawLinks {
    #[serde(rename = "websiteUrl", default)]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAuthor {
    #[serde(default)]
    name: Option<String>,
}

impl From<RawMod> for ProjectHit {
    fn from(raw: RawMod) -> Self {
        if let Some(status) = raw.status
            && status != ModStatus::Approved
        {
            log::debug!("CurseForge project {:?} has status {status:?}", raw.slug);
        }
        let project_id = raw.id.map(|id| id.to_string()).unwrap_or_default();
        let slug = raw.slug.unwrap_or_else(|| project_id.clone());
        ProjectHit {
            provider: ProviderId::CurseForge,
            title: raw.name.unwrap_or_else(|| slug.clone()),
            project_id,
            slug,
            author: raw.authors.into_iter().find_map(|author| author.name),
            description: raw.summary.unwrap_or_default(),
            // int64 on the wire; a float parse keeps a huge count readable.
            downloads: raw.download_count.unwrap_or_default() as u64,
            updated: raw.date_modified,
            icon_url: raw
                .logo
                .and_then(|logo| logo.thumbnail_url.or(logo.url))
                .filter(|url| !url.is_empty()),
            page_url: raw
                .links
                .and_then(|links| links.website_url)
                .filter(|url| !url.is_empty()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default)]
    id: Option<u64>,
    #[serde(rename = "modId", default)]
    mod_id: Option<u64>,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    #[serde(rename = "fileName", default)]
    file_name: Option<String>,
    #[serde(rename = "releaseType", default)]
    release_type: Option<u32>,
    #[serde(rename = "fileStatus", default)]
    file_status: Option<FileStatus>,
    #[serde(default)]
    hashes: Vec<RawHash>,
    #[serde(rename = "fileDate", default)]
    file_date: Option<String>,
    /// The download size; `fileSizeOnDisk` is the uncompressed one (§B.4).
    #[serde(rename = "fileLength", default)]
    file_length: Option<u64>,
    /// `null` for a file its author does not permit us to distribute (§B.5).
    #[serde(rename = "downloadUrl", default)]
    download_url: Option<String>,
    #[serde(rename = "sortableGameVersions", default)]
    sortable_game_versions: Vec<RawSortableGameVersion>,
    #[serde(rename = "isServerPack", default)]
    is_server_pack: bool,
    #[serde(rename = "fileFingerprint", default)]
    file_fingerprint: Option<u32>,
    #[serde(default)]
    dependencies: Vec<RawFileDependency>,
}

#[derive(Debug, Deserialize)]
struct RawSortableGameVersion {
    #[serde(rename = "gameVersionName", default)]
    game_version_name: Option<String>,
    /// Non-empty only on a real Minecraft version row.
    #[serde(rename = "gameVersion", default)]
    game_version: Option<String>,
    #[serde(rename = "gameVersionTypeId", default)]
    game_version_type_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawHash {
    #[serde(default)]
    value: Option<String>,
    /// 1 = SHA-1, 2 = MD5. There is no sha512 on CurseForge (§B.4).
    #[serde(default)]
    algo: Option<u32>,
}

impl RawHash {
    fn apply(self, file: &mut VersionFile) {
        let Some(value) = self.value.filter(|value| !value.is_empty()) else {
            return;
        };
        match self.algo {
            Some(1) => file.sha1 = Some(value.to_lowercase()),
            Some(2) => file.md5 = Some(value.to_lowercase()),
            other => log::debug!("Ignoring CurseForge hash algo {other:?}"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawFileDependency {
    #[serde(rename = "modId", default)]
    mod_id: Option<u64>,
    #[serde(rename = "relationType", default)]
    relation_type: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawDownloadInfo {
    #[serde(rename = "fileName", default)]
    file_name: Option<String>,
    #[serde(default)]
    hashes: Vec<RawHash>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFingerprints {
    #[serde(rename = "exactMatches", default)]
    exact_matches: Vec<RawFingerprintMatch>,
    #[serde(rename = "exactFingerprints", default)]
    exact_fingerprints: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct RawFingerprintMatch {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    file: Option<RawFile>,
}

impl RawFile {
    /// The loaders declared by the `Modloader` rows of
    /// `sortableGameVersions`; a `File` has no loader field of its own and
    /// `gameVersions[]` mixes versions, loaders and sides (§B.4).
    fn loaders(&self) -> Vec<Loader> {
        self.sortable_game_versions
            .iter()
            .filter(|entry| entry.game_version_type_id == Some(MODLOADER_TYPE_ID))
            .filter_map(|entry| match entry.game_version_name.as_deref() {
                Some(name) if name.eq_ignore_ascii_case("forge") => Some(Loader::Forge),
                Some(name) if name.eq_ignore_ascii_case("fabric") => Some(Loader::Fabric),
                Some(name) if name.eq_ignore_ascii_case("neoforge") => Some(Loader::NeoForge),
                _ => None,
            })
            .collect()
    }

    /// The real Minecraft versions: the rows with a non-empty `gameVersion`.
    fn game_versions(&self) -> Vec<String> {
        self.sortable_game_versions
            .iter()
            .filter_map(|entry| entry.game_version.clone())
            .filter(|version| !version.is_empty())
            .collect()
    }

    /// Server packs are zips, never install candidates (§E.2.6). A file with
    /// no declared loader is accepted for any loader, as Prism does.
    fn is_acceptable(&self, mc: &MinecraftVersion, loader: Loader) -> bool {
        if self.is_server_pack {
            log::debug!("Skipping CurseForge server pack {:?}", self.file_name);
            return false;
        }
        let loaders = self.loaders();
        if !loaders.is_empty() && !loaders.contains(&loader) {
            return false;
        }
        let versions = self.game_versions();
        versions.is_empty() || versions.iter().any(|version| version == &mc.to_string())
    }

    fn into_version(self, project_id: &str) -> Version {
        if let Some(status) = self.file_status
            && status != FileStatus::Approved
        {
            log::debug!("CurseForge file {:?} has status {status:?}", self.file_name);
        }
        let loaders = self.loaders();
        let game_versions = self.game_versions();
        let file_id = self.id.map(|id| id.to_string());
        let file_name = self.file_name.unwrap_or_default();
        let mut file = VersionFile {
            file_name: file_name.clone(),
            // Blocked when null, "" or absent — never rebuilt from the id.
            url: self.download_url.filter(|url| !url.is_empty()),
            size: self.file_length.unwrap_or_default(),
            // One file per CurseForge version, so it is always the primary.
            primary: true,
            sha1: None,
            sha512: None,
            md5: None,
            fingerprint: self.file_fingerprint,
            file_id: file_id.clone(),
        };
        for hash in self.hashes {
            hash.apply(&mut file);
        }
        Version {
            project_id: project_id.to_owned(),
            version_id: file_id.clone().unwrap_or_default(),
            name: self.display_name.unwrap_or_else(|| file_name.clone()),
            number: file_id.unwrap_or_default(),
            release: release_type(self.release_type),
            published: self.file_date.unwrap_or_default(),
            game_versions,
            loaders,
            files: vec![file],
            dependencies: self
                .dependencies
                .into_iter()
                .map(Dependency::from)
                .collect(),
        }
    }
}

impl From<RawFileDependency> for Dependency {
    fn from(raw: RawFileDependency) -> Self {
        Dependency {
            project_id: raw.mod_id.map(|id| id.to_string()),
            version_id: None,
            kind: match raw.relation_type {
                // 1 EmbeddedLibrary, 3 RequiredDependency, 5 Incompatible;
                // everything else is optional to us (§B.4).
                Some(1) => DependencyKind::Embedded,
                Some(3) => DependencyKind::Required,
                Some(5) => DependencyKind::Incompatible,
                _ => DependencyKind::Optional,
            },
        }
    }
}

/// 1 Release, 2 Beta, 3 Alpha; anything else is treated as an alpha so D3
/// only reaches it last.
fn release_type(value: Option<u32>) -> ReleaseType {
    match value {
        Some(1) => ReleaseType::Release,
        Some(2) => ReleaseType::Beta,
        _ => ReleaseType::Alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> SearchQuery {
        SearchQuery {
            query: "jei".into(),
            sort: SortKey::Downloads,
            limit: 20,
            offset: 0,
            mc: "1.20.1".parse().unwrap(),
            loader: Loader::Forge,
        }
    }

    fn provider() -> CurseForge {
        CurseForge::new("https://api.curseforge.invalid/v1", "secret")
    }

    #[test]
    fn omits_the_loader_filter_rather_than_sending_zero() {
        let url = provider().search_url(&query(), None);
        assert!(!url.contains("modLoaderType"), "{url}");
        let filtered = provider().search_url(&query(), Some(1));
        assert!(filtered.contains("modLoaderType=1"), "{filtered}");
        assert!(!filtered.contains("modLoaderType=0"), "{filtered}");
    }

    #[test]
    fn sends_the_mandatory_search_parameters() {
        let url = provider().search_url(&query(), Some(6));
        for expected in [
            "gameId=432",
            "classId=6",
            "searchFilter=jei",
            "gameVersion=1.20.1",
            "sortField=6",
            "sortOrder=desc",
            "pageSize=20",
            "index=0",
        ] {
            assert!(url.contains(expected), "{expected} missing from {url}");
        }
    }

    #[test]
    fn clamps_the_page_size_to_the_server_maximum() {
        let mut big = query();
        big.limit = 500;
        assert!(provider().search_url(&big, None).contains("pageSize=50"));
        let mut zero = query();
        zero.limit = 0;
        assert!(provider().search_url(&zero, None).contains("pageSize=1"));
    }

    #[test]
    fn maps_each_loader_to_its_documented_id() {
        assert_eq!(CurseForge::mod_loader_type(Loader::Forge), 1);
        assert_eq!(CurseForge::mod_loader_type(Loader::Fabric), 4);
        assert_eq!(CurseForge::mod_loader_type(Loader::NeoForge), 6);
    }
}
