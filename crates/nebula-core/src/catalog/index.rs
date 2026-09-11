//! `<server>/modindex.json`: what the catalog installed, keyed by file name.
//!
//! Losing this file degrades every mod to "직접 추가", never to data loss, so
//! reconciliation is **read-only** (D1): an entry whose file is gone is
//! flagged in memory and stays in the file until an explicit user mutation
//! removes it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::jarmeta::{self, LocalFile, LocalMod};
use super::{CatalogError, ProviderId, ReleaseType};
use crate::error::Error;
use crate::layout::ServerDir;
use crate::scan::{NAMESPACES, Namespace, discover_toggleable};

/// The only schema version written so far.
pub const INDEX_VERSION: u32 = 1;

/// The digests an installed file is verified and looked up by.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexHashes {
    /// Lowercase hex; Modrinth's bulk lookups are case sensitive (§A.5).
    pub sha1: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sha512: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub md5: Option<String>,
}

/// One managed file's provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub provider: ProviderId,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub project_slug: Option<String>,
    pub version_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon_url: Option<String>,
    /// The release type of the installed version, so an update check can
    /// refuse to walk a stable server onto a prerelease (D3). `None` is an
    /// entry written before this field existed and counts as a release.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub release: Option<ReleaseType>,
    pub hashes: IndexHashes,
    /// Unix seconds.
    pub installed_at: u64,
    pub namespace: Namespace,
    /// Project ids of the required dependencies resolved at install time, so
    /// a delete can warn about what else would break.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

impl IndexEntry {
    /// Builds an entry, stamping the install time.
    ///
    /// **CurseForge entries carry no free text.** Their terms forbid caching
    /// API data, so `title`, `project_slug` and `icon_url` are dropped here
    /// rather than at serialisation time, where a sibling field is invisible
    /// (D2). Their title comes back from the jar and their icon from the
    /// session memory cache.
    pub fn new(
        provider: ProviderId,
        project_id: impl Into<String>,
        version_id: impl Into<String>,
        hashes: IndexHashes,
        namespace: Namespace,
    ) -> Self {
        Self {
            provider,
            project_id: project_id.into(),
            project_slug: None,
            version_id: version_id.into(),
            file_id: None,
            title: None,
            icon_url: None,
            release: None,
            hashes,
            installed_at: now_secs(),
            namespace,
            dependencies: Vec::new(),
        }
    }

    /// Attaches the free text a provider is allowed to be remembered by.
    /// A CurseForge entry silently keeps none of it.
    pub fn with_text(
        mut self,
        slug: Option<String>,
        title: Option<String>,
        icon_url: Option<String>,
    ) -> Self {
        if self.provider == ProviderId::CurseForge {
            return self;
        }
        self.project_slug = slug;
        self.title = title;
        self.icon_url = icon_url;
        self
    }
}

/// The whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModIndex {
    pub version: u32,
    pub mods: BTreeMap<String, IndexEntry>,
}

impl Default for ModIndex {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            mods: BTreeMap::new(),
        }
    }
}

impl ModIndex {
    /// Reads `<server>/modindex.json`. A missing file is an empty index; a
    /// file written by a newer Nebula is an error rather than a silent
    /// downgrade that would drop fields on the next save.
    pub fn load(server: &ServerDir) -> Result<Self, CatalogError> {
        Self::load_at(&server.modindex_file())
    }

    /// [`ModIndex::load`] against an explicit path.
    pub fn load_at(path: &Path) -> Result<Self, CatalogError> {
        let raw = match std::fs::read(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        let index: Self = serde_json::from_slice(&raw)
            .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
        if index.version > INDEX_VERSION {
            return Err(Error::Invalid(format!(
                "{}의 형식 버전 {}은 이 Nebula보다 새롭습니다 — Nebula를 업데이트하세요",
                path.display(),
                index.version
            ))
            .into());
        }
        Ok(index)
    }

    /// Writes the index atomically: a `.tmp` sibling and a rename, so a crash
    /// mid-write leaves the previous index intact. Callers hold the server
    /// lock (reliability rule 3).
    pub fn save(&self, server: &ServerDir) -> Result<(), CatalogError> {
        self.save_at(&server.modindex_file())
    }

    /// [`ModIndex::save`] against an explicit path.
    pub fn save_at(&self, path: &Path) -> Result<(), CatalogError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(Error::from)?;
        std::fs::write(&temporary, body)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }
}

/// One row of the installed list.
#[derive(Debug, Clone)]
pub struct InstalledMod {
    pub file_name: String,
    pub namespace: Namespace,
    /// `None` for a jar somebody dropped in by hand (`직접 추가`).
    pub entry: Option<IndexEntry>,
    /// The index knows this file but the directory does not (`파일 없음`).
    pub missing: bool,
    /// The jar's own metadata, absent when the jar is missing or unreadable.
    pub local: Option<LocalMod>,
    /// Why the distribution scanners would abort on this file (`배포 불가`).
    pub unshippable: Option<String>,
}

impl InstalledMod {
    /// Whether nothing in the index claims this file.
    pub fn is_unmanaged(&self) -> bool {
        self.entry.is_none()
    }

    /// The best title available: the index's for Modrinth, the jar's for
    /// CurseForge and hand-placed files, the file name as a last resort.
    pub fn title(&self) -> &str {
        self.entry
            .as_ref()
            .and_then(|entry| entry.title.as_deref())
            .or(self.local.as_ref().map(|local| local.name.as_str()))
            .unwrap_or(&self.file_name)
    }
}

/// What one reconciliation saw.
#[derive(Debug, Clone)]
pub struct Reconciled {
    pub rows: Vec<InstalledMod>,
    /// False when the container could not be read at all — neither its own
    /// directory nor any of the three namespace directories is there — so
    /// nothing derived from this pass may be written back (D1). A readable
    /// container that happens to hold no files is a real state, not an
    /// unreadable one: it is what removing the last file leaves behind.
    pub persistable: bool,
}

/// Lists a server's mods by walking the three namespace directories and
/// matching them against the index. **Writes nothing.**
pub fn reconcile(server: &ServerDir, container: &Path) -> Result<Reconciled, CatalogError> {
    let index = ModIndex::load(server)?;
    reconcile_with(&index, container)
}

/// [`reconcile`] against an index already in hand.
pub fn reconcile_with(index: &ModIndex, container: &Path) -> Result<Reconciled, CatalogError> {
    let discovered = discover_toggleable(container).map_err(CatalogError::Other)?;
    // D1 asks whether the container could be *read*, which is not the same
    // question as whether it held anything: `discover` turns a missing
    // directory into an empty list and propagates every other read error, so
    // the only thing left to test is whether the directories are there. A
    // readable, empty container with stale index rows is persistable — that is
    // what lets a user remove the last 파일 없음 row.
    let persistable = !discovered.is_empty()
        || NAMESPACES
            .iter()
            .any(|namespace| container.join(namespace.dir_name()).is_dir());
    if !persistable {
        log::warn!(
            "{} has no readable namespace directory; treating the container as unreadable",
            container.display()
        );
    }

    let mut rows = Vec::with_capacity(discovered.len() + index.mods.len());
    for (namespace, candidate) in &discovered {
        let (local, unshippable) = match jarmeta::read(&candidate.path)? {
            LocalFile::Mod(local) => (Some(local), None),
            LocalFile::Unshippable { reason, .. } => (None, Some(reason)),
        };
        rows.push(InstalledMod {
            entry: index.mods.get(&candidate.file_name).cloned(),
            file_name: candidate.file_name.clone(),
            namespace: *namespace,
            missing: false,
            local,
            unshippable,
        });
    }

    let present: std::collections::BTreeSet<&str> = discovered
        .iter()
        .map(|(_, candidate)| candidate.file_name.as_str())
        .collect();
    for (file_name, entry) in &index.mods {
        if present.contains(file_name.as_str()) {
            continue;
        }
        rows.push(InstalledMod {
            file_name: file_name.clone(),
            namespace: entry.namespace,
            entry: Some(entry.clone()),
            missing: true,
            local: None,
            unshippable: None,
        });
    }
    Ok(Reconciled { rows, persistable })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}
