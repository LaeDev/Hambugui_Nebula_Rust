//! NeoForge resolver (Minecraft 1.20.2+). No upstream equivalent.
//!
//! NeoForge changes its processors between minor versions, so there is no
//! generated-file matrix like ForgeGradle 3's. Two sources describe what a
//! client needs, and both come out of the installer run:
//!
//! * the version manifest's `libraries` are the runtime classpath (all of them
//!   carry a download url today; an entry without one was generated locally and
//!   is copied out of the installer output);
//! * the four artifacts FML looks up under `-DlibraryDirectory` instead of
//!   listing them in the manifest. `ProductionClientProvider` requires
//!   `net.minecraft:client:<mc>-<neoform>` with the `srg` and `extra`
//!   classifiers, and `NeoForgeClientLaunchHandler` adds
//!   `net.neoforged:neoforge:<version>` with `client` and `universal`. Their
//!   versions are read back out of the manifest's own `--fml.*` arguments, so
//!   only the four classifiers are fixed here.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::maven::MavenId;
use crate::paths;
use crate::remote;
use crate::spec::{Module, ModuleType};
use crate::version::MinecraftVersion;

use super::fg3::{self, VersionManifestFg3};
use super::forge::{MINECRAFT_CLIENT_ARTIFACT, MINECRAFT_GROUP};
use super::{ResolveOptions, Resolver, artifact_for_file, copy_file, forge, plain_module};

pub const NEOFORGE_GROUP: &str = "net.neoforged";
pub const NEOFORGE_ARTIFACT: &str = "neoforge";

const ROOT_PREFIX: &str = "net.neoforged:neoforge:";
const ROOT_SUFFIX: &str = ":universal";

pub struct NeoForgeResolver {
    root: Root,
    mc: MinecraftVersion,
    version: String,
    java: Java,
    opts: ResolveOptions,
}

impl NeoForgeResolver {
    pub fn new(
        root: &Root,
        mc: &MinecraftVersion,
        version: &str,
        java: &Java,
        opts: ResolveOptions,
    ) -> Self {
        Self {
            root: root.clone(),
            mc: *mc,
            version: version.to_owned(),
            java: java.clone(),
            opts,
        }
    }

    fn version_name(&self) -> String {
        format!("{}-neoforge-{}", self.mc, self.version)
    }

    fn installer_id(&self) -> MavenId {
        MavenId {
            group: NEOFORGE_GROUP.to_owned(),
            artifact: NEOFORGE_ARTIFACT.to_owned(),
            version: self.version.clone(),
            classifier: Some("installer".to_owned()),
            extension: "jar".to_owned(),
        }
    }

    fn resolve(&self) -> Result<Module> {
        let installer_id = self.installer_id();
        let installer = self.root.lib_path(&installer_id);
        log::debug!(
            "Checking for neoforge installer at {}..",
            installer.display()
        );
        if !installer.exists() {
            log::debug!("NeoForge installer not found locally, initializing download..");
            remote::download_to(&remote::neoforge_installer_url(&self.version), &installer)?;
        } else {
            log::debug!("Using locally discovered neoforge installer.");
        }

        let cache = self.root.neoforge_cache_dir(&self.version);
        let mut install = true;
        if cache.exists() {
            if self.opts.invalidate_cache {
                log::info!("Removing existing cache {}..", cache.display());
                fs::remove_dir_all(&cache).map_err(io(format!("removing {}", cache.display())))?;
            } else {
                install = false;
                log::info!("Using cached results at {}.", cache.display());
            }
        }
        if install {
            forge::run_installer_headless_then_gui(&self.java, &installer, &cache, || {
                discover_version_manifest(&cache).is_ok()
            })?;
        }

        let installed_manifest = match discover_version_manifest(&cache) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_dir_all(&cache);
                return Err(error);
            }
        };

        log::debug!("Processing Version Manifest");
        let raw = fs::read(&installed_manifest)
            .map_err(io(format!("reading {}", installed_manifest.display())))?;
        let manifest: VersionManifestFg3 = serde_json::from_slice(&raw)?;

        let name = self.version_name();
        let destination = self.root.version_manifest_path(&name);
        copy_file(&installed_manifest, &destination)?;
        let manifest_module = plain_module(
            name.clone(),
            "NeoForge (version.json)".to_owned(),
            ModuleType::VersionManifest,
            artifact_for_file(&destination, self.root.version_manifest_url(&name))?,
        );

        log::debug!("Processing generated neoforge files.");
        let generated = self.process_generated_files(&manifest, &cache)?;

        log::debug!("Processing Libraries");
        let libraries = self.process_libraries(&manifest, &cache)?;

        let module = assemble(generated, manifest_module, libraries)?;

        if self.opts.discard_output {
            log::info!("Removing installer output at {}..", cache.display());
            fs::remove_dir_all(&cache).map_err(io(format!("removing {}", cache.display())))?;
            log::info!("Removed successfully.");
        }

        Ok(module)
    }

    /// The jars the installer built for this client. They are absent from the
    /// manifest because FML finds them by maven coordinate under
    /// `-DlibraryDirectory`, so they are kept off the classpath exactly like
    /// ForgeGradle 3 keeps Forge's own universal, client, srg and extra jars.
    fn process_generated_files(
        &self,
        manifest: &VersionManifestFg3,
        cache: &Path,
    ) -> Result<Vec<Module>> {
        let lib_dir = cache.join("libraries");
        let (neoforge_version, mc_and_neoform) = fml_versions(manifest)?;

        let generated = [
            (
                "universal jar",
                NEOFORGE_GROUP,
                NEOFORGE_ARTIFACT,
                &neoforge_version,
                "universal",
            ),
            (
                "client jar",
                NEOFORGE_GROUP,
                NEOFORGE_ARTIFACT,
                &neoforge_version,
                "client",
            ),
            (
                "client srg",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                &mc_and_neoform,
                "srg",
            ),
            (
                "client extra",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                &mc_and_neoform,
                "extra",
            ),
        ];

        let mut modules = Vec::with_capacity(generated.len());
        for (name, group, artifact, version, classifier) in generated {
            let id = MavenId {
                group: group.to_owned(),
                artifact: artifact.to_owned(),
                version: version.clone(),
                classifier: Some(classifier.to_owned()),
                extension: "jar".to_owned(),
            };
            let source = lib_dir.join(id.path());
            if !source.exists() {
                return Err(Error::Invalid(format!(
                    "Required file {name} not found at the expected location:\n\t{}",
                    source.display()
                )));
            }

            let destination = self.root.lib_path(&id);
            copy_file(&source, &destination)?;

            let mut module = plain_module(
                id.id_without_extension(),
                format!("NeoForge ({name})"),
                ModuleType::Library,
                artifact_for_file(&destination, self.root.lib_url(&id))?,
            );
            module.classpath = Some(false);
            modules.push(module);
        }

        Ok(modules)
    }

    /// A library with a url is fetched from that url (and verified against the
    /// manifest's SHA-1); one without was generated by the installer and is
    /// copied out of its output directory.
    fn process_libraries(
        &self,
        manifest: &VersionManifestFg3,
        cache: &Path,
    ) -> Result<Vec<Module>> {
        let lib_dir = cache.join("libraries");
        let mut modules = Vec::with_capacity(manifest.libraries.len());

        for entry in &manifest.libraries {
            let Some(artifact) = &entry.downloads.artifact else {
                log::warn!("{} has no download artifact, skipping.", entry.name);
                continue;
            };
            log::debug!("Processing {}..", entry.name);

            let relative = paths::clean_relative(&artifact.path)?;
            let destination = self.root.repo_lib_dir().join(&relative);

            if artifact.url.is_empty() {
                // Generated by the installer.
                let source = lib_dir.join(&relative);
                if !source.exists() {
                    return Err(Error::Invalid(format!(
                        "The NeoForge installer did not produce {} ({})",
                        entry.name,
                        source.display()
                    )));
                }
                copy_file(&source, &destination)?;
            } else {
                let mut download = !destination.exists();
                if download {
                    log::debug!("Not found locally, downloading..");
                } else if !artifact.sha1.is_empty()
                    && hash::sha1_hex_of_file(&destination)? != artifact.sha1
                {
                    log::debug!("Hashes do not match, redownloading..");
                    download = true;
                }
                if download {
                    remote::download_to(&artifact.url, &destination)?;
                } else {
                    log::debug!("Using local copy.");
                }
            }

            let artifact_name = MavenId::parse(&entry.name, "jar")
                .map(|id| id.artifact)
                .unwrap_or_else(|_| entry.name.clone());

            modules.push(plain_module(
                entry.name.clone(),
                format!("NeoForge ({artifact_name})"),
                ModuleType::Library,
                // The url follows the manifest's own path, not the maven id.
                artifact_for_file(&destination, self.root.url(&format!("repo/lib/{relative}")))?,
            ));
        }

        Ok(modules)
    }
}

impl Resolver for NeoForgeResolver {
    fn module(&mut self) -> Result<Module> {
        self.resolve()
    }
}

/// The installer names its output directory after the version it installed, and
/// that name changes between NeoForge releases, so the manifest is discovered
/// rather than predicted: `<cache>/versions/<name>/<name>.json`, exactly one.
///
/// The installer also drops the vanilla profile it patched
/// (`versions/<mc>/<mc>.json` plus the client jar), so the candidates are
/// narrowed by content: only a profile that inherits from another version is a
/// loader profile.
pub fn discover_version_manifest(cache: &Path) -> Result<PathBuf> {
    let versions = cache.join("versions");
    let entries = fs::read_dir(&versions).map_err(io(format!("reading {}", versions.display())))?;

    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io(format!("reading {}", versions.display())))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let manifest = entry.path().join(format!("{name}.json"));
        if manifest.is_file() && inherits_from_another_version(&manifest) {
            found.push(manifest);
        }
    }
    found.sort();

    match found.len() {
        1 => Ok(found.remove(0)),
        count => Err(Error::Invalid(format!(
            "Expected exactly one <name>/<name>.json loader version manifest in {}, found {count}",
            versions.display()
        ))),
    }
}

fn inherits_from_another_version(manifest: &Path) -> bool {
    let Ok(raw) = fs::read(manifest) else {
        return false;
    };
    let Ok(profile) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return false;
    };
    profile
        .get("inheritsFrom")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|inherits| !inherits.is_empty())
}

/// The NeoForge version and the joined `<mc>-<neoform>` version the installer
/// used, read back from the launch arguments it wrote.
fn fml_versions(manifest: &VersionManifestFg3) -> Result<(String, String)> {
    let arguments = manifest.game_arguments();
    let value = |flag: &str| {
        fg3::argument_value(arguments, flag).ok_or_else(|| {
            Error::Invalid(format!(
                "{flag} not found in the NeoForge version.json.. did NeoForge change their format?"
            ))
        })
    };
    let neoforge = value("--fml.neoForgeVersion")?;
    let minecraft = value("--fml.mcVersion")?;
    let neoform = value("--fml.neoFormVersion")?;
    Ok((neoforge, format!("{minecraft}-{neoform}")))
}

/// `net.neoforged:neoforge:<v>:universal` is the loader root; the version
/// manifest leads its sub-modules, then the remaining generated jars, then the
/// manifest libraries in manifest order.
fn assemble(
    mut generated: Vec<Module>,
    version_manifest: Module,
    libraries: Vec<Module>,
) -> Result<Module> {
    let index = generated
        .iter()
        .position(|module| module.id.starts_with(ROOT_PREFIX) && module.id.ends_with(ROOT_SUFFIX))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "The NeoForge installer produced no {ROOT_PREFIX}<version>{ROOT_SUFFIX} jar!"
            ))
        })?;

    let mut root = generated.remove(index);
    root.module_type = ModuleType::NeoForge;
    root.name = "NeoForge".to_owned();

    let mut sub_modules = Vec::with_capacity(1 + generated.len() + libraries.len());
    sub_modules.push(version_manifest);
    sub_modules.extend(generated);
    sub_modules.extend(libraries);
    root.sub_modules = Some(sub_modules);
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Artifact;

    fn module(id: &str, module_type: ModuleType) -> Module {
        plain_module(
            id.to_owned(),
            format!("NeoForge ({id})"),
            module_type,
            Artifact {
                size: 1,
                url: "https://host/a.jar".to_owned(),
                md5: Some("m".to_owned()),
                sha256: Some("s".to_owned()),
                path: None,
            },
        )
    }

    fn manifest_module() -> Module {
        module("1.21.1-neoforge-21.1.77", ModuleType::VersionManifest)
    }

    fn generated() -> Vec<Module> {
        ["universal", "client"]
            .into_iter()
            .map(|classifier| {
                module(
                    &format!("net.neoforged:neoforge:21.1.77:{classifier}"),
                    ModuleType::Library,
                )
            })
            .chain(["srg", "extra"].into_iter().map(|classifier| {
                module(
                    &format!("net.minecraft:client:1.21.1-20240808.144430:{classifier}"),
                    ModuleType::Library,
                )
            }))
            .collect()
    }

    #[test]
    fn promotes_the_universal_entry_to_the_root_module() {
        let libraries = vec![
            module(
                "net.neoforged.fancymodloader:loader:4.0.31@jar",
                ModuleType::Library,
            ),
            module("org.ow2.asm:asm:9.7@jar", ModuleType::Library),
        ];
        let root = assemble(generated(), manifest_module(), libraries).unwrap();

        assert_eq!(root.id, "net.neoforged:neoforge:21.1.77:universal");
        assert_eq!(root.module_type, ModuleType::NeoForge);
        assert_eq!(root.name, "NeoForge");

        let subs = root.sub_modules.as_ref().unwrap();
        assert_eq!(subs[0].module_type, ModuleType::VersionManifest);
        assert_eq!(
            subs.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            [
                "1.21.1-neoforge-21.1.77",
                "net.neoforged:neoforge:21.1.77:client",
                "net.minecraft:client:1.21.1-20240808.144430:srg",
                "net.minecraft:client:1.21.1-20240808.144430:extra",
                "net.neoforged.fancymodloader:loader:4.0.31@jar",
                "org.ow2.asm:asm:9.7@jar",
            ]
        );
    }

    #[test]
    fn reports_an_installation_without_a_universal_jar() {
        let generated = vec![module(
            "net.neoforged:neoforge:21.1.77:client",
            ModuleType::Library,
        )];
        let error = assemble(generated, manifest_module(), Vec::new()).unwrap_err();
        assert!(error.to_string().contains("universal"));
    }

    #[test]
    fn reads_the_generated_versions_out_of_the_launch_arguments() {
        let manifest: VersionManifestFg3 = serde_json::from_str(
            r#"{"arguments":{"game":["--fml.neoForgeVersion","21.1.77","--fml.fmlVersion","4.0.31",
                "--fml.mcVersion","1.21.1","--fml.neoFormVersion","20240808.144430",
                "--launchTarget","forgeclient"]}}"#,
        )
        .unwrap();
        assert_eq!(
            fml_versions(&manifest).unwrap(),
            ("21.1.77".to_owned(), "1.21.1-20240808.144430".to_owned())
        );

        let error = fml_versions(&VersionManifestFg3::default()).unwrap_err();
        assert!(error.to_string().contains("--fml.neoForgeVersion"));
    }

    fn write_profile(versions: &Path, name: &str, body: &str) -> PathBuf {
        let manifest = versions.join(name).join(format!("{name}.json"));
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, body).unwrap();
        manifest
    }

    #[test]
    fn discovers_exactly_one_version_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let versions = dir.path().join("versions");

        // No versions directory at all.
        assert!(discover_version_manifest(dir.path()).is_err());

        // Directory without the matching json.
        std::fs::create_dir_all(versions.join("neoforge-21.1.77")).unwrap();
        let error = discover_version_manifest(dir.path()).unwrap_err();
        assert!(error.to_string().contains("found 0"));

        let manifest = write_profile(
            &versions,
            "neoforge-21.1.77",
            r#"{"id":"neoforge-21.1.77","inheritsFrom":"1.21.1"}"#,
        );
        // The vanilla profile the installer patched has no `inheritsFrom`, and
        // a stray file in `versions/` is not a profile directory.
        write_profile(&versions, "1.21.1", r#"{"id":"1.21.1","type":"release"}"#);
        std::fs::write(versions.join("stray.json"), b"{}").unwrap();
        assert_eq!(discover_version_manifest(dir.path()).unwrap(), manifest);

        // Two loader installs in one cache directory are ambiguous.
        write_profile(
            &versions,
            "neoforge-21.1.78",
            r#"{"id":"neoforge-21.1.78","inheritsFrom":"1.21.1"}"#,
        );
        let error = discover_version_manifest(dir.path()).unwrap_err();
        assert!(error.to_string().contains("found 2"));
        assert!(error.to_string().contains("versions"));
    }
}
