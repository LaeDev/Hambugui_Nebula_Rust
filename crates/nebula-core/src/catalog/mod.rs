//! Mod catalog: provider-neutral search, version and hash lookup over
//! Modrinth (primary) and CurseForge (secondary), plus the per-server install
//! index, the jar-metadata reader and the cross-process server lock.
//!
//! Prism Launcher is the behavioural reference for the endpoints and edge
//! cases recorded in `docs/plans/2026-09-11-mod-catalog-facts.md`. It is
//! GPL-3.0 and none of its code is reproduced here.

pub mod curseforge;
pub mod http;
pub mod icons;
pub mod index;
pub mod install;
pub mod jarmeta;
pub mod lock;
pub mod manage;
pub mod modrinth;
pub mod update;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::Error;
use crate::layout::ServerDir;
use crate::server::LoaderKind;
use crate::version::MinecraftVersion;

pub use crate::import::ManualDownload;

/// Production API roots. `Endpoints::default()` uses them; tests inject an
/// in-process stub instead (R5).
pub const MODRINTH_API: &str = "https://api.modrinth.com";
/// Shared with the modpack-import path so there is one CurseForge base URL.
pub const CURSEFORGE_API: &str = crate::curseforge::API_BASE;

/// The User-Agent every catalog request carries, per Modrinth's policy (§A.1).
pub const USER_AGENT: &str = concat!(
    "nebula/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/LaeDev/nebularust)"
);

/// Which provider a project, version or index entry came from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Modrinth,
    CurseForge,
}

impl ProviderId {
    /// The name shown in the UI and written to `modindex.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::Modrinth => "modrinth",
            ProviderId::CurseForge => "curseforge",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The three loaders Nebula ships; Quilt is out of scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loader {
    Forge,
    NeoForge,
    Fabric,
}

impl Loader {
    /// Modrinth's loader tag, which is also the `categories:` facet value.
    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
            Loader::Fabric => "fabric",
        }
    }

    /// How a loader is spelled to the user, in both the GUI and the core's own
    /// sentences — one spelling, so `1.20.1 Forge용 버전이 없습니다` reads the
    /// same wherever it is produced.
    pub fn label(self) -> &'static str {
        match self {
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
            Loader::Fabric => "Fabric",
        }
    }
}

impl std::fmt::Display for Loader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl From<LoaderKind> for Loader {
    fn from(kind: LoaderKind) -> Self {
        match kind {
            LoaderKind::Forge => Loader::Forge,
            LoaderKind::NeoForge => Loader::NeoForge,
            LoaderKind::Fabric => Loader::Fabric,
        }
    }
}

/// Modrinth `release`/`beta`/`alpha`; CurseForge 1/2/3.
///
/// The order is the D3 ladder itself: `Release < Beta < Alpha`, so "a worse
/// release type than the installed one" is a plain `>`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
}

/// How one version depends on another project (D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// Which digest a bulk hash lookup speaks, never guessed (§A.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Sha1,
    Sha512,
}

impl HashAlgo {
    /// The wire value Modrinth's `algorithm` field takes.
    pub fn as_str(self) -> &'static str {
        match self {
            HashAlgo::Sha1 => "sha1",
            HashAlgo::Sha512 => "sha512",
        }
    }

    /// The algorithm a hex digest must have been produced by: 128 hex
    /// characters is sha512, anything else is treated as sha1. A wrong
    /// algorithm is indistinguishable from "no matches" (§A.5).
    pub fn for_hex(hex: &str) -> Self {
        match hex.trim().len() >= 128 {
            true => HashAlgo::Sha512,
            false => HashAlgo::Sha1,
        }
    }
}

/// Search ordering, mapped per provider in their modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Relevance,
    Downloads,
    Newest,
}

/// One page of a provider search.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub sort: SortKey,
    pub limit: u32,
    pub offset: u32,
    pub mc: MinecraftVersion,
    pub loader: Loader,
}

/// The hits of one page plus the provider's (possibly capped) total.
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub hits: Vec<ProjectHit>,
    pub total: u64,
}

/// A project as a search result row or a detail dialog shows it.
#[derive(Debug, Clone)]
pub struct ProjectHit {
    pub provider: ProviderId,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub author: Option<String>,
    pub description: String,
    pub downloads: u64,
    pub updated: Option<String>,
    pub icon_url: Option<String>,
    pub page_url: Option<String>,
}

/// One installable version of a project.
#[derive(Debug, Clone)]
pub struct Version {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub number: String,
    pub release: ReleaseType,
    pub published: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<Loader>,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<Dependency>,
}

impl Version {
    /// The file to install: the primary one, else the first plain `.jar`
    /// (§A.4 — `file_type` is uploader-supplied and null proves nothing).
    pub fn primary_file(&self) -> Option<&VersionFile> {
        if let Some(primary) = self.files.iter().find(|file| file.primary) {
            return Some(primary);
        }
        self.files.iter().find(|file| {
            let lower = file.file_name.to_lowercase();
            lower.ends_with(".jar")
                && !lower.ends_with("-sources.jar")
                && !lower.ends_with("-dev.jar")
                && !lower.ends_with("-javadoc.jar")
        })
    }

    /// Whether any file of this version is the digest we already have on disk.
    /// "An update exists" is a hash comparison, never a version-id one (R12).
    pub fn has_file_hash(&self, hex: &str) -> bool {
        let hex = hex.to_lowercase();
        self.files.iter().any(|file| {
            [&file.sha1, &file.sha512, &file.md5]
                .into_iter()
                .flatten()
                .any(|digest| digest.eq_ignore_ascii_case(&hex))
        })
    }
}

/// One downloadable artifact of a version.
#[derive(Debug, Clone)]
pub struct VersionFile {
    pub file_name: String,
    /// `None` means the provider refuses third-party distribution (R3).
    pub url: Option<String>,
    pub size: u64,
    pub primary: bool,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub md5: Option<String>,
    pub fingerprint: Option<u32>,
    pub file_id: Option<String>,
}

/// One entry of a version's dependency list.
#[derive(Debug, Clone)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: DependencyKind,
}

/// Everything a provider answers. Implementations are stateless apart from
/// their base URL and, for CurseForge, the user's API key.
pub trait Provider: Send + Sync {
    /// Which provider this is.
    fn id(&self) -> ProviderId;
    /// One page of search results for a Minecraft version and loader.
    fn search(&self, q: &SearchQuery) -> Result<SearchPage, CatalogError>;
    /// One project by id or slug.
    fn project(&self, id: &str) -> Result<ProjectHit, CatalogError>;
    /// Every version for this Minecraft version and loader, newest first.
    fn versions(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Vec<Version>, CatalogError>;
    /// Identify installed files by digest. CurseForge takes decimal
    /// fingerprints in the same slot and ignores `algo` (D7, D8).
    fn versions_by_hashes(
        &self,
        algo: HashAlgo,
        hashes: &[String],
    ) -> Result<BTreeMap<String, Version>, CatalogError>;
    /// The version D3 would install today, or `None` when the project has
    /// nothing for this Minecraft version and loader.
    fn latest_file(
        &self,
        project_id: &str,
        mc: &MinecraftVersion,
        loader: Loader,
    ) -> Result<Option<Version>, CatalogError>;
    /// Bulk "is there a newer file for these digests" check. Only Modrinth has
    /// one endpoint for it; CurseForge answers through `latest_file` per mod,
    /// because its fingerprint matches are unfiltered by version (E.3.1).
    fn updates_by_hashes(
        &self,
        _algo: HashAlgo,
        _hashes: &[String],
        _mc: &MinecraftVersion,
        _loader: Loader,
    ) -> Result<BTreeMap<String, Version>, CatalogError> {
        Ok(BTreeMap::new())
    }
}

/// D3: newest release, else newest beta, else newest alpha. `versions` must
/// already be newest-first.
pub fn preferred_version(versions: &[Version]) -> Option<&Version> {
    for wanted in [ReleaseType::Release, ReleaseType::Beta, ReleaseType::Alpha] {
        if let Some(found) = versions.iter().find(|version| version.release == wanted) {
            return Some(found);
        }
    }
    None
}

/// One file an install will download, with the provenance the index records.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub provider: ProviderId,
    pub project_id: String,
    pub slug: Option<String>,
    pub title: String,
    /// Where `slug`, `title` and `version_name` came from, which is **not**
    /// always `provider`: a blocked CurseForge file downloaded from Modrinth's
    /// mirror keeps CurseForge's text, and D2 forbids writing that to disk.
    pub text_provider: ProviderId,
    /// The project id `text_provider` knows this mod by, when it is not
    /// `project_id`: the R3 mirror rewrites both `provider` and `project_id` to
    /// Modrinth's, so the pair the user actually picked would otherwise be lost
    /// (CF-03). `None` means `project_id` is already that id.
    pub source_project_id: Option<String>,
    pub version_id: String,
    pub version_name: String,
    /// The provider's version *number* (`0.92.0`), which §8.4's line shows —
    /// `version_name` is a free-text title that usually repeats the mod name.
    pub version_number: String,
    /// ISO-8601 publication date of the version, for §8.4's confirm line.
    pub published: String,
    pub release: ReleaseType,
    pub file: VersionFile,
}

impl PlannedFile {
    /// The provider and project **the user picked**, which is not always what
    /// the bytes come from: the R3 mirror downloads a blocked CurseForge file
    /// from Modrinth and rewrites `provider`/`project_id` with it. Anything
    /// that asks a provider about this mod again — the version picker, a
    /// re-plan — has to use this pair or it sends one provider's id to the
    /// other (CF-03).
    pub fn source(&self) -> (ProviderId, &str) {
        (
            self.text_provider,
            self.source_project_id
                .as_deref()
                .unwrap_or(&self.project_id),
        )
    }
}

/// A dependency of the root install, and whether it is already present.
#[derive(Debug, Clone)]
pub struct PlannedDep {
    pub file: PlannedFile,
    pub kind: DependencyKind,
    /// The installed file name when this project is already installed (D6).
    pub already_installed: Option<String>,
}

/// The resolved, not yet applied, shape of one install.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub server: String,
    pub index_sha1: String,
    pub resolved_at: SystemTime,
    pub root: PlannedFile,
    pub required: Vec<PlannedDep>,
    pub optional: Vec<PlannedDep>,
    pub blocked: Vec<ManualDownload>,
    pub incompatible: Vec<String>,
    pub unresolvable: Vec<String>,
}

/// Which optional dependencies the user kept; `--no-deps` leaves it empty.
#[derive(Debug, Clone, Default)]
pub struct InstallChoice {
    pub deps: BTreeSet<String>,
}

/// What an applied install actually did.
#[derive(Debug, Clone, Default)]
pub struct InstallReport {
    pub installed: Vec<String>,
    pub skipped_already_installed: Vec<String>,
    pub manual: Vec<ManualDownload>,
    pub rolled_back: bool,
    pub left_behind: Vec<PathBuf>,
}

/// One row of the update preview.
#[derive(Debug, Clone)]
pub struct UpdateCandidate {
    pub file_name: String,
    pub title: String,
    pub from: String,
    pub to: Version,
}

/// What one update check found.
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub candidates: Vec<UpdateCandidate>,
    /// File names of `직접 추가` jars the check recognised by hash and adopted
    /// into the index (§8.4 provenance recovery).
    pub adopted: Vec<String>,
}

/// Where the providers live. `Default` is production; tests point both at an
/// in-process stub so nothing reaches the real APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// Modrinth's host; the `/v2` prefix is added by `modrinth.rs`.
    pub modrinth: String,
    /// CurseForge's versioned base, `https://api.curseforge.com/v1`.
    pub curseforge: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            modrinth: MODRINTH_API.to_owned(),
            curseforge: CURSEFORGE_API.to_owned(),
        }
    }
}

/// The façade every catalog entry point takes first.
pub struct Catalog {
    providers: Vec<Box<dyn Provider>>,
    endpoints: Endpoints,
    cf_key: Option<String>,
    /// CurseForge icons, keyed by project id. Their terms forbid caching API
    /// data on disk, so this dies with the process (D2).
    pub(crate) icon_memory: Mutex<HashMap<String, icons::IconBytes>>,
}

impl Catalog {
    /// Modrinth is always present. CurseForge joins only with the user's own
    /// key (D2); without one every CurseForge surface is simply absent.
    pub fn new(endpoints: Endpoints, cf_key: Option<String>) -> Self {
        let cf_key = cf_key.filter(|key| !key.trim().is_empty());
        let mut providers: Vec<Box<dyn Provider>> =
            vec![Box::new(modrinth::Modrinth::new(&endpoints.modrinth))];
        if let Some(key) = &cf_key {
            providers.push(Box::new(curseforge::CurseForge::new(
                &endpoints.curseforge,
                key,
            )));
        }
        Self {
            providers,
            endpoints,
            cf_key,
            icon_memory: Mutex::new(HashMap::new()),
        }
    }

    /// The provider, or `None` when it is not configured.
    pub fn provider(&self, id: ProviderId) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(std::convert::AsRef::as_ref)
    }

    /// Every configured provider, in preference order.
    pub fn providers(&self) -> impl Iterator<Item = &dyn Provider> {
        self.providers.iter().map(std::convert::AsRef::as_ref)
    }

    /// The user's CurseForge key, for the download path that must send it too.
    pub fn curseforge_key(&self) -> Option<&str> {
        self.cf_key.as_deref()
    }
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("providers", &self.providers.len())
            .field("endpoints", &self.endpoints)
            .field("curseforge_key", &self.cf_key.is_some())
            .finish()
    }
}

/// What the catalog needs to know about one server: which versions fit it and
/// where its mods live.
#[derive(Debug, Clone)]
pub struct ServerContext {
    pub mc: MinecraftVersion,
    pub loader: Loader,
    /// `<server>/<loader>mods`, the container holding the three namespaces.
    pub container: PathBuf,
}

/// Reads a server's Minecraft version off its directory name — the same
/// `-<version>` suffix `list_servers` splits on — and its loader out of
/// `servermeta.json`. A server without a loader has nowhere to put a mod.
pub fn server_context(server: &ServerDir) -> Result<ServerContext, CatalogError> {
    let mc = server
        .dir_name
        .rsplit_once('-')
        .and_then(|(id, mc)| (!id.is_empty()).then_some(mc))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "서버 폴더 {}의 이름이 -<마인크래프트 버전>으로 끝나지 않습니다",
                server.dir_name
            ))
        })?
        .parse::<MinecraftVersion>()?;
    let meta = crate::server::read_server_meta(server)?;
    let (kind, _) = crate::server::server_loader(&meta).ok_or_else(|| {
        Error::Invalid(format!(
            "서버 {}에는 모드 로더가 없습니다 — 모드를 설치하려면 Forge, NeoForge 또는 \
             Fabric 서버를 쓰세요",
            server.dir_name
        ))
    })?;
    Ok(ServerContext {
        mc,
        loader: kind.into(),
        container: kind.mod_container(server),
    })
}

/// Everything the catalog can fail with. The GUI matches on the variant for
/// its copy, never on the message text.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{0}")]
    Network(String),
    #[error("rate limited; retry in {retry_after_secs}s")]
    RateLimited { retry_after_secs: u32 },
    #[error("not found")]
    NotFound,
    /// The project exists but has no file for this server's Minecraft version
    /// and loader — a different answer from [`CatalogError::NotFound`], and the
    /// only one §8.4 has a sentence for (CF-05).
    #[error("{mc} {loader}용 버전이 없습니다")]
    NoVersion { mc: String, loader: Loader },
    /// CurseForge answered 401/403: the key is missing, wrong or revoked. Not
    /// a network failure, and the prescription is the key, not the connection
    /// (CF-04).
    #[error("CurseForge API 키가 거부되었습니다 — 설정의 키를 확인하세요")]
    Unauthorized,
    #[error("{file}: expected {expected}, downloaded {got}")]
    HashMismatch {
        file: String,
        expected: String,
        got: String,
    },
    #[error("{} must be downloaded by hand: {}", .0.name, .0.url)]
    Blocked(ManualDownload),
    #[error("의존성을 찾을 수 없습니다: {dependency}")]
    Unresolvable { dependency: String },
    #[error("{a}와 {b}는 함께 설치할 수 없습니다")]
    Incompatible { a: String, b: String },
    #[error("제공자가 보낸 파일 이름을 쓸 수 없습니다: {0}")]
    BadFileName(String),
    /// The file name is already taken — by another index entry, by another
    /// file of the same install, or by a jar somebody placed by hand (F1).
    #[error("{file}은(는) 이미 있는 파일 이름입니다 — 먼저 그 파일을 지우세요")]
    NameTaken { file: String },
    #[error("다른 Nebula 작업이 이 서버를 사용 중입니다 (pid {pid})")]
    Locked { pid: u32 },
    /// The plan is older than [`install::PLAN_MAX_AGE`]; the caller silently
    /// re-resolves (§8.4).
    #[error("설치 계획이 오래되었습니다 — 다시 시도하세요")]
    Stale,
    /// The server or its index changed under an open plan. Distinct from
    /// [`CatalogError::Stale`], which is only about age: this one is refused
    /// and reported, never silently re-resolved (§8.4).
    #[error("서버가 바뀌었습니다 — 다시 검색하세요")]
    ServerChanged,
    /// A failed install whose rollback could not remove everything it had
    /// already renamed in. The cause is `source`; `left_behind` holds the
    /// absolute paths that survived, none of which has an index entry, so the
    /// next reconcile shows them as `직접 추가` (§8.4).
    #[error("{source}")]
    RolledBack {
        source: Box<CatalogError>,
        left_behind: Vec<PathBuf>,
    },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(#[from] Error),
}

/// Flattened for the CLI: everything becomes one of the `Error` shapes `main`
/// already knows how to print, and every user-visible one is a complete
/// Korean sentence rather than a debug fragment.
impl From<CatalogError> for Error {
    fn from(error: CatalogError) -> Self {
        match error {
            // The wrapper only adds the rollback's leftovers, which the CLI
            // prints separately; the message follows the real cause.
            CatalogError::RolledBack { source, .. } => Error::from(*source),
            // Unwrapped rather than re-wrapped, so a round trip loses nothing.
            CatalogError::Other(inner) => inner,
            CatalogError::Io(source) => Error::Io {
                context: "mod catalog".to_owned(),
                source,
            },
            // `Network` carries `"<url>: <what failed>"`, so the address is
            // already in the message; there is no separate url to put in an
            // `Error::Http`, whose empty-url Display would read as debug text.
            CatalogError::Network(message) => Error::Invalid(format!("네트워크 오류: {message}")),
            CatalogError::RateLimited { retry_after_secs } => Error::Invalid(format!(
                "요청이 너무 많습니다 — {retry_after_secs}초 후 다시 시도하세요"
            )),
            CatalogError::NotFound => {
                Error::Invalid("프로젝트 또는 파일을 찾을 수 없습니다".to_owned())
            }
            CatalogError::HashMismatch { file, .. } => {
                Error::Invalid(format!("다운로드한 파일의 해시가 다릅니다: {file}"))
            }
            CatalogError::Blocked(manual) => Error::Invalid(format!(
                "{}은(는) 제작자가 배포를 막았습니다 — 직접 내려받으세요: {}",
                manual.name, manual.url
            )),
            // The rest already spell complete Korean sentences in `#[error]`.
            rest => Error::Invalid(rest.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, primary: bool) -> VersionFile {
        VersionFile {
            file_name: name.to_owned(),
            url: Some(format!("https://cdn.invalid/{name}")),
            size: 1,
            primary,
            sha1: None,
            sha512: None,
            md5: None,
            fingerprint: None,
            file_id: None,
        }
    }

    fn version(release: ReleaseType, files: Vec<VersionFile>) -> Version {
        Version {
            project_id: "p".into(),
            version_id: "v".into(),
            name: "n".into(),
            number: "1".into(),
            release,
            published: "2026-01-01T00:00:00Z".into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec![Loader::Fabric],
            files,
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn picks_the_primary_file_then_the_first_plain_jar() {
        let flagged = version(
            ReleaseType::Release,
            vec![file("a-sources.jar", false), file("a.jar", true)],
        );
        assert_eq!(flagged.primary_file().unwrap().file_name, "a.jar");

        // Widespread legacy data: no file is flagged primary at all.
        let unflagged = version(
            ReleaseType::Release,
            vec![
                file("a-sources.jar", false),
                file("a-dev.jar", false),
                file("a-javadoc.jar", false),
                file("a.jar", false),
            ],
        );
        assert_eq!(unflagged.primary_file().unwrap().file_name, "a.jar");

        let nothing = version(ReleaseType::Release, vec![file("a.zip", false)]);
        assert!(nothing.primary_file().is_none());
    }

    #[test]
    fn prefers_a_release_over_a_newer_beta() {
        let versions = vec![
            version(ReleaseType::Beta, vec![file("b.jar", true)]),
            version(ReleaseType::Release, vec![file("r.jar", true)]),
        ];
        assert_eq!(
            preferred_version(&versions).unwrap().files[0].file_name,
            "r.jar"
        );
        assert!(preferred_version(&[]).is_none());
    }

    /// CF-03's pure decision: which (provider, project) a second question about
    /// this file goes to. The mirror branch is the only place the two pairs
    /// differ, and getting it wrong sends one provider's id to the other.
    #[test]
    fn the_source_pair_survives_the_mirror_rewrite() {
        let plain = PlannedFile {
            provider: ProviderId::Modrinth,
            project_id: "P7dR8mSH".into(),
            slug: Some("fabric-api".into()),
            title: "Fabric API".into(),
            text_provider: ProviderId::Modrinth,
            source_project_id: None,
            version_id: "v".into(),
            version_name: "n".into(),
            version_number: "0.92.0".into(),
            published: String::new(),
            release: ReleaseType::Release,
            file: file("fabric-api.jar", true),
        };
        assert_eq!(plain.source(), (ProviderId::Modrinth, "P7dR8mSH"));

        // R3: downloaded from Modrinth, picked from CurseForge.
        let mirrored = PlannedFile {
            provider: ProviderId::Modrinth,
            project_id: "mirror".into(),
            slug: None,
            text_provider: ProviderId::CurseForge,
            source_project_id: Some("238222".into()),
            ..plain
        };
        assert_eq!(mirrored.source(), (ProviderId::CurseForge, "238222"));
    }

    #[test]
    fn reads_the_algorithm_off_the_hash_length() {
        assert_eq!(HashAlgo::for_hex(&"a".repeat(40)), HashAlgo::Sha1);
        assert_eq!(HashAlgo::for_hex(&"a".repeat(128)), HashAlgo::Sha512);
        assert_eq!(HashAlgo::for_hex(""), HashAlgo::Sha1);
    }

    #[test]
    fn every_flattened_error_is_a_whole_korean_sentence() {
        let message = |error: CatalogError| Error::from(error).to_string();

        assert_eq!(
            message(CatalogError::NotFound),
            "프로젝트 또는 파일을 찾을 수 없습니다"
        );
        assert_eq!(
            message(CatalogError::RateLimited {
                retry_after_secs: 12
            }),
            "요청이 너무 많습니다 — 12초 후 다시 시도하세요"
        );
        assert_eq!(
            message(CatalogError::HashMismatch {
                file: "sodium.jar".into(),
                expected: "a".into(),
                got: "b".into(),
            }),
            "다운로드한 파일의 해시가 다릅니다: sodium.jar"
        );
        assert_eq!(
            message(CatalogError::ServerChanged),
            "서버가 바뀌었습니다 — 다시 검색하세요"
        );
        // CF-05: a project with no build for this server is not "not found".
        assert_eq!(
            message(CatalogError::NoVersion {
                mc: "1.20.1".into(),
                loader: Loader::Forge,
            }),
            "1.20.1 Forge용 버전이 없습니다"
        );
        // CF-04: a rejected key never reads as a network failure, and never as
        // a bare status code.
        assert_eq!(
            message(CatalogError::Unauthorized),
            "CurseForge API 키가 거부되었습니다 — 설정의 키를 확인하세요"
        );
        for error in [
            CatalogError::Network("https://api.invalid/x: timed out".into()),
            CatalogError::Unauthorized,
            CatalogError::NoVersion {
                mc: "1.21.1".into(),
                loader: Loader::NeoForge,
            },
            CatalogError::Blocked(ManualDownload {
                name: "JEI".into(),
                file_name: "jei.jar".into(),
                url: "https://example.invalid/files/1".into(),
            }),
            CatalogError::BadFileName("../x.jar".into()),
            CatalogError::NameTaken {
                file: "sodium.jar".into(),
            },
            CatalogError::Locked { pid: 7 },
            CatalogError::Stale,
        ] {
            let printed = message(error);
            // No `HTTP :` fragment and no bare English debug text.
            assert!(!printed.starts_with("HTTP"), "{printed}");
            assert!(
                printed.chars().any(|c| ('가'..='힣').contains(&c)),
                "{printed}"
            );
        }

        // The rollback wrapper keeps the cause's own sentence.
        assert_eq!(
            message(CatalogError::RolledBack {
                source: Box::new(CatalogError::NotFound),
                left_behind: vec![PathBuf::from("a.jar")],
            }),
            "프로젝트 또는 파일을 찾을 수 없습니다"
        );
        // An `Error::Http` with no url prints the message alone.
        assert_eq!(
            Error::Http {
                url: String::new(),
                message: "네트워크 오류".into(),
            }
            .to_string(),
            "네트워크 오류"
        );
    }

    #[test]
    fn hides_curseforge_without_a_key() {
        let catalog = Catalog::new(Endpoints::default(), None);
        assert!(catalog.provider(ProviderId::Modrinth).is_some());
        assert!(catalog.provider(ProviderId::CurseForge).is_none());

        // An empty settings field is not a key.
        let blank = Catalog::new(Endpoints::default(), Some("  ".into()));
        assert!(blank.provider(ProviderId::CurseForge).is_none());

        let keyed = Catalog::new(Endpoints::default(), Some("k".into()));
        assert!(keyed.provider(ProviderId::CurseForge).is_some());
        assert_eq!(keyed.curseforge_key(), Some("k"));
    }
}
