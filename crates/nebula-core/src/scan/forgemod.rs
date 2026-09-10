//! Port of `ForgeMod.struct.ts` plus its two flavours, `ForgeMod17` (1.7-1.12,
//! `mcmod.info`) and `ForgeMod113` (1.13+, `META-INF/mods.toml`), extended with
//! NeoForge.
//!
//! The flavour is always passed in, never derived from the Minecraft version:
//! NeoForge on year-based Minecraft (26.x) has no minor-range meaning.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zip::ZipArchive;

use crate::error::{Error, Result, io};
use crate::java::ClaritasMeta;
use crate::layout::ServerDir;
use crate::spec::{Module, ModuleType};
use crate::version::{MinecraftVersion, is_version_acceptable};

use super::toggle::{self, Namespace};
use super::{
    Candidate, ScanContext, UntrackedMatcher, build_artifact, capitalize, crude_inference,
    maven_module_id,
};

/// Upstream `BaseForgeModStructure.EXAMPLE_MOD_ID`.
pub const EXAMPLE_MOD_ID: &str = "examplemod";
/// Upstream `getClaritasExceptions()`.
pub const OPTIFINE_EXCEPTION: &str = "optifine";
pub const OPTIFINE_GROUP: &str = "net.optifine";
const JAR_VERSION_WILDCARD: &str = "${file.jarVersion}";
const IMPLEMENTATION_VERSION: &str = "Implementation-Version: ";

/// Which metadata file layout a mod jar uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeFlavour {
    /// Minecraft 1.7-1.12: `mcmod.info`.
    Legacy17,
    /// Minecraft 1.13+ and every NeoForge version: `META-INF/mods.toml`.
    Modern113,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLoader {
    Forge,
    NeoForge,
}

impl ModLoader {
    pub fn module_type(self) -> ModuleType {
        match self {
            ModLoader::Forge => ModuleType::ForgeMod,
            ModLoader::NeoForge => ModuleType::NeoForgeMod,
        }
    }

    /// Both the `untrackedFiles.appliesTo` name and the server-relative
    /// directory the mods live in.
    pub fn struct_root(self) -> &'static str {
        match self {
            ModLoader::Forge => "forgemods",
            ModLoader::NeoForge => "neoforgemods",
        }
    }

    /// Zip entries holding the mod metadata, in precedence order.
    pub fn toml_entries(self) -> &'static [&'static str] {
        match self {
            ModLoader::Forge => &["META-INF/mods.toml"],
            ModLoader::NeoForge => &["META-INF/neoforge.mods.toml", "META-INF/mods.toml"],
        }
    }

    pub fn container(self, server: &ServerDir) -> PathBuf {
        match self {
            ModLoader::Forge => server.forgemods_dir(),
            ModLoader::NeoForge => server.neoforgemods_dir(),
        }
    }
}

/// Port of `VersionSegmentedRegistry.getForgeModStruct`. Forge only: NeoForge
/// always uses [`ForgeFlavour::Modern113`].
pub fn forge_flavour_for(mc: &MinecraftVersion) -> Result<ForgeFlavour> {
    if is_version_acceptable(mc, &[7, 8, 9, 10, 11, 12]) {
        Ok(ForgeFlavour::Legacy17)
    } else if is_version_acceptable(mc, &[13, 14, 15, 16, 17, 18, 19, 20, 21]) {
        Ok(ForgeFlavour::Modern113)
    } else {
        Err(Error::Invalid(format!(
            "No forge mod structure found for Minecraft {mc}!"
        )))
    }
}

// ---------------------------------------------------------------------------
// Metadata files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ModsToml {
    pub mods: Vec<ModsTomlEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ModsTomlEntry {
    #[serde(rename = "modId")]
    pub mod_id: String,
    pub version: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct McModInfo {
    pub modid: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum McModInfoFile {
    List {
        #[serde(rename = "modList")]
        mod_list: Vec<McModInfo>,
    },
    Array(Vec<McModInfo>),
}

/// `None` means "absent or unusable", which sends the caller down the
/// Claritas/crude fallback path. An empty `[[mods]]` array counts as unusable:
/// upstream threw a `TypeError` on `mods[0]`.
pub(crate) fn parse_mods_toml(bytes: &[u8]) -> Option<ModsToml> {
    let parsed: ModsToml = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    (!parsed.mods.is_empty()).then_some(parsed)
}

/// Accepts both `mcmod.info` shapes: a bare array and `{ modListVersion,
/// modList }`. The first entry is taken to be the main mod.
pub(crate) fn parse_mcmod_info(bytes: &[u8]) -> Option<McModInfo> {
    let entries = match serde_json::from_slice::<McModInfoFile>(bytes).ok()? {
        McModInfoFile::List { mod_list } => mod_list,
        McModInfoFile::Array(entries) => entries,
    };
    entries.into_iter().next()
}

/// `Implementation-Version` from a `META-INF/MANIFEST.MF`, last occurrence
/// wins (upstream kept overwriting).
///
/// Divergence: upstream's regex ends in `[\r\n]` yet it is applied to lines
/// already split on `\n`, so it only ever matched CRLF manifests. We accept
/// both line endings.
pub(crate) fn implementation_version(manifest: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(manifest);
    text.split('\n')
        .filter_map(|line| line.strip_prefix(IMPLEMENTATION_VERSION))
        .map(|version| version.trim_end_matches('\r').to_owned())
        .rfind(|version| !version.is_empty())
}

/// OptiFine ships no mod metadata; upstream reads `changelog.txt` instead and
/// takes the first line plus its second space-separated token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptifineChangelog {
    pub first_line: String,
    pub version: Option<String>,
}

pub(crate) fn optifine_from_changelog(changelog: &[u8]) -> OptifineChangelog {
    let text = String::from_utf8_lossy(changelog);
    let first_line = text
        .split('\n')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let version = first_line
        .split(' ')
        .nth(1)
        .filter(|version| !version.is_empty())
        .map(str::to_owned);
    OptifineChangelog {
        first_line,
        version,
    }
}

// ---------------------------------------------------------------------------
// Metadata resolution (pure, so it is testable without Java or a jar)
// ---------------------------------------------------------------------------

/// The three values one mod jar contributes to its module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeModMeta {
    pub mod_id: String,
    pub version: String,
    pub display_name: String,
}

/// Upstream `discernResult`: Claritas wins unless it is missing or empty.
fn discern(claritas: Option<&str>, crude: &str) -> String {
    match claritas {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => crude.to_owned(),
    }
}

fn claritas_field<'a>(
    claritas: Option<&'a ClaritasMeta>,
    field: impl Fn(&'a ClaritasMeta) -> Option<&'a String>,
) -> Option<&'a str> {
    claritas.and_then(field).map(String::as_str)
}

pub(crate) fn optifine_meta(
    flavour: ForgeFlavour,
    file_name: &str,
    changelog: &[u8],
) -> ForgeModMeta {
    let info = optifine_from_changelog(changelog);
    // Upstream would emit `undefined` when the first line has no second token.
    let version = info.version.unwrap_or_else(|| crude_inference(file_name).1);
    ForgeModMeta {
        mod_id: OPTIFINE_EXCEPTION.to_owned(),
        version,
        display_name: match flavour {
            ForgeFlavour::Modern113 => "OptiFine".to_owned(),
            ForgeFlavour::Legacy17 => info.first_line,
        },
    }
}

/// 1.13+ resolution. `manifest` is only consulted for the
/// `${file.jarVersion}` wildcard.
pub(crate) fn forge_meta_113(
    file_name: &str,
    parsed: Option<ModsToml>,
    claritas: Option<&ClaritasMeta>,
    manifest: Option<&[u8]>,
) -> ForgeModMeta {
    let (crude_name, crude_version) = crude_inference(file_name);
    let claritas_id = claritas_field(claritas, |meta| meta.id.as_ref());

    let Some(entry) = parsed.and_then(|toml| toml.mods.into_iter().next()) else {
        return ForgeModMeta {
            mod_id: discern(claritas_id, &crude_name.to_lowercase()),
            version: crude_version,
            display_name: crude_name,
        };
    };

    // `Module.name` is not optional in the launcher contract, so a missing
    // displayName falls back to the crude name instead of being dropped.
    let mut mod_id = entry.mod_id;
    let mut display_name = entry.display_name.unwrap_or_else(|| crude_name.clone());
    if mod_id == EXAMPLE_MOD_ID {
        mod_id = discern(claritas_id, &crude_name.to_lowercase());
        display_name = crude_name.clone();
    }

    let version = match entry.version.as_deref() {
        Some(JAR_VERSION_WILDCARD) => manifest
            .and_then(implementation_version)
            .unwrap_or(crude_version),
        Some(version) => version.to_owned(),
        None => crude_version,
    };

    ForgeModMeta {
        mod_id,
        version,
        display_name,
    }
}

/// 1.7-1.12 resolution.
pub(crate) fn forge_meta_17(
    file_name: &str,
    parsed: Option<McModInfo>,
    claritas: Option<&ClaritasMeta>,
) -> ForgeModMeta {
    let (crude_name, crude_version) = crude_inference(file_name);
    let claritas_id = claritas_field(claritas, |meta| meta.id.as_ref());
    let claritas_name = claritas_field(claritas, |meta| meta.name.as_ref());
    let claritas_version = claritas_field(claritas, |meta| meta.version.as_ref());

    let Some(info) = parsed else {
        return ForgeModMeta {
            mod_id: discern(claritas_id, &crude_name.to_lowercase()),
            version: discern(claritas_version, &crude_version),
            display_name: discern(claritas_name, &crude_name),
        };
    };

    let mut mod_id = info.modid.unwrap_or_default();
    let mut display_name = info.name.unwrap_or_else(|| crude_name.clone());
    if mod_id.is_empty() || mod_id == EXAMPLE_MOD_ID {
        mod_id = discern(claritas_id, &crude_name.to_lowercase());
        display_name = discern(claritas_name, &crude_name);
    }

    let version = match info.version {
        Some(version) if !is_malformed_version(&version) => version,
        _ => discern(claritas_version, &crude_version),
    };

    ForgeModMeta {
        mod_id,
        version,
        display_name,
    }
}

/// Ex. empty, `@VERSION@`, `${version}`.
fn is_malformed_version(version: &str) -> bool {
    version.trim().is_empty() || version.contains('@') || version.contains('$')
}

/// `(module id, module name)`. The group comes from Claritas when it reported
/// one, otherwise from the module type (`generated.forgemod`,
/// `generated.neoforgemod`).
pub(crate) fn build_forge_module_from_meta(
    meta: &ForgeModMeta,
    claritas: Option<&ClaritasMeta>,
    module_type: ModuleType,
) -> (String, String) {
    let group = claritas_field(claritas, |meta| meta.group.as_ref())
        .filter(|group| !group.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| module_type.default_group());
    (
        maven_module_id(&group, &meta.mod_id, &meta.version, module_type),
        capitalize(&meta.display_name),
    )
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

pub fn scan_forge_mods(
    ctx: &ScanContext,
    loader: ModLoader,
    flavour: ForgeFlavour,
) -> Result<Vec<Module>> {
    let container = loader.container(ctx.server);
    let candidates = toggle::discover_toggleable(&container)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let claritas = invoke_claritas(ctx, &candidates)?;
    let container_real = crate::paths::real_path(&container)?;
    let matcher = UntrackedMatcher::new(loader.struct_root(), ctx.untracked)?;
    let module_type = loader.module_type();

    let mut modules = Vec::with_capacity(candidates.len());
    for (namespace, candidate) in &candidates {
        let claritas_meta = claritas_key(&candidate.path)
            .ok()
            .and_then(|key| claritas.get(&key));
        let meta = read_forge_mod_meta(
            loader,
            flavour,
            &candidate.file_name,
            &candidate.path,
            claritas_meta,
        )?;
        let (id, name) = build_forge_module_from_meta(&meta, claritas_meta, module_type);

        let rel = format!("{}/{}", namespace.dir_name(), candidate.file_name);
        let untracked = matcher.is_untracked(&rel);
        let artifact = build_artifact(
            &container_real,
            &candidate.path,
            candidate.size,
            ctx.server.url(&format!("{}/{rel}", loader.struct_root())),
            None,
            untracked,
            &ctx.policy,
        )?;
        modules.push(Module {
            id,
            name,
            module_type,
            classpath: None,
            required: namespace.required(),
            untracked: untracked.then_some(true),
            artifact,
            sub_modules: None,
        });
    }
    Ok(toggle::finish_modules(modules))
}

/// Reads one jar and resolves its metadata. Exposed so the flavour and the
/// `neoforge.mods.toml` precedence can be tested without Java.
pub(crate) fn read_forge_mod_meta(
    loader: ModLoader,
    flavour: ForgeFlavour,
    file_name: &str,
    jar: &Path,
    claritas: Option<&ClaritasMeta>,
) -> Result<ForgeModMeta> {
    let mut zip = crate::import::open(jar)?;

    if is_optifine(file_name) {
        let Some(changelog) = entry_bytes(&mut zip, "changelog.txt") else {
            return Err(Error::Invalid("Failed to read OptiFine changelog.".into()));
        };
        return Ok(optifine_meta(flavour, file_name, &changelog));
    }

    if claritas.is_none() {
        log::error!("Claritas failed to yield metadata for ForgeMod {file_name}!");
        log::error!("Is this mod malformatted or does Claritas need an update?");
    }

    match flavour {
        ForgeFlavour::Modern113 => {
            let raw = loader
                .toml_entries()
                .iter()
                .find_map(|entry| entry_bytes(&mut zip, entry));
            let parsed = match &raw {
                Some(bytes) => {
                    let parsed = parse_mods_toml(bytes);
                    if parsed.is_none() {
                        log::error!("ForgeMod {file_name} contains an invalid mods.toml file.");
                    }
                    parsed
                }
                None => {
                    log::error!("ForgeMod {file_name} does not contain mods.toml file.");
                    None
                }
            };
            // The manifest is only ever consulted for `${file.jarVersion}`, and
            // reading it means inflating another zip entry for every mod.
            let manifest = wants_jar_version(parsed.as_ref())
                .then(|| entry_bytes(&mut zip, "META-INF/MANIFEST.MF"))
                .flatten();
            Ok(forge_meta_113(
                file_name,
                parsed,
                claritas,
                manifest.as_deref(),
            ))
        }
        ForgeFlavour::Legacy17 => {
            let parsed = match entry_bytes(&mut zip, "mcmod.info") {
                Some(bytes) => {
                    let parsed = parse_mcmod_info(&bytes);
                    if parsed.is_none() {
                        log::error!("ForgeMod {file_name} contains an invalid mcmod.info file.");
                    }
                    parsed
                }
                None => {
                    log::warn!("ForgeMod {file_name} does not contain mcmod.info file.");
                    None
                }
            };
            log_mod_type(file_name, claritas);
            Ok(forge_meta_17(file_name, parsed, claritas))
        }
    }
}

/// Whether the mod defers its version to `META-INF/MANIFEST.MF`.
fn wants_jar_version(parsed: Option<&ModsToml>) -> bool {
    parsed
        .and_then(|toml| toml.mods.first())
        .and_then(|entry| entry.version.as_deref())
        == Some(JAR_VERSION_WILDCARD)
}

fn log_mod_type(file_name: &str, claritas: Option<&ClaritasMeta>) {
    match claritas_field(claritas, |meta| meta.mod_type.as_ref()) {
        Some("CORE_MOD") => log::info!(
            "CORE_MOD Discovered: ForgeMod {file_name} has no @Mod annotation. Metadata inference capabilities are limited."
        ),
        Some("TWEAKER") => log::info!(
            "TWEAKER Discovered: ForgeMod {file_name} has no @Mod annotation. Metadata inference capabilities may be limited."
        ),
        Some("UNKNOWN") => {
            log::error!("Jar file {file_name} is not a ForgeMod. Is it a library?")
        }
        _ => {}
    }
}

/// Runs Claritas once over every non-OptiFine candidate. A Claritas failure is
/// non-fatal upstream, so it degrades to an empty map and the crude fallbacks.
fn invoke_claritas(
    ctx: &ScanContext,
    candidates: &[(Namespace, Candidate)],
) -> Result<HashMap<PathBuf, ClaritasMeta>> {
    let (mut jars, mut proxies) = (Vec::new(), Vec::new());
    for (_, candidate) in candidates {
        let key = claritas_key(&candidate.path)?;
        if is_optifine(&candidate.file_name) {
            proxies.push(key);
        } else {
            jars.push(key);
        }
    }

    // OptiFine never goes through Claritas: its metadata is the proxy below and
    // its changelog, so a container holding only OptiFine needs no JDK at all.
    let mut result = HashMap::new();
    if !jars.is_empty() {
        let Some(java) = ctx.java else {
            return Err(Error::Config(
                "JAVA_EXECUTABLE is required to analyse Forge mods".into(),
            ));
        };
        let scratch = ctx.server.path.join("claritasWork");
        result = match java.run_claritas(&scratch, &ctx.mc, &jars)? {
            Some(result) => result,
            None => {
                log::error!("Failed to process Claritas result!");
                HashMap::new()
            }
        };
    }

    // Claritas echoes the jar paths back as object keys. If it normalised them
    // the lookup silently misses and every mod falls back to the default group.
    if !result.is_empty() && !jars.iter().any(|jar| result.contains_key(jar)) {
        log::warn!(
            "Claritas returned {} entries but none matched the scanned jar paths; module groups will fall back to defaults.",
            result.len()
        );
    }

    for key in proxies {
        result.insert(
            key,
            ClaritasMeta {
                group: Some(OPTIFINE_GROUP.to_owned()),
                ..ClaritasMeta::default()
            },
        );
    }
    Ok(result)
}

/// Claritas resolves relative paths against its own working directory, so both
/// the argument list and the lookup key use the absolute form. `absolute` is
/// purely lexical: `canonicalize` would yield `\\?\C:\...` on Windows, which
/// Java does not echo back verbatim.
fn claritas_key(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).map_err(io(format!("resolving {}", path.display())))
}

fn is_optifine(file_name: &str) -> bool {
    file_name.to_lowercase().contains(OPTIFINE_EXCEPTION)
}

fn entry_bytes<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MODS_TOML: &str = r#"
modLoader = "javafml"
loaderVersion = "[47,)"
license = "MIT"

[[mods]]
modId = "jei"
version = "${file.jarVersion}"
displayName = "Just Enough Items"
description = "Item and recipe viewing."
"#;

    fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn resolves_a_1_13_mod_with_a_jar_version_wildcard() {
        let jar_name = "jei-1.20.1-11.6.0.1024.jar";
        let parsed = parse_mods_toml(MODS_TOML.as_bytes()).unwrap();
        assert_eq!(parsed.mods[0].mod_id, "jei");
        assert_eq!(
            parsed.mods[0].display_name.as_deref(),
            Some("Just Enough Items")
        );
        // Only this shape makes `read_forge_mod_meta` open the manifest.
        assert!(wants_jar_version(Some(&parsed)));

        let manifest = b"Manifest-Version: 1.0\r\nImplementation-Version: 11.6.0.1024\r\n\r\n";
        let meta = forge_meta_113(
            jar_name,
            parse_mods_toml(MODS_TOML.as_bytes()),
            None,
            Some(manifest),
        );
        assert_eq!(
            meta,
            ForgeModMeta {
                mod_id: "jei".to_owned(),
                version: "11.6.0.1024".to_owned(),
                display_name: "Just Enough Items".to_owned(),
            }
        );

        let (id, name) = build_forge_module_from_meta(&meta, None, ModuleType::ForgeMod);
        assert_eq!(id, "generated.forgemod:jei:11.6.0.1024@jar");
        assert_eq!(name, "Just Enough Items");

        // Deliberate divergence: upstream's regex only matched CRLF manifests.
        assert_eq!(
            implementation_version(b"Implementation-Version: 1.2.3\n"),
            Some("1.2.3".to_owned())
        );
        // Last occurrence wins, empty values are ignored.
        assert_eq!(
            implementation_version(
                b"Implementation-Version: 1\r\nImplementation-Version: 2\r\nImplementation-Version: \r\n"
            ),
            Some("2".to_owned())
        );
        assert_eq!(implementation_version(b"Manifest-Version: 1.0\r\n"), None);

        // No manifest -> crude version.
        let meta = forge_meta_113(jar_name, parse_mods_toml(MODS_TOML.as_bytes()), None, None);
        assert_eq!(meta.version, "1.20.1-11.6.0.1024");
    }

    #[test]
    fn only_reads_the_manifest_for_the_jar_version_wildcard() {
        let fixed = br#"
[[mods]]
modId = "jei"
version = "1.0"
"#;
        assert!(!wants_jar_version(parse_mods_toml(fixed).as_ref()));
        assert!(!wants_jar_version(None));
    }

    #[test]
    fn falls_back_to_claritas_for_1_13_example_mods() {
        let claritas = ClaritasMeta {
            id: Some("realmod".to_owned()),
            group: Some("com.example.real".to_owned()),
            ..ClaritasMeta::default()
        };
        let toml = br#"
[[mods]]
modId = "examplemod"
version = "1.0"
displayName = "Example Mod"
"#;
        let meta = forge_meta_113(
            "RealMod-1.0.jar",
            parse_mods_toml(toml),
            Some(&claritas),
            None,
        );
        assert_eq!(meta.mod_id, "realmod");
        assert_eq!(meta.display_name, "RealMod");
        assert_eq!(meta.version, "1.0");

        let (id, name) =
            build_forge_module_from_meta(&meta, Some(&claritas), ModuleType::NeoForgeMod);
        assert_eq!(id, "com.example.real:realmod:1.0@jar");
        assert_eq!(name, "RealMod");

        // An empty `[[mods]]` array is unusable (upstream threw on `mods[0]`).
        assert!(parse_mods_toml(b"modLoader = \"javafml\"\n").is_none());
        assert!(parse_mods_toml(b"not = [toml").is_none());

        let meta = forge_meta_113("RealMod-1.0.jar", None, None, None);
        assert_eq!(meta.mod_id, "realmod");
        assert_eq!(meta.version, "1.0");
        let (id, _) = build_forge_module_from_meta(&meta, None, ModuleType::ForgeMod);
        assert_eq!(id, "generated.forgemod:realmod:1.0@jar");
    }

    #[test]
    fn parses_both_mcmod_info_shapes() {
        const ENTRY: &str = r#"{"modid":"jei","name":"Just Enough Items","version":"4.16.1.302"}"#;
        let array = format!("[{ENTRY}]");
        let list = format!(r#"{{"modListVersion":2,"modList":[{ENTRY}]}}"#);

        for raw in [array.as_bytes(), list.as_bytes()] {
            let info = parse_mcmod_info(raw).unwrap();
            let meta = forge_meta_17("jei_1.12.2-4.16.1.302.jar", Some(info), None);
            assert_eq!(meta.mod_id, "jei");
            assert_eq!(meta.version, "4.16.1.302");
            assert_eq!(meta.display_name, "Just Enough Items");
        }
        assert!(parse_mcmod_info(b"[]").is_none());
        assert!(parse_mcmod_info(b"not json").is_none());

        // A malformed version falls through to Claritas, then to crude inference.
        let info = parse_mcmod_info(br#"[{"modid":"jei","name":"JEI","version":"@VERSION@"}]"#);
        let meta = forge_meta_17("jei-4.16.1.302.jar", info, None);
        assert_eq!(meta.version, "4.16.1.302");

        let claritas = ClaritasMeta {
            id: Some("realid".to_owned()),
            name: Some("Real Mod".to_owned()),
            version: Some("9.9".to_owned()),
            mod_type: Some("CORE_MOD".to_owned()),
            ..ClaritasMeta::default()
        };
        let info = parse_mcmod_info(br#"[{"modid":"examplemod","name":"Example","version":""}]"#);
        let meta = forge_meta_17("whatever-1.0.jar", info, Some(&claritas));
        assert_eq!(meta.mod_id, "realid");
        assert_eq!(meta.display_name, "Real Mod");
        assert_eq!(meta.version, "9.9");

        // Nothing anywhere: everything comes from the file name.
        let meta = forge_meta_17("whatever-1.0.jar", None, None);
        assert_eq!(
            meta,
            ForgeModMeta {
                mod_id: "whatever".to_owned(),
                version: "1.0".to_owned(),
                display_name: "whatever".to_owned(),
            }
        );
    }

    #[test]
    fn handles_the_optifine_special_case_for_both_flavours() {
        let changelog = b"OptiFine 1.12.2_HD_U_G5\nchangelog body\n".as_slice();
        let parsed = optifine_from_changelog(changelog);
        assert_eq!(parsed.first_line, "OptiFine 1.12.2_HD_U_G5");
        assert_eq!(parsed.version.as_deref(), Some("1.12.2_HD_U_G5"));

        let jar_name = "OptiFine_1.12.2_HD_U_G5.jar";
        let legacy = optifine_meta(ForgeFlavour::Legacy17, jar_name, changelog);
        assert_eq!(legacy.mod_id, "optifine");
        assert_eq!(legacy.version, "1.12.2_HD_U_G5");
        assert_eq!(legacy.display_name, "OptiFine 1.12.2_HD_U_G5");

        let modern = optifine_meta(ForgeFlavour::Modern113, jar_name, changelog);
        assert_eq!(modern.mod_id, "optifine");
        assert_eq!(modern.version, "1.12.2_HD_U_G5");
        assert_eq!(modern.display_name, "OptiFine");

        // The group is the Claritas proxy metadata, never read from the jar.
        let proxy = ClaritasMeta {
            group: Some("net.optifine".to_owned()),
            ..ClaritasMeta::default()
        };
        let (id, name) = build_forge_module_from_meta(&modern, Some(&proxy), ModuleType::ForgeMod);
        assert_eq!(id, "net.optifine:optifine:1.12.2_HD_U_G5@jar");
        assert_eq!(name, "OptiFine");

        // Reading a real jar takes the same path, and a missing changelog fails.
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join(jar_name);
        write_jar(&jar, &[("changelog.txt", changelog)]);
        for flavour in [ForgeFlavour::Legacy17, ForgeFlavour::Modern113] {
            let meta =
                read_forge_mod_meta(ModLoader::Forge, flavour, jar_name, &jar, None).unwrap();
            assert_eq!(meta, optifine_meta(flavour, jar_name, changelog));
        }

        let empty = dir.path().join("OptiFine_empty.jar");
        write_jar(&empty, &[("README", b"nothing here")]);
        let error = read_forge_mod_meta(
            ModLoader::Forge,
            ForgeFlavour::Modern113,
            "OptiFine_empty.jar",
            &empty,
            None,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Failed to read OptiFine changelog."),
            "{error}"
        );
    }

    #[test]
    fn neoforge_metadata_takes_precedence_over_mods_toml() {
        assert_eq!(ModLoader::NeoForge.module_type(), ModuleType::NeoForgeMod);
        assert_eq!(ModLoader::NeoForge.struct_root(), "neoforgemods");
        assert_eq!(ModLoader::Forge.module_type(), ModuleType::ForgeMod);
        assert_eq!(ModLoader::Forge.struct_root(), "forgemods");
        assert_eq!(
            ModuleType::NeoForgeMod.default_group(),
            "generated.neoforgemod"
        );

        let neo = br#"
[[mods]]
modId = "neomod"
version = "2.0"
displayName = "Neo Mod"
"#;
        let forge = br#"
[[mods]]
modId = "forgemod"
version = "1.0"
displayName = "Forge Mod"
"#;

        let dir = tempfile::tempdir().unwrap();
        let both = dir.path().join("mymod-1.0.jar");
        write_jar(
            &both,
            &[
                ("META-INF/mods.toml", forge),
                ("META-INF/neoforge.mods.toml", neo),
            ],
        );

        let read = |loader| {
            read_forge_mod_meta(
                loader,
                ForgeFlavour::Modern113,
                "mymod-1.0.jar",
                &both,
                None,
            )
            .unwrap()
        };
        assert_eq!(read(ModLoader::NeoForge).mod_id, "neomod");
        assert_eq!(read(ModLoader::Forge).mod_id, "forgemod");

        // NeoForge still falls back to mods.toml.
        let legacy_only = dir.path().join("legacy-1.0.jar");
        write_jar(&legacy_only, &[("META-INF/mods.toml", forge)]);
        let meta = read_forge_mod_meta(
            ModLoader::NeoForge,
            ForgeFlavour::Modern113,
            "legacy-1.0.jar",
            &legacy_only,
            None,
        )
        .unwrap();
        assert_eq!(meta.mod_id, "forgemod");
        let (id, _) = build_forge_module_from_meta(&meta, None, ModuleType::NeoForgeMod);
        assert_eq!(id, "generated.neoforgemod:forgemod:1.0@jar");
    }
}
