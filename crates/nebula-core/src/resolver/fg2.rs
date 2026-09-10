//! ForgeGradle 2 adapter: Minecraft 1.7 - 1.12 (Forge <= 14.23.5.2847).
//!
//! The universal jar carries `version.json`; every library in it is fetched
//! from the Forge maven (or Mojang's for entries without a `url`).
//!
//! **Divergence from upstream:** `ForgeGradle2.resolver.ts` shipped the packed
//! file itself, with the id `group:artifact:version[:classifier]@jar.pack.xz`
//! and the *unpacked* jar's digests, because Helios unpacks a `.pack.xz`
//! download before checking its MD5. The Hambugui runtime never unpacks
//! anything: it verifies the SHA-256 and the length of the bytes it downloaded,
//! so a module describing the jar while serving the packed file cannot be
//! satisfied, and one describing the packed bytes would leave the launcher with
//! a `.pack.xz` on the classpath. Nebula therefore ships **plain jars only**:
//! `determine_extension` still probes for `.pack.xz` remotely, because that is
//! what Forge's maven serves for these old libraries, but the packed file is an
//! intermediate in `temp/` and the module always carries the `@jar` id, the
//! jar's size, digests and url. `validate` rejects any `@jar.pack.xz` id.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result, io};
use crate::java::Java;
use crate::layout::Root;
use crate::maven::MavenId;
use crate::remote;
use crate::spec::{Module, ModuleType};
use crate::version::{self, MinecraftVersion};

use super::forge;
use super::{
    Resolver, artifact_for_file, artifact_from_digests, join_maven, plain_module, verified_digests,
};

/// The extension Forge's maven serves for a packed library.
const PACKED: &str = "jar.pack.xz";

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifestFg2 {
    #[serde(default)]
    pub libraries: Vec<Fg2Library>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fg2Library {
    pub name: String,
    /// Absent for Mojang libraries, which also forces the `jar` extension.
    pub url: Option<String>,
    pub checksums: Option<Vec<String>>,
}

/// Port of `ForgeGradle2Adapter.isForVersion`.
pub fn is_for_version(mc: &MinecraftVersion, forge_version: &str) -> bool {
    if mc.minor == 12 && !version::is_one_dot_twelve_fg2(forge_version) {
        return false;
    }
    version::is_version_acceptable(mc, &[7, 8, 9, 10, 11, 12])
}

pub struct ForgeGradle2Adapter {
    root: Root,
    mc: MinecraftVersion,
    forge_version: String,
    artifact_version: String,
    java: Java,
}

impl ForgeGradle2Adapter {
    pub fn new(root: &Root, mc: &MinecraftVersion, forge_version: &str, java: &Java) -> Self {
        Self {
            root: root.clone(),
            mc: *mc,
            forge_version: forge_version.to_owned(),
            artifact_version: forge::infer_artifact_version(mc, forge_version),
            java: java.clone(),
        }
    }

    fn resolve(&self) -> Result<Module> {
        let universal_id = forge::forge_maven_id(&self.artifact_version, Some("universal"));
        let universal = self.root.lib_path(&universal_id);
        log::debug!("Checking for forge version at {}..", universal.display());
        if !universal.exists() {
            log::debug!("Forge not found locally, initializing download..");
            remote::download_to(
                &join_maven(remote::FORGE_MAVEN, &universal_id.path()),
                &universal,
            )?;
        } else {
            log::debug!("Using locally discovered forge.");
        }
        log::debug!(
            "Beginning processing of Forge v{} (Minecraft {})",
            self.forge_version,
            self.mc
        );

        let raw = forge::version_json_from_jar(&universal).map_err(|error| {
            Error::Invalid(format!(
                "Failed to find version.json in forge universal jar: {error}"
            ))
        })?;
        let manifest: VersionManifestFg2 = serde_json::from_slice(&raw)?;

        let mut forge_module = plain_module(
            universal_id.id_without_extension(),
            "Minecraft Forge".to_owned(),
            ModuleType::ForgeHosted,
            artifact_for_file(&universal, self.root.lib_url(&universal_id))?,
        );

        // Pass one, in manifest order: plain jars are finished here, packed ones
        // are staged in `temp/` so PackXZExtract runs once for all of them.
        let temp = self.root.temp_dir();
        let mut prepared = Vec::with_capacity(manifest.libraries.len());
        let mut staged = Vec::new();
        let mut intermediates = Vec::new();

        for lib in &manifest.libraries {
            if lib.name.starts_with("net.minecraftforge:forge:") {
                // We've already processed forge.
                continue;
            }
            log::debug!("Processing {}..", lib.name);

            let id = with_extension(&lib.name, "jar")?;
            let jar = self.root.lib_path(&id);

            if self.determine_extension(lib)? != PACKED {
                let digests = verified_digests(&jar, single_checksum(lib), || {
                    let base = lib.url.as_deref().unwrap_or(remote::MOJANG_MAVEN);
                    remote::download_to(&join_maven(base, &id.path()), &jar)
                })?;
                prepared.push(Prepared::Ready(plain_module(
                    id.id(),
                    format!("Minecraft Forge ({})", id.artifact),
                    ModuleType::Library,
                    artifact_from_digests(&digests, self.root.lib_url(&id)),
                )));
                continue;
            }

            if staged.is_empty() {
                reset_dir(&temp)?;
            }
            let packed_id = with_extension(&lib.name, PACKED)?;
            let packed = self.root.lib_path(&packed_id);
            let target = temp.join(file_name(&packed)?);
            if packed.exists() {
                // Shipped by an older Nebula; it is only an intermediate now.
                log::debug!("Using local copy.");
                super::copy_file(&packed, &target)?;
                intermediates.push(packed);
            } else {
                log::debug!("Not found locally, downloading..");
                let base = lib.url.as_deref().unwrap_or(remote::MOJANG_MAVEN);
                remote::download_to(&join_maven(base, &packed_id.path()), &target)?;
            }
            staged.push(target);
            // PackXZExtract drops the `.pack.xz` suffix in place.
            prepared.push(Prepared::Packed {
                unpacked: temp.join(file_name(&jar)?),
                jar,
                id,
            });
        }

        if !staged.is_empty() {
            log::debug!("Spawning PackXZExtract.");
            self.java.packxz_extract_unpack(&staged)?;
            log::debug!("All files extracted, storing the unpacked jars..");
        }

        // Pass two, same order: the unpacked jars land in `repo/lib` and every
        // module is built from the file the launcher will download.
        let mut sub_modules = Vec::with_capacity(prepared.len());
        for entry in prepared {
            sub_modules.push(match entry {
                Prepared::Ready(module) => module,
                Prepared::Packed { id, jar, unpacked } => {
                    super::copy_file(&unpacked, &jar)?;
                    plain_module(
                        id.id(),
                        format!("Minecraft Forge ({})", id.artifact),
                        ModuleType::Library,
                        artifact_for_file(&jar, self.root.lib_url(&id))?,
                    )
                }
            });
        }

        if !staged.is_empty() {
            log::debug!("Complete, removing temp directory..");
            let _ = fs::remove_dir_all(&temp);
        }
        for packed in intermediates {
            let _ = fs::remove_file(packed);
        }

        forge_module.sub_modules = Some(sub_modules);
        Ok(forge_module)
    }

    /// Port of `determineExtension`: a library without a `url` is a Mojang one
    /// and always a plain jar; otherwise prefer whatever already exists
    /// locally, then whatever the Forge maven answers a HEAD for.
    ///
    /// A second run finds the jar this resolver unpacked and never asks the
    /// network again: the packed intermediate is not kept.
    fn determine_extension(&self, lib: &Fg2Library) -> Result<String> {
        if lib.url.is_none() {
            return Ok("jar".to_owned());
        }
        log::debug!("Determining extension..");
        const POSSIBLE: [&str; 2] = [PACKED, "jar"];

        for extension in POSSIBLE {
            let id = with_extension(&lib.name, extension)?;
            if self.root.lib_path(&id).exists() {
                return Ok(extension.to_owned());
            }
        }
        for extension in POSSIBLE {
            let id = with_extension(&lib.name, extension)?;
            if remote::head_exists(&join_maven(remote::FORGE_MAVEN, &id.path())) {
                return Ok(extension.to_owned());
            }
        }
        Ok("jar".to_owned())
    }
}

/// One manifest library, waiting for its module.
enum Prepared {
    Ready(Module),
    Packed {
        id: MavenId,
        /// Where the jar belongs once it is unpacked.
        jar: PathBuf,
        /// What PackXZExtract writes in `temp/`.
        unpacked: PathBuf,
    },
}

/// Upstream only ever compared a single checksum, and the entries Forge serves
/// packed carry two (the jar's and the packed file's), so they are skipped.
fn single_checksum(lib: &Fg2Library) -> Option<&str> {
    match lib.checksums.as_deref() {
        Some([checksum]) => Some(checksum.as_str()),
        _ => None,
    }
}

fn reset_dir(dir: &Path) -> Result<()> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(io(format!("creating {}", dir.display())))
}

impl Resolver for ForgeGradle2Adapter {
    fn module(&mut self) -> Result<Module> {
        self.resolve()
    }
}

fn with_extension(name: &str, extension: &str) -> Result<MavenId> {
    let mut id = MavenId::parse(name, extension)?;
    id.extension = extension.to_owned();
    Ok(id)
}

fn file_name(path: &std::path::Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| Error::Invalid(format!("{} has no file name", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    #[test]
    fn claims_the_forge_gradle_2_versions() {
        for minor in 7..=11 {
            assert!(is_for_version(&mc(&format!("1.{minor}.0")), "10.13.4.1614"));
        }
        assert!(is_for_version(&mc("1.12.2"), "14.23.5.2847"));
        assert!(!is_for_version(&mc("1.12.2"), "14.23.5.2848"));
        assert!(!is_for_version(&mc("1.13.2"), "25.0.219"));
        assert!(!is_for_version(&mc("1.6.4"), "9.11.1.1345"));
    }

    /// The packed id only ever names the maven path of the intermediate
    /// download; every module this resolver emits carries the `@jar` one.
    #[test]
    fn builds_ids_for_both_extensions() {
        let id = with_extension("net.minecraftforge:forge:1.7.10:something", PACKED).unwrap();
        assert_eq!(
            id.path(),
            "net/minecraftforge/forge/1.7.10/forge-1.7.10-something.jar.pack.xz"
        );
        assert_eq!(
            id.id(),
            "net.minecraftforge:forge:1.7.10:something@jar.pack.xz"
        );
        assert_eq!(
            with_extension("org.ow2.asm:asm-all:5.0.3", "jar")
                .unwrap()
                .id(),
            "org.ow2.asm:asm-all:5.0.3@jar"
        );
    }

    /// Forge serves the packed libraries with two checksums (the jar's and the
    /// packed file's), and neither describes bytes upstream compared.
    #[test]
    fn only_compares_a_single_declared_checksum() {
        let lib = |checksums: Option<Vec<String>>| Fg2Library {
            name: "org.ow2.asm:asm-all:5.0.3".to_owned(),
            url: None,
            checksums,
        };
        assert_eq!(single_checksum(&lib(None)), None);
        assert_eq!(single_checksum(&lib(Some(vec!["a".into()]))), Some("a"));
        assert_eq!(
            single_checksum(&lib(Some(vec!["a".into(), "b".into()]))),
            None
        );
    }

    #[test]
    fn parses_a_forge_gradle_2_manifest() {
        let manifest: VersionManifestFg2 = serde_json::from_str(
            r#"{"id":"1.12.2-forge","libraries":[
                {"name":"net.minecraftforge:forge:1.12.2-14.23.5.2847","url":"http://x/"},
                {"name":"net.minecraft:launchwrapper:1.12","checksums":["111"]},
                {"name":"org.ow2.asm:asm-debug-all:5.2","url":"http://files.minecraftforge.net/maven/","checksums":["222"]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(manifest.libraries.len(), 3);
        assert_eq!(manifest.libraries[1].url, None);
        assert_eq!(
            manifest.libraries[2].checksums.as_deref(),
            Some(["222".to_owned()].as_slice())
        );
    }
}
