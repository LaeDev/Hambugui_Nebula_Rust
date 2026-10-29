//! The signature Nebula writes, checked by the launcher's own rule, plus the
//! key file round trip and the rollback serial.
//!
//! `launcher_verify` below is a line-for-line reimplementation of
//! `hambugui-distribution/src/signature.rs` and
//! `hambugui_domain::ed25519_public_key_from_hex`, kept independent of
//! `nebula_core::signing` so a shared mistake cannot pass both sides.

use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use nebula_core::config::Config;
use nebula_core::distribution::{
    GenerateOptions, generate_distribution, init_root, next_serial, previous_serial,
};
use nebula_core::layout::Root;
use nebula_core::signing::{self, DistributionKey};
use nebula_core::validate::{Severity, validate_value};
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";

// The vector the launcher's signature tests pin (contract C2): seed 0x42 x 32
// signs this body.
const SEED: &str = "4242424242424242424242424242424242424242424242424242424242424242";
const PUBLIC_KEY: &str = "2152f8d19b791d24453242e15f2eab6cb7cffa7b6a5ed30097960e069881db12";
const BODY: &[u8] = br#"{"version":"1.0.0"}"#;
const SIGNATURE: &str = "19dc1751b41cd7da14424e5e1e021cb8b38b5534d7b75a0740816c127a5757f22f2a128e63bb21492b25dfdb5d229fd94105c8995dfb84e5154c000b715e7305";

#[derive(Debug, PartialEq, Eq)]
enum LauncherError {
    Malformed,
    KeyInvalid,
    Mismatch,
}

fn lowercase_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The launcher's `verify_distribution_signature`.
fn launcher_verify(
    body: &[u8],
    signature_file: &[u8],
    public_key_hex: &str,
) -> Result<(), LauncherError> {
    let key = lowercase_hex::<32>(public_key_hex)
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
        .filter(|key| !key.is_weak())
        .ok_or(LauncherError::KeyInvalid)?;
    let text = signature_file.strip_suffix(b"\n").unwrap_or(signature_file);
    let signature = std::str::from_utf8(text)
        .ok()
        .and_then(lowercase_hex::<64>)
        .ok_or(LauncherError::Malformed)?;
    let message = [b"hambugui-distribution-v1\n".as_slice(), body].concat();
    key.verify_strict(&message, &Signature::from_bytes(&signature))
        .map_err(|_| LauncherError::Mismatch)
}

fn key() -> DistributionKey {
    DistributionKey::from_seed_hex(SEED, "test").unwrap()
}

fn config(root: &Path, helios: Option<&Path>) -> Config {
    Config {
        root: root.to_path_buf(),
        base_url: Url::parse(BASE).unwrap(),
        java_executable: None,
        helios_data_folder: helios.map(Path::to_path_buf),
        curseforge_api_key: None,
    }
}

fn empty_root(dir: &Path) {
    init_root(&Root::new(dir, Url::parse(BASE).unwrap())).unwrap();
}

#[test]
fn nebula_signs_the_launchers_shared_vector() {
    assert_eq!(signing::SIGNATURE_DOMAIN, b"hambugui-distribution-v1\n");
    assert_eq!(key().public_key_hex(), PUBLIC_KEY);
    assert_eq!(key().sign(BODY), SIGNATURE);
    assert_eq!(
        launcher_verify(BODY, SIGNATURE.as_bytes(), PUBLIC_KEY),
        Ok(())
    );
}

#[test]
fn a_generated_distribution_verifies_under_the_launchers_rule() {
    let root = tempfile::tempdir().unwrap();
    let helios = tempfile::tempdir().unwrap();
    empty_root(root.path());

    let result = generate_distribution(
        &config(root.path(), Some(helios.path())),
        &GenerateOptions {
            install_local: true,
            signing_key: Some(key()),
            ..Default::default()
        },
    )
    .unwrap();

    let signature_path = result.signature.clone().unwrap();
    assert_eq!(signature_path, root.path().join("distribution.json.sig"));
    for document in [result.path.clone(), result.installed_to.clone().unwrap()] {
        let body = std::fs::read(&document).unwrap();
        let signature = std::fs::read(signing::signature_path(&document)).unwrap();
        // 128 lowercase hex characters and one newline.
        assert_eq!(signature.len(), 129);
        assert_eq!(signature.last(), Some(&b'\n'));
        assert_eq!(launcher_verify(&body, &signature, PUBLIC_KEY), Ok(()));

        // The serial is part of what is signed.
        let text = String::from_utf8(body).unwrap();
        let serial = result.distribution.serial.unwrap();
        let older = text.replace(
            &format!("\"serial\": {serial}"),
            &format!("\"serial\": {}", serial - 1),
        );
        assert_ne!(older, text);
        assert_eq!(
            launcher_verify(older.as_bytes(), &signature, PUBLIC_KEY),
            Err(LauncherError::Mismatch)
        );
    }

    // Another key does not verify it.
    let other = DistributionKey::from_seed_hex(&"43".repeat(32), "other").unwrap();
    let body = std::fs::read(&result.path).unwrap();
    let signature = std::fs::read(&signature_path).unwrap();
    assert_eq!(
        launcher_verify(&body, &signature, &other.public_key_hex()),
        Err(LauncherError::Mismatch)
    );
}

#[test]
fn an_unsigned_generate_removes_a_stale_signature() {
    let root = tempfile::tempdir().unwrap();
    empty_root(root.path());
    let cfg = config(root.path(), None);

    let signed = generate_distribution(
        &cfg,
        &GenerateOptions {
            signing_key: Some(key()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(signed.signature.unwrap().is_file());

    let unsigned = generate_distribution(&cfg, &GenerateOptions::default()).unwrap();
    assert_eq!(unsigned.signature, None);
    assert!(!root.path().join("distribution.json.sig").exists());
}

#[test]
fn keygen_and_public_key_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys").join(signing::DEFAULT_KEY_FILE_NAME);

    let created = signing::write_new_key_file(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.len(), 65);
    assert!(text.ends_with('\n'));
    assert!(
        text[..64]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }

    // The public key printed at keygen is the one read back from the file, and
    // the one the launcher accepts as HAMBUGUI_DISTRIBUTION_PUBLIC_KEY.
    let public = created.public_key_hex();
    assert!(lowercase_hex::<32>(&public).is_some());
    let loaded = DistributionKey::from_file(&path).unwrap();
    assert_eq!(loaded.public_key_hex(), public);
    let resolved = signing::resolve_key(Some(&path), None).unwrap().unwrap();
    assert_eq!(resolved.key.public_key_hex(), public);
    assert!(resolved.source.contains("distribution-signing.key"));
    assert_eq!(
        launcher_verify(BODY, format!("{}\n", loaded.sign(BODY)).as_bytes(), &public),
        Ok(())
    );

    // Never overwritten.
    let error = signing::write_new_key_file(&path).unwrap_err().to_string();
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    // Two keygens make two keys.
    let second = signing::write_new_key_file(&dir.path().join("second.key")).unwrap();
    assert_ne!(second.public_key_hex(), public);
}

#[test]
fn a_named_key_file_that_is_missing_or_wrong_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.key");
    assert!(signing::resolve_key(Some(&missing), None).is_err());

    let zero = dir.path().join("zero.key");
    std::fs::write(&zero, format!("{}\n", "0".repeat(64))).unwrap();
    let error = signing::resolve_key(Some(&zero), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("all-zero"), "{error}");

    let big = dir.path().join("big.key");
    std::fs::write(&big, "a".repeat(4096)).unwrap();
    assert!(DistributionKey::from_file(&big).is_err());
}

#[test]
fn the_serial_strictly_increases() {
    // The clock wins when it is ahead; otherwise previous + 1.
    assert_eq!(next_serial(None, 1_761_730_000).unwrap(), 1_761_730_000);
    assert_eq!(next_serial(Some(5), 1_761_730_000).unwrap(), 1_761_730_000);
    assert_eq!(
        next_serial(Some(1_761_730_000), 1_761_730_000).unwrap(),
        1_761_730_001
    );
    assert_eq!(
        next_serial(Some(1_761_739_999), 1_761_730_000).unwrap(),
        1_761_740_000
    );
    assert!(next_serial(Some(u64::MAX), 0).is_err());

    let root = tempfile::tempdir().unwrap();
    empty_root(root.path());
    let cfg = config(root.path(), None);
    let path = root.path().join("distribution.json");
    assert_eq!(previous_serial(&path), None);

    // Several runs inside one second still each get a larger serial.
    let mut last = 0;
    for _ in 0..3 {
        let result = generate_distribution(&cfg, &GenerateOptions::default()).unwrap();
        let serial = result.distribution.serial.unwrap();
        assert!(serial > last, "{serial} after {last}");
        assert_eq!(previous_serial(&path), Some(serial));
        last = serial;
    }

    // A serial ahead of the clock (a clock set back since) is still exceeded.
    let ahead = last + 1_000_000;
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace(&last.to_string(), &ahead.to_string())).unwrap();
    let result = generate_distribution(&cfg, &GenerateOptions::default()).unwrap();
    assert_eq!(result.distribution.serial, Some(ahead + 1));
}

#[test]
fn the_validator_accepts_a_serial_and_rejects_a_wrong_type() {
    let with = |serial: serde_json::Value| {
        let mut value = serde_json::json!({ "version": "1.0.0", "servers": [] });
        value["serial"] = serial;
        validate_value(&value, true)
    };
    for good in [
        serde_json::json!(0),
        serde_json::json!(1_761_730_000u64),
        serde_json::json!(u64::MAX),
    ] {
        let report = with(good.clone());
        assert!(report.is_ok(), "{good}: {:?}", report.findings);
        assert!(report.findings.is_empty(), "{good}: {:?}", report.findings);
    }
    for bad in [
        serde_json::json!("1761730000"),
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(null),
        serde_json::json!([1]),
        serde_json::json!({ "n": 1 }),
    ] {
        let report = with(bad.clone());
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{bad}: {:?}", report.findings);
        assert_eq!(errors[0].location, "serial");
    }
    // No serial at all is still a valid (Helios-style) distribution.
    let plain = serde_json::json!({ "version": "1.0.0", "servers": [] });
    assert!(validate_value(&plain, true).findings.is_empty());
}
