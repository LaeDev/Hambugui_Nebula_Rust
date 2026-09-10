//! Server directories: creating them, listing them, and assembling one into the
//! `Server` object a distribution carries.
//!
//! Port of `structure/spec_model/Server.struct.ts`, extended with NeoForge.
//! The module order below is the launcher contract: Helios reads the *first*
//! loader module it finds, so Forge, NeoForge and Fabric must stay in that
//! order and each loader's mods must follow its own root module.

use std::path::{Path, PathBuf};

use url::Url;

use crate::error::{Error, Result, io};
use crate::java::Java;
use crate::layout::{Root, ServerDir};
use crate::meta::{self, LoaderMeta, ServerMeta, ServerMetaOptions};
use crate::paths;
use crate::resolver::{self, ResolveOptions};
use crate::scan::{self, ForgeFlavour, HashPolicy, ModLoader, Namespace, ScanContext};
use crate::spec::Server;
use crate::version::{self, MinecraftVersion};

/// Minecraft 1.20.3 dropped `--fml.modLists`, which is how Helios hands mods to
/// Forge. Forge servers on newer Minecraft generate, but will not load mods.
///
/// Printed here at distro time and by the CLI at `generate server`; both boxes
/// must look the same, so there is exactly one of them.
pub const FORGE_MOD_LIST_WARNING: &str = "\
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃                     !!  WARNING !!                      ┃
┃                                                         ┃
┃    Forge 1.20.3+ removed support for --fml.modLists.    ┃
┃  Helios Launcher can no longer load mods through Forge. ┃
┃      Please use Fabric or NeoForge instead.             ┃
┃                                                         ┃
┃                     !!  WARNING !!                      ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderKind {
    Forge,
    NeoForge,
    Fabric,
}

impl LoaderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
            LoaderKind::Fabric => "Fabric",
        }
    }

    /// The toggleable mod container this loader reads, `<server>/forgemods` and
    /// friends.
    pub fn mod_container(self, server: &ServerDir) -> PathBuf {
        match self {
            LoaderKind::Forge => server.forgemods_dir(),
            LoaderKind::NeoForge => server.neoforgemods_dir(),
            LoaderKind::Fabric => server.fabricmods_dir(),
        }
    }
}

impl std::fmt::Display for LoaderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CreateServerOptions {
    pub version: Option<String>,
    pub forge_version: Option<String>,
    pub fabric_version: Option<String>,
    pub neoforge_version: Option<String>,
}

impl CreateServerOptions {
    /// At most one loader may be set. NeoForge additionally has a Minecraft
    /// floor, which is worth catching before a directory is created.
    pub fn loader(&self, mc: &MinecraftVersion) -> Result<Option<(LoaderKind, String)>> {
        let mut chosen: Option<(LoaderKind, String)> = None;
        for (kind, version) in [
            (LoaderKind::Forge, &self.forge_version),
            (LoaderKind::NeoForge, &self.neoforge_version),
            (LoaderKind::Fabric, &self.fabric_version),
        ] {
            let Some(version) = version else { continue };
            if chosen.is_some() {
                return Err(Error::Invalid(
                    "Forge, NeoForge and Fabric cannot be used together on the same server.".into(),
                ));
            }
            chosen = Some((kind, version.clone()));
        }

        if matches!(chosen, Some((LoaderKind::NeoForge, _))) && !version::is_neoforge_supported(mc)
        {
            return Err(Error::Invalid(format!(
                "NeoForge는 Minecraft {mc}을(를) 지원하지 않습니다 — Minecraft 1.20.2 이상을 쓰거나 \
                 로더를 Forge로 바꾸세요"
            )));
        }
        Ok(chosen)
    }
}

#[derive(Debug, Clone)]
pub struct CreatedServer {
    pub dir: ServerDir,
    /// `<server>/<loader>mods`, absent when the server has no loader.
    pub mod_container: Option<PathBuf>,
    pub files_dir: PathBuf,
    pub loader: Option<LoaderKind>,
}

/// Upstream `ServerStructure.getEffectiveId`.
pub fn effective_id(id: &str, mc: &MinecraftVersion) -> String {
    format!("{id}-{mc}")
}

pub fn create_server(
    root: &Root,
    id: &str,
    mc: &MinecraftVersion,
    opts: &CreateServerOptions,
) -> Result<CreatedServer> {
    // The id becomes a directory name under `<root>/servers` and, later, a path
    // segment in every URL the server publishes.
    if !paths::is_single_segment(id) {
        return Err(Error::Invalid(format!(
            "invalid server id {id}: ids must be a single path segment (no / \\ : or ..)"
        )));
    }
    let loader = opts.loader(mc)?;
    let dir = ServerDir::new(root.clone(), effective_id(id, mc));
    if dir.path.exists() {
        return Err(Error::Invalid(format!(
            "서버 폴더 {}이(가) 이미 있습니다 — 다른 서버 ID를 쓰거나 기존 폴더를 지우세요",
            dir.dir_name
        )));
    }

    std::fs::create_dir_all(&dir.path).map_err(io(format!("creating {}", dir.path.display())))?;

    let mod_container = loader
        .as_ref()
        .map(|(kind, _)| kind.mod_container(&dir))
        .inspect(|container| log::debug!("Mod container: {}", container.display()));
    if let Some(container) = &mod_container {
        scan::init_toggleable(container)?;
    }

    let mut server_meta = meta::default_server_meta(
        id,
        mc,
        &ServerMetaOptions {
            version: opts.version.clone(),
            forge_version: opts.forge_version.clone(),
            fabric_version: opts.fabric_version.clone(),
            neoforge_version: opts.neoforge_version.clone(),
        },
    );
    server_meta.schema = Some(meta::schema_url(root, meta::SERVER_META_SCHEMA));
    write_server_meta(&dir, &server_meta)?;

    let files_dir = dir.files_dir();
    for sub in [dir.libraries_dir(), files_dir.clone()] {
        std::fs::create_dir_all(&sub).map_err(io(format!("creating {}", sub.display())))?;
    }

    log::info!("Created server {} at {}", dir.dir_name, dir.path.display());
    Ok(CreatedServer {
        dir,
        mod_container,
        files_dir,
        loader: loader.map(|(kind, _)| kind),
    })
}

pub fn write_server_meta(dir: &ServerDir, server_meta: &ServerMeta) -> Result<()> {
    let path = dir.server_meta_file();
    std::fs::write(&path, serde_json::to_string_pretty(server_meta)?)
        .map_err(io(format!("writing {}", path.display())))
}

#[derive(Debug, Clone)]
pub struct ServerSummary {
    /// The directory name, which is also the launcher-facing server id.
    pub dir_name: String,
    /// The part of `dir_name` before the last `-`.
    pub id: String,
    pub mc: MinecraftVersion,
    pub loader: Option<(LoaderKind, String)>,
    pub mods: usize,
    pub files: usize,
    pub libraries: usize,
    pub has_icon: bool,
    pub meta: ServerMeta,
}

/// Every server directory under `<root>/servers`, in name order, with the
/// counts a listing shows.
///
/// Counts are cheap: nothing is hashed and no jar is opened. Directories whose
/// name does not end in `-<minecraft version>` are logged and skipped, as
/// upstream did.
pub fn list_servers(root: &Root) -> Result<Vec<ServerSummary>> {
    server_dirs(root)?
        .into_iter()
        .map(|(dir_name, id, mc)| summarise(root, dir_name, id, mc))
        .collect()
}

/// `(directory name, id, minecraft version)` for every usable server directory.
///
/// Split out of [`list_servers`] because a distribution build hashes every mod,
/// file and library anyway; counting them first is pure duplicate work.
fn server_dirs(root: &Root) -> Result<Vec<(String, String, MinecraftVersion)>> {
    let container = root.servers_dir();
    let entries = match std::fs::read_dir(&container) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(format!("reading {}", container.display()))(error)),
    };

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io(format!("reading {}", container.display())))?;
        names.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        ));
    }
    names.sort_by(|a, b| a.0.cmp(&b.0));

    let mut usable = Vec::new();
    for (dir_name, path) in names {
        if !path.is_dir() {
            log::warn!("Path {dir_name} in server directory is not a directory!");
            continue;
        }
        let Some((id, raw_mc)) = split_dir_name(&dir_name) else {
            log::warn!("Server directory {dir_name} does not match the defined standard.");
            log::warn!("All server ids must end with -<minecraft version> (ex. -1.12.2)");
            continue;
        };
        let mc = match raw_mc.parse::<MinecraftVersion>() {
            Ok(mc) => mc,
            Err(error) => {
                log::warn!("Server directory {dir_name} is not usable: {error}");
                continue;
            }
        };
        usable.push((dir_name.clone(), id.to_owned(), mc));
    }
    Ok(usable)
}

fn summarise(
    root: &Root,
    dir_name: String,
    id: String,
    mc: MinecraftVersion,
) -> Result<ServerSummary> {
    let dir = ServerDir::new(root.clone(), &dir_name);
    let meta = read_server_meta(&dir)?;
    let loader = server_loader(&meta);

    let mods = match &loader {
        Some((kind, _)) => scan::discover_toggleable(&kind.mod_container(&dir))?.len(),
        None => 0,
    };
    let libraries = scan::discover(&dir.libraries_dir())?
        .iter()
        .filter(|candidate| candidate.file_name.to_lowercase().ends_with("jar"))
        .count();

    Ok(ServerSummary {
        has_icon: resolve_icon(&dir, &meta).is_some(),
        dir_name,
        id,
        mc,
        loader,
        mods,
        files: count_files(&dir.files_dir())?,
        libraries,
        meta,
    })
}

pub fn read_server_meta(dir: &ServerDir) -> Result<ServerMeta> {
    let path = dir.server_meta_file();
    let raw = std::fs::read_to_string(&path).map_err(io(format!("reading {}", path.display())))?;
    // Notepad and PowerShell's `Set-Content -Encoding UTF8` both write a BOM,
    // and `serde_json` refuses one — a file a person edited by hand would
    // otherwise fail with `expected value at line 1 column 1`.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    serde_json::from_str(raw).map_err(|error| {
        Error::Invalid(format!(
            "{} is not a valid servermeta.json: {error}",
            path.display()
        ))
    })
}

/// The loader shown in listings. A hand-edited `servermeta.json` may name more
/// than one; [`build_server_spec`] still emits every one it finds.
fn server_loader(meta: &ServerMeta) -> Option<(LoaderKind, String)> {
    [
        (LoaderKind::Forge, &meta.forge),
        (LoaderKind::NeoForge, &meta.neoforge),
        (LoaderKind::Fabric, &meta.fabric),
    ]
    .into_iter()
    .find_map(|(kind, loader)| {
        loader
            .as_ref()
            .map(|LoaderMeta { version }| (kind, version.clone()))
    })
}

/// Upstream's `/(.+-(.+)$)/`: the split is at the **last** `-`.
fn split_dir_name(dir_name: &str) -> Option<(&str, &str)> {
    let (id, mc) = dir_name.rsplit_once('-')?;
    (!id.is_empty() && !mc.is_empty()).then_some((id, mc))
}

fn count_files(dir: &Path) -> Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(io(format!("reading {}", dir.display()))(error)),
    };
    let mut total = 0;
    for entry in entries {
        let entry = entry.map_err(io(format!("reading {}", dir.display())))?;
        let path = entry.path();
        if path.is_dir() {
            total += count_files(&path)?;
        } else if !scan::FILE_NAME_BLACKLIST.contains(&entry.file_name().to_string_lossy().as_ref())
        {
            total += 1;
        }
    }
    Ok(total)
}

/// Upstream icon rules: a `meta.icon` that parses as a URL wins; otherwise the
/// **last** `.png`/`.jpg` in the server directory, in name order, does.
fn resolve_icon(dir: &ServerDir, meta: &ServerMeta) -> Option<String> {
    if let Some(icon) = &meta.meta.icon
        && Url::parse(icon).is_ok()
    {
        return Some(icon.clone());
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir.path)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            let picture = lower.ends_with(".png") || lower.ends_with(".jpg");
            (picture && entry.path().is_file()).then_some(name)
        })
        .collect();
    names.sort();
    names.last().map(|name| dir.url(name))
}

/// Every server under `<root>/servers`, resolved into its launcher spec.
///
/// The directories are enumerated directly: [`list_servers`] would count mods,
/// files and libraries first, and this walks all of them anyway.
pub fn build_all_server_specs(
    root: &Root,
    java: Option<&Java>,
    opts: &ResolveOptions,
    policy: HashPolicy,
) -> Result<Vec<Server>> {
    let mut servers = Vec::new();
    for (dir_name, _, mc) in server_dirs(root)? {
        log::info!("Beginning processing of {dir_name}.");
        let meta = read_server_meta(&ServerDir::new(root.clone(), &dir_name))?;
        servers.push(build_spec(root, &dir_name, mc, &meta, java, opts, policy)?);
    }
    Ok(servers)
}

/// Assembles one server: loader roots and their mods first, then libraries,
/// then loose files.
pub fn build_server_spec(
    root: &Root,
    summary: &ServerSummary,
    java: Option<&Java>,
    opts: &ResolveOptions,
    policy: HashPolicy,
) -> Result<Server> {
    build_spec(
        root,
        &summary.dir_name,
        summary.mc,
        &summary.meta,
        java,
        opts,
        policy,
    )
}

fn build_spec(
    root: &Root,
    dir_name: &str,
    mc: MinecraftVersion,
    meta: &ServerMeta,
    java: Option<&Java>,
    opts: &ResolveOptions,
    policy: HashPolicy,
) -> Result<Server> {
    let dir = ServerDir::new(root.clone(), dir_name);
    let untracked = meta.untracked_files.as_deref().unwrap_or_default();
    let ctx = ScanContext {
        server: &dir,
        mc,
        java,
        untracked,
        policy,
    };

    let mut modules = Vec::new();

    if let Some(forge) = &meta.forge {
        if mc >= MinecraftVersion::new(1, 20, Some(3)) {
            for line in FORGE_MOD_LIST_WARNING.lines() {
                log::error!(target: "ForgeSupport", "{line}");
            }
        }
        // `forge_resolver` logs the log4j advisory for vulnerable versions.
        let java = require_java(java, LoaderKind::Forge, dir_name)?;
        modules.push(resolver::forge_resolver(root, &mc, &forge.version, java, *opts)?.module()?);
        let flavour = scan::forge_flavour_for(&mc)?;
        modules.extend(scan::scan_forge_mods(&ctx, ModLoader::Forge, flavour)?);
    }

    if let Some(neoforge) = &meta.neoforge {
        let java = require_java(java, LoaderKind::NeoForge, dir_name)?;
        modules.push(
            resolver::neoforge_resolver(root, &mc, &neoforge.version, java, *opts)?.module()?,
        );
        // Never `forge_flavour_for`: NeoForge also runs on year-based Minecraft.
        modules.extend(scan::scan_forge_mods(
            &ctx,
            ModLoader::NeoForge,
            ForgeFlavour::Modern113,
        )?);
    }

    if let Some(fabric) = &meta.fabric {
        modules.push(resolver::fabric_resolver(root, &mc, &fabric.version)?.module()?);
        modules.extend(scan::scan_fabric_mods(&ctx)?);
    }

    modules.extend(scan::scan_libraries(&ctx)?);
    modules.extend(scan::scan_files(&ctx)?);

    let icon = resolve_icon(&dir, meta);
    if icon.is_none() {
        log::warn!("No icon file found for server {dir_name}.");
    }

    // Upstream emits the raw text after the last `-`, not a reparsed version.
    let minecraft_version = split_dir_name(dir_name)
        .map(|(_, mc)| mc.to_owned())
        .unwrap_or_else(|| mc.to_string());

    Ok(Server {
        id: dir_name.to_owned(),
        name: meta.meta.name.clone(),
        description: meta.meta.description.clone(),
        icon,
        version: meta.meta.version.clone(),
        address: meta.meta.address.clone(),
        minecraft_version,
        discord: meta.meta.discord.clone(),
        main_server: meta.meta.main_server,
        autoconnect: meta.meta.autoconnect,
        java_options: meta.meta.java_options.clone(),
        modules,
    })
}

fn require_java<'a>(java: Option<&'a Java>, loader: LoaderKind, server: &str) -> Result<&'a Java> {
    java.ok_or_else(|| {
        Error::Config(format!(
            "JAVA_EXECUTABLE이 설정되지 않았습니다 — 서버 {server}의 {loader}를 처리하려면 \
             설정 화면에서 Java 실행 파일 경로를 지정하세요"
        ))
    })
}

/// Which namespace an imported mod lands in.
pub(crate) fn namespace_dir(container: &Path, namespace: Namespace) -> PathBuf {
    container.join(namespace.dir_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(dir: &Path) -> Root {
        Root::new(dir, Url::parse("https://cdn.example.invalid/").unwrap())
    }

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    #[test]
    fn splits_on_the_last_dash() {
        assert_eq!(split_dir_name("Test-1.20.1"), Some(("Test", "1.20.1")));
        assert_eq!(
            split_dir_name("my-pack-1.12.2"),
            Some(("my-pack", "1.12.2"))
        );
        assert_eq!(split_dir_name("nodash"), None);
        assert_eq!(split_dir_name("-1.20.1"), None);
    }

    #[test]
    fn rejects_two_loaders() {
        let error = CreateServerOptions {
            forge_version: Some("47.2.0".into()),
            fabric_version: Some("0.14.18".into()),
            ..Default::default()
        }
        .loader(&mc("1.20.1"))
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Forge, NeoForge and Fabric cannot be used together on the same server."
        );
    }

    #[test]
    fn rejects_neoforge_below_1_20_2() {
        let opts = CreateServerOptions {
            neoforge_version: Some("47.1.0".into()),
            ..Default::default()
        };
        assert!(opts.loader(&mc("1.20.1")).is_err());
        assert_eq!(
            opts.loader(&mc("1.21.1")).unwrap(),
            Some((LoaderKind::NeoForge, "47.1.0".into()))
        );
    }

    #[test]
    fn rejects_ids_that_are_not_a_single_path_segment() {
        let dir = tempfile::tempdir().unwrap();
        let root = root(dir.path());
        for id in ["../evil", "C:/tmp/x", "", "a:b", "a\\b", ".."] {
            let error = create_server(&root, id, &mc("1.20.1"), &CreateServerOptions::default())
                .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "invalid server id {id}: ids must be a single path segment (no / \\ : or ..)"
                )
            );
        }
        // Nothing was created on the way to those errors.
        assert!(!root.servers_dir().exists());
        assert!(create_server(&root, "Ok", &mc("1.20.1"), &CreateServerOptions::default()).is_ok());
    }

    #[test]
    fn creates_the_loader_specific_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = root(dir.path());
        let created = create_server(
            &root,
            "Neo",
            &mc("1.21.1"),
            &CreateServerOptions {
                neoforge_version: Some("21.1.77".into()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(created.dir.dir_name, "Neo-1.21.1");
        assert_eq!(created.loader, Some(LoaderKind::NeoForge));
        let container = created.mod_container.unwrap();
        assert!(container.ends_with("neoforgemods"));
        for namespace in scan::NAMESPACES {
            assert!(container.join(namespace.dir_name()).is_dir());
        }
        assert!(created.files_dir.is_dir());
        assert!(created.dir.libraries_dir().is_dir());
        assert!(!created.dir.forgemods_dir().exists());

        let raw = std::fs::read_to_string(created.dir.server_meta_file()).unwrap();
        assert!(raw.starts_with("{\n  \"$schema\": \""), "{raw}");
        let meta: ServerMeta = serde_json::from_str(&raw).unwrap();
        assert_eq!(meta.neoforge.unwrap().version, "21.1.77");

        assert_eq!(
            create_server(&root, "Neo", &mc("1.21.1"), &CreateServerOptions::default())
                .unwrap_err()
                .to_string(),
            "서버 폴더 Neo-1.21.1이(가) 이미 있습니다 — 다른 서버 ID를 쓰거나 기존 폴더를 지우세요"
        );
    }

    #[test]
    fn summarises_what_is_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = root(dir.path());
        let created = create_server(
            &root,
            "Test",
            &mc("1.20.1"),
            &CreateServerOptions {
                fabric_version: Some("0.14.18".into()),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::create_dir_all(created.files_dir.join("config")).unwrap();
        std::fs::write(created.files_dir.join("config/a.cfg"), b"a").unwrap();
        std::fs::write(created.files_dir.join("b.cfg"), b"b").unwrap();
        std::fs::write(created.dir.libraries_dir().join("foo-1.0.jar"), b"j").unwrap();
        std::fs::write(created.dir.libraries_dir().join("notes.txt"), b"n").unwrap();
        std::fs::write(
            namespace_dir(
                &created.mod_container.clone().unwrap(),
                Namespace::OptionalOn,
            )
            .join("m.jar"),
            b"m",
        )
        .unwrap();
        std::fs::write(root.servers_dir().join("loose.txt"), b"x").unwrap();
        std::fs::create_dir_all(root.servers_dir().join("nodash")).unwrap();

        let summaries = list_servers(&root).unwrap();
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.dir_name, "Test-1.20.1");
        assert_eq!(summary.id, "Test");
        assert_eq!(summary.mc, mc("1.20.1"));
        assert_eq!(
            summary.loader,
            Some((LoaderKind::Fabric, "0.14.18".to_owned()))
        );
        assert_eq!(summary.files, 2);
        assert_eq!(summary.libraries, 1);
        assert_eq!(summary.mods, 1);
        assert!(!summary.has_icon);
    }

    #[test]
    fn prefers_a_url_icon_then_the_last_picture() {
        let dir = tempfile::tempdir().unwrap();
        let root = root(dir.path());
        let created =
            create_server(&root, "Ico", &mc("1.20.1"), &CreateServerOptions::default()).unwrap();
        let mut meta = read_server_meta(&created.dir).unwrap();

        // The default icon is a help sentence, not a URL.
        assert_eq!(resolve_icon(&created.dir, &meta), None);

        std::fs::write(created.dir.path.join("a.PNG"), b"a").unwrap();
        std::fs::write(created.dir.path.join("b.jpg"), b"b").unwrap();
        assert_eq!(
            resolve_icon(&created.dir, &meta),
            Some("https://cdn.example.invalid/servers/Ico-1.20.1/b.jpg".to_owned())
        );

        meta.meta.icon = Some("https://host.invalid/icon.png".into());
        assert_eq!(
            resolve_icon(&created.dir, &meta),
            Some("https://host.invalid/icon.png".to_owned())
        );
    }

    #[test]
    fn reads_a_servermeta_that_an_editor_saved_with_a_bom() {
        let dir = tempfile::tempdir().unwrap();
        let root = root(dir.path());
        let created =
            create_server(&root, "Bom", &mc("1.20.1"), &CreateServerOptions::default()).unwrap();

        let path = created.dir.server_meta_file();
        let body = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("\u{feff}{body}")).unwrap();

        // Byte for byte a BOM is there, and the file still parses.
        assert_eq!(std::fs::read(&path).unwrap()[..3], [0xEF, 0xBB, 0xBF]);
        assert_eq!(
            read_server_meta(&created.dir).unwrap().meta.name,
            "Bom (Minecraft 1.20.1)"
        );
        // …and the listing that reads it still sees the server.
        assert_eq!(list_servers(&root).unwrap().len(), 1);
    }
}
