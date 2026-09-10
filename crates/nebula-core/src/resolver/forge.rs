//! Shared Forge behaviour: artifact version inference, the log4j advisory, the
//! `version.json` entry of a Forge jar, and the installer run that Forge (FG3)
//! and NeoForge both use.

use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result, io};
use crate::java::Java;
use crate::maven::MavenId;
use crate::version::{self, MinecraftVersion};

pub const FORGE_GROUP: &str = "net.minecraftforge";
pub const FORGE_ARTIFACT: &str = "forge";
pub const FMLCORE_ARTIFACT: &str = "fmlcore";
pub const JAVAFMLLANGUAGE_ARTIFACT: &str = "javafmllanguage";
pub const MCLANGUAGE_ARTIFACT: &str = "mclanguage";
pub const LOWCODELANGUAGE_ARTIFACT: &str = "lowcodelanguage";
pub const MINECRAFT_GROUP: &str = "net.minecraft";
pub const MINECRAFT_CLIENT_ARTIFACT: &str = "client";

const DEFAULT_INSTALL_TIMEOUT_SECS: u64 = 600;

/// `net.minecraftforge:forge:<artifact version>[:classifier]@jar`.
pub fn forge_maven_id(artifact_version: &str, classifier: Option<&str>) -> MavenId {
    MavenId {
        group: FORGE_GROUP.to_owned(),
        artifact: FORGE_ARTIFACT.to_owned(),
        version: artifact_version.to_owned(),
        classifier: classifier.map(str::to_owned),
        extension: "jar".to_owned(),
    }
}

/// Port of `ForgeResolver.inferArtifactVersion`. Coverage is deliberately
/// partial upstream: only the legacy builds that shipped a Minecraft suffix in
/// their maven version are listed.
pub fn infer_artifact_version(mc: &MinecraftVersion, forge_version: &str) -> String {
    let version = format!("{mc}-{forge_version}");

    let parts: Vec<Option<u64>> = forge_version
        .split('.')
        .map(|part| part.parse().ok())
        .collect();
    let component = |index: usize| parts.get(index).copied().flatten();
    // Upstream compares `Number(...)` results, and every comparison against a
    // NaN is false, so a non-numeric or missing component keeps the plain id.
    let (Some(major), Some(minor), Some(revision), Some(extra)) =
        (component(0), component(1), component(2), component(3))
    else {
        return version;
    };

    let suffix = match major {
        10 if minor == 13 && revision >= 2 && extra >= 1300 => "-1.7.10",
        11 if minor == 15 && revision == 1 && extra >= 1890 => "-1.8.9",
        11 if minor == 15 && revision == 0 && extra <= 1654 => "-1.8.8",
        11 if minor == 14 && revision == 0 && extra <= 1295 => "-1.8",
        12 if minor == 17 && revision == 0 && extra <= 1936 => "-1.9.4",
        12 if minor == 16 && revision == 0 && extra <= 1885 => "-1.9",
        12 if minor == 16 && revision == 1 && extra == 1938 => "-1.9.0",
        _ => return version,
    };
    format!("{version}{suffix}")
}

/// First Forge build per Minecraft minor that carries the log4j RCE patch
/// (CVE-2021-44228). Minecraft 1.11 and older never got one, but upstream only
/// treats 1.12-1.18 as vulnerable.
fn log4j_patch(mc: &MinecraftVersion) -> Option<&'static str> {
    if mc.major != 1 {
        return None;
    }
    match mc.minor {
        12 => Some("14.23.5.2857"),
        13 => Some("25.0.222"),
        14 => Some("28.2.25"),
        15 => Some("31.2.56"),
        16 => Some("36.2.20"),
        17 => Some("37.1.1"),
        18 => Some("38.0.17"),
        _ => None,
    }
}

/// Port of `ForgeResolver.checkSecurity`, minus the 15 second sleep: the caller
/// logs every line of the returned message with `log::error!`.
pub fn log4j_warning(mc: &MinecraftVersion, forge_version: &str) -> Option<String> {
    let patch = log4j_patch(mc)?;
    if version::version_gte(forge_version, patch) {
        return None;
    }
    Some(format!(
        "==================================================================\n\
         \x20                          WARNING                                \n\
         \x20 This version of Forge is vulnerable to a CRITICAL RCE exploit.  \n\
         \x20                   DO NOT USE THIS VERSION!                      \n\
         \x20    A patch is available as of Minecraft Forge v{patch}       \n\
         ==================================================================\n\
         Nebula will continue. To abort, use CTRL + C."
    ))
}

/// Reads the `version.json` entry of a Forge universal or installer jar.
pub fn version_json_from_jar(jar: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(jar).map_err(io(format!("opening {}", jar.display())))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entry = archive.by_name("version.json")?;
    let mut buffer = Vec::new();
    entry
        .read_to_end(&mut buffer)
        .map_err(io(format!("reading version.json from {}", jar.display())))?;
    Ok(buffer)
}

/// Our divergence from upstream, which always opened the Swing installer and
/// asked the user to pick the directory:
///
/// (a) create `dir`, copy the installer in, write `launcher_profiles.json`;
/// (b) run `--installClient=<dir>` headlessly with a timeout;
/// (c) only when a GUI is reachable, recreate `dir` and re-run with no
///     arguments so the installer opens its window;
/// (d) delete `dir` and fail, naming the directory.
///
/// The `=` in (b) is not cosmetic. Both installers declare the option with
/// joptsimple's `withOptionalArg()`, which only accepts an attached value: with
/// `--installClient <dir>` the directory is parsed as a stray positional and the
/// installer falls back to its default, which is the cwd for Forge but the
/// user's real `.minecraft` for NeoForge.
///
/// `manifest_present` reports whether the installer produced its version
/// manifest: a fixed path for Forge, a directory scan for NeoForge.
pub fn run_installer_headless_then_gui(
    java: &Java,
    installer: &Path,
    dir: &Path,
    manifest_present: impl Fn() -> bool,
) -> Result<()> {
    let timeout = install_timeout();

    let working = stage_installer(installer, dir)?;
    let headless = installer_target(dir);
    log::info!("Running the installer headlessly with {headless}");
    let mut last_error = run(java, &working, &[headless], dir, timeout);
    if manifest_present() {
        return Ok(());
    }

    if gui_fallback_allowed() {
        log::info!("============== [ IMPORTANT ] ==============");
        log::info!("When the installer opens please set the client installation directory to:");
        log::info!("{}", dir.display());
        log::info!("===========================================");
        let _ = fs::remove_dir_all(dir);
        let working = stage_installer(installer, dir)?;
        last_error = run(java, &working, &[], dir, timeout);
        if manifest_present() {
            return Ok(());
        }
    }

    let _ = fs::remove_dir_all(dir);
    let mut message = format!(
        "The installer did not produce a version manifest. It was either not installed or \
         installed to the wrong location; the installation directory must be {}",
        dir.display()
    );
    if let Some(error) = last_error {
        message.push_str(&format!(" (installer error: {error})"));
    }
    Err(Error::Invalid(message))
}

/// A failed run is not fatal on its own: the installer may have timed out with
/// its window open, so the manifest check decides. The error is kept for the
/// final message, where a missing `java` executable would otherwise read as a
/// wrong installation directory.
fn run(
    java: &Java,
    installer: &Path,
    args: &[String],
    dir: &Path,
    timeout: Duration,
) -> Option<Error> {
    match java.run_installer(installer, args, dir, timeout) {
        Ok(status) if status.success() => None,
        Ok(status) => {
            let error = Error::Tool {
                tool: "forge-installer".to_owned(),
                code: status.code(),
            };
            log::warn!("{error}");
            Some(error)
        }
        Err(error) => {
            log::warn!("Forge installer failed to run: {error}");
            Some(error)
        }
    }
}

fn installer_target(dir: &Path) -> String {
    format!("--installClient={}", dir.display())
}

fn stage_installer(installer: &Path, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir).map_err(io(format!("creating {}", dir.display())))?;
    let name = installer
        .file_name()
        .map_or_else(|| PathBuf::from("installer.jar"), PathBuf::from);
    let working = dir.join(name);
    fs::copy(installer, &working).map_err(io(format!("copying {}", installer.display())))?;
    // Required for the installer to function.
    fs::write(dir.join("launcher_profiles.json"), b"{}").map_err(io(format!(
        "writing launcher_profiles.json in {}",
        dir.display()
    )))?;
    Ok(working)
}

fn install_timeout() -> Duration {
    let seconds = std::env::var("NEBULA_FORGE_INSTALL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_INSTALL_TIMEOUT_SECS);
    Duration::from_secs(seconds)
}

/// Never open a window in CI: `NEBULA_FORGE_GUI_FALLBACK=0` disables the
/// fallback outright, and otherwise somebody has to be able to see it.
fn gui_fallback_allowed() -> bool {
    if std::env::var("NEBULA_FORGE_GUI_FALLBACK").as_deref() == Ok("0") {
        return false;
    }
    std::io::stdin().is_terminal() || std::env::var("NEBULA_GUI").as_deref() == Ok("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc(version: &str) -> MinecraftVersion {
        version.parse().unwrap()
    }

    #[test]
    fn infers_the_legacy_artifact_versions() {
        // The seven rows of the upstream table.
        let rows = [
            ("1.7.10", "10.13.4.1614", "1.7.10-10.13.4.1614-1.7.10"),
            ("1.8.9", "11.15.1.2318", "1.8.9-11.15.1.2318-1.8.9"),
            ("1.8.8", "11.15.0.1654", "1.8.8-11.15.0.1654-1.8.8"),
            ("1.8", "11.14.0.1295", "1.8-11.14.0.1295-1.8"),
            ("1.9.4", "12.17.0.1936", "1.9.4-12.17.0.1936-1.9.4"),
            ("1.9", "12.16.0.1885", "1.9-12.16.0.1885-1.9"),
            ("1.9", "12.16.1.1938", "1.9-12.16.1.1938-1.9.0"),
        ];
        for (minecraft, forge, expected) in rows {
            assert_eq!(infer_artifact_version(&mc(minecraft), forge), expected);
        }
    }

    #[test]
    fn leaves_modern_artifact_versions_alone() {
        assert_eq!(
            infer_artifact_version(&mc("1.20.1"), "47.2.0"),
            "1.20.1-47.2.0"
        );
        assert_eq!(
            infer_artifact_version(&mc("1.12.2"), "14.23.5.2860"),
            "1.12.2-14.23.5.2860"
        );
        // Right major, but outside every legacy window: the upstream table
        // stops one build short of the real 1.8.8 release.
        assert_eq!(
            infer_artifact_version(&mc("1.8.8"), "11.15.0.1655"),
            "1.8.8-11.15.0.1655"
        );
        assert_eq!(
            infer_artifact_version(&mc("1.8.9"), "11.15.1.1889"),
            "1.8.9-11.15.1.1889"
        );
        assert_eq!(
            infer_artifact_version(&mc("1.9"), "12.16.1.1939"),
            "1.9-12.16.1.1939"
        );
        assert_eq!(
            infer_artifact_version(&mc("1.7.10"), "10.13.1.1300"),
            "1.7.10-10.13.1.1300"
        );
    }

    #[test]
    fn warns_about_vulnerable_forge_builds() {
        // Outside the 1.12-1.18 window nothing is reported.
        assert!(log4j_warning(&mc("1.11.2"), "13.20.0.2201").is_none());
        assert!(log4j_warning(&mc("1.19.2"), "43.0.0").is_none());

        // Boundaries: the patch build itself is safe.
        assert!(log4j_warning(&mc("1.12.2"), "14.23.5.2856").is_some());
        assert!(log4j_warning(&mc("1.12.2"), "14.23.5.2857").is_none());
        assert!(log4j_warning(&mc("1.12.2"), "14.23.5.2860").is_none());
        assert!(log4j_warning(&mc("1.18.1"), "38.0.16").is_some());
        assert!(log4j_warning(&mc("1.18.1"), "38.0.17").is_none());
        assert!(log4j_warning(&mc("1.13.2"), "25.0.221").is_some());
        assert!(log4j_warning(&mc("1.14.4"), "28.2.24").is_some());
        assert!(log4j_warning(&mc("1.15.2"), "31.2.55").is_some());
        assert!(log4j_warning(&mc("1.16.5"), "36.2.19").is_some());
        assert!(log4j_warning(&mc("1.17.1"), "37.1.0").is_some());

        let warning = log4j_warning(&mc("1.16.5"), "36.2.19").unwrap();
        assert!(warning.contains("CRITICAL RCE exploit"));
        assert!(warning.contains("Minecraft Forge v36.2.20"));
        assert_eq!(warning.lines().count(), 7);
    }

    #[test]
    fn reads_version_json_out_of_a_jar() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("forge-universal.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("version.json", options).unwrap();
        std::io::Write::write_all(&mut writer, br#"{"id":"1.12.2-forge"}"#).unwrap();
        writer.finish().unwrap();

        assert_eq!(
            version_json_from_jar(&jar).unwrap(),
            br#"{"id":"1.12.2-forge"}"#
        );

        let empty = dir.path().join("empty.jar");
        let file = std::fs::File::create(&empty).unwrap();
        zip::ZipWriter::new(file).finish().unwrap();
        assert!(version_json_from_jar(&empty).is_err());
    }

    #[test]
    fn builds_forge_maven_ids() {
        let id = forge_maven_id("1.20.1-47.2.0", Some("universal"));
        assert_eq!(
            id.id_without_extension(),
            "net.minecraftforge:forge:1.20.1-47.2.0:universal"
        );
        assert_eq!(
            id.path(),
            "net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-universal.jar"
        );
    }
}
