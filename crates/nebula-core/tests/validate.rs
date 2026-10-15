//! Validation against the launcher fixtures copied out of Hambugui Launcher.
//!
//! The fixtures are what the launcher's own parser tests use, so a disagreement
//! here is a disagreement with the launcher.

use nebula_core::spec::{Artifact, Distribution, Module, ModuleType, Server};
use nebula_core::validate::{Report, Severity, validate, validate_json};

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/launcher")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

fn check(name: &str, strict: bool) -> Report {
    validate_json(&fixture(name), strict).unwrap()
}

fn messages(report: &Report, severity: Severity) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|finding| finding.severity == severity)
        .map(|finding| format!("{} {}", finding.location, finding.message))
        .collect()
}

#[test]
fn accepts_the_valid_fixture_with_digest_warnings() {
    let report = check("nebula-valid.json", false);
    assert_eq!(
        report.errors(),
        0,
        "{:?}",
        messages(&report, Severity::Error)
    );
    assert_eq!(
        report.warnings(),
        3,
        "{:?}",
        messages(&report, Severity::Warning)
    );
    assert!(report.is_ok());
    assert_eq!(report.artifacts, 3);
    assert_eq!(report.sha256, 2);

    // The MD5-only library is the one that turns into an error under --strict.
    let strict = check("nebula-valid.json", true);
    assert_eq!(strict.errors(), 1);
    assert!(!strict.is_ok());
}

#[test]
fn rejects_an_escaping_artifact_path() {
    let report = check("nebula-invalid-path.json", false);
    let errors = messages(&report, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("path"), "{errors:?}");
    assert!(
        errors[0].starts_with("servers[0].modules[0].artifact.path "),
        "{errors:?}"
    );
}

#[test]
fn rejects_an_unknown_module_type() {
    let report = check("nebula-invalid-module.json", false);
    let errors = messages(&report, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("ShaderPack"), "{errors:?}");
}

#[test]
fn rejects_an_artifact_without_any_digest() {
    let report = check("nebula-invalid-hash.json", false);
    let errors = messages(&report, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("neither MD5 nor SHA256"), "{errors:?}");
    assert_eq!(report.sha256, 0);
}

#[test]
fn accepts_generated_nebula_output_cleanly() {
    let report = check("nebula-hardening-generated.json", false);
    assert_eq!(
        report.errors(),
        0,
        "{:?}",
        messages(&report, Severity::Error)
    );
    assert_eq!(
        report.warnings(),
        0,
        "{:?}",
        messages(&report, Severity::Warning)
    );
    assert_eq!(report.artifacts, 1);
    assert_eq!(report.sha256, 1);
    assert!(check("nebula-hardening-generated.json", true).is_ok());
}

#[test]
fn reports_the_legacy_sample_without_panicking() {
    let report = check("legacy-sample.json", false);
    assert!(report.errors() > 0);
    assert!(report.artifacts > 0);
    // Every legacy artifact predates SHA-256.
    assert_eq!(report.sha256, 0);
    assert!(check("legacy-sample.json", true).errors() >= report.errors());
}

#[test]
fn warns_once_per_server_about_neoforge_modules() {
    let module = |id: &str, module_type| Module {
        id: id.into(),
        name: "NeoForge".into(),
        module_type,
        classpath: None,
        required: None,
        untracked: None,
        artifact: Artifact {
            size: 1,
            url: "https://cdn.example.invalid/a.jar".into(),
            md5: Some("00112233445566778899aabbccddeeff".into()),
            sha256: Some("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".into()),
            path: None,
        },
        sub_modules: None,
    };

    let distribution = Distribution {
        version: "1.0.0".into(),
        rss: "https://example.invalid/rss".into(),
        discord: None,
        servers: vec![Server {
            id: "Neo-1.21.1".into(),
            name: "Neo".into(),
            description: "Neo".into(),
            icon: None,
            version: "1.0.0".into(),
            address: "localhost:25565".into(),
            minecraft_version: "1.21.1".into(),
            discord: None,
            main_server: false,
            autoconnect: false,
            java_options: None,
            modules: vec![Module {
                sub_modules: Some(vec![module(
                    "generated.neoforgemod:jei:1.0@jar",
                    ModuleType::NeoForgeMod,
                )]),
                ..module(
                    "net.neoforged:neoforge:21.1.77:universal@jar",
                    ModuleType::NeoForge,
                )
            }],
        }],
    };

    let report = validate(&distribution, false).unwrap();
    assert_eq!(
        report.errors(),
        0,
        "{:?}",
        messages(&report, Severity::Error)
    );
    assert_eq!(
        report.warnings(),
        1,
        "{:?}",
        messages(&report, Severity::Warning)
    );
    assert_eq!(report.findings[0].location, "servers[0]");
    assert!(report.findings[0].message.contains("NeoForge"));
    assert_eq!(report.artifacts, 2);
    assert_eq!(report.sha256, 2);
}
