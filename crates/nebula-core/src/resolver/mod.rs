//! Loader resolvers: they turn a Forge, NeoForge or Fabric version into the
//! `Module` tree the launcher downloads, and populate `repo/lib` +
//! `repo/versions` on the way.
//!
//! Ported from `src/resolver/**` of upstream Nebula plus the Hambugui
//! hardening patch (`SHA256` next to the legacy `MD5`).

pub mod fabric;
pub mod fg2;
pub mod fg3;
pub mod forge;
pub mod neoforge;

use std::path::Path;

use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::spec::{Artifact, Module, ModuleType};
use crate::version::{self, MinecraftVersion};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolveOptions {
    /// Delete the installer output directory once the modules are built.
    pub discard_output: bool,
    /// Ignore and remove an existing installer output directory.
    pub invalidate_cache: bool,
}

pub trait Resolver {
    fn module(&mut self) -> Result<Module>;
}

/// Port of `BaseResolver.generateArtifact` with the hardening patch: `size`
/// from the file, `MD5` and `SHA256` from its bytes. Resolver artifacts never
/// carry a `path`; their module ids are maven coordinates and the launcher
/// derives the on-disk location from them.
pub fn artifact_for_file(path: &Path, url: String) -> Result<Artifact> {
    let digests = hash::digests_of_file(path)?;
    Ok(Artifact {
        size: digests.size,
        url,
        md5: Some(digests.md5),
        sha256: Some(digests.sha256),
        path: None,
    })
}

/// FG2 for 1.7-1.12 (Forge <= 14.23.5.2847), FG3 for 1.12 (> 2847) to 1.21.
pub fn forge_resolver(
    root: &Root,
    mc: &MinecraftVersion,
    forge_version: &str,
    java: &Java,
    opts: ResolveOptions,
) -> Result<Box<dyn Resolver>> {
    if let Some(warning) = forge::log4j_warning(mc, forge_version) {
        for line in warning.lines() {
            log::error!(target: "ForgeSecurity", "{line}");
        }
    }
    if fg2::is_for_version(mc, forge_version) {
        Ok(Box::new(fg2::ForgeGradle2Adapter::new(
            root,
            mc,
            forge_version,
            java,
        )))
    } else if fg3::is_for_version(mc, forge_version) {
        Ok(Box::new(fg3::ForgeGradle3Adapter::new(
            root,
            mc,
            forge_version,
            java,
            opts,
        )))
    } else {
        Err(Error::Invalid(format!(
            "No forge resolver found for Minecraft {mc}!"
        )))
    }
}

/// `--fabric.addMods` support landed in loader 0.12.3; older loaders cannot be
/// driven by the launcher.
pub fn fabric_resolver(
    root: &Root,
    mc: &MinecraftVersion,
    loader_version: &str,
) -> Result<Box<dyn Resolver>> {
    if !fabric::is_for_version(loader_version) {
        return Err(Error::Invalid(format!(
            "Fabric resolver does not support Fabric {loader_version}!"
        )));
    }
    Ok(Box::new(fabric::FabricResolver::new(
        root,
        mc,
        loader_version,
    )))
}

/// `net.neoforged:neoforge` exists for Minecraft 1.20.2 and up only.
pub fn neoforge_resolver(
    root: &Root,
    mc: &MinecraftVersion,
    version: &str,
    java: &Java,
    opts: ResolveOptions,
) -> Result<Box<dyn Resolver>> {
    if !version::is_neoforge_supported(mc) {
        return Err(Error::Invalid(format!(
            "NeoForge resolver does not support Minecraft {mc}!"
        )));
    }
    Ok(Box::new(neoforge::NeoForgeResolver::new(
        root, mc, version, java, opts,
    )))
}

pub(crate) fn plain_module(
    id: String,
    name: String,
    module_type: ModuleType,
    artifact: Artifact,
) -> Module {
    Module {
        id,
        name,
        module_type,
        classpath: None,
        required: None,
        untracked: None,
        artifact,
        sub_modules: None,
    }
}

/// Upstream resolved maven paths with `new URL(relative, base)`, which drops
/// the last segment of a base that does not end in `/`. We always append.
pub(crate) fn join_maven(base: &str, path: &str) -> String {
    match base.ends_with('/') {
        true => format!("{base}{path}"),
        false => format!("{base}/{path}"),
    }
}

pub(crate) fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    std::fs::copy(source, destination).map_err(io(format!(
        "copying {} to {}",
        source.display(),
        destination.display()
    )))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn test_root(dir: &Path) -> Root {
        Root::new(dir, Url::parse("https://cdn.example.invalid/").unwrap())
    }

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    #[test]
    fn hashes_an_artifact_without_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jar");
        std::fs::write(&file, b"abc").unwrap();

        let artifact = artifact_for_file(&file, "https://host/a.jar".into()).unwrap();
        assert_eq!(artifact.size, 3);
        assert_eq!(
            artifact.md5.as_deref(),
            Some("900150983cd24fb0d6963f7d28e17f72")
        );
        assert_eq!(
            artifact.sha256.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(artifact.path, None);
    }

    #[test]
    fn rejects_versions_no_forge_adapter_covers() {
        let dir = tempfile::tempdir().unwrap();
        let root = test_root(dir.path());
        let java = Java::new("java", &root);

        let error = forge_resolver(
            &root,
            &mc("1.22.0"),
            "60.0.0",
            &java,
            ResolveOptions::default(),
        )
        .err()
        .unwrap();
        assert_eq!(
            error.to_string(),
            "No forge resolver found for Minecraft 1.22.0!"
        );
    }

    #[test]
    fn rejects_unsupported_loaders() {
        let dir = tempfile::tempdir().unwrap();
        let root = test_root(dir.path());
        let java = Java::new("java", &root);

        assert_eq!(
            fabric_resolver(&root, &mc("1.20.1"), "0.12.2")
                .err()
                .unwrap()
                .to_string(),
            "Fabric resolver does not support Fabric 0.12.2!"
        );
        assert!(fabric_resolver(&root, &mc("1.20.1"), "0.12.3").is_ok());

        assert_eq!(
            neoforge_resolver(
                &root,
                &mc("1.20.1"),
                "47.1.0",
                &java,
                ResolveOptions::default()
            )
            .err()
            .unwrap()
            .to_string(),
            "NeoForge resolver does not support Minecraft 1.20.1!"
        );
        assert!(
            neoforge_resolver(
                &root,
                &mc("1.21.1"),
                "21.1.77",
                &java,
                ResolveOptions::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn appends_to_maven_bases() {
        assert_eq!(
            join_maven("https://host/maven/", "a/b.jar"),
            "https://host/maven/a/b.jar"
        );
        assert_eq!(
            join_maven("https://host/maven", "a/b.jar"),
            "https://host/maven/a/b.jar"
        );
    }
}
