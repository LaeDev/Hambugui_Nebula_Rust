//! Update checking and the one ordering that changes a managed file's version.
//!
//! [`replace_file`] is the **only** way a managed jar changes version — update
//! apply, `교체` and downgrades all go through it — because its three steps
//! guarantee the namespace directory never holds two versions of one mod at
//! any instant.

use std::collections::BTreeMap;

use super::index::{IndexEntry, IndexHashes, ModIndex};
use super::install;
use super::lock::ServerLock;
use super::{
    Catalog, CatalogError, CheckReport, HashAlgo, PlannedFile, Provider, ProviderId, ReleaseType,
    ServerContext, UpdateCandidate, Version, preferred_version, server_context,
};
use crate::error::Error;
use crate::hash;
use crate::layout::ServerDir;
use crate::scan::{Candidate, Namespace, discover_toggleable};
use crate::server::namespace_dir;

/// Every managed mod with a newer file for this server's Minecraft version and
/// loader (D7).
///
/// Modrinth answers in one bulk request; CurseForge needs one filtered
/// `/files` call per managed mod, spaced by the shared 250 ms throttle (R7),
/// because its fingerprint matches are unfiltered by version. "Is this an
/// update" is a hash difference, never a version-id one (R12).
pub fn check_updates(catalog: &Catalog, server: &ServerDir) -> Result<CheckReport, CatalogError> {
    let ctx = server_context(server)?;
    let discovered = discover_toggleable(&ctx.container).map_err(CatalogError::Other)?;
    let mut index = ModIndex::load(server)?;

    // Hand-placed jars the providers can still recognise become managed before
    // anything is compared, so the very same run reports their updates.
    let adopted = adopt_unmanaged(catalog, server, &discovered, &mut index)?;

    let present: std::collections::BTreeSet<&str> = discovered
        .iter()
        .map(|(_, candidate)| candidate.file_name.as_str())
        .collect();
    let managed: Vec<(&String, &IndexEntry)> = index
        .mods
        .iter()
        // An entry whose file is gone is `파일 없음`, not out of date.
        .filter(|(file_name, _)| present.contains(file_name.as_str()))
        .collect();

    let mut candidates = Vec::new();

    let modrinth_hashes: Vec<String> = managed
        .iter()
        .filter(|(_, entry)| entry.provider == ProviderId::Modrinth)
        .map(|(_, entry)| entry.hashes.sha1.to_lowercase())
        .filter(|hash| !hash.is_empty())
        .collect();
    if !modrinth_hashes.is_empty()
        && let Some(modrinth) = catalog.provider(ProviderId::Modrinth)
    {
        let newer =
            modrinth.updates_by_hashes(HashAlgo::Sha1, &modrinth_hashes, &ctx.mc, ctx.loader)?;
        for (file_name, entry) in &managed {
            if entry.provider != ProviderId::Modrinth {
                continue;
            }
            let Some(version) = newer.get(&entry.hashes.sha1.to_lowercase()) else {
                continue;
            };
            // Modrinth's bulk endpoint answers with the newest file of any
            // kind, so it happily walks a stable server onto a beta. D3 is
            // ours to apply here as it is everywhere else.
            let installed = entry.release.unwrap_or(ReleaseType::Release);
            let offered = match version.release > installed {
                false => Some(version.clone()),
                true => acceptable_version(modrinth, entry, &ctx, installed)?,
            };
            if let Some(candidate) = offered.and_then(|v| candidate(file_name, entry, &v)) {
                candidates.push(candidate);
            }
        }
    }

    if let Some(curseforge) = catalog.provider(ProviderId::CurseForge) {
        for (file_name, entry) in &managed {
            if entry.provider != ProviderId::CurseForge {
                continue;
            }
            // One project that has since been taken down must not cost the
            // user every other mod's answer; the planner already treats a
            // 404 this way (`install::resolve_dependency`).
            let latest = match curseforge.latest_file(&entry.project_id, &ctx.mc, ctx.loader) {
                Ok(latest) => latest,
                Err(CatalogError::NotFound) => {
                    log::warn!(
                        "CurseForge project {} ({file_name}) is gone; skipping it in the update \
                         check",
                        entry.project_id
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            let Some(version) = latest else { continue };
            if let Some(candidate) = candidate(file_name, entry, &version) {
                candidates.push(candidate);
            }
        }
    }

    candidates.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(CheckReport {
        candidates,
        adopted,
    })
}

/// The newest version this entry may be moved to when the bulk answer was a
/// worse release type than what is installed: the D3 ladder over the project's
/// own version list, and only when it does not itself drop a rank.
fn acceptable_version(
    provider: &dyn Provider,
    entry: &IndexEntry,
    ctx: &ServerContext,
    installed: ReleaseType,
) -> Result<Option<Version>, CatalogError> {
    let versions = match provider.versions(&entry.project_id, &ctx.mc, ctx.loader) {
        Ok(versions) => versions,
        Err(CatalogError::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(preferred_version(&versions)
        .filter(|version| version.release <= installed)
        .cloned())
}

/// Recognises `직접 추가` jars by their digests and writes the matches into the
/// index as managed entries (§8.4's provenance recovery).
///
/// One bulk request per provider, never one per jar: Modrinth by sha1 and
/// CurseForge by fingerprint, which is the only thing it identifies files by.
/// A CurseForge match keeps no free text (D2) — the jar's own name is the
/// title anyway.
fn adopt_unmanaged(
    catalog: &Catalog,
    server: &ServerDir,
    discovered: &[(Namespace, Candidate)],
    index: &mut ModIndex,
) -> Result<Vec<String>, CatalogError> {
    let unmanaged: Vec<&(Namespace, Candidate)> = discovered
        .iter()
        .filter(|(_, candidate)| !index.mods.contains_key(&candidate.file_name))
        .filter(|(_, candidate)| candidate.file_name.to_lowercase().ends_with(".jar"))
        .collect();
    if unmanaged.is_empty() {
        return Ok(Vec::new());
    }

    // One pass over each jar, for both lookups.
    let mut scanned: Vec<(Namespace, &Candidate, hash::Digests)> =
        Vec::with_capacity(unmanaged.len());
    for (namespace, candidate) in unmanaged {
        match hash::digests_of_file(&candidate.path) {
            Ok(found) => scanned.push((*namespace, candidate, found)),
            Err(error) => log::debug!("Cannot hash {}: {error}", candidate.path.display()),
        }
    }
    if scanned.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches: BTreeMap<&str, (Namespace, ProviderId, Version, &hash::Digests)> =
        BTreeMap::new();
    if let Some(modrinth) = catalog.provider(ProviderId::Modrinth) {
        let hashes: Vec<String> = scanned
            .iter()
            .map(|(_, _, found)| found.sha1.to_lowercase())
            .collect();
        match modrinth.versions_by_hashes(HashAlgo::Sha1, &hashes) {
            Ok(found) => {
                for (namespace, candidate, digest) in &scanned {
                    if let Some(version) = found.get(&digest.sha1.to_lowercase()) {
                        matches.insert(
                            candidate.file_name.as_str(),
                            (*namespace, ProviderId::Modrinth, version.clone(), digest),
                        );
                    }
                }
            }
            // Adoption is a convenience; a failed lookup must not fail the
            // update check the user actually asked for.
            Err(error) => log::debug!("Modrinth could not identify the unmanaged jars: {error}"),
        }
    }
    if let Some(curseforge) = catalog.provider(ProviderId::CurseForge) {
        // The fingerprint costs two more passes over the file, so it is only
        // computed for what Modrinth did not already recognise.
        let rest: Vec<(Namespace, &Candidate, &hash::Digests, u32)> =
            scanned
                .iter()
                .filter(|(_, candidate, _)| !matches.contains_key(candidate.file_name.as_str()))
                .filter_map(|(namespace, candidate, digest)| {
                    match hash::curseforge_fingerprint_of_file(&candidate.path) {
                        Ok(fingerprint) => Some((*namespace, *candidate, digest, fingerprint)),
                        Err(error) => {
                            log::debug!("Cannot fingerprint {}: {error}", candidate.path.display());
                            None
                        }
                    }
                })
                .collect();
        if !rest.is_empty() {
            let fingerprints: Vec<String> = rest
                .iter()
                .map(|(_, _, _, fingerprint)| fingerprint.to_string())
                .collect();
            match curseforge.versions_by_hashes(HashAlgo::Sha1, &fingerprints) {
                Ok(found) => {
                    for (namespace, candidate, digest, fingerprint) in rest {
                        if let Some(version) = found.get(&fingerprint.to_string()) {
                            matches.insert(
                                candidate.file_name.as_str(),
                                (namespace, ProviderId::CurseForge, version.clone(), digest),
                            );
                        }
                    }
                }
                Err(error) => {
                    log::debug!("CurseForge could not identify the unmanaged jars: {error}");
                }
            }
        }
    }
    if matches.is_empty() {
        return Ok(Vec::new());
    }

    // The index is only ever written under the lock (reliability rule 6). A
    // refused lock is not this job's failure: the check is a read-only job the
    // user asked for, so the adoption simply waits for the next run.
    let lock = match ServerLock::acquire(server) {
        Ok(lock) => lock,
        Err(error) => {
            log::info!("Not adopting unmanaged jars right now: {error}");
            return Ok(Vec::new());
        }
    };
    *index = ModIndex::load(server)?;
    let mut adopted = Vec::new();
    for (file_name, (namespace, provider, version, digest)) in matches {
        if index.mods.contains_key(file_name) {
            continue;
        }
        let mut entry = IndexEntry::new(
            provider,
            version.project_id.clone(),
            version.version_id.clone(),
            IndexHashes {
                sha1: digest.sha1.clone(),
                sha512: None,
                md5: Some(digest.md5.clone()),
            },
            namespace,
        );
        entry.release = Some(version.release);
        entry.file_id = version
            .files
            .iter()
            .find(|file| file.file_name == file_name)
            .or_else(|| version.primary_file())
            .and_then(|file| file.file_id.clone());
        log::info!("Adopted {file_name} as {provider} {}", version.project_id);
        index.mods.insert(file_name.to_owned(), entry);
        adopted.push(file_name.to_owned());
    }
    if !adopted.is_empty() {
        index.save(server)?;
    }
    drop(lock);
    Ok(adopted)
}

/// An update exists when none of the installed digests is among the returned
/// file's (R12).
fn candidate(file_name: &str, entry: &IndexEntry, version: &Version) -> Option<UpdateCandidate> {
    if installed_digests(&entry.hashes).any(|digest| version.has_file_hash(digest)) {
        return None;
    }
    Some(UpdateCandidate {
        file_name: file_name.to_owned(),
        // CurseForge entries carry no cached title (D2); the file name is the
        // honest fallback and the installed list has the jar's own name.
        title: entry.title.clone().unwrap_or_else(|| file_name.to_owned()),
        from: entry.version_id.clone(),
        to: version.clone(),
    })
}

fn installed_digests(hashes: &IndexHashes) -> impl Iterator<Item = &str> {
    [
        Some(hashes.sha1.as_str()),
        hashes.sha512.as_deref(),
        hashes.md5.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|digest| !digest.is_empty())
}

/// Moves one managed file to another version.
///
/// The order is the whole point: download and verify into `.nebula-tmp`, move
/// the old file **out** of the namespace directory, move the new one in, then
/// rewrite the index under the new key. A crash between the second and third
/// step leaves the mod absent with its entry intact — `파일 없음`, and
/// re-installable — never two versions of one mod side by side.
pub fn replace_file(
    catalog: &Catalog,
    server: &ServerDir,
    old_file_name: &str,
    new: &Version,
    progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
) -> Result<(), CatalogError> {
    let ctx = server_context(server)?;
    let lock = ServerLock::acquire(server)?;
    install::sweep_staging(server);
    let mut index = ModIndex::load(server)?;
    let planned = planned_from(&index, old_file_name, new)?;

    progress(1, 1, &planned.file.file_name);
    let staging = install::staging_dir(server)?;
    let dependencies = index
        .mods
        .get(old_file_name)
        .map(|entry| entry.dependencies.clone())
        .unwrap_or_default();
    let outcome = replace_file_locked(
        catalog,
        &ctx,
        &staging,
        &mut index,
        old_file_name,
        &planned,
        dependencies,
    );
    install::discard(&staging);
    outcome?;
    index.save(server)?;
    drop(lock);
    Ok(())
}

/// [`replace_file`] for a caller that already holds the lock and the index.
///
/// Split out because [`ServerLock::acquire`] uses `create_new(true)`: a
/// re-entrant acquire from `install::apply` would come back as
/// `Locked { pid: <our own pid> }`, a self-deadlock that reads as contention.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replace_file_locked(
    catalog: &Catalog,
    ctx: &ServerContext,
    staging: &std::path::Path,
    index: &mut ModIndex,
    old_file_name: &str,
    new: &PlannedFile,
    dependencies: Vec<String>,
) -> Result<(), CatalogError> {
    let entry = index
        .mods
        .get(old_file_name)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{old_file_name}은 Nebula가 설치한 모드가 아닙니다 — 교체하려면 먼저 지우고 \
                 다시 설치하세요"
            ))
        })?
        .clone();
    install::check_file_name(&new.file.file_name)?;
    install::check_name_free(
        index,
        &ctx.container,
        &std::collections::BTreeSet::new(),
        &new.file.file_name,
        Some(old_file_name),
    )?;

    // 1. The bytes exist and are verified before anything on disk moves.
    let (staged, hashes) = install::fetch_verified(catalog, new.provider, &new.file, staging)?;

    let directory = namespace_dir(&ctx.container, entry.namespace);
    std::fs::create_dir_all(&directory)?;
    let old_path = directory.join(old_file_name);
    let new_path = directory.join(&new.file.file_name);

    // 2. The old file leaves the namespace directory first, so the two
    //    versions are never both visible to a scanner.
    let parked = staging.join(format!("old-{old_file_name}"));
    let moved_out = match std::fs::rename(&old_path, &parked) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };

    // 3. The new file takes its place, and the index follows.
    if let Err(error) = std::fs::rename(&staged, &new_path) {
        if moved_out && let Err(back) = std::fs::rename(&parked, &old_path) {
            log::error!("Could not restore {}: {back}", old_path.display());
        }
        return Err(error.into());
    }

    index.mods.remove(old_file_name);
    let mut replacement = install::entry_for(new, hashes, entry.namespace, dependencies);
    // A namespace move is `move_mod`'s job, and a replace keeps whatever the
    // user chose when the mod was installed.
    replacement.namespace = entry.namespace;
    index.mods.insert(new.file.file_name.clone(), replacement);

    // The old bytes only die once the new ones are in place and indexed.
    if moved_out && let Err(error) = std::fs::remove_file(&parked) {
        log::warn!("Could not delete {}: {error}", parked.display());
    }
    Ok(())
}

/// One mod's own outcome inside an [`apply_updates`] run, keyed by the file
/// name it was asked about.
pub type UpdateOutcome = (String, Result<(), CatalogError>);

/// Applies candidates one at a time, so one failing mod never rolls back the
/// ones that already worked (§8.3's per-mod failure list).
///
/// The outer `Result` is the run itself: a refused lock, an unreadable
/// `servermeta.json` or a broken index is nobody's per-mod failure, and
/// flattening it into every row cost the variant the GUI chooses its copy by
/// (§8.4). The inner ones are the per-mod outcomes §8.3 lists afterwards.
pub fn apply_updates(
    catalog: &Catalog,
    server: &ServerDir,
    candidates: &[UpdateCandidate],
    progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
) -> Result<Vec<UpdateOutcome>, CatalogError> {
    let ctx = server_context(server)?;
    let lock = ServerLock::acquire(server)?;
    install::sweep_staging(server);

    let mut index = ModIndex::load(server)?;

    let total = candidates.len();
    let mut results = Vec::with_capacity(total);
    for (position, candidate) in candidates.iter().enumerate() {
        let name = candidate
            .to
            .primary_file()
            .map(|file| file.file_name.clone())
            .unwrap_or_else(|| candidate.file_name.clone());
        progress(position + 1, total, &name);

        let outcome = (|| {
            let planned = planned_from(&index, &candidate.file_name, &candidate.to)?;
            let dependencies = index
                .mods
                .get(&candidate.file_name)
                .map(|entry| entry.dependencies.clone())
                .unwrap_or_default();
            let staging = install::staging_dir(server)?;
            let outcome = replace_file_locked(
                catalog,
                &ctx,
                &staging,
                &mut index,
                &candidate.file_name,
                &planned,
                dependencies,
            );
            install::discard(&staging);
            outcome?;
            // Saved per mod: a crash halfway leaves the index describing
            // exactly what is on disk.
            index.save(server)
        })();
        results.push((candidate.file_name.clone(), outcome));
    }
    drop(lock);
    Ok(results)
}

/// The provenance a replacement is written with: the provider comes from the
/// entry being replaced, because a `Version` does not carry one.
fn planned_from(
    index: &ModIndex,
    old_file_name: &str,
    new: &Version,
) -> Result<PlannedFile, CatalogError> {
    let entry = index.mods.get(old_file_name).ok_or_else(|| {
        Error::Invalid(format!(
            "{old_file_name}은 Nebula가 설치한 모드가 아닙니다 — 교체하려면 먼저 지우고 \
             다시 설치하세요"
        ))
    })?;
    let file = new.primary_file().ok_or(CatalogError::NotFound)?.clone();
    Ok(PlannedFile {
        provider: entry.provider,
        project_id: new.project_id.clone(),
        slug: entry.project_slug.clone(),
        title: entry
            .title
            .clone()
            .unwrap_or_else(|| old_file_name.to_owned()),
        // The text is whatever the entry already carries, so it obeys the same
        // provider rule the entry was written under.
        text_provider: entry.provider,
        version_id: new.version_id.clone(),
        version_name: new.name.clone(),
        version_number: new.number.clone(),
        published: new.published.clone(),
        release: new.release,
        file,
    })
}
