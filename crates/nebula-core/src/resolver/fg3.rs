//! ForgeGradle 3 adapter: Minecraft 1.12 (Forge > 14.23.5.2847) through 1.21.
//!
//! Two paths. From 1.13 the installer has to run because it *generates* jars
//! that exist nowhere else; the files it must produce are the hard-coded matrix
//! in [`generated_files`]. Minecraft 1.12 has no generated files, so the
//! installer jar is only opened for its `version.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io};
use crate::hash;
use crate::java::Java;
use crate::layout::Root;
use crate::maven::MavenId;
use crate::paths;
use crate::remote;
use crate::spec::{Module, ModuleType};
use crate::version::{self, MinecraftVersion};

use super::forge::{
    self, FMLCORE_ARTIFACT, FORGE_ARTIFACT, FORGE_GROUP, JAVAFMLLANGUAGE_ARTIFACT,
    LOWCODELANGUAGE_ARTIFACT, MCLANGUAGE_ARTIFACT, MINECRAFT_CLIENT_ARTIFACT, MINECRAFT_GROUP,
};
use super::{
    ResolveOptions, Resolver, artifact_for_file, artifact_from_digests, copy_file, join_maven,
    plain_module, verified_digests,
};

pub const WILDCARD_MCP_VERSION: &str = "${mcpVersion}";

/// One file the Forge installer is expected to leave behind. `classifiers` is
/// tried in order and the first hit wins, which is how Forge's `slim` /
/// `slim-stable` renames are absorbed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub name: &'static str,
    pub group: &'static str,
    pub artifact: &'static str,
    pub version: String,
    pub classifiers: Vec<Option<&'static str>>,
    pub skip_if_not_present: bool,
    pub classpath: Option<bool>,
}

impl GeneratedFile {
    fn new(
        name: &'static str,
        group: &'static str,
        artifact: &'static str,
        version: String,
        classifier: Option<&'static str>,
    ) -> Self {
        Self {
            name,
            group,
            artifact,
            version,
            classifiers: vec![classifier],
            skip_if_not_present: false,
            classpath: None,
        }
    }

    fn forge(name: &'static str, artifact_version: &str, classifier: Option<&'static str>) -> Self {
        Self::new(
            name,
            FORGE_GROUP,
            FORGE_ARTIFACT,
            artifact_version.to_owned(),
            classifier,
        )
    }

    fn classpath(mut self, classpath: bool) -> Self {
        self.classpath = Some(classpath);
        self
    }

    fn skip_if_not_present(mut self) -> Self {
        self.skip_if_not_present = true;
        self
    }

    fn classifiers(mut self, classifiers: [&'static str; 2]) -> Self {
        self.classifiers = classifiers.into_iter().map(Some).collect();
        self
    }
}

/// 1.20.3 replaced the launch wrapper with an executable jar.
pub fn is_executable_jar(mc: &MinecraftVersion) -> bool {
    *mc >= MinecraftVersion::new(1, 20, Some(3))
}

/// Port of `ForgeGradle3Adapter.isForVersion`.
pub fn is_for_version(mc: &MinecraftVersion, forge_version: &str) -> bool {
    if mc.minor == 12 && version::is_one_dot_twelve_fg2(forge_version) {
        return false;
    }
    version::is_version_acceptable(mc, &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21])
}

/// Port of `ForgeGradle3Adapter.configure`. Order is load-bearing: the first
/// entry becomes the `ForgeHosted` root module and the rest become its
/// sub-modules, after the version manifest.
pub fn generated_files(mc: &MinecraftVersion, artifact_version: &str) -> Vec<GeneratedFile> {
    if is_executable_jar(mc) {
        return vec![
            GeneratedFile::forge("universal jar", artifact_version, Some("universal")),
            GeneratedFile::forge("client jar", artifact_version, Some("client")),
            GeneratedFile::forge("client shim", artifact_version, Some("shim")).classpath(false),
            GeneratedFile::new(
                "fmlcore",
                FORGE_GROUP,
                FMLCORE_ARTIFACT,
                artifact_version.to_owned(),
                None,
            ),
            GeneratedFile::new(
                "javafmllanguage",
                FORGE_GROUP,
                JAVAFMLLANGUAGE_ARTIFACT,
                artifact_version.to_owned(),
                None,
            ),
            GeneratedFile::new(
                "mclanguage",
                FORGE_GROUP,
                MCLANGUAGE_ARTIFACT,
                artifact_version.to_owned(),
                None,
            ),
            GeneratedFile::new(
                "lowcodelanguage",
                FORGE_GROUP,
                LOWCODELANGUAGE_ARTIFACT,
                artifact_version.to_owned(),
                None,
            ),
        ];
    }

    if !version::is_version_acceptable(mc, &[13, 14, 15, 16, 17, 18, 19, 20]) {
        // 1.12 has nothing to configure; anything else has no adapter.
        return Vec::new();
    }

    // https://github.com/MinecraftForge/MinecraftForge/commit/97d4652
    let mcp_unified = format!("{mc}-{WILDCARD_MCP_VERSION}");

    let mut files = vec![
        GeneratedFile::forge("universal jar", artifact_version, Some("universal")).classpath(false),
        GeneratedFile::forge("client jar", artifact_version, Some("client")).classpath(false),
        GeneratedFile::new(
            "client data",
            MINECRAFT_GROUP,
            MINECRAFT_CLIENT_ARTIFACT,
            mc.to_string(),
            Some("data"),
        )
        .skip_if_not_present()
        .classpath(false),
        GeneratedFile::new(
            "client srg",
            MINECRAFT_GROUP,
            MINECRAFT_CLIENT_ARTIFACT,
            mcp_unified.clone(),
            Some("srg"),
        )
        .classpath(false),
    ];

    if version::is_version_acceptable(mc, &[13, 14, 15, 16]) {
        // Base jar present for 1.13-1.16.
        files.splice(
            0..0,
            [GeneratedFile::forge("base jar", artifact_version, None)],
        );
    }

    if version::is_version_acceptable(mc, &[17, 18, 19, 20]) {
        // Added in 1.17+.
        files.splice(
            0..0,
            [
                GeneratedFile::new(
                    "fmlcore",
                    FORGE_GROUP,
                    FMLCORE_ARTIFACT,
                    artifact_version.to_owned(),
                    None,
                ),
                GeneratedFile::new(
                    "javafmllanguage",
                    FORGE_GROUP,
                    JAVAFMLLANGUAGE_ARTIFACT,
                    artifact_version.to_owned(),
                    None,
                ),
                GeneratedFile::new(
                    "mclanguage",
                    FORGE_GROUP,
                    MCLANGUAGE_ARTIFACT,
                    artifact_version.to_owned(),
                    None,
                ),
            ],
        );
    }

    if version::is_version_acceptable(mc, &[18, 19, 20]) {
        // Added in 1.18+.
        files.splice(
            0..0,
            [GeneratedFile::new(
                "lowcodelanguage",
                FORGE_GROUP,
                LOWCODELANGUAGE_ARTIFACT,
                artifact_version.to_owned(),
                None,
            )],
        );
    }

    if version::is_version_acceptable(mc, &[13, 14, 15]) {
        // 13, 14, 15 use just the MC version.
        files.push(
            GeneratedFile::new(
                "client slim",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                mc.to_string(),
                None,
            )
            .classifiers(["slim", "slim-stable"]),
        );
        files.push(
            GeneratedFile::new(
                "client extra",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                mc.to_string(),
                None,
            )
            .classifiers(["extra", "extra-stable"]),
        );
    } else {
        // 16+ uses the mcp unified version.
        files.push(
            GeneratedFile::new(
                "client slim",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                mcp_unified.clone(),
                None,
            )
            .classifiers(["slim", "slim-stable"])
            .classpath(false),
        );
        files.push(
            GeneratedFile::new(
                "client extra",
                MINECRAFT_GROUP,
                MINECRAFT_CLIENT_ARTIFACT,
                mcp_unified,
                None,
            )
            .classifiers(["extra", "extra-stable"])
            .classpath(false),
        );
    }

    files
}

/// `arguments.game` is absent from the legacy 1.12 manifests, which is why it
/// is optional here; the wildcard it feeds is only used from 1.13 on.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VersionManifestFg3 {
    #[serde(default)]
    pub arguments: Option<Fg3Arguments>,
    #[serde(default)]
    pub libraries: Vec<Fg3Library>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Fg3Arguments {
    #[serde(default)]
    pub game: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Fg3Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Fg3Downloads,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Fg3Downloads {
    #[serde(default)]
    pub artifact: Option<Fg3Artifact>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Fg3Artifact {
    #[serde(default)]
    pub path: String,
    /// Empty for the jars the installer generates locally.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

impl VersionManifestFg3 {
    pub fn game_arguments(&self) -> &[String] {
        self.arguments
            .as_ref()
            .map_or(&[], |arguments| arguments.game.as_slice())
    }
}

/// The value that follows `flag` in a manifest's launch arguments.
pub fn argument_value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

/// The MCP version follows `--fml.mcpVersion` in the launch arguments.
pub fn mcp_version(arguments: &[String]) -> Option<String> {
    argument_value(arguments, "--fml.mcpVersion")
}

pub struct ForgeGradle3Adapter {
    root: Root,
    mc: MinecraftVersion,
    forge_version: String,
    artifact_version: String,
    java: Java,
    opts: ResolveOptions,
}

impl ForgeGradle3Adapter {
    pub fn new(
        root: &Root,
        mc: &MinecraftVersion,
        forge_version: &str,
        java: &Java,
        opts: ResolveOptions,
    ) -> Self {
        Self {
            root: root.clone(),
            mc: *mc,
            forge_version: forge_version.to_owned(),
            artifact_version: forge::infer_artifact_version(mc, forge_version),
            java: java.clone(),
            opts,
        }
    }

    /// `repo/versions/<mc>-forge-<forge>` — note the *forge* version, while the
    /// module ids use the inferred artifact version.
    fn version_name(&self) -> String {
        format!("{}-forge-{}", self.mc, self.forge_version)
    }

    fn resolve(&self) -> Result<Module> {
        let installer_id = forge::forge_maven_id(&self.artifact_version, Some("installer"));
        let installer = self.root.lib_path(&installer_id);
        log::debug!("Checking for forge installer at {}..", installer.display());
        if !installer.exists() {
            log::debug!("Forge installer not found locally, initializing download..");
            remote::download_to(
                &join_maven(remote::FORGE_MAVEN, &installer_id.path()),
                &installer,
            )?;
        } else {
            log::debug!("Using locally discovered forge installer.");
        }
        log::debug!(
            "Beginning processing of Forge v{} (Minecraft {})",
            self.forge_version,
            self.mc
        );

        let generated = generated_files(&self.mc, &self.artifact_version);
        if generated.is_empty() {
            self.process_without_installer(&installer)
        } else {
            self.process_with_installer(&installer, generated)
        }
    }

    fn process_with_installer(
        &self,
        installer: &Path,
        generated: Vec<GeneratedFile>,
    ) -> Result<Module> {
        let cache = self.root.forge_cache_dir(&self.artifact_version);
        let name = self.version_name();
        let installed_manifest = cache
            .join("versions")
            .join(&name)
            .join(format!("{name}.json"));

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
            forge::run_installer_headless_then_gui(
                &self.java,
                installer,
                &cache,
                self.opts.gui_fallback,
                || installed_manifest.exists(),
            )?;
        } else if !installed_manifest.exists() {
            fs::remove_dir_all(&cache).map_err(io(format!("removing {}", cache.display())))?;
            return Err(Error::Invalid(format!(
                "The cached Forge installation in {} has no version manifest; re-run with an \
                 invalidated cache",
                cache.display()
            )));
        }

        log::debug!("Processing Version Manifest");
        let raw = fs::read(&installed_manifest)
            .map_err(io(format!("reading {}", installed_manifest.display())))?;
        let manifest: VersionManifestFg3 = serde_json::from_slice(&raw)?;

        let destination = self.root.version_manifest_path(&name);
        copy_file(&installed_manifest, &destination)?;
        let manifest_module = plain_module(
            self.artifact_version.clone(),
            "Minecraft Forge (version.json)".to_owned(),
            ModuleType::VersionManifest,
            artifact_for_file(&destination, self.root.version_manifest_url(&name))?,
        );

        log::debug!("Processing generated forge files.");
        let mut forge_module = self.process_generated_files(generated, &manifest, &cache)?;

        let mut sub_modules = forge_module.sub_modules.take().unwrap_or_default();
        sub_modules.insert(0, manifest_module);

        log::debug!("Processing Libraries");
        sub_modules.extend(self.process_libraries(&manifest, &cache)?);
        forge_module.sub_modules = Some(sub_modules);

        if self.opts.discard_output {
            log::info!("Removing installer output at {}..", cache.display());
            fs::remove_dir_all(&cache).map_err(io(format!("removing {}", cache.display())))?;
            log::info!("Removed successfully.");
        }

        Ok(forge_module)
    }

    fn process_generated_files(
        &self,
        generated: Vec<GeneratedFile>,
        manifest: &VersionManifestFg3,
        cache: &Path,
    ) -> Result<Module> {
        let lib_dir = cache.join("libraries");
        let generated = resolve_wildcards(generated, manifest)?;

        let mut modules: Vec<Module> = Vec::with_capacity(generated.len());
        for entry in &generated {
            let mut targets = Vec::with_capacity(entry.classifiers.len());
            let mut located = false;

            for classifier in &entry.classifiers {
                let id = MavenId {
                    group: entry.group.to_owned(),
                    artifact: entry.artifact.to_owned(),
                    version: entry.version.clone(),
                    classifier: classifier.map(str::to_owned),
                    extension: "jar".to_owned(),
                };
                let target = lib_dir.join(id.path());
                targets.push(target.display().to_string());
                if !target.exists() {
                    continue;
                }

                let destination = self.root.lib_path(&id);
                copy_file(&target, &destination)?;

                let mut module = plain_module(
                    id.id_without_extension(),
                    format!("Minecraft Forge ({})", entry.name),
                    ModuleType::Library,
                    artifact_for_file(&destination, self.root.lib_url(&id))?,
                );
                module.classpath = Some(entry.classpath.unwrap_or(true));
                // Upstream emits an empty array on every generated module.
                module.sub_modules = Some(Vec::new());
                modules.push(module);

                located = true;
                break;
            }

            if !entry.skip_if_not_present && !located {
                return Err(Error::Invalid(format!(
                    "Required file {} not found at any expected location:\n\t{}",
                    entry.name,
                    targets.join("\n\t")
                )));
            }
        }

        if modules.is_empty() {
            return Err(Error::Invalid(
                "The Forge installer produced none of the expected files.".to_owned(),
            ));
        }
        let mut forge_module = modules.remove(0);
        forge_module.module_type = ModuleType::ForgeHosted;
        forge_module.sub_modules = Some(modules);
        Ok(forge_module)
    }

    /// Only manifest libraries with a download url are emitted: that is how the
    /// forge entry itself (generated, hence url-less) is skipped.
    fn process_libraries(
        &self,
        manifest: &VersionManifestFg3,
        cache: &Path,
    ) -> Result<Vec<Module>> {
        let lib_dir = cache.join("libraries");
        let mut modules = Vec::with_capacity(manifest.libraries.len());

        for entry in &manifest.libraries {
            let Some(artifact) = &entry.downloads.artifact else {
                log::debug!("{} has no download artifact, skipping.", entry.name);
                continue;
            };
            if artifact.url.is_empty() {
                continue;
            }

            let target = lib_dir.join(paths::clean_relative(&artifact.path)?);
            if !target.exists() {
                return Err(Error::Invalid(format!(
                    "Expected library {} not found!",
                    entry.name
                )));
            }

            let id = MavenId::parse(&entry.name, "jar")?;
            let destination = self.root.lib_path(&id);
            copy_file(&target, &destination)?;

            modules.push(plain_module(
                entry.name.clone(),
                format!("Minecraft Forge ({})", id.artifact),
                ModuleType::Library,
                artifact_for_file(&destination, self.root.lib_url(&id))?,
            ));
        }

        Ok(modules)
    }

    /// Minecraft 1.12 with Forge > 14.23.5.2847: no generated files, so the
    /// installer jar is only a container for `version.json`.
    fn process_without_installer(&self, installer: &Path) -> Result<Module> {
        let raw = forge::version_json_from_jar(installer).map_err(|error| {
            Error::Invalid(format!(
                "Failed to find version.json in forge installer jar: {error}"
            ))
        })?;
        let value: serde_json::Value = serde_json::from_slice(&raw)?;
        let manifest: VersionManifestFg3 = serde_json::from_value(value.clone())?;

        let name = self.version_name();
        let manifest_path = self.root.version_manifest_path(&name);
        write_json_4_spaces(&manifest_path, &value)?;

        let universal_id = forge::forge_maven_id(&self.artifact_version, Some("universal"));
        let universal = self.root.lib_path(&universal_id);
        log::debug!(
            "Checking for Forge Universal jar at {}..",
            universal.display()
        );

        let forge_entry = manifest
            .libraries
            .iter()
            .find(|lib| lib.name.starts_with("net.minecraftforge:forge:"))
            .ok_or_else(|| Error::Invalid("Forge entry not found in version.json!".to_owned()))?;
        let forge_artifact = forge_entry.downloads.artifact.as_ref().ok_or_else(|| {
            Error::Invalid("Forge entry in version.json has no download artifact!".to_owned())
        })?;

        // The local copy is hashed once: the SHA-1 decides whether it is the jar
        // the manifest describes, and the same pass feeds the module artifact.
        let mut local = None;
        if universal.exists() {
            let digests = hash::digests_of_file(&universal)?;
            if digests.sha1 == forge_artifact.sha1 {
                log::debug!("Using locally discovered forge.");
                local = Some(digests);
            } else {
                log::debug!("SHA-1 of local universal jar does not match version.json entry.");
                log::debug!("Redownloading Forge Universal jar..");
            }
        } else {
            log::debug!("Forge Universal jar not found locally, initializing download..");
        }
        let universal_digests = match local {
            Some(digests) => digests,
            None => {
                remote::download_to(
                    &join_maven(remote::FORGE_MAVEN, &universal_id.path()),
                    &universal,
                )?;
                hash::digests_of_file(&universal)?
            }
        };

        log::debug!(
            "Beginning processing of Forge v{} (Minecraft {})",
            self.forge_version,
            self.mc
        );

        let mut forge_module = plain_module(
            universal_id.id_without_extension(),
            "Minecraft Forge".to_owned(),
            ModuleType::ForgeHosted,
            artifact_from_digests(&universal_digests, self.root.lib_url(&universal_id)),
        );

        let mut sub_modules = vec![plain_module(
            self.artifact_version.clone(),
            "Minecraft Forge (version.json)".to_owned(),
            ModuleType::VersionManifest,
            artifact_for_file(&manifest_path, self.root.version_manifest_url(&name))?,
        )];

        for lib in &manifest.libraries {
            if lib.name.starts_with("net.minecraftforge:forge:") {
                // We've already processed forge.
                continue;
            }
            log::debug!("Processing {}..", lib.name);
            let Some(artifact) = &lib.downloads.artifact else {
                log::warn!("{} has no download artifact, skipping.", lib.name);
                continue;
            };

            let id = MavenId::parse(&lib.name, "jar")?;
            let local = self.root.lib_path(&id);

            let digests = verified_digests(
                &local,
                (!artifact.sha1.is_empty()).then_some(artifact.sha1.as_str()),
                || {
                    if artifact.url.is_empty() {
                        return Err(Error::Invalid(format!(
                            "Library {} has no download url in version.json!",
                            lib.name
                        )));
                    }
                    remote::download_to(&artifact.url, &local)
                },
            )?;

            sub_modules.push(plain_module(
                id.id(),
                format!("Minecraft Forge ({})", id.artifact),
                ModuleType::Library,
                artifact_from_digests(&digests, self.root.lib_url(&id)),
            ));
        }

        forge_module.sub_modules = Some(sub_modules);
        Ok(forge_module)
    }
}

impl Resolver for ForgeGradle3Adapter {
    fn module(&mut self) -> Result<Module> {
        self.resolve()
    }
}

fn resolve_wildcards(
    generated: Vec<GeneratedFile>,
    manifest: &VersionManifestFg3,
) -> Result<Vec<GeneratedFile>> {
    if !generated
        .iter()
        .any(|file| file.version.contains(WILDCARD_MCP_VERSION))
    {
        return Ok(generated);
    }
    let mcp = mcp_version(manifest.game_arguments()).ok_or_else(|| {
        Error::Invalid("MCP Version not found.. did forge change their format?".to_owned())
    })?;
    Ok(generated
        .into_iter()
        .map(|mut file| {
            file.version = file.version.replace(WILDCARD_MCP_VERSION, &mcp);
            file
        })
        .collect())
}

/// Upstream wrote the 1.12 manifest with `writeJson(.., { spaces: 4 })`, which
/// is four space indentation plus a trailing newline. The bytes are hashed into
/// the module, so the formatting is part of the contract.
///
/// Deviation: serde_json orders object keys alphabetically (enabling
/// `preserve_order` would change the dependency graph). The digests describe
/// the file we write, so the manifest stays self-consistent.
fn write_json_4_spaces(path: &PathBuf, value: &serde_json::Value) -> Result<()> {
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    value.serialize(&mut serializer)?;
    buffer.push(b'\n');

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    fs::write(path, buffer).map_err(io(format!("writing {}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    fn names(version: &str) -> Vec<&'static str> {
        generated_files(&mc(version), "artifact-version")
            .into_iter()
            .map(|file| file.name)
            .collect()
    }

    #[test]
    fn claims_the_forge_gradle_3_versions() {
        assert!(!is_for_version(&mc("1.12.2"), "14.23.5.2847"));
        assert!(is_for_version(&mc("1.12.2"), "14.23.5.2848"));
        assert!(is_for_version(&mc("1.13.2"), "25.0.219"));
        assert!(is_for_version(&mc("1.21.1"), "52.0.0"));
        assert!(!is_for_version(&mc("1.11.2"), "13.20.1.2588"));
        assert!(!is_for_version(&mc("1.22.0"), "60.0.0"));
    }

    #[test]
    fn splits_the_executable_jar_era_at_1_20_3() {
        assert!(!is_executable_jar(&mc("1.20.2")));
        assert!(is_executable_jar(&mc("1.20.3")));
        assert!(is_executable_jar(&mc("1.21")));
    }

    #[test]
    fn builds_the_generated_file_matrix() {
        assert_eq!(names("1.12.2").len(), 0);

        assert_eq!(names("1.16.5").len(), 7);
        assert_eq!(names("1.16.5")[0], "base jar");

        assert_eq!(names("1.17.1").len(), 9);
        assert_eq!(names("1.17.1")[0], "fmlcore");

        assert_eq!(names("1.18.2").len(), 10);
        assert_eq!(names("1.18.2")[0], "lowcodelanguage");
        assert_eq!(names("1.20.1").len(), 10);
        assert_eq!(names("1.20.1")[0], "lowcodelanguage");

        assert_eq!(names("1.20.4").len(), 7);
        assert_eq!(names("1.20.4")[0], "universal jar");
        assert_eq!(names("1.21.1").len(), 7);
        assert_eq!(names("1.21.1")[0], "universal jar");
    }

    #[test]
    fn keeps_the_unshifted_groups_in_order() {
        assert_eq!(
            names("1.20.1"),
            [
                "lowcodelanguage",
                "fmlcore",
                "javafmllanguage",
                "mclanguage",
                "universal jar",
                "client jar",
                "client data",
                "client srg",
                "client slim",
                "client extra",
            ]
        );
        assert_eq!(
            names("1.20.4"),
            [
                "universal jar",
                "client jar",
                "client shim",
                "fmlcore",
                "javafmllanguage",
                "mclanguage",
                "lowcodelanguage",
            ]
        );
    }

    #[test]
    fn marks_classpath_and_optional_entries() {
        let files = generated_files(&mc("1.20.1"), "1.20.1-47.2.0");
        let entry = |name: &str| files.iter().find(|file| file.name == name).unwrap().clone();

        assert_eq!(entry("universal jar").classpath, Some(false));
        assert!(!entry("universal jar").skip_if_not_present);
        assert!(entry("client data").skip_if_not_present);
        assert_eq!(
            entry("client slim").classifiers,
            [Some("slim"), Some("slim-stable")]
        );
        assert_eq!(entry("client slim").classpath, Some(false));

        // 1.13-1.15 keep the plain MC version and the default classpath.
        let old = generated_files(&mc("1.15.2"), "1.15.2-31.2.57");
        let slim = old.iter().find(|file| file.name == "client slim").unwrap();
        assert_eq!(slim.version, "1.15.2");
        assert_eq!(slim.classpath, None);

        // 1.20.3+ puts only the shim off the classpath.
        let modern = generated_files(&mc("1.21.1"), "1.21.1-52.0.0");
        assert_eq!(modern[0].classpath, None);
        assert_eq!(modern[2].classpath, Some(false));
    }

    #[test]
    fn expands_the_mcp_wildcard() {
        let files = generated_files(&mc("1.20.1"), "1.20.1-47.2.0");
        let srg = files.iter().find(|file| file.name == "client srg").unwrap();
        assert_eq!(srg.version, "1.20.1-${mcpVersion}");

        let manifest: VersionManifestFg3 = serde_json::from_str(
            r#"{"arguments":{"game":["--launchTarget","forgeclient","--fml.mcpVersion","20230612.114412"]}}"#,
        )
        .unwrap();
        let resolved = resolve_wildcards(files, &manifest).unwrap();
        let srg = resolved
            .iter()
            .find(|file| file.name == "client srg")
            .unwrap();
        assert_eq!(srg.version, "1.20.1-20230612.114412");

        // Nothing to expand for 1.20.4+, so a manifest without arguments is fine.
        let modern = generated_files(&mc("1.21.1"), "1.21.1-52.0.0");
        assert!(resolve_wildcards(modern, &VersionManifestFg3::default()).is_ok());
    }

    #[test]
    fn reports_a_missing_mcp_version() {
        let files = generated_files(&mc("1.20.1"), "1.20.1-47.2.0");
        let error = resolve_wildcards(files, &VersionManifestFg3::default()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "MCP Version not found.. did forge change their format?"
        );
    }

    #[test]
    fn extracts_the_mcp_version() {
        let arguments: Vec<String> = ["--fml.forgeVersion", "47.2.0", "--fml.mcpVersion", "1234"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(mcp_version(&arguments), Some("1234".to_owned()));
        assert_eq!(mcp_version(&arguments[..2]), None);
        assert_eq!(mcp_version(&arguments[..3]), None);
        assert_eq!(mcp_version(&[]), None);
    }

    #[test]
    fn tolerates_the_legacy_1_12_manifest_shape() {
        // 1.12 uses `minecraftArguments`, not `arguments`.
        let manifest: VersionManifestFg3 = serde_json::from_str(
            r#"{"id":"1.12.2-forge","minecraftArguments":"--tweakClass x",
                "libraries":[{"name":"net.minecraftforge:forge:1.12.2-14.23.5.2860",
                  "downloads":{"artifact":{"path":"a/b.jar","url":"","sha1":"abc","size":1}}},
                 {"name":"org.ow2.asm:asm-debug-all:5.2"}]}"#,
        )
        .unwrap();
        assert!(manifest.arguments.is_none());
        assert!(manifest.game_arguments().is_empty());
        assert_eq!(
            manifest.libraries[0]
                .downloads
                .artifact
                .as_ref()
                .unwrap()
                .sha1,
            "abc"
        );
        assert!(manifest.libraries[1].downloads.artifact.is_none());
    }

    #[test]
    fn writes_manifests_with_four_spaces_and_a_final_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("version.json");
        let value: serde_json::Value =
            serde_json::from_str(r#"{"id":"x","libraries":[]}"#).unwrap();
        write_json_4_spaces(&path, &value).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n    \"id\": \"x\",\n    \"libraries\": []\n}\n"
        );
    }
}
