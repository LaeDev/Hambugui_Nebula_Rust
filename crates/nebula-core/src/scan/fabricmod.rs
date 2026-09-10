//! Port of `FabricMod.struct.ts`, including the entry-point package group
//! inference adapted from Claritas' `DataUtil`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use zip::ZipArchive;

use crate::error::{Result, io};
use crate::spec::{Module, ModuleType};

use super::toggle;
use super::{
    ScanContext, UntrackedMatcher, build_artifact, capitalize, crude_inference, maven_module_id,
};

pub const STRUCT_ROOT: &str = "fabricmods";

#[derive(Debug, Clone, Deserialize)]
pub struct FabricModJson {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub entrypoints: Option<BTreeMap<String, Vec<FabricEntryPoint>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FabricEntryPoint {
    Plain(String),
    /// `{ "adapter": "kotlin", "value": "..." }`
    Adapted {
        value: String,
    },
}

impl FabricEntryPoint {
    pub fn value(&self) -> &str {
        match self {
            FabricEntryPoint::Plain(value) | FabricEntryPoint::Adapted { value } => value,
        }
    }
}

pub fn parse_fabric_mod_json(bytes: &[u8]) -> Option<FabricModJson> {
    serde_json::from_slice(bytes).ok()
}

/// First element of the first non-empty `main` / `client` / `server` list.
pub fn entry_point(json: &FabricModJson) -> Option<&str> {
    let entrypoints = json.entrypoints.as_ref()?;
    ["main", "client", "server"].into_iter().find_map(|key| {
        entrypoints
            .get(key)
            .and_then(|entries| entries.first())
            .map(FabricEntryPoint::value)
    })
}

/// Walks up from a fully qualified entry-point class until a package segment
/// looks like a real group. `forge`/`fabric`/`bukkit`/`sponge` are only
/// blacklisted when they differ from the mod id, exactly as upstream; the same
/// holds implicitly for the other six because the blacklist is only consulted
/// when `term != mod_id`.
pub fn infer_group(entry_class: &str, mod_id: &str) -> String {
    const BLACKLIST: [&str; 10] = [
        "common", "util", "internal", "tweaker", "tweak", "client", "forge", "fabric", "bukkit",
        "sponge",
    ];
    let mut bits: Vec<&str> = entry_class.split('.').collect();
    // The entry point is a class name, so the first pop always drops it.
    while !bits.is_empty() {
        bits.pop();
        let Some(term) = bits.last().copied() else {
            break;
        };
        let blacklisted = BLACKLIST.contains(&term) && term != mod_id;
        if (term != mod_id && !blacklisted)
            || bits.len() == 1
            || (bits.len() == 2 && term == mod_id)
        {
            break;
        }
    }
    bits.join(".")
}

/// `(module id, module name)` for one mod jar. `parsed` is `None` when the jar
/// has no usable `fabric.mod.json`.
pub fn fabric_module_id_and_name(
    file_name: &str,
    parsed: Option<FabricModJson>,
) -> (String, String) {
    const TYPE: ModuleType = ModuleType::FabricMod;
    let (crude_name, crude_version) = crude_inference(file_name);
    let json = parsed.unwrap_or_else(|| FabricModJson {
        id: crude_name.to_lowercase(),
        version: crude_version,
        name: Some(crude_name),
        entrypoints: None,
    });

    // Upstream relies on `''` being falsy for both the group and the name.
    let group = entry_point(&json)
        .map(|entry| infer_group(entry, &json.id))
        .filter(|group| !group.is_empty())
        .unwrap_or_else(|| TYPE.default_group());
    let name = json
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&json.id);

    (
        maven_module_id(&group, &json.id, &json.version, TYPE),
        capitalize(name),
    )
}

pub fn scan_fabric_mods(ctx: &ScanContext) -> Result<Vec<Module>> {
    const TYPE: ModuleType = ModuleType::FabricMod;
    let container = ctx.server.fabricmods_dir();
    let candidates = toggle::discover_toggleable(&container)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let matcher = UntrackedMatcher::new(STRUCT_ROOT, ctx.untracked)?;

    let mut modules = Vec::with_capacity(candidates.len());
    for (namespace, candidate) in &candidates {
        let parsed = read_fabric_mod_json(&candidate.file_name, &candidate.path)?;
        let (id, name) = fabric_module_id_and_name(&candidate.file_name, parsed);

        let rel = format!("{}/{}", namespace.dir_name(), candidate.file_name);
        let untracked = matcher.is_untracked(&rel);
        let artifact = build_artifact(
            &container,
            &candidate.path,
            candidate.size,
            ctx.server.url(&format!("{STRUCT_ROOT}/{rel}")),
            None,
            untracked,
            &ctx.policy,
        )?;
        modules.push(Module {
            id,
            name,
            module_type: TYPE,
            classpath: None,
            required: namespace.required(),
            untracked: untracked.then_some(true),
            artifact,
            sub_modules: None,
        });
    }
    Ok(toggle::finish_modules(modules))
}

fn read_fabric_mod_json(file_name: &str, jar: &Path) -> Result<Option<FabricModJson>> {
    let file = File::open(jar).map_err(io(format!("opening {}", jar.display())))?;
    let mut zip = ZipArchive::new(file)?;
    let raw = match zip.by_name("fabric.mod.json") {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(io(format!(
                "reading fabric.mod.json from {}",
                jar.display()
            )))?;
            Some(bytes)
        }
        Err(_) => None,
    };

    Ok(match raw {
        Some(bytes) => {
            let parsed = parse_fabric_mod_json(&bytes);
            if parsed.is_none() {
                log::error!("FabricMod {file_name} contains an invalid fabric.mod.json file.");
            }
            parsed
        }
        None => {
            log::error!("FabricMod {file_name} does not contain fabric.mod.json file.");
            None
        }
    })
}
