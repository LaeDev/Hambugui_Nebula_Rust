//! Module scanner tests. Synthetic jars are built with the `zip` crate; the
//! Forge/NeoForge metadata pipeline is exercised through its pure parsers so
//! Claritas (and therefore a JDK) is only needed by the one gated test.

use std::fs;
use std::io::Write;
use std::path::Path;

use nebula_core::java::Java;
use nebula_core::layout::{Root, ServerDir};
use nebula_core::meta::UntrackedFilesOption;
use nebula_core::scan::{self, ForgeFlavour, HashPolicy, ModLoader, Namespace, ScanContext};
use nebula_core::spec::{ModuleType, Required};
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";
const DIR: &str = "Test-1.20.1";

const MODS_TOML: &str = r#"
modLoader = "javafml"
loaderVersion = "[47,)"
license = "MIT"

[[mods]]
modId = "jei"
version = "${file.jarVersion}"
displayName = "Just Enough Items"
description = "Item and recipe viewing."
"#;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    _root: tempfile::TempDir,
    server: ServerDir,
}

fn fixture() -> Fixture {
    let root_dir = tempfile::tempdir().unwrap();
    let root = Root::new(root_dir.path(), Url::parse(BASE).unwrap());
    let server = ServerDir::new(root, DIR);
    fs::create_dir_all(&server.path).unwrap();
    Fixture {
        _root: root_dir,
        server,
    }
}

fn context<'a>(
    server: &'a ServerDir,
    untracked: &'a [UntrackedFilesOption],
    policy: HashPolicy,
) -> ScanContext<'a> {
    ScanContext {
        server,
        mc: "1.20.1".parse().unwrap(),
        java: None,
        untracked,
        policy,
    }
}

fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn fabric_jar(path: &Path, json: &str) {
    write_jar(path, &[("fabric.mod.json", json.as_bytes())]);
}

fn untracked_option(applies_to: &str, pattern: &str) -> UntrackedFilesOption {
    UntrackedFilesOption {
        applies_to: vec![applies_to.to_owned()],
        patterns: vec![pattern.to_owned()],
    }
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[test]
fn crude_inference_reproduces_the_unanchored_upstream_regex() {
    let crude = |name: &str| {
        let (n, v) = scan::crude_inference(name);
        (n, v)
    };
    assert_eq!(crude("foo-1.0.jar"), ("foo".to_owned(), "1.0".to_owned()));
    // Unanchored: the trailing `.bak` does not stop the match.
    assert_eq!(
        crude("foo-1.0.jar.bak"),
        ("foo".to_owned(), "1.0".to_owned())
    );
    assert_eq!(crude("foo.jar"), ("foo".to_owned(), "0.0.0".to_owned()));
    // Group 1 is lazy (first dash), group 2 greedy (last `jar`).
    assert_eq!(
        crude("a-b-1.2.3.JAR"),
        ("a".to_owned(), "b-1.2.3".to_owned())
    );
    assert_eq!(
        crude("jei-1.20.1-11.6.0.1024.jar"),
        ("jei".to_owned(), "1.20.1-11.6.0.1024".to_owned())
    );
    // No dot at all: upstream `substring(0, -1)` yields an empty stem.
    assert_eq!(crude("plain"), (String::new(), "0.0.0".to_owned()));
}

#[test]
fn builds_names_and_maven_identifiers_like_upstream() {
    assert_eq!(scan::capitalize("just enough items"), "Just enough items");
    assert_eq!(scan::capitalize(""), "");
    assert_eq!(scan::capitalize("Ábc"), "Ábc");
    assert_eq!(
        scan::maven_module_id("generated.library", "foo", "1.0", ModuleType::Library),
        "generated.library:foo:1.0@jar"
    );
    assert_eq!(
        scan::maven_module_id("g", "a", "v", ModuleType::VersionManifest),
        "g:a:v@json"
    );
    assert_eq!(
        scan::maven_module_id("g", "a", "v", ModuleType::File),
        "g:a:v"
    );
}

#[test]
fn discovery_is_sorted_and_skips_blacklisted_names() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["b.jar", "a.jar", ".gitkeep"] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    fs::create_dir(dir.path().join("subdir")).unwrap();
    let candidates = scan::discover(dir.path()).unwrap();
    let names: Vec<&str> = candidates.iter().map(|c| c.file_name.as_str()).collect();
    assert_eq!(names, ["a.jar", "b.jar"]);
    assert_eq!(candidates[0].size, 1);
    // A missing directory is not an error.
    assert!(scan::discover(&dir.path().join("nope")).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// files / libraries
// ---------------------------------------------------------------------------

#[test]
fn scans_nested_files_with_clean_relative_paths() {
    let fx = fixture();
    let files = fx.server.files_dir();
    fs::create_dir_all(files.join("config")).unwrap();
    fs::write(files.join("config").join("a.cfg"), b"hello world 123").unwrap();
    fs::write(files.join(".gitkeep"), b"").unwrap();

    let modules = scan::scan_files(&context(&fx.server, &[], HashPolicy::default())).unwrap();
    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.id, "a.cfg");
    assert_eq!(module.name, "a.cfg");
    assert_eq!(module.module_type, ModuleType::File);
    assert_eq!(module.artifact.size, 15);
    assert_eq!(module.artifact.path.as_deref(), Some("config/a.cfg"));
    assert_eq!(
        module.artifact.url,
        "https://cdn.example.invalid/servers/Test-1.20.1/files/config/a.cfg"
    );
    assert!(module.artifact.md5.is_some());
    assert!(module.artifact.sha256.is_some());
    assert_eq!(module.untracked, None);
    assert_eq!(module.required, None);
}

#[test]
fn rejects_symlinks_in_the_files_structure() {
    let fx = fixture();
    let files = fx.server.files_dir();
    fs::create_dir_all(&files).unwrap();
    let target = fx.server.path.join("outside.cfg");
    fs::write(&target, b"payload").unwrap();

    if let Err(error) = symlink_file(&target, &files.join("link.cfg")) {
        println!("skipping: this platform refuses symlink creation ({error})");
        return;
    }

    let error = scan::scan_files(&context(&fx.server, &[], HashPolicy::default())).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Unsafe symbolic link in files structure"),
        "{error}"
    );
}

#[test]
fn scans_libraries_with_crude_maven_ids() {
    let fx = fixture();
    let libraries = fx.server.libraries_dir();
    fs::create_dir_all(&libraries).unwrap();
    fs::write(libraries.join("foo-1.0.jar"), b"lib").unwrap();
    fs::write(libraries.join("bar-2.0.JAR"), b"lib").unwrap();
    fs::write(libraries.join("notes.txt"), b"ignored").unwrap();

    let modules = scan::scan_libraries(&context(&fx.server, &[], HashPolicy::default())).unwrap();
    let ids: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "generated.library:bar:2.0@jar",
            "generated.library:foo:1.0@jar"
        ]
    );
    assert_eq!(modules[1].name, "foo");
    assert_eq!(modules[1].module_type, ModuleType::Library);
    assert_eq!(modules[1].artifact.path, None);
    assert_eq!(
        modules[1].artifact.url,
        "https://cdn.example.invalid/servers/Test-1.20.1/libraries/foo-1.0.jar"
    );
}

// ---------------------------------------------------------------------------
// Untracked files
// ---------------------------------------------------------------------------

#[test]
fn untracked_patterns_control_digests_per_policy() {
    for helios_untracked in [false, true] {
        let fx = fixture();
        let container = fx.server.fabricmods_dir();
        scan::init_toggleable(&container).unwrap();
        fabric_jar(
            &container.join("required").join("tracked-1.0.jar"),
            r#"{"id":"tracked","version":"1.0"}"#,
        );
        fabric_jar(
            &container.join("optionalon").join("loose-1.0.jar"),
            r#"{"id":"loose","version":"1.0"}"#,
        );

        let options = [untracked_option("fabricmods", "optionalon/*.jar")];
        let modules = scan::scan_fabric_mods(&context(
            &fx.server,
            &options,
            HashPolicy { helios_untracked },
        ))
        .unwrap();
        assert_eq!(modules.len(), 2);

        let loose = modules.iter().find(|m| m.id.contains("loose")).unwrap();
        let tracked = modules.iter().find(|m| m.id.contains("tracked")).unwrap();

        assert_eq!(loose.untracked, Some(true));
        assert_eq!(tracked.untracked, None);
        assert!(tracked.artifact.md5.is_some());
        assert!(tracked.artifact.sha256.is_some());
        assert_eq!(loose.artifact.md5.is_some(), !helios_untracked);
        assert_eq!(loose.artifact.sha256.is_some(), !helios_untracked);
        assert!(loose.artifact.size > 0);
    }
}

#[test]
fn untracked_globs_do_not_cross_directory_separators() {
    let fx = fixture();
    let files = fx.server.files_dir();
    fs::create_dir_all(files.join("config")).unwrap();
    fs::write(files.join("top.cfg"), b"top").unwrap();
    fs::write(files.join("config").join("nested.cfg"), b"nested").unwrap();

    let options = [untracked_option("files", "*.cfg")];
    let modules = scan::scan_files(&context(
        &fx.server,
        &options,
        HashPolicy {
            helios_untracked: true,
        },
    ))
    .unwrap();

    let nested = modules.iter().find(|m| m.id == "nested.cfg").unwrap();
    let top = modules.iter().find(|m| m.id == "top.cfg").unwrap();
    assert_eq!(top.untracked, Some(true));
    assert!(top.artifact.md5.is_none());
    assert_eq!(nested.untracked, None);
    assert!(nested.artifact.sha256.is_some());
}

// ---------------------------------------------------------------------------
// Toggleable namespaces
// ---------------------------------------------------------------------------

#[test]
fn toggleable_namespaces_map_to_required_and_sort_case_insensitively() {
    assert_eq!(Namespace::Required.required(), None);
    assert_eq!(
        Namespace::OptionalOn.required(),
        Some(Required {
            value: false,
            def: None
        })
    );
    assert_eq!(
        Namespace::OptionalOff.required(),
        Some(Required {
            value: false,
            def: Some(false)
        })
    );

    let fx = fixture();
    let container = fx.server.fabricmods_dir();
    scan::init_toggleable(&container).unwrap();
    for namespace in scan::NAMESPACES {
        assert!(container.join(namespace.dir_name()).is_dir());
    }
    fabric_jar(
        &container.join("required").join("Beta-1.0.jar"),
        r#"{"id":"beta","version":"1.0"}"#,
    );
    fabric_jar(
        &container.join("optionalon").join("alpha-1.0.jar"),
        r#"{"id":"alpha","version":"1.0"}"#,
    );
    fabric_jar(
        &container.join("optionaloff").join("Gamma-1.0.jar"),
        r#"{"id":"gamma","version":"1.0"}"#,
    );

    let modules = scan::scan_fabric_mods(&context(&fx.server, &[], HashPolicy::default())).unwrap();
    let order: Vec<&str> = modules.iter().map(scan::url_file_name).collect();
    assert_eq!(order, ["alpha-1.0.jar", "Beta-1.0.jar", "Gamma-1.0.jar"]);

    assert_eq!(modules[0].required, Namespace::OptionalOn.required());
    assert_eq!(modules[1].required, None);
    assert_eq!(modules[2].required, Namespace::OptionalOff.required());
    assert_eq!(
        modules[1].artifact.url,
        "https://cdn.example.invalid/servers/Test-1.20.1/fabricmods/required/Beta-1.0.jar"
    );
    assert_eq!(modules[1].artifact.path, None);
}

// ---------------------------------------------------------------------------
// Fabric mods
// ---------------------------------------------------------------------------

#[test]
fn infers_fabric_groups_from_entrypoints() {
    assert_eq!(
        scan::infer_group("net.foo.bar.mymod.client.MyModClient", "mymod"),
        "net.foo.bar"
    );
    assert_eq!(
        scan::infer_group("com.example.mymod.MyMod", "mymod"),
        "com.example"
    );
    // A two-segment class name leaves nothing to infer.
    assert_eq!(scan::infer_group("MyMod", "mymod"), "");
}

#[test]
fn scans_fabric_mods_with_entrypoint_groups_and_fallbacks() {
    let fx = fixture();
    let container = fx.server.fabricmods_dir();
    scan::init_toggleable(&container).unwrap();
    let required = container.join("required");

    fabric_jar(
        &required.join("a-client-1.0.jar"),
        r#"{"id":"mymod","version":"1.0.0","name":"My Mod","entrypoints":{"client":["net.foo.bar.mymod.client.MyModClient"]}}"#,
    );
    fabric_jar(
        &required.join("b-adapted-2.0.jar"),
        r#"{"id":"mymod","version":"2.0.0","entrypoints":{"main":[{"adapter":"kotlin","value":"com.example.mymod.MyMod"}],"client":["zz.zz.zz.Client"]}}"#,
    );
    // No fabric.mod.json at all -> crude inference.
    write_jar(&required.join("broken-3.0.jar"), &[("README", b"nope")]);
    // Empty inferred group falls back to the default group.
    fabric_jar(
        &required.join("d-flat-4.0.jar"),
        r#"{"id":"flat","version":"4.0.0","entrypoints":{"main":["FlatMod"]}}"#,
    );

    let modules = scan::scan_fabric_mods(&context(&fx.server, &[], HashPolicy::default())).unwrap();
    let ids: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "net.foo.bar:mymod:1.0.0@jar",
            // `main` wins over `client`.
            "com.example:mymod:2.0.0@jar",
            "generated.fabricmod:broken:3.0@jar",
            "generated.fabricmod:flat:4.0.0@jar",
        ]
    );
    assert_eq!(modules[0].name, "My Mod");
    // capitalize(name || id)
    assert_eq!(modules[1].name, "Mymod");
    assert_eq!(modules[2].name, "Broken");
    assert_eq!(modules[0].module_type, ModuleType::FabricMod);
}

// ---------------------------------------------------------------------------
// Forge / NeoForge metadata
//
// The metadata parsers themselves are `pub(crate)` and tested inline in
// `src/scan/forgemod.rs`; what is left here goes through the public scanner.
// ---------------------------------------------------------------------------

#[test]
fn selects_the_forge_mod_flavour_by_minecraft_version() {
    let flavour = |version: &str| scan::forge_flavour_for(&version.parse().unwrap());
    assert_eq!(flavour("1.7.10").unwrap(), ForgeFlavour::Legacy17);
    assert_eq!(flavour("1.12.2").unwrap(), ForgeFlavour::Legacy17);
    assert_eq!(flavour("1.13").unwrap(), ForgeFlavour::Modern113);
    assert_eq!(flavour("1.21.1").unwrap(), ForgeFlavour::Modern113);
    assert!(
        flavour("1.6.4")
            .unwrap_err()
            .to_string()
            .contains("No forge mod structure found for Minecraft 1.6.4!")
    );
    // Year-based Minecraft has no minor range, which is why NeoForge callers
    // pass the flavour instead of deriving it.
    assert!(flavour("26.2").is_err());
}

// ---------------------------------------------------------------------------
// Forge scanning
// ---------------------------------------------------------------------------

#[test]
fn forge_scanning_requires_java_only_when_there_are_candidates() {
    let empty = fixture();
    scan::init_toggleable(&empty.server.forgemods_dir()).unwrap();
    assert!(
        scan::scan_forge_mods(
            &context(&empty.server, &[], HashPolicy::default()),
            ModLoader::Forge,
            ForgeFlavour::Modern113,
        )
        .unwrap()
        .is_empty()
    );

    let fx = fixture();
    let container = fx.server.forgemods_dir();
    scan::init_toggleable(&container).unwrap();
    write_jar(
        &container.join("required").join("jei-1.0.jar"),
        &[("META-INF/mods.toml", MODS_TOML.as_bytes())],
    );
    let error = scan::scan_forge_mods(
        &context(&fx.server, &[], HashPolicy::default()),
        ModLoader::Forge,
        ForgeFlavour::Modern113,
    )
    .unwrap_err();
    assert!(matches!(error, nebula_core::Error::Config(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "JAVA_EXECUTABLE is required to analyse Forge mods"
    );
}

/// OptiFine is the one candidate Claritas never sees: its metadata comes from
/// `changelog.txt` and a fixed group, so a container holding only OptiFine
/// resolves without a JDK.
#[test]
fn optifine_only_containers_need_no_java() {
    let fx = fixture();
    let container = fx.server.forgemods_dir();
    scan::init_toggleable(&container).unwrap();
    write_jar(
        &container
            .join("required")
            .join("OptiFine_1.20.1_HD_U_I6.jar"),
        &[("changelog.txt", b"OptiFine 1.20.1_HD_U_I6\nbody\n")],
    );

    let modules = scan::scan_forge_mods(
        &context(&fx.server, &[], HashPolicy::default()),
        ModLoader::Forge,
        ForgeFlavour::Modern113,
    )
    .unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].id, "net.optifine:optifine:1.20.1_HD_U_I6@jar");
    assert_eq!(modules[0].name, "OptiFine");
    assert!(modules[0].artifact.sha256.is_some());
}

#[test]
fn scans_forge_mods_end_to_end_with_claritas() {
    let Ok(exe) = std::env::var("JAVA_EXECUTABLE") else {
        println!("skipping: JAVA_EXECUTABLE is not set, so Claritas cannot run");
        return;
    };

    let fx = fixture();
    let container = fx.server.forgemods_dir();
    scan::init_toggleable(&container).unwrap();
    let jar_name = "jei-1.20.1-11.6.0.1024.jar";
    let jar = container.join("required").join(jar_name);
    write_jar(
        &jar,
        &[
            ("META-INF/mods.toml", MODS_TOML.as_bytes()),
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nImplementation-Version: 11.6.0.1024\r\n\r\n",
            ),
        ],
    );

    let java = Java::new(exe, &fx.server.root);
    let ctx = ScanContext {
        server: &fx.server,
        mc: "1.20.1".parse().unwrap(),
        java: Some(&java),
        untracked: &[],
        policy: HashPolicy::default(),
    };
    let modules = scan::scan_forge_mods(&ctx, ModLoader::Forge, ForgeFlavour::Modern113).unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].id, "generated.forgemod:jei:11.6.0.1024@jar");
    assert_eq!(modules[0].module_type, ModuleType::ForgeMod);
    assert!(modules[0].artifact.sha256.is_some());
    assert!(!fx.server.path.join("claritasWork").exists());

    // Claritas echoes the jar paths back as object keys; if they stop matching
    // every module silently falls back to the default group.
    let absolute = std::path::absolute(&jar).unwrap();
    let result = java
        .run_claritas(
            &fx.server.path.join("claritasWork"),
            &"1.20.1".parse().unwrap(),
            std::slice::from_ref(&absolute),
        )
        .unwrap();
    if let Some(result) = result
        && !result.is_empty()
    {
        assert!(
            result.contains_key(&absolute),
            "Claritas keys did not round-trip: {:?}",
            result.keys().collect::<Vec<_>>()
        );
    }
}
