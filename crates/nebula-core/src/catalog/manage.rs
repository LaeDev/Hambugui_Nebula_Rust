//! Whole-server and single-file management: delete, duplicate, list, remove
//! and move. Every one of them is a mutation, so every one takes the server
//! lock (reliability rule 6).

use std::path::{Path, PathBuf};

use super::index::{InstalledMod, ModIndex, reconcile, reconcile_with};
use super::install::{STALE_TMP, TMP_DIR};
use super::lock::{LOCK_FILE, ServerLock};
use super::{CatalogError, server_context};
use crate::error::Error;
use crate::layout::{Root, ServerDir};
use crate::paths;
use crate::scan::Namespace;
use crate::server::{self, namespace_dir};

/// The suffix a half-copied duplicate carries. Its tail is not a Minecraft
/// version, so `list_servers` skips it with a warning instead of showing it.
const PARTIAL_SUFFIX: &str = ".partial";

/// Deletes one server directory and everything in it.
///
/// The guards run in this order and none of them is redundant: containment
/// alone is not enough, because `<root>/servers/.` canonicalises to the
/// servers directory itself and would pass it. On Windows `canonicalize` also
/// normalises a trailing space away, which is exactly why the resolved file
/// name has to equal the name that was asked for.
pub fn delete_server(root: &Root, dir_name: &str) -> Result<(), CatalogError> {
    if !paths::is_single_segment(dir_name) || dir_name == "." || dir_name == ".." {
        return Err(Error::Invalid(format!(
            "서버 폴더 이름 {dir_name}을(를) 쓸 수 없습니다 — 이름은 한 단계 폴더여야 합니다"
        ))
        .into());
    }
    let server = ServerDir::new(root.clone(), dir_name);
    if !server.path.is_dir() {
        return Err(Error::Invalid(format!("서버 폴더 {dir_name}을(를) 찾을 수 없습니다")).into());
    }

    let real = paths::real_path(&server.path).map_err(CatalogError::Other)?;
    let servers = paths::real_path(&root.servers_dir()).map_err(CatalogError::Other)?;
    if real.parent() != Some(servers.as_path()) {
        return Err(Error::UnsafePath(format!(
            "{} is not a server directory of {}",
            real.display(),
            servers.display()
        ))
        .into());
    }
    // Windows normalises a trailing space away while resolving, so the name
    // that was asked for is not always the name that was found — and the user
    // needs to be told that, not that their server is in the wrong place.
    if !real.file_name().is_some_and(|name| name == dir_name) {
        return Err(Error::Invalid(format!(
            "서버 폴더 이름이 정확히 일치하지 않습니다: \"{dir_name}\" — 앞뒤 공백을 확인하세요"
        ))
        .into());
    }

    let lock = ServerLock::acquire(&server)?;
    std::fs::remove_dir_all(&server.path)?;
    // The lock file went with the directory; `Drop` tolerates that.
    drop(lock);
    log::info!("Deleted server {dir_name}");
    Ok(())
}

/// Copies one server directory under a new id, returning the new directory
/// name.
///
/// The copy is staged in `<root>/servers/.<new dir>.partial/` and renamed into
/// place as the very last step, so a crash never leaves a half-built server
/// that `list_servers` would show. `.nebula-tmp` and `.nebula-lock` are not
/// copied; a symlinked entry is refused rather than followed.
pub fn duplicate_server(root: &Root, dir_name: &str, new_id: &str) -> Result<String, CatalogError> {
    // The same rule `create_server` applies, and the same message.
    if !paths::is_single_segment(new_id) {
        return Err(Error::Invalid(format!(
            "invalid server id {new_id}: ids must be a single path segment (no / \\ : or ..)"
        ))
        .into());
    }
    let source = ServerDir::new(root.clone(), dir_name);
    if !source.path.is_dir() {
        return Err(Error::Invalid(format!("서버 폴더 {dir_name}을(를) 찾을 수 없습니다")).into());
    }
    let (source_id, mc) = dir_name
        .rsplit_once('-')
        .filter(|(id, _)| !id.is_empty())
        .ok_or_else(|| {
            Error::Invalid(format!(
                "서버 폴더 {dir_name}의 이름이 -<마인크래프트 버전>으로 끝나지 않습니다"
            ))
        })?;
    let new_dir = format!("{new_id}-{mc}");
    let target = root.servers_dir().join(&new_dir);
    if target.exists() {
        return Err(Error::Invalid(format!(
            "서버 폴더 {new_dir}이(가) 이미 있습니다 — 다른 서버 ID를 쓰거나 기존 폴더를 지우세요"
        ))
        .into());
    }

    let staging = root
        .servers_dir()
        .join(format!(".{new_dir}{PARTIAL_SUFFIX}"));
    sweep_partials(root, &staging);

    let lock = ServerLock::acquire(&source)?;
    let outcome = (|| -> Result<(), CatalogError> {
        // Our own reserved name, and only it: a kill mid-copy leaves this
        // directory behind, and copying over it would merge the dead run's
        // files into the new server. Somebody else's `.partial` is left alone
        // by `sweep_partials`, which is the point of the age gate there.
        if staging.exists() {
            log::warn!("Restarting an interrupted copy into {}", staging.display());
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        copy_tree(&source.path, &staging)?;
        rename_copy(&staging, source_id, new_id, mc)?;
        std::fs::rename(&staging, &target)?;
        Ok(())
    })();
    drop(lock);
    if let Err(error) = outcome {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    log::info!("Duplicated server {dir_name} as {new_dir}");
    Ok(new_dir)
}

/// Deletes whatever an interrupted duplicate left behind.
///
/// Only entries older than [`STALE_TMP`], and never `keep` — the staging
/// directory this call is about to create. `servers/` is shared by every
/// process and the lock taken around a duplicate is the *source* server's, so
/// a younger `.partial` is far more likely to be another duplicate still
/// copying into it than a leftover (the same age gate `sweep_staging` uses).
fn sweep_partials(root: &Root, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root.servers_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') || !name.ends_with(PARTIAL_SUFFIX) || entry.path() == keep {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_TMP);
        if !stale {
            log::debug!("Leaving {name} alone: it may be another duplicate in progress");
            continue;
        }
        log::warn!("Removing an interrupted duplicate: {name}");
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// Copies one directory tree. `modindex.json` comes across as-is — the copy is
/// the same set of mods, so its provenance is still true.
fn copy_tree(from: &Path, to: &Path) -> Result<(), CatalogError> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == TMP_DIR || name == LOCK_FILE {
            continue;
        }
        let source = entry.path();
        let meta = std::fs::symlink_metadata(&source)?;
        if meta.file_type().is_symlink() {
            return Err(Error::UnsafePath(format!(
                "{} is a symlink; a duplicate would follow it out of the root",
                source.display()
            ))
            .into());
        }
        let destination = to.join(&name);
        if meta.is_dir() {
            std::fs::create_dir_all(&destination)?;
            copy_tree(&source, &destination)?;
        } else {
            std::fs::copy(&source, &destination)?;
        }
    }
    Ok(())
}

/// Renames the copy inside its staging directory, so the duplicate does not
/// present itself as the server it was copied from.
fn rename_copy(
    staging: &Path,
    source_id: &str,
    new_id: &str,
    mc: &str,
) -> Result<(), CatalogError> {
    let path = staging.join("servermeta.json");
    if !path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let mut meta: crate::meta::ServerMeta = serde_json::from_str(raw)
        .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
    // The same shape `default_server_meta` writes for a new server; a custom
    // name is deliberately not carried over, because two servers sharing one
    // display name is worse than a predictable one.
    meta.meta.name = format!("{new_id} (Minecraft {mc})");
    // `default_server_meta` writes the id into the description too, so a copy
    // that keeps it advertises the server it came from in distribution.json.
    // The whole generated prefix is matched, never the bare id: a hand-written
    // description that merely starts with the id is the user's own text.
    let generated = format!("{source_id} Running Minecraft");
    if let Some(rest) = meta.meta.description.strip_prefix(&generated) {
        meta.meta.description = format!("{new_id} Running Minecraft{rest}");
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&meta).map_err(Error::from)?,
    )?;
    Ok(())
}

/// Every mod the server has, managed or hand-placed, with the rows §8.3 draws:
/// `직접 추가` for an unmanaged jar, `파일 없음` for an entry whose file is
/// gone and `배포 불가` for a file the distribution scanners would abort on.
pub fn list_installed(server: &ServerDir) -> Result<Vec<InstalledMod>, CatalogError> {
    let ctx = server_context(server)?;
    let mut rows = reconcile(server, &ctx.container)?.rows;
    rows.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(rows)
}

/// The managed mods that recorded `file_name`'s project as a required
/// dependency when they were installed, by title (§8.3's delete warning).
pub fn dependents_of(index: &ModIndex, file_name: &str) -> Vec<String> {
    let Some(target) = index.mods.get(file_name) else {
        return Vec::new();
    };
    index
        .mods
        .iter()
        .filter(|(name, _)| name.as_str() != file_name)
        .filter(|(_, entry)| entry.dependencies.contains(&target.project_id))
        // A CurseForge entry carries no title (D2); its file name is what the
        // installed list shows for it too.
        .map(|(name, entry)| entry.title.clone().unwrap_or_else(|| name.clone()))
        .collect()
}

/// Deletes one mod file and its index entry.
pub fn remove_mod(server: &ServerDir, file_name: &str) -> Result<(), CatalogError> {
    let ctx = server_context(server)?;
    let _lock = ServerLock::acquire(server)?;
    let mut index = ModIndex::load(server)?;

    // D1: a container whose directories are not there to read is unreadable,
    // not empty — every row would show `파일 없음` and dropping an entry here
    // would silently unmanage a mod whose jar is fine. A readable container
    // that holds nothing is a normal state and the delete goes through.
    if !reconcile_with(&index, &ctx.container)?.persistable {
        return Err(Error::Invalid(format!(
            "{}의 모드 폴더를 읽을 수 없습니다 — 폴더를 확인한 뒤 다시 시도하세요: {}",
            server.dir_name,
            ctx.container.display()
        ))
        .into());
    }

    let path = locate(&ctx.container, &index, file_name)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        // The entry outliving its file is exactly the `파일 없음` state; the
        // user asking to delete it means dropping the entry.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    index.mods.remove(file_name);
    index.save(server)?;
    log::info!("Removed {file_name} from {}", server.dir_name);
    Ok(())
}

/// Moves one mod between namespaces, which is how Nebula enables and disables
/// it — there is no `.disabled` rename.
pub fn move_mod(
    server: &ServerDir,
    file_name: &str,
    namespace: Namespace,
) -> Result<(), CatalogError> {
    let ctx = server_context(server)?;
    let _lock = ServerLock::acquire(server)?;
    let mut index = ModIndex::load(server)?;

    let from = locate(&ctx.container, &index, file_name)?;
    let directory = namespace_dir(&ctx.container, namespace);
    std::fs::create_dir_all(&directory)?;
    let to = directory.join(file_name);
    if from != to {
        std::fs::rename(&from, &to)?;
    }
    if let Some(entry) = index.mods.get_mut(file_name) {
        entry.namespace = namespace;
        index.save(server)?;
    }
    log::info!(
        "Moved {file_name} to {} in {}",
        namespace.dir_name(),
        server.dir_name
    );
    Ok(())
}

/// Where one mod file currently is: the index knows for a managed mod, and
/// the three namespace directories are searched for a hand-placed one.
///
/// The name is checked for path escapes but **not** against R6: that rule
/// governs what a provider may name a file we write, while a file already in
/// the directory may be anything — including the `배포 불가` `notes.txt` the
/// installed list asks the user to delete (§8.3).
fn locate(container: &Path, index: &ModIndex, file_name: &str) -> Result<PathBuf, CatalogError> {
    if !paths::is_single_segment(file_name) || file_name == "." || file_name == ".." {
        return Err(Error::Invalid(format!("파일 이름 {file_name}을(를) 쓸 수 없습니다")).into());
    }
    if let Some(entry) = index.mods.get(file_name) {
        return Ok(namespace_dir(container, entry.namespace).join(file_name));
    }
    for namespace in crate::scan::NAMESPACES {
        let candidate = namespace_dir(container, namespace).join(file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(Error::Invalid(format!("{file_name}을(를) 찾을 수 없습니다")).into())
}

/// One summary line per server, for the CLI's server lookup.
pub fn server_names(root: &Root) -> Vec<String> {
    server::list_servers(root)
        .unwrap_or_default()
        .into_iter()
        .map(|summary| summary.dir_name)
        .collect()
}
