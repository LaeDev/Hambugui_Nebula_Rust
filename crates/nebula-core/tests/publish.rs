//! Publishing: plan extraction, change tracking, URL rewriting, and a WebDAV
//! upload driven against an in-process stub server.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::{stub_created, summaries};
use nebula_core::hash::Digests;
use nebula_core::layout::Root;
use nebula_core::publish::webdav::WebDavTarget;
use nebula_core::publish::{
    PublishOptions, PublishState, Target, gdrive, plan_from_distribution, publish,
    rewrite_distribution,
};
use nebula_core::{DistributionKey, Result};
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";
const GOLDEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/launcher/nebula-hardening-generated.json"
));
const GOLDEN_RELATIVE: &str = "servers/Hambugui-1.20.1/files/config/client.cfg";
/// Exactly the 15 bytes the golden fixture claims for `client.cfg`.
const CLIENT_CFG: &[u8] = b"client=true\n#42";

/// The seed the launcher's signature tests use (contract C2).
fn key() -> DistributionKey {
    DistributionKey::from_seed_hex(&"42".repeat(32), "test").unwrap()
}

/// The golden fixture as Nebula writes it now: with a `serial`, which a
/// publish requires.
fn golden_body() -> String {
    let body = GOLDEN.replacen(
        "\"version\": \"1.0.0\",",
        "\"version\": \"1.0.0\",\n  \"serial\": 1761730000,",
        1,
    );
    assert_ne!(body, GOLDEN);
    body
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: Root,
    distribution: PathBuf,
}

impl Fixture {
    /// A `<root>` holding a distribution and, optionally, the file it points at.
    fn new(body: &str, files: &[(&str, &[u8])]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
        let distribution = dir.path().join("distribution.json");
        std::fs::write(&distribution, body).unwrap();
        for (relative, content) in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        Self {
            _dir: dir,
            root,
            distribution,
        }
    }

    fn golden() -> Self {
        Self::new(&golden_body(), &[(GOLDEN_RELATIVE, CLIENT_CFG)])
    }
}

/// Records what it was asked to upload and hands back a predictable URL.
#[derive(Default)]
struct Recorder {
    uploads: Vec<(String, u64)>,
}

impl Target for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn upload(&mut self, _local: &Path, relative: &str, digests: &Digests) -> Result<String> {
        self.uploads.push((relative.to_owned(), digests.size));
        Ok(format!("https://fake.invalid/{relative}"))
    }
}

fn server_json(icon: &str, modules: &str) -> String {
    format!(r#"{{"serial":1,"servers":[{{"id":"S-1.20.1","icon":{icon},"modules":[{modules}]}}]}}"#)
}

// --- plan extraction ------------------------------------------------------

#[test]
fn plans_the_files_the_golden_distribution_references() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();

    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].relative, GOLDEN_RELATIVE);
    assert_eq!(plan.files[0].local, fixture.root.path.join(GOLDEN_RELATIVE));
    assert!(plan.files[0].local.is_file());
    assert_eq!(plan.distribution_path, fixture.distribution);
}

#[test]
fn refuses_to_publish_a_distribution_with_missing_files() {
    let fixture = Fixture::new(GOLDEN, &[]);
    let error = plan_from_distribution(&fixture.root, &fixture.distribution)
        .unwrap_err()
        .to_string();
    assert!(error.contains(GOLDEN_RELATIVE), "{error}");
}

#[test]
fn collects_icons_and_sub_modules_and_skips_foreign_urls() {
    let body = server_json(
        r#""https://cdn.example.invalid/servers/S-1.20.1/icon.png""#,
        concat!(
            r#"{"artifact":{"url":"https://cdn.example.invalid/servers/S-1.20.1/files/a.cfg"},"#,
            r#""subModules":[{"artifact":{"url":"https://cdn.example.invalid/repo/lib/x/y.jar"}}]},"#,
            r#"{"artifact":{"url":"https://maven.example.invalid/net/x/z.jar"}}"#
        ),
    );
    let fixture = Fixture::new(
        &body,
        &[
            ("servers/S-1.20.1/icon.png", b"png"),
            ("servers/S-1.20.1/files/a.cfg", b"cfg"),
            ("repo/lib/x/y.jar", b"jar"),
        ],
    );

    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let relatives: Vec<&str> = plan.files.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(
        relatives,
        [
            "servers/S-1.20.1/icon.png",
            "servers/S-1.20.1/files/a.cfg",
            "repo/lib/x/y.jar",
        ]
    );
}

#[test]
fn deduplicates_a_file_two_modules_share() {
    let url = r#"{"artifact":{"url":"https://cdn.example.invalid/repo/lib/x/y.jar"}}"#;
    let body = server_json("null", &format!("{url},{url}"));
    let fixture = Fixture::new(&body, &[("repo/lib/x/y.jar", b"jar")]);

    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    assert_eq!(plan.files.len(), 1);
}

#[test]
fn round_trips_a_percent_encoded_file_name() {
    let name = "servers/S-1.20.1/files/a b#1.cfg";
    let root = Root::new("ignored", Url::parse(BASE).unwrap());
    let url = root.url(name);
    assert_eq!(
        url,
        "https://cdn.example.invalid/servers/S-1.20.1/files/a%20b%231.cfg"
    );

    let body = server_json("null", &format!(r#"{{"artifact":{{"url":"{url}"}}}}"#));
    let fixture = Fixture::new(&body, &[(name, b"cfg")]);
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    assert_eq!(plan.files[0].relative, name);
    assert!(plan.files[0].local.is_file());
}

#[test]
fn rejects_a_url_that_escapes_the_root() {
    let body = server_json(
        "null",
        r#"{"artifact":{"url":"https://cdn.example.invalid/servers/..%2f..%2fetc/passwd"}}"#,
    );
    let fixture = Fixture::new(&body, &[]);
    assert!(plan_from_distribution(&fixture.root, &fixture.distribution).is_err());
}

// --- change tracking ------------------------------------------------------

#[test]
fn skips_files_whose_hash_is_already_published() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let opts = PublishOptions {
        only_changed: true,
        rewrite_urls: false,
        signing_key: Some(key()),
    };

    let mut target = Recorder::default();
    let first = publish(&fixture.root, &plan, &mut target, &opts).unwrap();
    // The artifact, then the distribution, then its signature: 128 hex
    // characters and a newline.
    assert_eq!((first.uploaded, first.skipped), (3, 0));
    assert_eq!(
        target.uploads,
        [
            (GOLDEN_RELATIVE.to_owned(), CLIENT_CFG.len() as u64),
            ("distribution.json".to_owned(), golden_body().len() as u64),
            ("distribution.json.sig".to_owned(), 129),
        ]
    );
    assert_eq!(
        first.distribution_url,
        "https://fake.invalid/distribution.json"
    );
    assert_eq!(
        first.signature_url,
        "https://fake.invalid/distribution.json.sig"
    );
    assert!(first.rewritten_distribution.is_none());
    assert!(PublishState::path(&fixture.root).is_file());

    // Nothing changed: only the distribution and its signature are sent again.
    let mut target = Recorder::default();
    let second = publish(&fixture.root, &plan, &mut target, &opts).unwrap();
    assert_eq!((second.uploaded, second.skipped), (2, 1));
    assert_eq!(target.uploads.len(), 2);
    assert_eq!(target.uploads[0].0, "distribution.json");
    assert_eq!(target.uploads[1].0, "distribution.json.sig");

    // Touching the file brings it back.
    std::fs::write(&plan.files[0].local, b"client=false\n#7").unwrap();
    let mut target = Recorder::default();
    let third = publish(&fixture.root, &plan, &mut target, &opts).unwrap();
    assert_eq!((third.uploaded, third.skipped), (3, 0));
}

#[test]
fn uploads_everything_when_change_tracking_is_off() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let opts = PublishOptions {
        only_changed: false,
        rewrite_urls: false,
        signing_key: Some(key()),
    };

    publish(&fixture.root, &plan, &mut Recorder::default(), &opts).unwrap();
    let again = publish(&fixture.root, &plan, &mut Recorder::default(), &opts).unwrap();
    assert_eq!((again.uploaded, again.skipped), (3, 0));
}

#[test]
fn keys_the_state_by_target_name() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    publish(
        &fixture.root,
        &plan,
        &mut Recorder::default(),
        &PublishOptions {
            signing_key: Some(key()),
            ..PublishOptions::default()
        },
    )
    .unwrap();

    let raw = std::fs::read_to_string(PublishState::path(&fixture.root)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let sha = state["recorder"][GOLDEN_RELATIVE].as_str().unwrap();
    assert_eq!(sha.len(), 64);
    assert!(state["webdav"].is_null());
}

// --- distribution rewriting -----------------------------------------------

#[test]
fn rewrites_artifact_urls_in_sub_modules_and_icons() {
    let mut value: serde_json::Value = serde_json::from_str(
        r#"{
            "version": "1.0.0",
            "rss": "https://cdn.example.invalid/servers/S-1.20.1/files/a.cfg.rss",
            "servers": [{
                "icon": "https://cdn.example.invalid/servers/S-1.20.1/icon.png",
                "address": "localhost:25565",
                "modules": [{
                    "id": "generated.forgemod:jei:1.0@jar",
                    "artifact": { "url": "https://cdn.example.invalid/servers/S-1.20.1/files/a.cfg", "SHA256": "dead" },
                    "subModules": [
                        { "artifact": { "url": "https://cdn.example.invalid/repo/lib/x/y.jar" } }
                    ]
                }]
            }]
        }"#,
    )
    .unwrap();

    let map: HashMap<String, String> = [
        (
            "https://cdn.example.invalid/servers/S-1.20.1/icon.png",
            "https://drive.invalid/?id=icon",
        ),
        (
            "https://cdn.example.invalid/servers/S-1.20.1/files/a.cfg",
            "https://drive.invalid/?id=cfg",
        ),
        (
            "https://cdn.example.invalid/repo/lib/x/y.jar",
            "https://drive.invalid/?id=jar",
        ),
    ]
    .into_iter()
    .map(|(from, to)| (from.to_owned(), to.to_owned()))
    .collect();

    rewrite_distribution(&mut value, &map);

    let server = &value["servers"][0];
    assert_eq!(server["icon"], "https://drive.invalid/?id=icon");
    assert_eq!(
        server["modules"][0]["artifact"]["url"],
        "https://drive.invalid/?id=cfg"
    );
    assert_eq!(
        server["modules"][0]["subModules"][0]["artifact"]["url"],
        "https://drive.invalid/?id=jar"
    );
    // Everything that is not a published URL is left exactly as it was.
    assert_eq!(value["version"], "1.0.0");
    assert_eq!(
        value["rss"],
        "https://cdn.example.invalid/servers/S-1.20.1/files/a.cfg.rss"
    );
    assert_eq!(server["address"], "localhost:25565");
    assert_eq!(server["modules"][0]["id"], "generated.forgemod:jei:1.0@jar");
    assert_eq!(server["modules"][0]["artifact"]["SHA256"], "dead");
}

#[test]
fn publishes_a_rewritten_distribution_beside_the_original() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();

    let mut target = Recorder::default();
    let report = publish(
        &fixture.root,
        &plan,
        &mut target,
        &PublishOptions {
            only_changed: true,
            rewrite_urls: true,
            signing_key: Some(key()),
        },
    )
    .unwrap();

    let rewritten = report.rewritten_distribution.clone().unwrap();
    assert_eq!(
        rewritten,
        fixture.root.path.join("distribution.recorder.json")
    );
    assert_eq!(
        report.distribution_url,
        "https://fake.invalid/distribution.recorder.json"
    );
    // The rewritten copy is what ships, so it is what is signed.
    let uploaded: Vec<&str> = target
        .uploads
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        uploaded[uploaded.len() - 2..],
        [
            "distribution.recorder.json",
            "distribution.recorder.json.sig"
        ]
    );
    let signature =
        std::fs::read_to_string(fixture.root.path.join("distribution.recorder.json.sig")).unwrap();
    assert_eq!(
        signature.trim_end(),
        key().sign(&std::fs::read(&rewritten).unwrap())
    );

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rewritten).unwrap()).unwrap();
    // Rewriting keeps the serial.
    assert_eq!(value["serial"], 1761730000);
    assert_eq!(
        value["servers"][0]["modules"][0]["artifact"]["url"],
        format!("https://fake.invalid/{GOLDEN_RELATIVE}")
    );
    // The generated distribution is untouched.
    assert_eq!(
        std::fs::read_to_string(&fixture.distribution).unwrap(),
        golden_body()
    );
}

#[test]
fn rewriting_never_trusts_the_publish_state_to_supply_a_url() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let opts = PublishOptions {
        only_changed: true,
        rewrite_urls: true,
        signing_key: Some(key()),
    };

    publish(&fixture.root, &plan, &mut Recorder::default(), &opts).unwrap();
    let mut target = Recorder::default();
    let again = publish(&fixture.root, &plan, &mut target, &opts).unwrap();

    assert_eq!(again.skipped, 0);
    assert_eq!(target.uploads[0].0, GOLDEN_RELATIVE);
}

// --- WebDAV ---------------------------------------------------------------

#[test]
fn uploads_over_webdav_creating_every_collection_first() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let (addr, calls) = stub_created();

    let mut target = WebDavTarget::new(
        Url::parse(&format!("http://{addr}/nebula-dist/")).unwrap(),
        "nebula".into(),
        "secret".into(),
        Url::parse(BASE).unwrap(),
    )
    .unwrap();

    let report = publish(
        &fixture.root,
        &plan,
        &mut target,
        &PublishOptions {
            only_changed: false,
            rewrite_urls: false,
            signing_key: Some(key()),
        },
    )
    .unwrap();

    // WebDAV mirrors the static layout, so the launcher URL never changes.
    assert_eq!(
        report.distribution_url,
        "https://cdn.example.invalid/distribution.json"
    );
    // Exactly where the launcher looks for it: the same path plus `.sig`.
    assert_eq!(
        report.signature_url,
        "https://cdn.example.invalid/distribution.json.sig"
    );
    assert_eq!((report.uploaded, report.skipped), (3, 0));

    let calls = summaries(&calls);
    let seen: Vec<(&str, &str, usize)> = calls
        .iter()
        .map(|(method, target, length)| (method.as_str(), target.as_str(), *length))
        .collect();
    assert_eq!(
        seen,
        [
            ("MKCOL", "/nebula-dist/servers", 0),
            ("MKCOL", "/nebula-dist/servers/Hambugui-1.20.1", 0),
            ("MKCOL", "/nebula-dist/servers/Hambugui-1.20.1/files", 0),
            (
                "MKCOL",
                "/nebula-dist/servers/Hambugui-1.20.1/files/config",
                0
            ),
            (
                "PUT",
                "/nebula-dist/servers/Hambugui-1.20.1/files/config/client.cfg",
                CLIENT_CFG.len()
            ),
            ("PUT", "/nebula-dist/distribution.json", golden_body().len()),
            ("PUT", "/nebula-dist/distribution.json.sig", 129),
        ]
    );
}

// --- signing and the serial -----------------------------------------------

#[test]
fn refuses_to_publish_without_a_signing_key_before_uploading_anything() {
    let fixture = Fixture::golden();
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    for rewrite_urls in [false, true] {
        let mut target = Recorder::default();
        let error = publish(
            &fixture.root,
            &plan,
            &mut target,
            &PublishOptions {
                only_changed: false,
                rewrite_urls,
                signing_key: None,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("publish refused"), "{error}");
        assert!(
            error.contains(nebula_core::publish::NO_SIGNING_KEY),
            "{error}"
        );
        assert!(target.uploads.is_empty(), "{:?}", target.uploads);
    }
    assert!(!PublishState::path(&fixture.root).exists());
    assert!(!fixture.root.path.join("distribution.json.sig").exists());
}

#[test]
fn refuses_to_publish_a_distribution_without_a_serial() {
    // The fixture as upstream wrote it: no serial.
    let fixture = Fixture::new(GOLDEN, &[(GOLDEN_RELATIVE, CLIENT_CFG)]);
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let mut target = Recorder::default();
    let error = publish(
        &fixture.root,
        &plan,
        &mut target,
        &PublishOptions {
            signing_key: Some(key()),
            ..PublishOptions::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("has no serial"), "{error}");
    assert!(target.uploads.is_empty());
}

#[test]
fn the_signature_url_is_the_launchers_rule() {
    assert_eq!(
        nebula_core::publish::signature_url_for(
            "https://cdn.example.invalid/dist/distribution.json"
        )
        .as_deref(),
        Some("https://cdn.example.invalid/dist/distribution.json.sig")
    );
    // Google Drive serves by id, so the launcher's rule cannot find a
    // signature uploaded next to it; publish warns about exactly this.
    assert_eq!(
        nebula_core::publish::signature_url_for(&gdrive::public_url("abc")).as_deref(),
        Some("https://drive.usercontent.google.com/download.sig?id=abc&export=download&confirm=t")
    );
    assert_ne!(
        nebula_core::publish::signature_url_for(&gdrive::public_url("abc")),
        Some(gdrive::public_url("def"))
    );
}

#[test]
fn creates_each_webdav_collection_once_per_run() {
    let name = "servers/S-1.20.1/files/config/a b#1.cfg";
    let other = "servers/S-1.20.1/files/config/b.cfg";
    let modules = format!(
        r#"{{"artifact":{{"url":"{}"}}}},{{"artifact":{{"url":"{}"}}}}"#,
        Root::new("ignored", Url::parse(BASE).unwrap()).url(name),
        Root::new("ignored", Url::parse(BASE).unwrap()).url(other),
    );
    let fixture = Fixture::new(
        &server_json("null", &modules),
        &[(name, b"one"), (other, b"two")],
    );
    let plan = plan_from_distribution(&fixture.root, &fixture.distribution).unwrap();
    let (addr, calls) = stub_created();

    let mut target = WebDavTarget::new(
        Url::parse(&format!("http://{addr}/")).unwrap(),
        "nebula".into(),
        "secret".into(),
        Url::parse(BASE).unwrap(),
    )
    .unwrap();
    publish(
        &fixture.root,
        &plan,
        &mut target,
        &PublishOptions {
            only_changed: false,
            rewrite_urls: false,
            signing_key: Some(key()),
        },
    )
    .unwrap();

    let calls = summaries(&calls);
    let mkcols: Vec<&str> = calls
        .iter()
        .filter(|(method, ..)| method == "MKCOL")
        .map(|(_, target, _)| target.as_str())
        .collect();
    assert_eq!(
        mkcols,
        [
            "/servers",
            "/servers/S-1.20.1",
            "/servers/S-1.20.1/files",
            "/servers/S-1.20.1/files/config",
        ]
    );

    // The upload path is percent-encoded exactly like the published URL.
    let puts: Vec<&str> = calls
        .iter()
        .filter(|(method, ..)| method == "PUT")
        .map(|(_, target, _)| target.as_str())
        .collect();
    assert_eq!(puts[0], "/servers/S-1.20.1/files/config/a%20b%231.cfg");
}

// --- Google Drive (pure pieces only; the HTTP flow is manual QA) -----------

#[test]
fn builds_the_consent_url() {
    let url = gdrive::consent_url("client-id", "http://127.0.0.1:53421", "s7");
    assert!(
        url.starts_with(&format!("{}?", gdrive::AUTH_ENDPOINT)),
        "{url}"
    );

    let parsed = Url::parse(&url).unwrap();
    let pairs: HashMap<String, String> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(pairs["client_id"], "client-id");
    assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:53421");
    assert_eq!(pairs["response_type"], "code");
    assert_eq!(pairs["scope"], gdrive::SCOPE);
    // Without offline access there is no refresh token to store.
    assert_eq!(pairs["access_type"], "offline");
    assert_eq!(pairs["prompt"], "consent");
    assert_eq!(pairs["state"], "s7");
}

#[test]
fn escapes_names_in_a_drive_query() {
    assert_eq!(
        gdrive::escape_query_value("Tinkers' Construct.jar"),
        r"Tinkers\' Construct.jar"
    );
    // The backslash goes first, or the escape would be escaped again.
    assert_eq!(gdrive::escape_query_value(r"a\b"), r"a\\b");
    assert_eq!(gdrive::escape_query_value(r"a\'b"), r"a\\\'b");
    assert_eq!(gdrive::escape_query_value("plain.jar"), "plain.jar");
}

#[test]
fn splits_a_relative_path_into_folders_and_a_name() {
    assert_eq!(
        gdrive::folder_chain("servers/S-1.20.1/files/config/a.cfg"),
        (vec!["servers", "S-1.20.1", "files", "config"], "a.cfg")
    );
    assert_eq!(
        gdrive::folder_chain("distribution.json"),
        (Vec::new(), "distribution.json")
    );
    assert_eq!(
        gdrive::folder_chain("repo\\lib\\x\\y.jar"),
        (vec!["repo", "lib", "x"], "y.jar")
    );
}

#[test]
fn uses_the_direct_download_form_for_public_urls() {
    assert_eq!(
        gdrive::public_url("1AbC"),
        "https://drive.usercontent.google.com/download?id=1AbC&export=download&confirm=t"
    );
}
