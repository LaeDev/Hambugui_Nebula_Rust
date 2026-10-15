//! Byte comparison against the ADR-pinned fixture two launchers are tested on.
//!
//! The 15 bytes upstream hashed for `client.cfg` are unknown, so the two digest
//! values are blanked on both sides; every other byte, key order included, must
//! be identical. The generated file is also left in `target/golden-output.json`
//! for the launcher contract harness.

use std::path::{Path, PathBuf};

use nebula_core::config::Config;
use nebula_core::distribution::{GenerateOptions, generate_distribution, init_root};
use nebula_core::layout::Root;
use nebula_core::server::{CreateServerOptions, create_server};
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";
const GOLDEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/launcher/nebula-hardening-generated.json"
));

/// Blanks `"MD5"` and `"SHA256"` values without touching key order, which a
/// `serde_json::Value` round trip would not preserve.
fn blank_digests(json: &str) -> String {
    json.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            for key in ["\"MD5\": ", "\"SHA256\": "] {
                if trimmed.starts_with(key) {
                    let indent = &line[..line.len() - trimmed.len()];
                    let comma = if line.ends_with(',') { "," } else { "" };
                    return format!("{indent}{key}\"\"{comma}");
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_target() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

/// Builds the ADR fixture root in `dir` and generates from it. With
/// `catalog_artefacts`, everything the mod catalog writes is present too.
fn generate(dir: &Path, catalog_artefacts: bool) -> nebula_core::GenerateResult {
    let base_url = Url::parse(BASE).unwrap();
    let root = Root::new(dir, base_url.clone());
    init_root(&root).unwrap();

    let created = create_server(
        &root,
        "Hambugui",
        &"1.20.1".parse().unwrap(),
        &CreateServerOptions::default(),
    )
    .unwrap();

    let config_dir = created.files_dir.join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("client.cfg"), b"client=true\n#42").unwrap();

    if catalog_artefacts {
        let server = root.servers_dir().join("Hambugui-1.20.1");
        std::fs::write(server.join("modindex.json"), br#"{"version":1,"mods":{}}"#).unwrap();
        std::fs::write(server.join(".nebula-lock"), br#"{"pid":1,"started_at":0}"#).unwrap();
        let staging = server.join(".nebula-tmp").join("1234-abcdef");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("half-downloaded.jar"), b"partial").unwrap();
        let icons = root.cache_dir().join("icons");
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("deadbeef.png"), b"icon bytes").unwrap();
    }

    generate_distribution(
        &Config {
            root: dir.to_path_buf(),
            base_url,
            java_executable: None,
            helios_data_folder: None,
            curseforge_api_key: None,
        },
        &GenerateOptions::default(),
    )
    .unwrap()
}

#[test]
fn reproduces_the_golden_distribution() {
    let dir = tempfile::tempdir().unwrap();
    let result = generate(dir.path(), false);

    let generated = std::fs::read_to_string(&result.path).unwrap();
    assert_eq!(result.installed_to, None);
    assert_eq!(
        result.sha256,
        nebula_core::hash::digests_of_bytes(generated.as_bytes()).sha256
    );
    assert!(
        !generated.ends_with('\n'),
        "upstream writes no trailing newline"
    );

    // Kept for the launcher contract harness (Task 8's gate).
    let target = workspace_target();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("golden-output.json"), &generated).unwrap();

    assert_eq!(
        blank_digests(&generated),
        blank_digests(GOLDEN.replace("\r\n", "\n").trim_end())
    );

    // The digests we blanked are still real ones.
    let artifact = &result.distribution.servers[0].modules[0].artifact;
    assert_eq!(artifact.size, 15);
    assert_eq!(artifact.md5.as_ref().unwrap().len(), 32);
    assert_eq!(artifact.sha256.as_ref().unwrap().len(), 64);
}

/// Plan reliability rule 5: nothing the mod catalog writes may reach the
/// distribution. `modindex.json`, `.nebula-lock` and `.nebula-tmp/` sit at the
/// server's top level, which no scanner reads, and `cache/icons` is outside
/// every scanned root — so the bytes must be identical with them present.
#[test]
fn the_catalogs_own_files_do_not_change_the_distribution() {
    let plain = tempfile::tempdir().unwrap();
    let with_catalog = tempfile::tempdir().unwrap();

    let before = generate(plain.path(), false);
    let after = generate(with_catalog.path(), true);

    assert_eq!(
        std::fs::read_to_string(&before.path).unwrap(),
        std::fs::read_to_string(&after.path).unwrap()
    );
    assert_eq!(before.sha256, after.sha256);
}
