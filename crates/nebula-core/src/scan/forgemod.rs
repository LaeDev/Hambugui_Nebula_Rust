//! Port of `ForgeMod.struct.ts` plus its two flavours, `ForgeMod17` (1.7-1.12,
//! `mcmod.info`) and `ForgeMod113` (1.13+, `META-INF/mods.toml`), extended with
//! NeoForge.
//!
//! The flavour is always passed in, never derived from the Minecraft version:
//! NeoForge on year-based Minecraft (26.x) has no minor-range meaning.

use std::collections::HashMap;
use std::fs::File;
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
pub struct ModsToml {
    pub mods: Vec<ModsTomlEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModsTomlEntry {
    #[serde(rename = "modId")]
    pub mod_id: String,
    pub version: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McModInfo {
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
pub fn parse_mods_toml(bytes: &[u8]) -> Option<ModsToml> {
    let parsed: ModsToml = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    (!parsed.mods.is_empty()).then_some(parsed)
}

/// Accepts both `mcmod.info` shapes: a bare array and `{ modListVersion,
/// modList }`. The first entry is taken to be the main mod.
pub fn parse_mcmod_info(bytes: &[u8]) -> Option<McModInfo> {
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
pub fn implementation_version(manifest: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(manifest);
    text.split('\n')
        .filter_map(|line| line.strip_prefix(IMPLEMENTATION_VERSION))
        .map(|version| version.trim_end_matches('\r').to_owned())
        .rfind(|version| !version.is_empty())
}

/// OptiFine ships no mod metadata; upstream reads `changelog.txt` instead and
/// takes the first line plus its second space-separated token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptifineChangelog {
    pub first_line: String,
    pub version: Option<String>,
}

pub fn optifine_from_changelog(changelog: &[u8]) -> OptifineChangelog {
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
pub struct ForgeModMeta {
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

pub fn optifine_meta(flavour: ForgeFlavour, file_name: &str, changelog: &[u8]) -> ForgeModMeta {
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
pub fn forge_meta_113(
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
pub fn forge_meta_17(
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
pub fn build_forge_module_from_meta(
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
            &container,
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
pub fn read_forge_mod_meta(
    loader: ModLoader,
    flavour: ForgeFlavour,
    file_name: &str,
    jar: &Path,
    claritas: Option<&ClaritasMeta>,
) -> Result<ForgeModMeta> {
    let file = File::open(jar).map_err(io(format!("opening {}", jar.display())))?;
    let mut zip = ZipArchive::new(file)?;

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
            let manifest = entry_bytes(&mut zip, "META-INF/MANIFEST.MF");
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
    let Some(java) = ctx.java else {
        return Err(Error::Config(
            "JAVA_EXECUTABLE is required to analyse Forge mods".into(),
        ));
    };

    let (mut jars, mut proxies) = (Vec::new(), Vec::new());
    for (_, candidate) in candidates {
        let key = claritas_key(&candidate.path)?;
        if is_optifine(&candidate.file_name) {
            proxies.push(key);
        } else {
            jars.push(key);
        }
    }

    let scratch = ctx.server.path.join("claritasWork");
    let mut result = match java.run_claritas(&scratch, &ctx.mc, &jars)? {
        Some(result) => result,
        None => {
            log::error!("Failed to process Claritas result!");
            HashMap::new()
        }
    };

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
