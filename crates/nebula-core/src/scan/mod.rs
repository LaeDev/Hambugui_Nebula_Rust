//! Module scanners: the shared candidate discovery, hashing policy and
//! artifact assembly that `Module.struct.ts` implemented on the base class,
//! plus one module per upstream structure.
//!
//! Everything consumers need is re-exported here, so callers only ever say
//! `nebula_core::scan::...`.

pub mod fabricmod;
pub mod files;
pub mod forgemod;
pub mod library;
pub mod toggle;

pub use fabricmod::{
    FabricEntryPoint, FabricModJson, entry_point, fabric_module_id_and_name, infer_group,
    parse_fabric_mod_json, scan_fabric_mods,
};
pub use files::scan_files;
// The metadata parsers behind these two are `pub(crate)` and tested inline in
// `forgemod.rs`: they are one jar's `mcmod.info` / `mods.toml` details, not a
// surface anything outside this crate should depend on.
pub use forgemod::{ForgeFlavour, ModLoader, forge_flavour_for, scan_forge_mods};
pub use library::scan_libraries;
pub use toggle::{
    NAMESPACES, Namespace, discover_toggleable, finish_modules, init_toggleable, url_file_name,
};

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::error::{Error, Result, io};
use crate::hash::digests_of_file;
use crate::java::Java;
use crate::layout::ServerDir;
use crate::meta::UntrackedFilesOption;
use crate::paths;
use crate::spec::{Artifact, ModuleType};
use crate::version::MinecraftVersion;

/// Upstream `ModuleStructure.FILE_NAME_BLACKLIST`.
pub const FILE_NAME_BLACKLIST: [&str; 1] = [".gitkeep"];

/// Upstream `ModuleStructure.DEFAULT_VERSION`.
pub const DEFAULT_VERSION: &str = "0.0.0";

/// How untracked files are hashed.
///
/// Upstream emitted untracked artifacts without any digest, which
/// `hambugui-distribution::parse_digest` rejects outright. We hash them anyway
/// and flag the module instead; `helios_untracked` restores the upstream
/// behaviour for Helios-only deployments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HashPolicy {
    pub helios_untracked: bool,
}

/// Everything a scanner needs about the server it is scanning.
pub struct ScanContext<'a> {
    pub server: &'a ServerDir,
    pub mc: MinecraftVersion,
    pub java: Option<&'a Java>,
    pub untracked: &'a [UntrackedFilesOption],
    pub policy: HashPolicy,
}

/// One file that may become a module (upstream `ModuleCandidate`).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub file_name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// Non-recursive listing of `dir`, sorted by name. Symlinks are skipped
/// (`lstat().isFile()` upstream), as is every blacklisted name. A missing
/// directory yields an empty list (upstream guarded with `pathExists`).
pub fn discover(dir: &Path) -> Result<Vec<Candidate>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(format!("reading {}", dir.display()))(error)),
    };

    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io(format!("reading {}", dir.display())))?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if FILE_NAME_BLACKLIST.contains(&file_name.as_str()) {
            continue;
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(io(format!("inspecting {}", path.display())))?;
        if !meta.is_file() {
            continue;
        }
        candidates.push(Candidate {
            file_name,
            path,
            size: meta.len(),
        });
    }
    candidates.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(candidates)
}

/// Port of upstream `attemptCrudeInference`, whose regex is
/// `/(.+?)-(.+).[jJ][aA][rR]/`: **unanchored**, and the `.` before `jar` is
/// unescaped, so it matches any single character. Reproducing it exactly:
///
/// * a match exists iff a match starting at offset 0 exists (group 1 is `.+?`,
///   so it can always be widened to reach the same `-`), hence group 1 is the
///   text before the *leftmost* `-` at index >= 1 that admits a suffix match;
/// * group 2 is greedy, so it stops at the *last* `jar` occurrence that still
///   leaves it at least one character.
///
/// `foo-1.0.jar.bak` therefore yields `("foo", "1.0")`, not `("foo", "1.0.jar")`.
/// The fallback reproduces JS `substring(0, lastIndexOf('.'))`, which is `""`
/// when the name has no dot at all.
pub fn crude_inference(file_name: &str) -> (String, String) {
    let chars: Vec<char> = file_name.chars().collect();
    for dash in 1..chars.len() {
        if chars[dash] != '-' {
            continue;
        }
        // Group 2 needs >= 1 char and one wildcard char before `jar`.
        let mut last_jar = None;
        let mut at = dash + 3;
        while at + 3 <= chars.len() {
            if chars[at].eq_ignore_ascii_case(&'j')
                && chars[at + 1].eq_ignore_ascii_case(&'a')
                && chars[at + 2].eq_ignore_ascii_case(&'r')
            {
                last_jar = Some(at);
            }
            at += 1;
        }
        if let Some(jar) = last_jar {
            return (
                chars[..dash].iter().collect(),
                chars[dash + 1..jar - 1].iter().collect(),
            );
        }
    }

    let stem = match file_name.rfind('.') {
        Some(dot) => &file_name[..dot],
        None => "",
    };
    (stem.to_owned(), DEFAULT_VERSION.to_owned())
}

/// Port of upstream `capitalize`: only the first character changes.
pub fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `group:id:version@ext` (upstream `generateMavenIdentifier`).
pub fn maven_module_id(group: &str, id: &str, version: &str, module_type: ModuleType) -> String {
    match module_type.default_extension() {
        Some(extension) => format!("{group}:{id}:{version}@{extension}"),
        None => format!("{group}:{id}:{version}"),
    }
}

/// The `untrackedFiles` patterns that apply to one structure directory.
///
/// `literal_separator(true)` gives minimatch semantics: `*` does not cross `/`.
pub struct UntrackedMatcher(GlobSet);

impl UntrackedMatcher {
    pub fn new(struct_root: &str, options: &[UntrackedFilesOption]) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for option in options
            .iter()
            .filter(|option| option.applies_to.iter().any(|root| root == struct_root))
        {
            for pattern in &option.patterns {
                let glob = GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|error| {
                        Error::Invalid(format!("Invalid untracked file pattern {pattern}: {error}"))
                    })?;
                builder.add(glob);
            }
        }
        builder
            .build()
            .map(UntrackedMatcher)
            .map_err(|error| Error::Invalid(format!("Invalid untracked file patterns: {error}")))
    }

    /// `rel` is the path relative to the structure's container directory, with
    /// forward slashes (`optionalon/jei.jar`, `config/client.cfg`).
    pub fn is_untracked(&self, rel: &str) -> bool {
        self.0.is_match(rel)
    }
}

/// Shared tail of upstream `parseModule` plus the hardening fork's containment
/// check, which the fork applies to mods and libraries too.
///
/// `container_real` is the canonicalized container: a scan resolves it once
/// (`paths::real_path`) instead of once per file.
pub fn build_artifact(
    container_real: &Path,
    path: &Path,
    size: u64,
    url: String,
    rel_path: Option<String>,
    untracked: bool,
    policy: &HashPolicy,
) -> Result<Artifact> {
    paths::assert_contained_in(container_real, path)?;
    let digests = if untracked && policy.helios_untracked {
        None
    } else {
        Some(digests_of_file(path)?)
    };
    Ok(Artifact {
        size,
        url,
        md5: digests.as_ref().map(|digests| digests.md5.clone()),
        sha256: digests.map(|digests| digests.sha256),
        path: rel_path.as_deref().map(paths::clean_relative).transpose()?,
    })
}
