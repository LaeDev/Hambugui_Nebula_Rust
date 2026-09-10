//! End-to-end generation from an empty root, offline: no loader, so no JDK and
//! no network are involved.

use std::path::Path;

use nebula_core::config::Config;
use nebula_core::distribution::{GenerateOptions, generate_distribution, init_root};
use nebula_core::layout::Root;
use nebula_core::meta::LoaderMeta;
use nebula_core::server::{
    CreateServerOptions, create_server, read_server_meta, write_server_meta,
};
use nebula_core::spec::{Distribution, ModuleType};
use nebula_core::validate;
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";

fn config(root: &Path) -> Config {
    Config {
        root: root.to_path_buf(),
        base_url: Url::parse(BASE).unwrap(),
        java_executable: None,
        helios_data_folder: None,
    }
}

#[test]
fn generates_a_loaderless_server() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
    init_root(&root).unwrap();

    let created = create_server(
        &root,
        "Test",
        &"1.20.1".parse().unwrap(),
        &CreateServerOptions::default(),
    )
    .unwrap();
    assert!(created.mod_container.is_none());

    std::fs::create_dir_all(created.files_dir.join("config")).unwrap();
    std::fs::write(
        created.files_dir.join("config/client.cfg"),
        b"client=true\n#42",
    )
    .unwrap();
    std::fs::write(
        created.dir.libraries_dir().join("foo-1.0.jar"),
        b"not really a jar",
    )
    .unwrap();

    let helios = tempfile::tempdir().unwrap();
    let result = generate_distribution(
        &Config {
            helios_data_folder: Some(helios.path().to_path_buf()),
            ..config(dir.path())
        },
        &GenerateOptions {
            name: "distribution".into(),
            install_local: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.path, dir.path().join("distribution.json"));
    assert_eq!(
        result.installed_to,
        Some(helios.path().join("distribution.json"))
    );
    let written = std::fs::read_to_string(&result.path).unwrap();
    // Kept for the launcher contract harness: unlike the golden fixture this
    // one carries a pathless Maven `Library` id for the launcher to resolve.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("generated-library.json"), &written).unwrap();
    assert_eq!(
        written,
        std::fs::read_to_string(result.installed_to.as_ref().unwrap()).unwrap()
    );

    // Round trips through the emitter model unchanged.
    let parsed: Distribution = serde_json::from_str(&written).unwrap();
    assert_eq!(parsed, result.distribution);
    assert_eq!(parsed.version, "1.0.0");

    let server = &parsed.servers[0];
    assert_eq!(server.id, "Test-1.20.1");
    assert_eq!(server.minecraft_version, "1.20.1");
    assert_eq!(server.name, "Test (Minecraft 1.20.1)");
    assert_eq!(server.icon, None);

    // Libraries are emitted before files.
    assert_eq!(server.modules.len(), 2);
    let library = &server.modules[0];
    assert_eq!(library.module_type, ModuleType::Library);
    assert_eq!(library.id, "generated.library:foo:1.0@jar");
    assert_eq!(library.name, "foo");
    assert_eq!(library.artifact.path, None);
    assert_eq!(
        library.artifact.url,
        "https://cdn.example.invalid/servers/Test-1.20.1/libraries/foo-1.0.jar"
    );

    let file = &server.modules[1];
    assert_eq!(file.module_type, ModuleType::File);
    assert_eq!(file.id, "client.cfg");
    assert_eq!(file.artifact.path.as_deref(), Some("config/client.cfg"));
    assert_eq!(
        file.artifact.url,
        "https://cdn.example.invalid/servers/Test-1.20.1/files/config/client.cfg"
    );
    assert_eq!(file.artifact.size, 15);

    for module in &server.modules {
        assert!(module.artifact.md5.is_some(), "{}", module.id);
        assert!(module.artifact.sha256.is_some(), "{}", module.id);
    }

    let report = validate::validate(&parsed, true).unwrap();
    assert_eq!(report.errors(), 0, "{:?}", report.findings);
    assert_eq!(report.artifacts, 2);
    assert_eq!(report.sha256, 2);
}

#[test]
fn refuses_a_neoforge_server_without_a_java_executable() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
    init_root(&root).unwrap();

    let created = create_server(
        &root,
        "Neo",
        &"1.21.1".parse().unwrap(),
        &CreateServerOptions {
            neoforge_version: Some("21.1.77".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut meta = read_server_meta(&created.dir).unwrap();
    assert_eq!(
        meta.neoforge,
        Some(LoaderMeta {
            version: "21.1.77".into()
        })
    );

    let error =
        generate_distribution(&config(dir.path()), &GenerateOptions::default()).unwrap_err();
    assert!(error.to_string().contains("JAVA_EXECUTABLE"), "{error}");
    assert!(error.to_string().contains("NeoForge"), "{error}");
    assert!(error.to_string().contains("Neo-1.21.1"), "{error}");

    // Forge reports the same way; Fabric needs no JDK and is left to the gated
    // network tests, which are the only place a loader is actually resolved.
    meta.neoforge = None;
    meta.forge = Some(LoaderMeta {
        version: "47.2.0".into(),
    });
    write_server_meta(&created.dir, &meta).unwrap();
    let error =
        generate_distribution(&config(dir.path()), &GenerateOptions::default()).unwrap_err();
    assert!(error.to_string().contains("JAVA_EXECUTABLE"), "{error}");
    assert!(error.to_string().contains("Forge"), "{error}");
}
