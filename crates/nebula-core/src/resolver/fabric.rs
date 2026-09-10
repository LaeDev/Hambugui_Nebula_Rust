//! Fabric resolver: the loader's own profile json lists every library, so
//! there is no installer to run.

use std::fs;

use crate::error::{Error, Result, io};
use crate::layout::Root;
use crate::maven::MavenId;
use crate::remote::{self, FabricProfileJson};
use crate::spec::{Module, ModuleType};
use crate::version::{self, MinecraftVersion};

use super::{Resolver, artifact_for_file, join_maven, plain_module};

const LOADER_PREFIX: &str = "net.fabricmc:fabric-loader";

/// `--fabric.addMods` landed in loader 0.12.3; the launcher needs it.
pub fn is_for_version(loader_version: &str) -> bool {
    version::version_gte(loader_version, "0.12.3")
}

pub struct FabricResolver {
    root: Root,
    mc: MinecraftVersion,
    loader_version: String,
}

impl FabricResolver {
    pub fn new(root: &Root, mc: &MinecraftVersion, loader_version: &str) -> Self {
        Self {
            root: root.clone(),
            mc: *mc,
            loader_version: loader_version.to_owned(),
        }
    }

    fn version_name(&self) -> String {
        format!("{}-fabric-{}", self.mc, self.loader_version)
    }

    fn resolve(&self) -> Result<Module> {
        let name = self.version_name();
        let manifest_path = self.root.version_manifest_path(&name);

        log::debug!(
            "Checking for fabric profile json at {}..",
            manifest_path.display()
        );
        if !manifest_path.exists() {
            log::debug!("Fabric profile not found locally, initializing download..");
            // The raw document is stored, not a round trip through our model:
            // the launcher reads fields we do not describe.
            let profile: serde_json::Value = remote::get_json(&format!(
                "{}loader/{}/{}/profile/json",
                remote::FABRIC_META,
                self.mc,
                self.loader_version
            ))?;
            if let Some(parent) = manifest_path.parent() {
                fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
            }
            // Compact, exactly like upstream's `JSON.stringify(manifest)`.
            fs::write(&manifest_path, serde_json::to_vec(&profile)?)
                .map_err(io(format!("writing {}", manifest_path.display())))?;
        }

        let raw =
            fs::read(&manifest_path).map_err(io(format!("reading {}", manifest_path.display())))?;
        let profile: FabricProfileJson = serde_json::from_slice(&raw)?;

        let mut modules = vec![plain_module(
            name.clone(),
            "Fabric (version.json)".to_owned(),
            ModuleType::VersionManifest,
            artifact_for_file(&manifest_path, self.root.version_manifest_url(&name))?,
        )];

        for lib in &profile.libraries {
            log::debug!("Processing {}..", lib.name);
            let id = MavenId::parse(&lib.name, "jar")?;
            let local = self.root.lib_path(&id);

            if local.exists() {
                log::debug!("Using local copy.");
            } else {
                log::debug!("Not found locally, downloading..");
                remote::download_to(&join_maven(&lib.url, &id.path()), &local)?;
            }

            modules.push(plain_module(
                lib.name.clone(),
                format!("Fabric ({})", id.artifact),
                ModuleType::Library,
                artifact_for_file(&local, self.root.lib_url(&id))?,
            ));
        }

        assemble(modules)
    }
}

impl Resolver for FabricResolver {
    fn module(&mut self) -> Result<Module> {
        self.resolve()
    }
}

/// The fabric-loader entry becomes the root module. Upstream's
/// `modules.splice(index)` keeps only the modules *before* the loader as
/// sub-modules; the loader is the last profile library today, so nothing is
/// lost, but we warn if that ever changes.
fn assemble(mut modules: Vec<Module>) -> Result<Module> {
    let index = modules
        .iter()
        .position(|module| module.id.starts_with(LOADER_PREFIX))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "No {LOADER_PREFIX} entry found in the Fabric profile json!"
            ))
        })?;

    let trailing = modules.len() - index - 1;
    if trailing > 0 {
        log::warn!(
            "{trailing} Fabric module(s) follow the loader entry and are dropped, reproducing \
             upstream's truncation."
        );
    }

    let mut loader = modules.remove(index);
    modules.truncate(index);
    loader.module_type = ModuleType::Fabric;
    loader.sub_modules = Some(modules);
    Ok(loader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Artifact;

    fn module(id: &str) -> Module {
        plain_module(
            id.to_owned(),
            id.to_owned(),
            ModuleType::Library,
            Artifact {
                size: 1,
                url: "https://host/a.jar".to_owned(),
                md5: Some("m".to_owned()),
                sha256: Some("s".to_owned()),
                path: None,
            },
        )
    }

    fn ids(modules: &[Module]) -> Vec<&str> {
        modules.iter().map(|module| module.id.as_str()).collect()
    }

    #[test]
    fn requires_loader_0_12_3() {
        assert!(!is_for_version("0.12.2"));
        assert!(is_for_version("0.12.3"));
        assert!(is_for_version("0.16.10"));
        assert!(!is_for_version("0.11.7"));
    }

    #[test]
    fn promotes_the_loader_to_the_root_module() {
        let modules = vec![
            module("1.20.1-fabric-0.14.21"),
            module("org.ow2.asm:asm:9.5"),
            module("net.fabricmc:fabric-loader:0.14.21"),
        ];
        let root = assemble(modules).unwrap();

        assert_eq!(root.id, "net.fabricmc:fabric-loader:0.14.21");
        assert_eq!(root.module_type, ModuleType::Fabric);
        assert_eq!(
            ids(root.sub_modules.as_ref().unwrap()),
            ["1.20.1-fabric-0.14.21", "org.ow2.asm:asm:9.5"]
        );
    }

    #[test]
    fn truncates_everything_after_the_loader() {
        let modules = vec![
            module("1.20.1-fabric-0.14.21"),
            module("net.fabricmc:fabric-loader:0.14.21"),
            module("net.fabricmc:sponge-mixin:0.12.5"),
        ];
        let root = assemble(modules).unwrap();

        assert_eq!(root.id, "net.fabricmc:fabric-loader:0.14.21");
        assert_eq!(
            ids(root.sub_modules.as_ref().unwrap()),
            ["1.20.1-fabric-0.14.21"]
        );
    }

    #[test]
    fn reports_a_profile_without_a_loader() {
        let error = assemble(vec![module("org.ow2.asm:asm:9.5")]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "No net.fabricmc:fabric-loader entry found in the Fabric profile json!"
        );
    }
}
