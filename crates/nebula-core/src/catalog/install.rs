//! Resolving, planning and applying one install.
//!
//! Nothing here writes into a namespace directory until the bytes are on disk
//! and verified: every download is staged in `<server>/.nebula-tmp/`, hashed,
//! and only then renamed across (reliability rule 1). A failure other than a
//! blocked CurseForge file aborts with a best-effort rollback.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::index::{IndexEntry, IndexHashes, ModIndex};
use super::jarmeta::{self, LocalFile};
use super::lock::ServerLock;
use super::{
    Catalog, CatalogError, DependencyKind, HashAlgo, InstallChoice, InstallPlan, InstallReport,
    Loader, ManualDownload, PlannedDep, PlannedFile, ProjectHit, Provider, ProviderId,
    ServerContext, Version, VersionFile, preferred_version, server_context,
};
use crate::error::Error;
use crate::hash;
use crate::layout::ServerDir;
use crate::scan::{Namespace, discover_toggleable};
use crate::server::namespace_dir;
use crate::version::MinecraftVersion;

/// Staging lives inside the server directory, never in `<root>/temp`, which a
/// 1.7-1.12 Forge build deletes wholesale (reliability rule 1).
pub const TMP_DIR: &str = ".nebula-tmp";
/// Leftover staging directories older than this are swept before a mutation.
pub const STALE_TMP: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a resolved plan may sit before `apply` refuses it.
pub const PLAN_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// How deep the required-dependency walk goes (D6).
pub const MAX_DEPTH: usize = 6;

/// Windows refuses these as file names whatever the extension, so `CON.jar`
/// is as unusable as `CON` (R6).
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes every staging directory of one process unique without a `rand`
/// dependency: pid plus nanoseconds plus a counter.
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// The version D3 would install: newest release, else newest beta, else newest
/// alpha. `None` means the project has nothing for this Minecraft version and
/// loader, which the caller reports rather than treating as an error.
pub fn resolve_version(
    catalog: &Catalog,
    provider: ProviderId,
    project_id: &str,
    mc: &MinecraftVersion,
    loader: Loader,
) -> Result<Option<Version>, CatalogError> {
    let versions = require_provider(catalog, provider)?.versions(project_id, mc, loader)?;
    Ok(preferred_version(&versions).cloned())
}

/// The configured provider, or the refusal D2 asks for when CurseForge has no
/// key of the user's own.
pub fn require_provider(
    catalog: &Catalog,
    provider: ProviderId,
) -> Result<&dyn Provider, CatalogError> {
    catalog.provider(provider).ok_or_else(|| {
        Error::Invalid(
            "CurseForge API 키가 없습니다 — 설정 화면의 CurseForge API 키에 값을 입력하세요 \
             (console.curseforge.com)"
                .to_owned(),
        )
        .into()
    })
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Resolves one install completely without touching the mod directories: the
/// root version, its required dependencies recursively (D6), the optional
/// ones, and the CurseForge files nobody may download for the user (R3).
///
/// `version` names an exact version id; `None` applies the D3 ladder.
pub fn plan_install(
    catalog: &Catalog,
    server: &ServerDir,
    provider: ProviderId,
    project_id: &str,
    version: Option<&str>,
) -> Result<InstallPlan, CatalogError> {
    let ctx = server_context(server)?;
    let index = ModIndex::load(server)?;
    let mut installed = Installed::new(&index, &ctx.container);
    let source = require_provider(catalog, provider)?;

    let hit = source.project(project_id)?;
    let versions = source.versions(&hit.project_id, &ctx.mc, ctx.loader)?;
    let root_version = match version {
        Some(wanted) => versions
            .into_iter()
            .find(|candidate| candidate.version_id == wanted),
        None => preferred_version(&versions).cloned(),
    }
    .ok_or(CatalogError::NotFound)?;

    let root = planned_file(catalog, provider, &hit, &root_version)?;
    let mut plan = InstallPlan {
        server: server.dir_name.clone(),
        index_sha1: index_sha1(server)?,
        resolved_at: SystemTime::now(),
        root,
        required: Vec::new(),
        optional: Vec::new(),
        blocked: Vec::new(),
        incompatible: Vec::new(),
        unresolvable: Vec::new(),
    };
    collect_blocked(&plan.root, &hit, &mut plan.blocked);

    let mut seen: BTreeSet<String> = BTreeSet::from([hit.project_id.clone()]);
    // Breadth first, so `seen` records every project at its *shortest* depth
    // and the D6 bound means what it says: a diamond whose long arm is deeper
    // than `MAX_DEPTH` is still resolved through its short one.
    let mut frontier = VecDeque::from([(root_version.dependencies.clone(), 1usize)]);
    while let Some((dependencies, depth)) = frontier.pop_front() {
        for dependency in &dependencies {
            let Some(dep_id) = dependency.project_id.clone() else {
                // A Modrinth dependency may name only a version id, which no
                // endpoint of ours resolves; a required one is never dropped.
                if dependency.kind == DependencyKind::Required {
                    plan.unresolvable
                        .push(dependency.version_id.clone().unwrap_or_default());
                }
                continue;
            };
            if !seen.insert(dep_id.clone()) {
                continue;
            }
            match dependency.kind {
                // Already inside the jar; installing it would duplicate a mod id.
                DependencyKind::Embedded => continue,
                DependencyKind::Incompatible => {
                    // The slug is what lets a hand-placed jar be recognised by
                    // its mod id, exactly as the required/optional arm does.
                    // Only the project is fetched, never its versions: a
                    // conflicting mod may have no build for this server's
                    // Minecraft version and still be sitting in the directory,
                    // which is the case this branch exists for.
                    let hit = match source.project(&dep_id) {
                        Ok(hit) => hit,
                        Err(CatalogError::NotFound) => continue,
                        Err(error) => return Err(error),
                    };
                    if let Some(file) = installed.find(provider, &hit.project_id, Some(&hit.slug)) {
                        plan.incompatible.push(file);
                    }
                    continue;
                }
                DependencyKind::Required | DependencyKind::Optional => {}
            }

            let Some((dep_hit, dep_version)) = resolve_dependency(source, &dep_id, &ctx)? else {
                if dependency.kind == DependencyKind::Required {
                    plan.unresolvable.push(dep_id);
                }
                continue;
            };
            let already = installed.find(provider, &dep_hit.project_id, Some(&dep_hit.slug));
            let file = planned_file(catalog, provider, &dep_hit, &dep_version)?;
            if already.is_none() {
                collect_blocked(&file, &dep_hit, &mut plan.blocked);
            }
            let planned = PlannedDep {
                file,
                kind: dependency.kind,
                already_installed: already.clone(),
            };
            match dependency.kind {
                DependencyKind::Required => {
                    // An installed dependency's own tree came with it;
                    // recursing would re-resolve the graph for nothing.
                    if already.is_none() && depth < MAX_DEPTH {
                        frontier.push_back((dep_version.dependencies.clone(), depth + 1));
                    }
                    plan.required.push(planned);
                }
                // D6 lists optional dependencies; it does not walk them.
                _ => plan.optional.push(planned),
            }
        }
    }
    Ok(plan)
}

/// The digest `apply` re-checks the index against: the bytes of
/// `modindex.json`, or of nothing at all when the server has no index yet.
/// One function, because a second convention would make every plan stale.
pub fn index_sha1(server: &ServerDir) -> Result<String, CatalogError> {
    let raw = match std::fs::read(server.modindex_file()) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    Ok(hash::digests_of_bytes(&raw).sha1)
}

/// R6: the provider's `filename` is written verbatim, but only after a check
/// stricter than [`crate::paths::is_single_segment`]. A failing name is a hard
/// failure and never sanitised — the name is the index key.
pub fn check_file_name(name: &str) -> Result<(), CatalogError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.starts_with([' ', '.'])
        || name.ends_with([' ', '.'])
        || !name.to_lowercase().ends_with(".jar")
        || crate::scan::FILE_NAME_BLACKLIST.contains(&name)
        || RESERVED
            .iter()
            .any(|reserved| stem(name).eq_ignore_ascii_case(reserved));
    match bad {
        true => Err(CatalogError::BadFileName(name.to_owned())),
        false => Ok(()),
    }
}

/// The part before the first dot, which is what Windows compares against its
/// device names.
fn stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// Refuses a file name that is already taken, for every meaning of "taken".
///
/// The index is keyed by file name, so writing over one would overwrite that
/// jar on disk **and** drop its provenance in one silent step — the one way
/// two projects can still collide once (provider, project id) says they are
/// different. The same name can also be taken by a jar somebody placed by
/// hand, which the index has never heard of, or by another file of the very
/// apply that is running: every name is checked before the first
/// `index.mods.insert`, so the index alone cannot see that one either.
///
/// `replacing` names the file a replace is allowed to take over — including
/// the ordinary case where the new version carries the same name as the old.
pub(crate) fn check_name_free(
    index: &ModIndex,
    container: &Path,
    claimed: &BTreeSet<String>,
    file_name: &str,
    replacing: Option<&str>,
) -> Result<(), CatalogError> {
    if replacing == Some(file_name) {
        return Ok(());
    }
    let taken = index.mods.contains_key(file_name)
        || claimed.contains(file_name)
        // Cross-namespace matters: two files with one name would hand
        // `discover_toggleable` two modules that cannot both be shipped.
        // `is_file`, not `exists`: anything else sitting on that path is not
        // a mod somebody would lose, and the rename refuses it anyway.
        || crate::scan::NAMESPACES
            .iter()
            .any(|ns| namespace_dir(container, *ns).join(file_name).is_file());
    match taken {
        true => Err(CatalogError::NameTaken {
            file: file_name.to_owned(),
        }),
        false => Ok(()),
    }
}

/// Builds the provenance one file is installed with. A CurseForge file the
/// author blocked is retried on Modrinth by its sha1 first (R3); only when
/// that misses does it stay blocked.
fn planned_file(
    catalog: &Catalog,
    provider: ProviderId,
    hit: &ProjectHit,
    version: &Version,
) -> Result<PlannedFile, CatalogError> {
    let file = version
        .primary_file()
        .ok_or(CatalogError::NotFound)?
        .clone();
    if file.url.is_none()
        && let Some(sha1) = file.sha1.as_deref()
        && let Some(modrinth) = catalog.provider(ProviderId::Modrinth)
        && let Some(mirrored) = modrinth_by_sha1(modrinth, sha1)
        && let Some(mirrored_file) = mirrored.primary_file().filter(|f| f.url.is_some())
    {
        log::info!(
            "{} is blocked on CurseForge; using Modrinth's copy",
            hit.title
        );
        return Ok(PlannedFile {
            provider: ProviderId::Modrinth,
            project_id: mirrored.project_id.clone(),
            // The Modrinth slug is unknown here and a CurseForge one would be
            // the wrong provider's; the title still comes from the hit, which
            // is why `text_provider` stays CurseForge — the download moved
            // provider, the free text did not, and D2 keys on the latter.
            slug: None,
            title: hit.title.clone(),
            text_provider: provider,
            version_id: mirrored.version_id.clone(),
            version_name: mirrored.name.clone(),
            version_number: mirrored.number.clone(),
            published: mirrored.published.clone(),
            release: mirrored.release,
            file: mirrored_file.clone(),
        });
    }
    Ok(PlannedFile {
        provider,
        project_id: hit.project_id.clone(),
        slug: Some(hit.slug.clone()),
        title: hit.title.clone(),
        text_provider: provider,
        version_id: version.version_id.clone(),
        version_name: version.name.clone(),
        version_number: version.number.clone(),
        published: version.published.clone(),
        release: version.release,
        file,
    })
}

/// A lookup failure here is not the install's failure: the file is simply not
/// mirrored, and the user gets a manual download link instead.
fn modrinth_by_sha1(modrinth: &dyn Provider, sha1: &str) -> Option<Version> {
    match modrinth.versions_by_hashes(HashAlgo::Sha1, &[sha1.to_lowercase()]) {
        Ok(found) => found.into_values().next(),
        Err(error) => {
            log::debug!("Modrinth sha1 fallback for {sha1} failed: {error}");
            None
        }
    }
}

/// Moves a file nobody may download for the user into the manual list, before
/// any request is made.
fn collect_blocked(planned: &PlannedFile, hit: &ProjectHit, blocked: &mut Vec<ManualDownload>) {
    if planned.file.url.is_some() {
        return;
    }
    blocked.push(ManualDownload {
        name: planned.title.clone(),
        file_name: planned.file.file_name.clone(),
        url: manual_url(hit, planned.file.file_id.as_deref()),
    });
}

/// The project's own web page plus the file id. **Never an `edge.forgecdn.net`
/// path**: reconstructing the CDN URL works even for files whose author opted
/// out of third-party distribution, and doing it is forbidden (R4).
fn manual_url(hit: &ProjectHit, file_id: Option<&str>) -> String {
    let page = hit.page_url.clone().unwrap_or_else(|| {
        format!(
            "https://www.curseforge.com/minecraft/mc-mods/{}",
            hit.slug.trim()
        )
    });
    match file_id {
        Some(id) => format!("{}/files/{id}", page.trim_end_matches('/')),
        None => page,
    }
}

/// A dependency that no longer exists, or has no build for this Minecraft
/// version and loader, is `None` — the caller decides whether that is fatal.
fn resolve_dependency(
    source: &dyn Provider,
    project_id: &str,
    ctx: &ServerContext,
) -> Result<Option<(ProjectHit, Version)>, CatalogError> {
    let hit = match source.project(project_id) {
        Ok(hit) => hit,
        Err(CatalogError::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    let versions = match source.versions(&hit.project_id, &ctx.mc, ctx.loader) {
        Ok(versions) => versions,
        Err(CatalogError::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(preferred_version(&versions)
        .cloned()
        .map(|version| (hit, version)))
}

/// The D6 "already installed" test: the index holds the project in any
/// namespace, or a jar in any namespace reports a matching mod id.
struct Installed<'a> {
    by_project: BTreeMap<(ProviderId, String), String>,
    container: &'a Path,
    /// Read on the first slug miss only.
    ///
    /// ponytail: `jarmeta::read` opens and hashes every jar four times, so a
    /// 300-mod server pays for that once per plan; a `mod_id`-only reader in
    /// `jarmeta` is the upgrade if planning ever feels slow.
    jar_mods: Option<BTreeMap<String, String>>,
}

impl<'a> Installed<'a> {
    fn new(index: &ModIndex, container: &'a Path) -> Self {
        Self {
            by_project: index
                .mods
                .iter()
                .map(|(file_name, entry)| {
                    (
                        (entry.provider, entry.project_id.clone()),
                        file_name.clone(),
                    )
                })
                .collect(),
            container,
            jar_mods: None,
        }
    }

    /// The installed file name, or `None`.
    fn find(
        &mut self,
        provider: ProviderId,
        project_id: &str,
        slug: Option<&str>,
    ) -> Option<String> {
        if let Some(file) = self.by_project.get(&(provider, project_id.to_owned())) {
            return Some(file.clone());
        }
        let slug = slug?;
        self.jar_mods().get(&normalise(slug)).cloned()
    }

    fn jar_mods(&mut self) -> &BTreeMap<String, String> {
        let container = self.container;
        self.jar_mods.get_or_insert_with(|| {
            let mut ids = BTreeMap::new();
            let Ok(discovered) = discover_toggleable(container) else {
                return ids;
            };
            for (_, candidate) in discovered {
                if let Ok(LocalFile::Mod(local)) = jarmeta::read(&candidate.path) {
                    ids.insert(normalise(&local.mod_id), candidate.file_name.clone());
                }
            }
            ids
        })
    }
}

/// Best-effort slug matching: `fabric-api` and `fabric_api` are one mod id.
fn normalise(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// Downloads and installs a plan under the server lock.
///
/// `chosen` names the dependency projects the user kept, so `--no-deps` is an
/// empty set. `progress` is called once before each file with the 1-based
/// index, the total and the file name.
pub fn apply(
    catalog: &Catalog,
    server: &ServerDir,
    plan: &InstallPlan,
    chosen: &InstallChoice,
    ns: Namespace,
    progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
) -> Result<InstallReport, CatalogError> {
    let ctx = server_context(server)?;
    let _lock = ServerLock::acquire(server)?;
    sweep_staging(server);

    // §8.4 keeps these two apart: a server that moved under an open plan is
    // refused and reported, while a merely old plan is re-resolved silently.
    if plan.server != server.dir_name || index_sha1(server)? != plan.index_sha1 {
        return Err(CatalogError::ServerChanged);
    }
    match plan.resolved_at.elapsed() {
        Ok(age) if age <= PLAN_MAX_AGE => {}
        _ => return Err(CatalogError::Stale),
    }

    let mut index = ModIndex::load(server)?;
    let held: BTreeMap<(ProviderId, String), String> = index
        .mods
        .iter()
        .map(|(file, entry)| ((entry.provider, entry.project_id.clone()), file.clone()))
        .collect();

    let mut report = InstallReport {
        manual: plan.blocked.clone(),
        ..InstallReport::default()
    };
    let required_ids: Vec<String> = plan
        .required
        .iter()
        .map(|dep| dep.file.project_id.clone())
        .collect();
    for dep in plan.required.iter().chain(&plan.optional) {
        if let Some(file) = &dep.already_installed
            && chosen.deps.contains(&dep.file.project_id)
        {
            report.skipped_already_installed.push(file.clone());
        }
    }

    // Fresh files go in first; the replace is last, because its own three-step
    // ordering is the only thing standing between a crash and a duplicate jar.
    let mut fresh: Vec<(&PlannedFile, bool)> = Vec::new();
    let mut replace: Option<(&PlannedFile, String)> = None;
    // Every name is checked before the first `index.mods.insert`, so the
    // index cannot yet see the files this very apply is about to write.
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for (planned, is_root) in wanted(plan, chosen) {
        if planned.file.url.is_none() {
            continue; // Blocked; already listed in `report.manual`.
        }
        check_file_name(&planned.file.file_name)?;
        match held.get(&(planned.provider, planned.project_id.clone())) {
            // Re-checked under the lock, as D6 requires.
            Some(old) => match is_root {
                true => {
                    check_name_free(
                        &index,
                        &ctx.container,
                        &claimed,
                        &planned.file.file_name,
                        Some(old),
                    )?;
                    replace = Some((planned, old.clone()));
                    claimed.insert(planned.file.file_name.clone());
                }
                false => report.skipped_already_installed.push(old.clone()),
            },
            None => {
                check_name_free(
                    &index,
                    &ctx.container,
                    &claimed,
                    &planned.file.file_name,
                    None,
                )?;
                fresh.push((planned, is_root));
                claimed.insert(planned.file.file_name.clone());
            }
        }
    }

    let total = fresh.len() + usize::from(replace.is_some());
    let staging = staging_dir(server)?;
    let mut renamed: Vec<PathBuf> = Vec::new();
    let mut step = 0;

    for (planned, is_root) in fresh {
        step += 1;
        progress(step, total, &planned.file.file_name);
        let dependencies = match is_root {
            true => required_ids.clone(),
            false => Vec::new(),
        };
        match install_one(catalog, &ctx, &staging, planned, ns, dependencies) {
            Ok((path, entry)) => {
                index.mods.insert(planned.file.file_name.clone(), entry);
                report.installed.push(planned.file.file_name.clone());
                renamed.push(path);
            }
            Err(error) => return Err(abort(error, &renamed, &staging)),
        }
    }

    if let Some((planned, old)) = replace {
        step += 1;
        progress(step, total, &planned.file.file_name);
        if let Err(error) = super::update::replace_file_locked(
            catalog,
            &ctx,
            &staging,
            &mut index,
            &old,
            planned,
            required_ids,
        ) {
            return Err(abort(error, &renamed, &staging));
        }
        report.installed.push(planned.file.file_name.clone());
    }

    index.save(server)?;
    discard(&staging);
    Ok(report)
}

/// The dependencies the user kept, then the root — so the one file that may
/// have to replace an installed version is handled last.
fn wanted<'a>(plan: &'a InstallPlan, chosen: &InstallChoice) -> Vec<(&'a PlannedFile, bool)> {
    let mut files: Vec<(&PlannedFile, bool)> = plan
        .required
        .iter()
        .chain(&plan.optional)
        .filter(|dep| dep.already_installed.is_none())
        .filter(|dep| chosen.deps.contains(&dep.file.project_id))
        .map(|dep| (&dep.file, false))
        .collect();
    files.push((&plan.root, true));
    files
}

/// Stages, verifies and renames one file into its namespace directory.
fn install_one(
    catalog: &Catalog,
    ctx: &ServerContext,
    staging: &Path,
    planned: &PlannedFile,
    ns: Namespace,
    dependencies: Vec<String>,
) -> Result<(PathBuf, IndexEntry), CatalogError> {
    let (staged, hashes) = fetch_verified(catalog, planned.provider, &planned.file, staging)?;
    let destination = namespace_dir(&ctx.container, ns).join(&planned.file.file_name);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&staged, &destination)?;
    Ok((destination, entry_for(planned, hashes, ns, dependencies)))
}

/// The index row one installed file gets.
///
/// Free text is dropped when it **came from** CurseForge, not when it will be
/// downloaded from there: a blocked file mirrored by Modrinth still carries
/// CurseForge's title, and `IndexEntry::with_text`'s own guard keys on the
/// entry's provider, which by then reads Modrinth (D2).
pub(crate) fn entry_for(
    planned: &PlannedFile,
    hashes: IndexHashes,
    ns: Namespace,
    dependencies: Vec<String>,
) -> IndexEntry {
    let (slug, title) = match planned.text_provider {
        ProviderId::CurseForge => (None, None),
        _ => (planned.slug.clone(), Some(planned.title.clone())),
    };
    let mut entry = IndexEntry::new(
        planned.provider,
        planned.project_id.clone(),
        planned.version_id.clone(),
        hashes,
        ns,
    )
    .with_text(slug, title, None);
    entry.file_id = planned.file.file_id.clone();
    entry.release = Some(planned.release);
    entry.dependencies = dependencies;
    entry
}

/// Downloads one file into `staging` and verifies it: sha512 when the provider
/// declared one, else sha1, else md5, else CurseForge's fingerprint. A file
/// with no digest at all is refused rather than trusted.
pub(crate) fn fetch_verified(
    catalog: &Catalog,
    provider: ProviderId,
    file: &VersionFile,
    staging: &Path,
) -> Result<(PathBuf, IndexHashes), CatalogError> {
    let url = file.url.as_deref().ok_or_else(|| {
        CatalogError::Blocked(ManualDownload {
            name: file.file_name.clone(),
            file_name: file.file_name.clone(),
            url: String::new(),
        })
    })?;
    let destination = staging.join(&file.file_name);
    let headers = provider_headers(catalog, provider);
    super::http::download(provider, url, &headers, &destination)?;

    let digests = hash::digests_of_file(&destination)?;
    let sha512 = match file.sha512.is_some() {
        true => Some(hash::sha512_hex_of_file(&destination)?),
        false => None,
    };
    let verdict = if let (Some(expected), Some(got)) = (&file.sha512, &sha512) {
        Some((expected.clone(), got.clone()))
    } else if let Some(expected) = &file.sha1 {
        Some((expected.clone(), digests.sha1.clone()))
    } else if let Some(expected) = &file.md5 {
        Some((expected.clone(), digests.md5.clone()))
    } else if let Some(expected) = file.fingerprint {
        let got = hash::curseforge_fingerprint_of_file(&destination)?;
        Some((expected.to_string(), got.to_string()))
    } else {
        None
    };
    let Some((expected, got)) = verdict else {
        let _ = std::fs::remove_file(&destination);
        return Err(Error::Invalid(format!(
            "해시 없는 파일은 설치하지 않습니다: {}",
            file.file_name
        ))
        .into());
    };
    if !expected.eq_ignore_ascii_case(&got) {
        let _ = std::fs::remove_file(&destination);
        return Err(CatalogError::HashMismatch {
            file: file.file_name.clone(),
            expected,
            got,
        });
    }
    Ok((
        destination,
        IndexHashes {
            sha1: digests.sha1,
            sha512,
            md5: Some(digests.md5),
        },
    ))
}

/// CurseForge wants its key on the CDN too, and it has to survive the redirect
/// to `mediafilez.forgecdn.net` (R2).
pub(crate) fn provider_headers(catalog: &Catalog, provider: ProviderId) -> Vec<(&str, &str)> {
    match (provider, catalog.curseforge_key()) {
        (ProviderId::CurseForge, Some(key)) => vec![("x-api-key", key)],
        _ => Vec::new(),
    }
}

/// Removes what an aborted apply had already renamed in, and wraps the cause
/// with whatever survived. The index is never saved on this path, so a file
/// left behind carries no entry and the next reconcile shows it as `직접 추가`.
fn abort(cause: CatalogError, renamed: &[PathBuf], staging: &Path) -> CatalogError {
    let mut left_behind = Vec::new();
    for path in renamed {
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::error!("Could not roll back {}: {error}", path.display());
            left_behind.push(path.clone());
        }
    }
    discard(staging);
    match left_behind.is_empty() {
        true => cause,
        false => CatalogError::RolledBack {
            source: Box::new(cause),
            left_behind,
        },
    }
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

/// A fresh `<server>/.nebula-tmp/<pid>-<unique>/`, on the same volume as the
/// namespace directories so the final move is a rename.
pub(crate) fn staging_dir(server: &ServerDir) -> Result<PathBuf, CatalogError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = server
        .path
        .join(TMP_DIR)
        .join(format!("{}-{nanos:09}{sequence:04}", std::process::id()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Deletes one staging directory; a leftover is swept by the next mutation.
pub(crate) fn discard(staging: &Path) {
    if let Err(error) = std::fs::remove_dir_all(staging)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("Could not remove {}: {error}", staging.display());
    }
}

/// Removes staging directories older than a day at the start of a mutation
/// (reliability rule 1). Failures are logged, never fatal.
pub(crate) fn sweep_staging(server: &ServerDir) {
    let root = server.path.join(TMP_DIR);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_TMP);
        if !stale {
            continue;
        }
        let path = entry.path();
        log::debug!("Sweeping stale staging directory {}", path.display());
        let removed = match path.is_dir() {
            true => std::fs::remove_dir_all(&path),
            false => std::fs::remove_file(&path),
        };
        if let Err(error) = removed {
            log::warn!("Could not sweep {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_every_file_name_the_index_cannot_key_on() {
        for name in [
            "",
            ".",
            "..",
            "../../../servermeta.json",
            "a/b.jar",
            "a\\b.jar",
            "C:x.jar",
            "CON.jar",
            "con.JAR",
            "lpt9.jar",
            " sodium.jar",
            "sodium.jar ",
            ".sodium.jar",
            "sodium.jar.",
            "notes.txt",
            ".gitkeep",
        ] {
            assert!(check_file_name(name).is_err(), "{name} should be refused");
        }
        // The Sodium pair: a literal `%2B` and a literal `+` are both legal
        // names and neither is ever decoded into the other.
        for name in [
            "sodium-fabric-mc1.20.1-0.4.10%2Bbuild.24.jar",
            "sodium-fabric-mc1.20.1-0.4.10+build.24.jar",
            "한글 모드.jar",
            "JEI-1.20.1-forge-15.3.0.4.jar",
        ] {
            check_file_name(name).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn normalises_slugs_for_the_jar_id_match() {
        assert_eq!(normalise("fabric-api"), normalise("fabric_api"));
        assert_eq!(normalise("JEI"), "jei");
    }
}
