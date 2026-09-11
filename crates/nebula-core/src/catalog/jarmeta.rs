//! One jar, read once: the mod metadata, the icon and the digests the
//! providers identify it by.
//!
//! The parsers are the distribution scanners' own, so the installed list and
//! `distribution.json` agree on every name, id and version. Claritas is never
//! invoked — it needs a JDK and the catalog list must work without one — so a
//! jar whose metadata is missing falls back to `crude_inference`, exactly as
//! the scanners do when Claritas yields nothing.

use std::io::{Read, Seek};
use std::path::Path;

use serde::Deserialize;
use zip::ZipArchive;

use super::CatalogError;
use crate::error::Error;
use crate::hash;
use crate::scan::fabricmod::{FabricIcon, parse_fabric_mod_json};
use crate::scan::forgemod::{
    ForgeFlavour, ModLoader, entry_bytes, forge_meta_17, forge_meta_113, is_optifine,
    optifine_meta, parse_mcmod_info, parse_mods_toml, wants_jar_version,
};
use crate::scan::{capitalize, crude_inference};

/// What one file in a namespace directory turned out to be.
#[derive(Debug, Clone)]
pub enum LocalFile {
    /// A jar whose metadata we could resolve.
    Mod(LocalMod),
    /// A file the distribution scanners abort the whole build on: it is not a
    /// `.jar`, or it is not a readable zip (R9, §8.3 `배포 불가`).
    Unshippable { file_name: String, reason: String },
}

/// A mod as its own jar describes it, plus the digests one pass over the file
/// yields.
///
/// sha512 and the CurseForge fingerprint are **not** here: they cost two
/// further passes over every jar on a list refresh and had no reader. The
/// caller that needs one asks for it per file through
/// [`hash::sha512_hex_of_file`] / [`hash::curseforge_fingerprint_of_file`].
#[derive(Debug, Clone)]
pub struct LocalMod {
    pub name: String,
    pub version: String,
    pub mod_id: String,
    pub description: Option<String>,
    /// Raw bytes of the declared icon entry; decoding is the GUI's job, and
    /// nebula-core has no image dependency on this path.
    pub icon: Option<Vec<u8>>,
    pub sha1: String,
    pub md5: String,
}

/// Reads one file. A jar that cannot be parsed is a [`LocalFile`], not an
/// error: one bad mod must never fail the whole installed list (R9).
pub fn read(path: &Path) -> Result<LocalFile, CatalogError> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    if !file_name.to_lowercase().ends_with(".jar") {
        return Ok(LocalFile::Unshippable {
            file_name,
            reason: "the name does not end in .jar".to_owned(),
        });
    }

    let mut zip = match crate::import::open(path) {
        Ok(zip) => zip,
        // `import::open` reports a broken archive as `Invalid` and a missing
        // or unreadable file as `Io`; only the first is a shippability
        // problem, the second is a real failure.
        Err(Error::Invalid(reason)) => return Ok(LocalFile::Unshippable { file_name, reason }),
        Err(other) => return Err(other.into()),
    };

    let meta = probe(&mut zip, &file_name);
    let icon = meta
        .icon_path
        .as_deref()
        .and_then(|entry| read_icon(&mut zip, entry));
    drop(zip);

    let digests = hash::digests_of_file(path)?;
    Ok(LocalFile::Mod(LocalMod {
        name: meta.name,
        version: meta.version,
        mod_id: meta.mod_id,
        description: meta.description.filter(|text| !text.trim().is_empty()),
        icon,
        sha1: digests.sha1,
        md5: digests.md5,
    }))
}

/// What the jar's own metadata said, before hashing.
struct Probed {
    mod_id: String,
    version: String,
    name: String,
    description: Option<String>,
    icon_path: Option<String>,
}

/// Probes every metadata layout in turn rather than trusting the server's
/// loader: a hand-placed jar may be anything.
fn probe<R: Read + Seek>(zip: &mut ZipArchive<R>, file_name: &str) -> Probed {
    if is_optifine(file_name)
        && let Some(changelog) = entry_bytes(zip, "changelog.txt")
    {
        // OptiFine ships no metadata at all; the flavour only decides the
        // display name, and the catalog always uses the modern form.
        let meta = optifine_meta(ForgeFlavour::Modern113, file_name, &changelog);
        return Probed {
            mod_id: meta.mod_id,
            version: meta.version,
            name: capitalize(&meta.display_name),
            description: None,
            icon_path: None,
        };
    }

    if let Some(bytes) = entry_bytes(zip, "fabric.mod.json")
        && let Some(json) = parse_fabric_mod_json(&bytes)
    {
        return Probed {
            name: capitalize(json.name.as_deref().unwrap_or(&json.id)),
            mod_id: json.id,
            version: json.version,
            description: json.description,
            icon_path: json
                .icon
                .as_ref()
                .and_then(FabricIcon::largest)
                .map(str::to_owned),
        };
    }

    if let Some(bytes) = entry_bytes(zip, "quilt.mod.json")
        && let Ok(json) = serde_json::from_slice::<QuiltModJson>(&bytes)
    {
        let loader = json.quilt_loader;
        let metadata = loader.metadata.unwrap_or_default();
        let (crude_name, crude_version) = crude_inference(file_name);
        let id = loader.id.unwrap_or_else(|| crude_name.to_lowercase());
        return Probed {
            name: capitalize(metadata.name.as_deref().unwrap_or(&id)),
            mod_id: id,
            version: loader.version.unwrap_or(crude_version),
            description: metadata.description,
            icon_path: metadata
                .icon
                .as_ref()
                .and_then(FabricIcon::largest)
                .map(str::to_owned),
        };
    }

    // NeoForge's own file wins over the Forge one, as the scanners do.
    if let Some(bytes) = ModLoader::NeoForge
        .toml_entries()
        .iter()
        .find_map(|entry| entry_bytes(zip, entry))
        && let Some(parsed) = parse_mods_toml(&bytes)
    {
        let entry = parsed.mods.first();
        let description = entry.and_then(|entry| entry.description.clone());
        let icon_path = entry
            .and_then(|entry| entry.logo_file.clone())
            .or_else(|| parsed.logo_file.clone());
        let manifest = wants_jar_version(Some(&parsed))
            .then(|| entry_bytes(zip, "META-INF/MANIFEST.MF"))
            .flatten();
        let meta = forge_meta_113(file_name, Some(parsed), None, manifest.as_deref());
        return Probed {
            mod_id: meta.mod_id,
            version: meta.version,
            name: capitalize(&meta.display_name),
            description,
            icon_path,
        };
    }

    if let Some(bytes) = entry_bytes(zip, "mcmod.info")
        && let Some(parsed) = parse_mcmod_info(&bytes)
    {
        let description = parsed.description.clone();
        let icon_path = parsed.logo_file.clone();
        let meta = forge_meta_17(file_name, Some(parsed), None);
        return Probed {
            mod_id: meta.mod_id,
            version: meta.version,
            name: capitalize(&meta.display_name),
            description,
            icon_path,
        };
    }

    log::debug!("{file_name} carries no mod metadata; inferring it from the name");
    let (crude_name, crude_version) = crude_inference(file_name);
    Probed {
        mod_id: crude_name.to_lowercase(),
        version: crude_version,
        name: capitalize(&crude_name),
        description: None,
        icon_path: None,
    }
}

/// Icon entries are read **out of the archive**, never joined to a filesystem
/// path, so a `logoFile` of `../../etc/passwd` is just a missing zip entry.
fn read_icon<R: Read + Seek>(zip: &mut ZipArchive<R>, entry: &str) -> Option<Vec<u8>> {
    let entry = entry.trim().trim_start_matches('/');
    if entry.is_empty() {
        return None;
    }
    entry_bytes(zip, entry)
}

/// Quilt is read-only here: Nebula cannot run it, but a hand-placed Quilt jar
/// still deserves a name in the list.
#[derive(Debug, Deserialize)]
struct QuiltModJson {
    quilt_loader: QuiltLoader,
}

#[derive(Debug, Deserialize)]
struct QuiltLoader {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    metadata: Option<QuiltMetadata>,
}

#[derive(Debug, Default, Deserialize)]
struct QuiltMetadata {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<FabricIcon>,
}
