//! ForgeGradle 2 adapter: Minecraft 1.7 - 1.12 (Forge <= 14.23.5.2847).
//!
//! The universal jar carries `version.json`; every library in it is fetched
//! from the Forge maven (or Mojang's for entries without a `url`), and the
//! `.pack.xz` ones are unpacked once to hash their real contents.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::maven::MavenId;
use crate::remote;
use crate::spec::{Module, ModuleType};
use crate::version::{self, MinecraftVersion};

use super::forge;
use super::{Resolver, artifact_for_file, join_maven, plain_module};

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

        let mut sub_modules = Vec::with_capacity(manifest.libraries.len());
        let mut post_process = Vec::new();

        for lib in &manifest.libraries {
            if lib.name.starts_with("net.minecraftforge:forge:") {
                // We've already processed forge.
                continue;
            }
            log::debug!("Processing {}..", lib.name);

            let extension = self.determine_extension(lib)?;
            let id = with_extension(&lib.name, &extension)?;
            let local = self.root.lib_path(&id);
            let packed = extension == "jar.pack.xz";

            let mut download = !local.exists();
            if download {
                log::debug!("Not found locally, downloading..");
            } else if !packed {
                // Checksums for .pack.xz in the version.json are useless.
                if let Some([checksum]) = lib.checksums.as_deref()
                    && &hash::sha1_hex_of_file(&local)? != checksum
                {
                    log::debug!("Hashes do not match, redownloading..");
                    download = true;
                }
            }

            if download {
                let base = lib.url.as_deref().unwrap_or(remote::MOJANG_MAVEN);
                remote::download_to(&join_maven(base, &id.path()), &local)?;
            } else {
                log::debug!("Using local copy.");
            }

            sub_modules.push(plain_module(
                id.id(),
                format!("Minecraft Forge ({})", id.artifact),
                ModuleType::Library,
                artifact_for_file(&local, self.root.lib_url(&id))?,
            ));

            if packed {
                post_process.push((id.id(), local));
            }
        }

        for (id, digests) in self.process_pack_xz_files(&post_process)? {
            match sub_modules.iter_mut().find(|module| module.id == id) {
                Some(module) => {
                    // The launcher unpacks the artifact before checking it, so
                    // the digests describe the jar and the size the download.
                    module.artifact.md5 = Some(digests.md5);
                    module.artifact.sha256 = Some(digests.sha256);
                }
                None => log::error!("Error during post processing, could not update {id}"),
            }
        }

        forge_module.sub_modules = Some(sub_modules);
        Ok(forge_module)
    }

    /// Port of `determineExtension`: a library without a `url` is a Mojang one
    /// and always a plain jar; otherwise prefer whatever already exists
    /// locally, then whatever the Forge maven answers a HEAD for.
    fn determine_extension(&self, lib: &Fg2Library) -> Result<String> {
        if lib.url.is_none() {
            return Ok("jar".to_owned());
        }
        log::debug!("Determining extension..");
        const POSSIBLE: [&str; 2] = ["jar.pack.xz", "jar"];

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

    /// `.pack.xz` libraries are unpacked in `temp/` so the module can carry the
    /// digests of the jar the launcher ends up with.
    fn process_pack_xz_files(
        &self,
        queue: &[(String, PathBuf)],
    ) -> Result<Vec<(String, hash::Digests)>> {
        if queue.is_empty() {
            return Ok(Vec::new());
        }

        let temp = self.root.temp_dir();
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).map_err(io(format!("creating {}", temp.display())))?;

        let mut packed = Vec::with_capacity(queue.len());
        for (_, local) in queue {
            let staged = temp.join(file_name(local)?);
            super::copy_file(local, &staged)?;
            packed.push(staged);
        }

        log::debug!("Spawning PackXZExtract.");
        self.java.packxz_extract_unpack(&packed)?;
        log::debug!("All files extracted, calculating hashes..");

        let mut digests = Vec::with_capacity(queue.len());
        for (id, local) in queue {
            let name = file_name(local)?;
            let unpacked = temp.join(name.trim_end_matches(".pack.xz"));
            digests.push((id.clone(), hash::digests_of_file(&unpacked)?));
        }

        log::debug!("Complete, removing temp directory..");
        let _ = fs::remove_dir_all(&temp);
        Ok(digests)
    }
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

    #[test]
    fn keeps_the_pack_xz_extension_in_the_module_id() {
        let id =
            with_extension("net.minecraftforge:forge:1.7.10:something", "jar.pack.xz").unwrap();
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
