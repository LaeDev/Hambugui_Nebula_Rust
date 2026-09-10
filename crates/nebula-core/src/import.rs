//! Modpack import: one auto-detecting entry point over the three formats Prism
//! Launcher can export.
//!
//! | Format | Detected by | Loader |
//! |---|---|---|
//! | CurseForge `.zip` | `manifest.json` | `minecraft.modLoaders[].id` |
//! | Modrinth `.mrpack` | `modrinth.index.json` | `dependencies` |
//! | Prism/MultiMC `.zip` | `mmc-pack.json` | `components[].uid` |
//!
//! Quilt is rejected everywhere: neither launcher can run it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zip::ZipArchive;

use crate::curseforge;
use crate::error::{Error, Result, io};
use crate::layout::Root;
use crate::paths;
use crate::remote;
use crate::scan::Namespace;
use crate::server::{
    self, CreateServerOptions, CreatedServer, LoaderKind, create_server, read_server_meta,
    write_server_meta,
};
use crate::version::MinecraftVersion;

/// How a mod file is fetched: `(url, destination, expected sha1)`. Production
/// passes [`default_download`]; tests pass a closure over local files.
///
/// `Send + Sync` so an import can run on a worker thread, which is where the
/// GUI puts it.
pub type DownloadFn = dyn Fn(&str, &Path, Option<&str>) -> Result<()> + Send + Sync;

pub fn default_download(url: &str, dest: &Path, expected_sha1: Option<&str>) -> Result<()> {
    remote::download_to_verified(url, dest, expected_sha1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    CurseForge,
    Modrinth,
    PrismInstance,
}

impl ImportFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportFormat::CurseForge => "CurseForge",
            ImportFormat::Modrinth => "Modrinth",
            ImportFormat::PrismInstance => "Prism",
        }
    }
}

impl std::fmt::Display for ImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mod the pack declares but does not permit third-party downloads for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualDownload {
    pub name: String,
    pub file_name: String,
    pub url: String,
}

#[derive(Debug)]
pub struct ImportReport {
    pub created: CreatedServer,
    pub format: ImportFormat,
    pub downloaded: usize,
    pub copied: usize,
    pub skipped_unsupported: usize,
    pub manual: Vec<ManualDownload>,
}

/// What every branch accumulates while it walks the pack.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub downloaded: usize,
    pub copied: usize,
    pub skipped_unsupported: usize,
    pub manual: Vec<ManualDownload>,
}

pub fn detect_format(zip_path: &Path) -> Result<ImportFormat> {
    let mut zip = open(zip_path)?;
    for (entry, format) in [
        ("modrinth.index.json", ImportFormat::Modrinth),
        ("mmc-pack.json", ImportFormat::PrismInstance),
        ("manifest.json", ImportFormat::CurseForge),
    ] {
        if zip.by_name(entry).is_ok() {
            return Ok(format);
        }
    }
    Err(Error::Invalid(format!(
        "{} is not a CurseForge, Modrinth or Prism modpack (no manifest.json, modrinth.index.json or mmc-pack.json).",
        zip_path.display()
    )))
}

/// `file` is used as given when it exists, otherwise looked up by name below
/// `<root>/modpacks`.
pub fn resolve_input(root: &Root, file: &Path) -> Result<PathBuf> {
    if file.is_file() {
        return Ok(file.to_path_buf());
    }
    let wanted = file.file_name().unwrap_or(file.as_os_str());
    let mut stack = vec![root.modpacks_dir()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry.file_name() == wanted {
                return Ok(path);
            }
        }
    }
    Err(Error::Invalid(format!(
        "모드팩 {}을(를) 찾을 수 없습니다 — {} 안에 넣거나 전체 경로를 입력하세요",
        file.display(),
        root.modpacks_dir().display()
    )))
}

/// No branch of an import runs Java: mod metadata is only read when the server
/// is scanned, which is `generate distro`'s job.
pub fn import_modpack(
    root: &Root,
    id: &str,
    file: &Path,
    download: &DownloadFn,
) -> Result<ImportReport> {
    let path = resolve_input(root, file)?;
    let format = detect_format(&path)?;
    log::info!("Importing {} as a {format} modpack.", path.display());

    let mut counters = Counters::default();
    let created = match format {
        ImportFormat::CurseForge => curseforge::import(root, id, &path, download, &mut counters)?,
        ImportFormat::Modrinth => import_modrinth(root, id, &path, download, &mut counters)?,
        ImportFormat::PrismInstance => import_prism(root, id, &path, &mut counters)?,
    };

    if !counters.manual.is_empty() {
        // The list itself belongs to whoever can show it: the GUI has a box for
        // it and the CLI prints the banner.
        log::warn!("{} files require manual download", counters.manual.len());
    }
    log::info!(
        "Imported {}: {} downloaded, {} copied, {} skipped.",
        created.dir.dir_name,
        counters.downloaded,
        counters.copied,
        counters.skipped_unsupported
    );
    Ok(ImportReport {
        created,
        format,
        downloaded: counters.downloaded,
        copied: counters.copied,
        skipped_unsupported: counters.skipped_unsupported,
        manual: counters.manual,
    })
}

// ---------------------------------------------------------------------------
// Modrinth
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ModrinthIndex {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "versionId", default)]
    version_id: Option<String>,
    #[serde(default)]
    files: Vec<ModrinthFile>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    path: String,
    #[serde(default)]
    hashes: ModrinthHashes,
    #[serde(default)]
    env: Option<ModrinthEnv>,
    #[serde(default)]
    downloads: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ModrinthHashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthEnv {
    #[serde(default)]
    client: Option<String>,
}

fn import_modrinth(
    root: &Root,
    id: &str,
    path: &Path,
    download: &DownloadFn,
    counters: &mut Counters,
) -> Result<CreatedServer> {
    let mut zip = open(path)?;
    let index: ModrinthIndex = read_json(&mut zip, "modrinth.index.json")?;

    let mc = index
        .dependencies
        .get("minecraft")
        .ok_or_else(|| {
            Error::Invalid("Modrinth pack does not declare a minecraft version.".into())
        })?
        .parse::<MinecraftVersion>()?;
    let loader = modrinth_loader(&index.dependencies)?;

    let created = create_server(
        root,
        id,
        &mc,
        &options(index.version_id.clone(), loader.as_ref()),
    )?;
    rename_server(&created, index.name.as_deref())?;

    for file in &index.files {
        let rel = match paths::clean_relative(&file.path) {
            Ok(rel) => rel,
            Err(error) => {
                log::warn!("Skipping {}: {error}", file.path);
                continue;
            }
        };
        let namespace = match file.env.as_ref().and_then(|env| env.client.as_deref()) {
            Some("unsupported") => {
                log::debug!("{rel} is client-unsupported; skipping.");
                counters.skipped_unsupported += 1;
                continue;
            }
            Some("optional") => Namespace::OptionalOn,
            _ => Namespace::Required,
        };
        let Some(url) = file.downloads.first() else {
            log::warn!("{rel} declares no download url; skipping.");
            continue;
        };
        let destination = destination(&created, &rel, namespace);
        create_parent(&destination)?;
        download(url, &destination, file.hashes.sha1.as_deref())?;
        counters.downloaded += 1;
    }

    // `client-overrides` is applied last so it wins; `server-overrides` is not
    // ours to ship.
    extract_roots(
        &mut zip,
        &["overrides/".into(), "client-overrides/".into()],
        &["modrinth.index.json", "server-overrides/"],
        &created,
        |_| false,
        counters,
    )?;
    Ok(created)
}

fn modrinth_loader(
    dependencies: &BTreeMap<String, String>,
) -> Result<Option<(LoaderKind, String)>> {
    if dependencies.contains_key("quilt-loader") {
        return Err(Error::Invalid(
            "Quilt 모드팩은 지원하지 않습니다 — Helios도 Hambugui도 Quilt를 실행할 수 없습니다. \
             Forge, NeoForge 또는 Fabric 모드팩을 쓰세요"
                .into(),
        ));
    }
    Ok([
        ("forge", LoaderKind::Forge),
        ("neoforge", LoaderKind::NeoForge),
        ("fabric-loader", LoaderKind::Fabric),
    ]
    .into_iter()
    .find_map(|(key, kind)| dependencies.get(key).map(|v| (kind, v.clone()))))
}

// ---------------------------------------------------------------------------
// Prism / MultiMC instance
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MmcPack {
    #[serde(default)]
    components: Vec<MmcComponent>,
}

#[derive(Debug, Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: Option<String>,
}

/// Directories Prism keeps inside the instance that are not distributable.
const PRISM_EXCLUDED_DIRS: [&str; 6] = [
    "logs/",
    "crash-reports/",
    "saves/",
    "screenshots/",
    ".fabric/",
    ".mixin.out/",
];
/// `options.txt` is deliberately *not* here: it is a config pack owners ship.
const PRISM_EXCLUDED_FILES: [&str; 2] = ["usercache.json", "usernamecache.json"];

fn prism_excluded(rel: &str) -> bool {
    PRISM_EXCLUDED_DIRS.iter().any(|dir| rel.starts_with(dir))
        || PRISM_EXCLUDED_FILES.contains(&rel)
        || rel.to_lowercase().ends_with(".log")
}

fn import_prism(
    root: &Root,
    id: &str,
    path: &Path,
    counters: &mut Counters,
) -> Result<CreatedServer> {
    let mut zip = open(path)?;
    let pack: MmcPack = read_json(&mut zip, "mmc-pack.json")?;

    let mut mc = None;
    let mut loader = None;
    for component in &pack.components {
        let kind = match component.uid.as_str() {
            "net.minecraft" => {
                mc = component.version.clone();
                continue;
            }
            "net.minecraftforge" => LoaderKind::Forge,
            "net.neoforged" => LoaderKind::NeoForge,
            "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
            "org.quiltmc.quilt-loader" => {
                return Err(Error::Invalid(
                    "Quilt 인스턴스는 지원하지 않습니다 — Helios도 Hambugui도 Quilt를 실행할 수 \
                     없습니다. Forge, NeoForge 또는 Fabric 인스턴스를 쓰세요"
                        .into(),
                ));
            }
            _ => continue,
        };
        let Some(version) = component.version.clone() else {
            continue;
        };
        loader = Some((kind, version));
    }
    let mc = mc
        .ok_or_else(|| Error::Invalid("Prism instance declares no net.minecraft version.".into()))?
        .parse::<MinecraftVersion>()?;

    // Prism's own `gameRoot()` rule.
    let game_dir = [".minecraft/", "minecraft/"]
        .into_iter()
        .find(|prefix| {
            (0..zip.len()).any(|i| zip.name_for_index(i).is_some_and(|n| n.starts_with(prefix)))
        })
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{} contains neither a .minecraft nor a minecraft directory.",
                path.display()
            ))
        })?;

    let created = create_server(root, id, &mc, &options(None, loader.as_ref()))?;
    rename_server(&created, instance_name(&mut zip).as_deref())?;

    extract_roots(
        &mut zip,
        &[game_dir.into()],
        &["mmc-pack.json", "instance.cfg", ".packignore", "icon.png"],
        &created,
        prism_excluded,
        counters,
    )?;
    Ok(created)
}

/// `instance.cfg` is an INI-ish key/value file; only `name=` interests us.
fn instance_name(zip: &mut ZipArchive<File>) -> Option<String> {
    let mut raw = String::new();
    zip.by_name("instance.cfg")
        .ok()?
        .read_to_string(&mut raw)
        .ok()?;
    raw.lines()
        .find_map(|line| line.trim().strip_prefix("name="))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// Opens `path` as a zip. **The one place a zip is opened in this crate**, so
/// that `ZipError`'s own message — which never names a file — cannot reach a
/// user as a bare `invalid Zip archive: Could not find EOCD` about one jar out
/// of three hundred.
pub(crate) fn open(path: &Path) -> Result<ZipArchive<File>> {
    let file = File::open(path).map_err(io(format!("opening {}", path.display())))?;
    ZipArchive::new(file).map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    zip: &mut ZipArchive<File>,
    entry: &str,
) -> Result<T> {
    let mut raw = Vec::new();
    zip.by_name(entry)?
        .read_to_end(&mut raw)
        .map_err(io(format!("reading {entry}")))?;
    serde_json::from_slice(&raw)
        .map_err(|error| Error::Invalid(format!("{entry} is not valid: {error}")))
}

pub(crate) fn options(
    version: Option<String>,
    loader: Option<&(LoaderKind, String)>,
) -> CreateServerOptions {
    let mut options = CreateServerOptions {
        version,
        ..Default::default()
    };
    match loader {
        Some((LoaderKind::Forge, v)) => options.forge_version = Some(v.clone()),
        Some((LoaderKind::NeoForge, v)) => options.neoforge_version = Some(v.clone()),
        Some((LoaderKind::Fabric, v)) => options.fabric_version = Some(v.clone()),
        None => log::warn!("The modpack declares no mod loader; importing it as a vanilla server."),
    }
    options
}

/// The pack's own display name replaces the generated `<id> (Minecraft <mc>)`.
pub(crate) fn rename_server(created: &CreatedServer, name: Option<&str>) -> Result<()> {
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        return Ok(());
    };
    let mut meta = read_server_meta(&created.dir)?;
    meta.meta.name = name.to_owned();
    write_server_meta(&created.dir, &meta)
}

/// Where one game-directory-relative path lands under the created server.
///
/// A `.jar` under `mods/` goes to the loader's toggleable container; a
/// `.disabled` one lands in `optionaloff` with the suffix removed. Everything
/// else keeps its relative path under `files/`.
pub(crate) fn destination(created: &CreatedServer, rel: &str, namespace: Namespace) -> PathBuf {
    let (rel, namespace) = match rel.strip_suffix(".disabled") {
        Some(enabled) if enabled.to_lowercase().ends_with(".jar") => {
            (enabled, Namespace::OptionalOff)
        }
        _ => (rel, namespace),
    };
    let file_name = rel.rsplit('/').next().unwrap_or(rel);
    let is_mod = rel.starts_with("mods/") && file_name.to_lowercase().ends_with(".jar");
    match (&created.mod_container, is_mod) {
        (Some(container), true) => server::namespace_dir(container, namespace).join(file_name),
        _ => created.files_dir.join(rel),
    }
}

pub(crate) fn create_parent(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))
}

/// Copies every file below `roots`, in order, so a later root overwrites an
/// earlier one. Entry names go through [`paths::clean_relative`], so a crafted
/// archive cannot write outside the server directory.
pub(crate) fn extract_roots(
    zip: &mut ZipArchive<File>,
    roots: &[String],
    consumed: &[&str],
    created: &CreatedServer,
    excluded: fn(&str) -> bool,
    counters: &mut Counters,
) -> Result<()> {
    for index in 0..zip.len() {
        let name = zip.by_index(index)?.name().to_owned();
        let routed = name.ends_with('/')
            || roots.iter().any(|root| name.starts_with(root.as_str()))
            || consumed.iter().any(|prefix| name.starts_with(prefix));
        if !routed {
            log::warn!("Skipping {name}: it is outside the modpack's file roots.");
        }
    }

    for root in roots {
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let Some(raw) = entry.name().strip_prefix(root.as_str()).map(str::to_owned) else {
                continue;
            };
            let rel = match paths::clean_relative(&raw) {
                Ok(rel) => rel,
                Err(error) => {
                    log::warn!("Skipping {root}{raw}: {error}");
                    continue;
                }
            };
            if excluded(&rel) {
                log::debug!("Skipping {rel}: excluded from imports.");
                continue;
            }
            let destination = destination(created, &rel, Namespace::Required);
            create_parent(&destination)?;
            let mut out = File::create(&destination)
                .map_err(io(format!("creating {}", destination.display())))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(io(format!("writing {}", destination.display())))?;
            counters.copied += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::ServerDir;
    use url::Url;

    fn created(mod_container: bool) -> CreatedServer {
        let root = Root::new("H:/root", Url::parse("https://host/").unwrap());
        let dir = ServerDir::new(root, "Test-1.20.1");
        CreatedServer {
            mod_container: mod_container.then(|| dir.forgemods_dir()),
            files_dir: dir.files_dir(),
            loader: mod_container.then_some(LoaderKind::Forge),
            dir,
        }
    }

    #[test]
    fn routes_mods_to_the_container_and_the_rest_to_files() {
        let created = created(true);
        let container = created.mod_container.clone().unwrap();

        assert_eq!(
            destination(&created, "mods/jei.jar", Namespace::Required),
            container.join("required").join("jei.jar")
        );
        assert_eq!(
            destination(&created, "mods/jei.jar", Namespace::OptionalOn),
            container.join("optionalon").join("jei.jar")
        );
        assert_eq!(
            destination(&created, "mods/jei.jar.disabled", Namespace::Required),
            container.join("optionaloff").join("jei.jar")
        );
        assert_eq!(
            destination(&created, "mods/1.20.1/deep.jar", Namespace::Required),
            container.join("required").join("deep.jar")
        );

        for rel in [
            "config/client.cfg",
            "options.txt",
            "resourcepacks/pack.zip",
            "shaderpacks/shader.zip",
            "mods/readme.txt",
        ] {
            assert_eq!(
                destination(&created, rel, Namespace::Required),
                created.files_dir.join(rel),
                "{rel}"
            );
        }
    }

    #[test]
    fn keeps_mods_as_files_without_a_container() {
        let created = created(false);
        assert_eq!(
            destination(&created, "mods/jei.jar", Namespace::Required),
            created.files_dir.join("mods/jei.jar")
        );
    }

    #[test]
    fn excludes_only_the_prism_scratch_directories() {
        for rel in [
            "logs/latest.log",
            "crash-reports/crash.txt",
            "saves/world/level.dat",
            "screenshots/a.png",
            ".fabric/x",
            ".mixin.out/y",
            "usercache.json",
            "usernamecache.json",
            "debug.LOG",
        ] {
            assert!(prism_excluded(rel), "{rel} should be excluded");
        }
        for rel in ["options.txt", "config/a.cfg", "servers.dat", "mods/jei.jar"] {
            assert!(!prism_excluded(rel), "{rel} should be kept");
        }
    }

    #[test]
    fn a_broken_archive_names_the_file_it_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("Broken-Mod-1.0.jar");
        std::fs::write(&jar, b"PK-not-really-a-jar").unwrap();

        // `ZipError`'s own message names no file, and one bad jar among three
        // hundred is unfindable without the path.
        let message = open(&jar).unwrap_err().to_string();
        assert!(message.contains("Broken-Mod-1.0.jar"), "{message}");
        assert!(message.contains("Zip"), "{message}");

        let missing = open(&dir.path().join("absent.jar"))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("absent.jar"), "{missing}");
    }

    #[test]
    fn rejects_quilt_dependencies() {
        let mut dependencies = BTreeMap::new();
        dependencies.insert("quilt-loader".to_owned(), "0.20.0".to_owned());
        assert!(
            modrinth_loader(&dependencies)
                .unwrap_err()
                .to_string()
                .contains("Quilt")
        );
    }
}
