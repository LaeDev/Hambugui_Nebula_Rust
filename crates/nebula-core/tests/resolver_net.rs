//! Resolver tests that hit the real Forge, NeoForge and Fabric endpoints.
//!
//! They only run with `NEBULA_NET_TESTS=1`; the Forge and NeoForge ones also
//! need `JAVA_EXECUTABLE`. Run them one at a time, they share nothing but the
//! network:
//!
//! ```text
//! NEBULA_NET_TESTS=1 JAVA_EXECUTABLE=/path/to/java \
//!   cargo test -p nebula-core --test resolver_net -- --nocapture --test-threads=1
//! ```

use std::path::PathBuf;
use std::time::Instant;

use nebula_core::java::Java;
use nebula_core::layout::Root;
use nebula_core::maven::MavenId;
use nebula_core::resolver::{self, ResolveOptions};
use nebula_core::spec::{Module, ModuleType};
use nebula_core::version::{MinecraftVersion, Promotion};
use nebula_core::{remote, spec};
use url::Url;

const BASE_URL: &str = "https://cdn.example.invalid/";

fn enabled(name: &str) -> bool {
    if std::env::var("NEBULA_NET_TESTS").as_deref() != Ok("1") {
        println!("skipping {name}: set NEBULA_NET_TESTS=1 to run the network tests");
        return false;
    }
    true
}

/// The installer must never open a window here, whatever the terminal looks
/// like: these tests also run unattended.
fn options() -> ResolveOptions {
    ResolveOptions {
        gui_fallback: false,
        ..ResolveOptions::default()
    }
}

fn java_executable(name: &str) -> Option<PathBuf> {
    match std::env::var_os("JAVA_EXECUTABLE") {
        Some(exe) => Some(PathBuf::from(exe)),
        None => {
            println!("skipping {name}: JAVA_EXECUTABLE is not set");
            None
        }
    }
}

fn root_in(dir: &std::path::Path) -> Root {
    Root::new(dir, Url::parse(BASE_URL).unwrap())
}

fn mc(version: &str) -> MinecraftVersion {
    version.parse().unwrap()
}

/// Every published artifact must be downloadable and verifiable: an https url
/// that maps onto a file we actually wrote, plus both digests the launcher
/// contract asks for.
fn check_artifact(root: &Root, module: &Module) {
    let artifact = &module.artifact;
    assert!(
        artifact.md5.is_some() && artifact.sha256.is_some(),
        "{} is missing a digest: {artifact:?}",
        module.id
    );
    assert!(
        artifact.url.starts_with("https://"),
        "{} has a non-https url: {}",
        module.id,
        artifact.url
    );
    assert!(artifact.size > 0, "{} is empty", module.id);

    let relative = artifact
        .url
        .strip_prefix(BASE_URL)
        .unwrap_or_else(|| panic!("{} is not served from the base url", module.id));
    let local = root.path.join(relative);
    assert!(local.is_file(), "{} was not written", local.display());
    assert_eq!(
        std::fs::metadata(&local).unwrap().len(),
        artifact.size,
        "{} has a stale size",
        local.display()
    );
}

/// Returns the module count (root + sub-modules).
fn check_tree(root: &Root, module: &Module, expected: ModuleType, manifest_id: &str) -> usize {
    assert_eq!(module.module_type, expected, "unexpected root module type");

    let sub_modules = module
        .sub_modules
        .as_ref()
        .expect("the root module must have sub-modules");
    assert_eq!(
        sub_modules[0].module_type,
        ModuleType::VersionManifest,
        "the version manifest must be the first sub-module"
    );
    assert_eq!(sub_modules[0].id, manifest_id);

    for entry in std::iter::once(module).chain(sub_modules) {
        check_artifact(root, entry);
        if entry.id != manifest_id {
            MavenId::parse(&entry.id, "jar").unwrap_or_else(|error| {
                panic!("module id {} is not a maven identifier: {error}", entry.id)
            });
        }
    }

    // The tree has to survive the trip through the launcher's model.
    let json = serde_json::to_string(module).unwrap();
    let parsed: spec::Module = serde_json::from_str(&json).unwrap();
    assert_eq!(&parsed, module);

    1 + sub_modules.len()
}

#[test]
fn fabric_1_20_1_recommended() {
    let name = "fabric_1_20_1_recommended";
    if !enabled(name) {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = root_in(dir.path());
    let minecraft = mc("1.20.1");
    let loader = remote::get_promoted_fabric_version(Promotion::Recommended).unwrap();
    println!("{name}: fabric loader {loader}");

    let started = Instant::now();
    let module = resolver::fabric_resolver(&root, &minecraft, &loader)
        .unwrap()
        .module()
        .unwrap();
    let elapsed = started.elapsed();

    let count = check_tree(
        &root,
        &module,
        ModuleType::Fabric,
        &format!("1.20.1-fabric-{loader}"),
    );
    assert!(module.id.starts_with("net.fabricmc:fabric-loader:"));
    println!("{name}: {count} modules, root {}, {elapsed:?}", module.id);
}

#[test]
fn forge_1_12_2_fg3_without_installer() {
    let name = "forge_1_12_2_fg3_without_installer";
    if !enabled(name) {
        return;
    }
    let Some(exe) = java_executable(name) else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let root = root_in(dir.path());
    let java = Java::new(exe, &root);
    let minecraft = mc("1.12.2");
    // > 14.23.5.2847, so FG3 takes it, and 1.12 has no generated files: the
    // installer is never run.
    let forge_version = "14.23.5.2860";

    let started = Instant::now();
    let module = resolver::forge_resolver(&root, &minecraft, forge_version, &java, options())
        .unwrap()
        .module()
        .unwrap();
    let elapsed = started.elapsed();

    let count = check_tree(
        &root,
        &module,
        ModuleType::ForgeHosted,
        &format!("1.12.2-{forge_version}"),
    );
    assert_eq!(
        module.id,
        format!("net.minecraftforge:forge:1.12.2-{forge_version}:universal")
    );
    // The 4-space manifest upstream writes for this path.
    let manifest = root.version_manifest_path(&format!("1.12.2-forge-{forge_version}"));
    let text = std::fs::read_to_string(&manifest).unwrap();
    assert!(
        text.starts_with("{\n    \""),
        "manifest is not 4-space indented"
    );
    assert!(text.ends_with("}\n"), "manifest has no trailing newline");

    println!("{name}: {count} modules, root {}, {elapsed:?}", module.id);
}

/// FG2 ships plain jars only: the `.pack.xz` files Forge's maven serves are
/// unpacked into `repo/lib` and the packed intermediates are thrown away, so
/// every module id ends in `@jar` and every artifact describes bytes the
/// Hambugui runtime can verify.
#[test]
fn forge_1_7_10_fg2() {
    let name = "forge_1_7_10_fg2";
    if !enabled(name) {
        return;
    }
    let Some(exe) = java_executable(name) else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let root = root_in(dir.path());
    let java = Java::new(exe, &root);
    let minecraft = mc("1.7.10");
    let forge_version = "10.13.4.1614";
    println!("{name}: forge {forge_version} (PackXZExtract unpacks the scala libraries)");

    let started = Instant::now();
    let module = resolver::forge_resolver(&root, &minecraft, forge_version, &java, options())
        .unwrap()
        .module()
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(module.module_type, ModuleType::ForgeHosted);
    assert_eq!(
        module.id,
        "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10:universal"
    );
    // FG2 has no version manifest at all: `version.json` lives inside the
    // universal jar the launcher already downloads.
    let sub_modules = module.sub_modules.as_ref().expect("sub-modules");
    assert!(!sub_modules.is_empty());
    for entry in sub_modules {
        assert!(
            entry.id.ends_with("@jar"),
            "{} is not a plain jar module",
            entry.id
        );
        assert_eq!(entry.module_type, ModuleType::Library);
        MavenId::parse(&entry.id, "jar")
            .unwrap_or_else(|error| panic!("{} is not a maven identifier: {error}", entry.id));
    }

    // Every artifact must describe the bytes on disk, digest for digest.
    for entry in std::iter::once(&module).chain(sub_modules) {
        check_artifact(&root, entry);
        let relative = entry.artifact.url.strip_prefix(BASE_URL).unwrap();
        let local = root.path.join(relative);
        assert!(
            relative.starts_with("repo/lib/") && relative.ends_with(".jar"),
            "{} is not a jar under repo/lib: {relative}",
            entry.id
        );
        let digests = nebula_core::hash::digests_of_file(&local).unwrap();
        assert_eq!(digests.size, entry.artifact.size, "{}", entry.id);
        assert_eq!(
            Some(digests.sha256.as_str()),
            entry.artifact.sha256.as_deref(),
            "{}",
            entry.id
        );
    }
    // Nothing packed is left behind, in `repo/lib` or in `temp/`.
    assert!(!root.temp_dir().exists(), "the temp directory was kept");

    println!(
        "{name}: {} modules, root {}, {elapsed:?}",
        1 + sub_modules.len(),
        module.id
    );
}

#[test]
fn neoforge_1_21_1_recommended() {
    let name = "neoforge_1_21_1_recommended";
    if !enabled(name) {
        return;
    }
    let Some(exe) = java_executable(name) else {
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let root = root_in(dir.path());
    let java = Java::new(exe, &root);
    let minecraft = mc("1.21.1");
    let version =
        remote::get_promoted_neoforge_version(&minecraft, Promotion::Recommended).unwrap();
    println!("{name}: neoforge {version} (headless installer, ~100 MB of downloads)");

    let started = Instant::now();
    let module = resolver::neoforge_resolver(&root, &minecraft, &version, &java, options())
        .unwrap()
        .module()
        .unwrap();
    let elapsed = started.elapsed();

    let count = check_tree(
        &root,
        &module,
        ModuleType::NeoForge,
        &format!("1.21.1-neoforge-{version}"),
    );
    assert_eq!(
        module.id,
        format!("net.neoforged:neoforge:{version}:universal")
    );
    assert_eq!(module.name, "NeoForge");
    // The four installer-generated jars are found by FML under
    // `-DlibraryDirectory`, so they stay off the classpath; the manifest
    // libraries are the classpath and carry no flag at all.
    assert_eq!(module.classpath, Some(false));
    let sub_modules = module.sub_modules.as_ref().unwrap();
    assert_eq!(
        sub_modules
            .iter()
            .filter(|entry| entry.classpath == Some(false))
            .count(),
        3,
        "expected the client, srg and extra jars off the classpath"
    );
    for classifier in ["client", "srg", "extra"] {
        assert!(
            sub_modules
                .iter()
                .any(|entry| entry.id.ends_with(&format!(":{classifier}"))),
            "the {classifier} jar is missing"
        );
    }
    println!("{name}: {count} modules, root {}, {elapsed:?}", module.id);
}
