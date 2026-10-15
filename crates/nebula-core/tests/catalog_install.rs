//! Install, update, icons and server management — all offline, against the
//! in-process stub in `common`. Nothing here reaches a real provider or a real
//! CDN: every download URL points back at the stub.
//!
//! The distribution regression of reliability rule 5 lives in `golden.rs`
//! (`the_catalogs_own_files_do_not_change_the_distribution`), which already
//! builds a fixture root with `modindex.json`, `.nebula-lock`, `.nebula-tmp/`
//! and `cache/icons/` present and asserts the output is byte-identical.

mod common;

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{Calls, requests, stub};
use nebula_core::catalog::icons::{self, ICON_SIZE, IconRef};
use nebula_core::catalog::index::{IndexEntry, IndexHashes, InstalledMod, ModIndex};
use nebula_core::catalog::{
    self, Catalog, CatalogError, Endpoints, InstallChoice, Loader, ProviderId, ReleaseType,
    UpdateCandidate, Version, VersionFile, install, manage, update,
};
use nebula_core::layout::{Root, ServerDir};
use nebula_core::scan::Namespace;
use nebula_core::version::MinecraftVersion;
use nebula_core::{CreateServerOptions, create_server, hash, list_servers};
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";
/// Route bodies name the stub with this placeholder, which the stub replaces
/// with its own `http://127.0.0.1:<port>` once it knows the port.
const STUB: &str = "STUB";

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

/// A root with one Fabric server, ready for the catalog to write into.
struct Fixture {
    root: Root,
    server: ServerDir,
    container: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
        let created = create_server(
            &root,
            "Test",
            &mc(),
            &CreateServerOptions {
                fabric_version: Some("0.14.18".into()),
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            container: created.mod_container.clone().unwrap(),
            server: created.dir,
            root,
            _dir: dir,
        }
    }

    fn namespace(&self, namespace: Namespace) -> PathBuf {
        self.container.join(namespace.dir_name())
    }

    fn required(&self, file_name: &str) -> PathBuf {
        self.namespace(Namespace::Required).join(file_name)
    }

    fn index(&self) -> ModIndex {
        ModIndex::load(&self.server).unwrap()
    }

    /// Writes an index straight to disk, which is how a test says "this was
    /// installed earlier".
    fn write_index(&self, entries: &[(&str, IndexEntry)]) {
        let mut index = ModIndex::default();
        for (file_name, entry) in entries {
            index.mods.insert((*file_name).to_owned(), entry.clone());
        }
        index.save(&self.server).unwrap();
    }
}

fn mc() -> MinecraftVersion {
    "1.20.1".parse().unwrap()
}

fn catalog(addr: SocketAddr) -> Catalog {
    Catalog::new(
        Endpoints {
            modrinth: format!("http://{addr}"),
            curseforge: format!("http://{addr}/v1"),
        },
        Some("test-key".into()),
    )
}

/// A stub that answers an exact path table and 404s everything else.
///
/// A text body's `STUB` placeholder becomes the stub's own base URL, so a
/// route can hand out a download link to the very server serving it.
fn serve(entries: Vec<(String, u16, Vec<u8>)>) -> (SocketAddr, Calls) {
    let table: HashMap<String, (u16, Vec<u8>)> = entries
        .into_iter()
        .map(|(path, status, body)| (path, (status, body)))
        .collect();
    let address: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
    let known = Arc::clone(&address);
    let (addr, calls) = stub(move |req| match table.get(req.path()) {
        Some((status, body)) => (*status, "application/octet-stream", patch(body, &known)),
        None => (404, "text/plain", Vec::new()),
    });
    address.set(format!("http://{addr}")).unwrap();
    (addr, calls)
}

/// Only a body that actually carries the marker is touched, so jar bytes are
/// served exactly as the test wrote them.
fn patch(body: &[u8], address: &OnceLock<String>) -> Vec<u8> {
    let Some(base) = address.get() else {
        return body.to_vec();
    };
    if !body
        .windows(STUB.len())
        .any(|window| window == STUB.as_bytes())
    {
        return body.to_vec();
    }
    match std::str::from_utf8(body) {
        Ok(text) => text.replace(STUB, base).into_bytes(),
        Err(_) => body.to_vec(),
    }
}

fn route(path: &str, body: String) -> (String, u16, Vec<u8>) {
    (path.to_owned(), 200, body.into_bytes())
}

fn bytes_route(path: &str, body: &[u8]) -> (String, u16, Vec<u8>) {
    (path.to_owned(), 200, body.to_vec())
}

fn sha1(bytes: &[u8]) -> String {
    hash::digests_of_bytes(bytes).sha1
}

/// A Modrinth project document.
fn project(id: &str) -> String {
    format!(r#"{{"id":"{id}","slug":"{id}","title":"{id}","description":"d","downloads":1}}"#)
}

/// One Modrinth version with a single primary file.
fn version_body(
    project_id: &str,
    version_id: &str,
    file_name: &str,
    digest: &str,
    dependencies: &str,
) -> String {
    format!(
        r#"{{"id":"{version_id}","project_id":"{project_id}","name":"{version_id}",
            "version_number":"{version_id}","version_type":"release",
            "date_published":"2026-01-01T00:00:00Z","game_versions":["1.20.1"],
            "loaders":["fabric"],
            "files":[{{"filename":"{file_name}","url":"{STUB}/files/{file_name}",
                       "primary":true,"size":3,"hashes":{{"sha1":"{digest}"}}}}],
            "dependencies":[{dependencies}]}}"#
    )
}

fn dependency(project_id: &str, kind: &str) -> String {
    format!(r#"{{"project_id":"{project_id}","dependency_type":"{kind}"}}"#)
}

/// Everything one Modrinth mod needs: its project document, its version list
/// and the bytes behind its download URL.
fn modrinth_mod(
    id: &str,
    file_name: &str,
    body: &[u8],
    dependencies: &str,
) -> Vec<(String, u16, Vec<u8>)> {
    vec![
        route(&format!("/v2/project/{id}"), project(id)),
        route(
            &format!("/v2/project/{id}/version"),
            format!(
                "[{}]",
                version_body(id, &format!("{id}-1"), file_name, &sha1(body), dependencies)
            ),
        ),
        bytes_route(&format!("/files/{file_name}"), body),
    ]
}

/// A version a test hands to `replace_file` directly, pointed at a live route.
fn version_at(
    addr: SocketAddr,
    project_id: &str,
    version_id: &str,
    file_name: &str,
    body: &[u8],
) -> Version {
    Version {
        project_id: project_id.to_owned(),
        version_id: version_id.to_owned(),
        name: version_id.to_owned(),
        number: version_id.to_owned(),
        release: ReleaseType::Release,
        published: "2026-02-01T00:00:00Z".to_owned(),
        game_versions: vec!["1.20.1".to_owned()],
        loaders: vec![Loader::Fabric],
        files: vec![VersionFile {
            file_name: file_name.to_owned(),
            url: Some(format!("http://{addr}/files/{file_name}")),
            size: body.len() as u64,
            primary: true,
            sha1: Some(sha1(body)),
            sha512: None,
            md5: None,
            fingerprint: None,
            file_id: None,
        }],
        dependencies: Vec::new(),
    }
}

fn no_progress(_: usize, _: usize, _: &str) {}

/// Ages one directory, which is how a test says "this leftover is stale".
/// Windows needs `FILE_FLAG_BACKUP_SEMANTICS` to open a directory at all.
fn backdate(path: &Path, age: Duration) {
    #[cfg(windows)]
    let handle = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::File::options()
            .write(true)
            .custom_flags(0x0200_0000)
            .open(path)
            .unwrap()
    };
    #[cfg(not(windows))]
    let handle = std::fs::File::open(path).unwrap();
    handle
        .set_times(std::fs::FileTimes::new().set_modified(SystemTime::now() - age))
        .unwrap();
}

/// A progress sink plus the calls it recorded.
type Steps = Arc<Mutex<Vec<(usize, usize, String)>>>;

fn steps() -> (Steps, impl Fn(usize, usize, &str) + Send + Sync) {
    let seen: Steps = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    (seen, move |index, total, file: &str| {
        sink.lock().unwrap().push((index, total, file.to_owned()));
    })
}

fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn entry(provider: ProviderId, project_id: &str, digest: &str) -> IndexEntry {
    IndexEntry::new(
        provider,
        project_id,
        "v1",
        IndexHashes {
            sha1: digest.to_owned(),
            sha512: None,
            md5: None,
        },
        Namespace::Required,
    )
}

fn jars(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry.path().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .ends_with(".jar")
        })
        .count()
}

fn plan_and_apply(
    catalog: &Catalog,
    fixture: &Fixture,
    project_id: &str,
    ns: Namespace,
) -> Result<nebula_core::catalog::InstallReport, CatalogError> {
    let plan = install::plan_install(
        catalog,
        &fixture.server,
        ProviderId::Modrinth,
        project_id,
        None,
    )?;
    let choice = InstallChoice {
        deps: plan
            .required
            .iter()
            .map(|dep| dep.file.project_id.clone())
            .collect(),
    };
    install::apply(catalog, &fixture.server, &plan, &choice, ns, &no_progress)
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

#[test]
fn plan_lists_required_optional_incompatible_and_unresolvable_dependencies() {
    let fixture = Fixture::new();
    // `bad` is incompatible and already installed, so it blocks the install.
    fixture.write_index(&[("bad.jar", entry(ProviderId::Modrinth, "bad", "aa"))]);

    let mut table = modrinth_mod(
        "root",
        "root.jar",
        b"root",
        &[
            dependency("req", "required"),
            dependency("opt", "optional"),
            dependency("bad", "incompatible"),
            dependency("gone", "required"),
            dependency("inner", "embedded"),
        ]
        .join(","),
    );
    table.extend(modrinth_mod("req", "req.jar", b"req", ""));
    table.extend(modrinth_mod("opt", "opt.jar", b"opt", ""));
    // The incompatible project is resolved for its slug, exactly as the other
    // kinds are, so a hand-placed jar can be matched by its mod id (F10).
    table.push(route("/v2/project/bad", project("bad")));
    let (addr, _) = serve(table);

    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    assert_eq!(plan.server, "Test-1.20.1");
    assert_eq!(plan.root.file.file_name, "root.jar");
    assert_eq!(plan.required.len(), 1);
    assert_eq!(plan.required[0].file.project_id, "req");
    assert_eq!(plan.required[0].already_installed, None);
    assert_eq!(plan.optional.len(), 1);
    assert_eq!(plan.optional[0].file.project_id, "opt");
    // `gone` 404s on `/v2/project/gone`, which is not a network failure but a
    // required dependency that cannot be satisfied.
    assert_eq!(plan.unresolvable, ["gone"]);
    assert_eq!(plan.incompatible, ["bad.jar"]);
    // An embedded dependency ships inside the jar and is never installed.
    assert!(
        !plan
            .required
            .iter()
            .chain(&plan.optional)
            .any(|dep| dep.file.project_id == "inner")
    );
}

#[test]
fn plan_resolves_required_dependencies_recursively_without_looping() {
    let fixture = Fixture::new();
    // root -> a -> b -> root: a cycle the walk must survive.
    let mut table = modrinth_mod("root", "root.jar", b"root", &dependency("a", "required"));
    table.extend(modrinth_mod(
        "a",
        "a.jar",
        b"a",
        &dependency("b", "required"),
    ));
    table.extend(modrinth_mod(
        "b",
        "b.jar",
        b"b",
        &dependency("root", "required"),
    ));
    let (addr, _) = serve(table);

    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    let ids: Vec<&str> = plan
        .required
        .iter()
        .map(|dep| dep.file.project_id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"a") && ids.contains(&"b"));
    assert!(plan.unresolvable.is_empty());
}

#[test]
fn plan_reaches_a_dependency_through_its_shortest_path() {
    let fixture = Fixture::new();
    // root -> deep1 -> … -> deep7 -> shared, and root -> shared directly. The
    // long arm is past MAX_DEPTH, so only a breadth-first walk keeps `shared`.
    let mut table = modrinth_mod(
        "root",
        "root.jar",
        b"root",
        &[
            dependency("deep1", "required"),
            dependency("shared", "required"),
        ]
        .join(","),
    );
    for step in 1..=7 {
        let next = match step {
            7 => "shared".to_owned(),
            other => format!("deep{}", other + 1),
        };
        table.extend(modrinth_mod(
            &format!("deep{step}"),
            &format!("deep{step}.jar"),
            format!("deep{step}").as_bytes(),
            &dependency(&next, "required"),
        ));
    }
    table.extend(modrinth_mod("shared", "shared.jar", b"shared", ""));
    let (addr, _) = serve(table);

    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    let ids: Vec<&str> = plan
        .required
        .iter()
        .map(|dep| dep.file.project_id.as_str())
        .collect();
    assert!(ids.contains(&"shared"), "{ids:?}");
    assert!(plan.unresolvable.is_empty(), "{:?}", plan.unresolvable);
    // The walk stops at depth 6, so the tail of the long arm is not resolved.
    assert!(!ids.contains(&"deep7"), "{ids:?}");
}

#[test]
fn plan_marks_a_dependency_installed_by_index_and_by_jar_mod_id() {
    let fixture = Fixture::new();
    fixture.write_index(&[("req.jar", entry(ProviderId::Modrinth, "req", "aa"))]);
    // A hand-placed jar whose own mod id matches the dependency's slug.
    write_jar(
        &fixture.required("fabric-api-0.92.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"fabric_api","version":"0.92.0","name":"Fabric API"}"#,
        )],
    );

    let mut table = modrinth_mod(
        "root",
        "root.jar",
        b"root",
        &[
            dependency("req", "required"),
            dependency("fabric-api", "required"),
        ]
        .join(","),
    );
    table.extend(modrinth_mod("req", "req.jar", b"req", ""));
    table.extend(modrinth_mod("fabric-api", "fapi.jar", b"fapi", ""));
    let (addr, _) = serve(table);

    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    let by_id: HashMap<&str, Option<&str>> = plan
        .required
        .iter()
        .map(|dep| {
            (
                dep.file.project_id.as_str(),
                dep.already_installed.as_deref(),
            )
        })
        .collect();
    assert_eq!(by_id["req"], Some("req.jar"));
    // `fabric-api` and `fabric_api` are one mod (best-effort slug match, D6).
    assert_eq!(by_id["fabric-api"], Some("fabric-api-0.92.jar"));
}

/// A CurseForge project whose single file is either downloadable or blocked,
/// plus the `download-info` document R3 recovers the digests from.
fn curseforge_mod(downloadable: bool) -> Vec<(String, u16, Vec<u8>)> {
    let digest = sha1(b"jei-bytes");
    let url = match downloadable {
        true => format!("\"{STUB}/files/jei.jar\""),
        false => "null".to_owned(),
    };
    vec![
        route(
            "/v1/mods/123",
            r#"{"data":{"id":123,"name":"JEI","slug":"jei","summary":"items",
                "downloadCount":9,"status":4,
                "links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei"}}}"#
                .to_owned(),
        ),
        route(
            "/v1/mods/123/files",
            format!(
                r#"{{"data":[{{"id":555,"modId":123,"displayName":"JEI 15.0","fileName":"jei.jar",
                    "releaseType":1,"fileStatus":4,"fileDate":"2026-01-01T00:00:00Z",
                    "fileLength":9,"downloadUrl":{url},
                    "hashes":[{{"value":"{digest}","algo":1}}],
                    "sortableGameVersions":[
                        {{"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125}},
                        {{"gameVersionName":"Fabric","gameVersionTypeId":68441}}],
                    "isServerPack":false,"fileFingerprint":1,"dependencies":[]}}]}}"#
            ),
        ),
        route(
            "/v1/mods/123/files/555/download-info",
            format!(
                r#"{{"data":[{{"fileName":"jei.jar","downloadUrl":"",
                    "hashes":[{{"value":"{digest}","algo":1}}]}}]}}"#
            ),
        ),
        bytes_route("/files/jei.jar", b"jei-bytes"),
    ]
}

#[test]
fn plan_moves_a_blocked_curseforge_file_out_before_any_download() {
    let fixture = Fixture::new();
    let mut table = curseforge_mod(false);
    // Modrinth does not have the same bytes, so the fallback misses.
    table.push(route("/v2/version_files", "{}".to_owned()));
    let (addr, calls) = serve(table);
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap();

    assert!(plan.root.file.url.is_none());
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].file_name, "jei.jar");
    assert_eq!(plan.blocked[0].name, "JEI");
    assert_eq!(
        plan.blocked[0].url,
        "https://www.curseforge.com/minecraft/mc-mods/jei/files/555"
    );

    // Applying downloads nothing and reports the file as a manual download.
    let report = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap();
    assert!(report.installed.is_empty());
    assert_eq!(report.manual.len(), 1);
    assert!(!fixture.required("jei.jar").exists());

    // R4: the CDN URL is never reconstructed, so nothing asks forgecdn.
    for request in requests(&calls) {
        assert!(!request.target.contains("forgecdn"), "{}", request.target);
    }
}

#[test]
fn plan_falls_back_to_modrinths_copy_of_a_blocked_file() {
    let fixture = Fixture::new();
    let payload = b"jei-bytes";
    let digest = sha1(payload);
    let mut table = curseforge_mod(false);
    table.push(route(
        "/v2/version_files",
        format!(
            "{{\"{digest}\": {}}}",
            version_body("mirror", "mirror-1", "jei-mirror.jar", &digest, "")
        ),
    ));
    table.push(bytes_route("/files/jei-mirror.jar", payload));
    let (addr, _) = serve(table);
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap();

    // R3: a blocked file is not blocked when Modrinth serves the same bytes.
    assert!(plan.blocked.is_empty());
    assert_eq!(plan.root.provider, ProviderId::Modrinth);
    assert_eq!(plan.root.file.file_name, "jei-mirror.jar");
    // CF-03: the download moved provider, the *hit* did not. Anything asking a
    // provider about this mod again — 다른 버전… above all — has to use the
    // pair the user picked, or it sends a Modrinth id to CurseForge.
    assert_eq!(plan.root.source(), (ProviderId::CurseForge, "123"));
    assert_eq!(plan.root.project_id, "mirror");

    install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap();
    assert!(fixture.required("jei-mirror.jar").is_file());
    assert_eq!(
        fixture.index().mods["jei-mirror.jar"].provider,
        ProviderId::Modrinth
    );
    // D2 keys on where the text came from, not on who serves the bytes: the
    // title here is still the CurseForge project's, so none of it is written.
    let raw = std::fs::read_to_string(fixture.server.modindex_file()).unwrap();
    assert!(!raw.contains("JEI"), "CurseForge text reached disk:\n{raw}");
    assert!(!raw.contains("\"jei\""), "{raw}");
}

#[test]
fn installing_from_a_second_provider_replaces_the_jar_instead_of_adding_one() {
    let fixture = Fixture::new();
    // JEI as Modrinth installed it, jar and index entry both.
    write_jar(
        &fixture.required("jei-modrinth.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"jei","version":"19.51","name":"Just Enough Items"}"#,
        )],
    );
    fixture.write_index(&[(
        "jei-modrinth.jar",
        entry(ProviderId::Modrinth, "u6dRKJwZ", "aa"),
    )]);

    // The same mod, from CurseForge: a different (provider, project id) pair,
    // and a different file name, so nothing but the mod id can see it (CF-01).
    let (addr, _) = serve(curseforge_mod(true));
    let catalog = catalog(addr);
    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap();
    let report = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap();

    // One jar, not two: the same mod id twice stops the game from starting.
    assert_eq!(report.installed, ["jei.jar"]);
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 1);
    assert!(fixture.required("jei.jar").is_file());
    assert!(!fixture.required("jei-modrinth.jar").exists());

    let index = fixture.index();
    assert_eq!(index.mods.len(), 1);
    assert_eq!(index.mods["jei.jar"].provider, ProviderId::CurseForge);
    assert_eq!(index.mods["jei.jar"].project_id, "123");
}

#[test]
fn a_hand_placed_jar_of_the_same_mod_is_refused_before_anything_downloads() {
    let fixture = Fixture::new();
    // Same mod id, but nobody's index entry: there is nothing to replace, so
    // the install is refused outright rather than left to fail mid-apply.
    write_jar(
        &fixture.required("jei-by-hand.jar"),
        &[("fabric.mod.json", br#"{"id":"jei","version":"19.51"}"#)],
    );

    let (addr, calls) = serve(curseforge_mod(true));
    let catalog = catalog(addr);
    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap();
    let before = requests(&calls).len();
    let error = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap_err();

    assert!(
        matches!(&error, CatalogError::NameTaken { file } if file == "jei-by-hand.jar"),
        "{error:?}"
    );
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 1);
    assert_eq!(requests(&calls).len(), before, "bytes were downloaded");
}

#[test]
fn adoption_never_rewrites_a_managed_entry() {
    let fixture = Fixture::new();
    write_jar(
        &fixture.required("managed.jar"),
        &[("fabric.mod.json", br#"{"id":"managed","version":"1"}"#)],
    );
    write_jar(
        &fixture.required("hand.jar"),
        &[("fabric.mod.json", br#"{"id":"hand","version":"1"}"#)],
    );
    let managed_sha1 = sha1(&std::fs::read(fixture.required("managed.jar")).unwrap());
    let hand_sha1 = sha1(&std::fs::read(fixture.required("hand.jar")).unwrap());
    fixture.write_index(&[(
        "managed.jar",
        entry(ProviderId::CurseForge, "123", &managed_sha1),
    )]);

    // Modrinth would happily claim both jars; only the unmanaged one is its
    // business (CF-01).
    let (addr, calls) = serve(vec![
        route(
            "/v2/version_files",
            format!(
                "{{\"{managed_sha1}\": {}, \"{hand_sha1}\": {}}}",
                version_body("stolen", "stolen-1", "managed.jar", &managed_sha1, ""),
                version_body("handproj", "hand-1", "hand.jar", &hand_sha1, "")
            ),
        ),
        route("/v2/version_files/update", "{}".to_owned()),
        route("/v1/mods/123/files", r#"{"data":[]}"#.to_owned()),
    ]);

    let report = update::check_updates(&catalog(addr), &fixture.server).unwrap();
    assert_eq!(report.adopted, ["hand.jar"]);

    let index = fixture.index();
    // The managed entry kept its provider, its project id and its version id.
    let entry = &index.mods["managed.jar"];
    assert_eq!(entry.provider, ProviderId::CurseForge);
    assert_eq!(entry.project_id, "123");
    assert_eq!(entry.version_id, "v1");
    assert_eq!(index.mods["hand.jar"].provider, ProviderId::Modrinth);
    // Sanity: the bulk lookup really did offer the managed jar too.
    let asked = requests(&calls)
        .iter()
        .any(|request| request.target.contains("/v2/version_files"));
    assert!(asked, "the adoption lookup never ran");
}

#[test]
fn a_project_with_no_build_for_this_server_is_not_a_missing_project() {
    let fixture = Fixture::new();
    let mut table = curseforge_mod(true);
    // The project exists; it simply has nothing for 1.20.1 + Fabric.
    table.retain(|(path, _, _)| path != "/v1/mods/123/files");
    table.push(route("/v1/mods/123/files", r#"{"data":[]}"#.to_owned()));
    let (addr, _) = serve(table);
    let catalog = catalog(addr);

    let error = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap_err();
    assert!(
        matches!(&error, CatalogError::NoVersion { mc, loader }
            if mc == "1.20.1" && *loader == Loader::Fabric),
        "{error:?}"
    );
    assert_eq!(
        nebula_core::Error::from(error).to_string(),
        "1.20.1 Fabric용 버전이 없습니다"
    );

    // A project nobody has is still the other sentence (CF-05).
    let missing = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "999",
        None,
    )
    .unwrap_err();
    assert!(matches!(missing, CatalogError::NotFound), "{missing:?}");
}

#[test]
fn a_curseforge_install_keeps_no_free_text_on_disk() {
    let fixture = Fixture::new();
    let (addr, _) = serve(curseforge_mod(true));
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::CurseForge,
        "123",
        None,
    )
    .unwrap();
    install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap();

    let raw = std::fs::read_to_string(fixture.server.modindex_file()).unwrap();
    assert!(raw.contains("curseforge"), "{raw}");
    for text in ["JEI", "jei\"", "items"] {
        assert!(!raw.contains(text), "{text} reached disk:\n{raw}");
    }
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

#[test]
fn apply_writes_every_file_into_the_namespace_and_the_index() {
    let fixture = Fixture::new();
    let mut table = modrinth_mod("root", "root.jar", b"root", &dependency("req", "required"));
    table.extend(modrinth_mod("req", "req.jar", b"req", ""));
    let (addr, _) = serve(table);
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();
    let choice = InstallChoice {
        deps: plan
            .required
            .iter()
            .map(|dep| dep.file.project_id.clone())
            .collect(),
    };
    let (seen, progress) = steps();
    let report = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &choice,
        Namespace::OptionalOn,
        &progress,
    )
    .unwrap();

    assert_eq!(report.installed.len(), 2);
    assert!(!report.rolled_back);
    assert!(report.left_behind.is_empty());
    for name in ["root.jar", "req.jar"] {
        let path = fixture.namespace(Namespace::OptionalOn).join(name);
        assert!(path.is_file(), "{} missing", path.display());
    }
    // Dependencies go in first, so the root is always the last step.
    assert_eq!(
        *seen.lock().unwrap(),
        [(1, 2, "req.jar".to_owned()), (2, 2, "root.jar".to_owned())]
    );

    let index = fixture.index();
    assert_eq!(index.mods.len(), 2);
    let root = &index.mods["root.jar"];
    assert_eq!(root.provider, ProviderId::Modrinth);
    assert_eq!(root.namespace, Namespace::OptionalOn);
    assert_eq!(root.hashes.sha1, sha1(b"root"));
    assert_eq!(root.dependencies, ["req"]);
    assert_eq!(root.title.as_deref(), Some("root"));
    // Staging is emptied on the way out.
    assert_eq!(
        std::fs::read_dir(fixture.server.path.join(".nebula-tmp"))
            .map(|entries| entries.count())
            .unwrap_or(0),
        0
    );
}

#[test]
fn apply_installs_the_root_alone_when_no_dependency_was_kept() {
    let fixture = Fixture::new();
    let mut table = modrinth_mod("root", "root.jar", b"root", &dependency("req", "required"));
    table.extend(modrinth_mod("req", "req.jar", b"req", ""));
    let (addr, _) = serve(table);
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();
    // `--no-deps`.
    let report = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap();

    assert_eq!(report.installed, ["root.jar"]);
    assert!(!fixture.required("req.jar").exists());
    // The entry still records what the root needs, so a delete can warn.
    assert_eq!(fixture.index().mods["root.jar"].dependencies, ["req"]);
}

#[test]
fn apply_writes_a_percent_encoded_file_name_verbatim() {
    let fixture = Fixture::new();
    let name = "sodium-fabric-mc1.20.1-0.4.10%2Bbuild.24.jar";
    let (addr, _) = serve(modrinth_mod("sodium", name, b"so", ""));

    plan_and_apply(&catalog(addr), &fixture, "sodium", Namespace::Required).unwrap();

    // `%2B` is never decoded into `+`: the name is the index key.
    assert!(fixture.required(name).is_file());
    assert!(fixture.index().mods.contains_key(name));
    assert!(
        !fixture
            .required("sodium-fabric-mc1.20.1-0.4.10+build.24.jar")
            .exists()
    );
}

#[test]
fn apply_rolls_back_every_file_when_the_last_hash_is_wrong() {
    let fixture = Fixture::new();
    let mut table = modrinth_mod(
        "root",
        "root.jar",
        b"root",
        &[dependency("a", "required"), dependency("b", "required")].join(","),
    );
    table.extend(modrinth_mod("a", "a.jar", b"a", ""));
    table.extend(modrinth_mod("b", "b.jar", b"b", ""));
    // The root's route serves bytes that do not match its declared sha1.
    table.push(bytes_route("/files/root.jar", b"tampered"));
    let (addr, _) = serve(table);

    let error = plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap_err();

    assert!(
        matches!(&error, CatalogError::HashMismatch { file, .. } if file == "root.jar"),
        "{error}"
    );
    // Files 1 and 2 of 3 are gone again and nothing was indexed.
    for name in ["a.jar", "b.jar", "root.jar"] {
        assert!(!fixture.required(name).exists(), "{name} survived");
    }
    assert!(fixture.index().mods.is_empty());
    assert!(!fixture.server.modindex_file().exists());
}

#[test]
fn apply_refuses_a_file_name_it_cannot_key_the_index_on() {
    let fixture = Fixture::new();
    let (addr, _) = serve(modrinth_mod("evil", "../x.jar", b"x", ""));
    let catalog = catalog(addr);

    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::Modrinth,
        "evil",
        None,
    )
    .unwrap();
    let error = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap_err();

    assert!(
        matches!(&error, CatalogError::BadFileName(name) if name == "../x.jar"),
        "{error}"
    );
    // The name is refused before a single byte is written anywhere.
    assert!(!fixture.server.path.join("x.jar").exists());
    assert!(!fixture.server.modindex_file().exists());
}

#[test]
fn apply_refuses_to_take_over_a_file_name_another_mod_owns() {
    let fixture = Fixture::new();
    // A different project whose primary file carries the same name.
    std::fs::write(fixture.required("root.jar"), b"theirs").unwrap();
    fixture.write_index(&[(
        "root.jar",
        entry(ProviderId::Modrinth, "someone-else", &sha1(b"theirs")),
    )]);

    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let error = plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap_err();

    // The index is keyed by file name, so overwriting would drop a whole mod.
    assert!(error.to_string().contains("root.jar"), "{error}");
    assert_eq!(
        std::fs::read(fixture.required("root.jar")).unwrap(),
        b"theirs"
    );
    assert_eq!(fixture.index().mods["root.jar"].project_id, "someone-else");
}

#[test]
fn apply_refuses_a_plan_that_no_longer_matches_the_server() {
    let fixture = Fixture::new();
    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let catalog = catalog(addr);
    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    let refused = |plan: &_| {
        install::apply(
            &catalog,
            &fixture.server,
            plan,
            &InstallChoice::default(),
            Namespace::Required,
            &no_progress,
        )
        .unwrap_err()
    };

    // §8.4 keeps the two apart: a server that moved is reported, an old plan
    // is re-resolved. Only the second one is `Stale`.
    let mut other_server = plan.clone();
    other_server.server = "Other-1.20.1".into();
    assert!(matches!(
        refused(&other_server),
        CatalogError::ServerChanged
    ));

    let mut old = plan.clone();
    old.resolved_at = SystemTime::now() - Duration::from_secs(11 * 60);
    assert!(matches!(refused(&old), CatalogError::Stale));

    // Somebody else installed a mod between the plan and the apply.
    fixture.write_index(&[("other.jar", entry(ProviderId::Modrinth, "other", "aa"))]);
    assert!(matches!(refused(&plan), CatalogError::ServerChanged));
    assert!(!fixture.required("root.jar").exists());
    // The Korean sentence §8.4 specifies reaches the CLI intact.
    assert_eq!(
        nebula_core::error::Error::from(CatalogError::ServerChanged).to_string(),
        "서버가 바뀌었습니다 — 다시 검색하세요"
    );
}

#[test]
fn apply_refuses_while_another_process_holds_the_server() {
    let fixture = Fixture::new();
    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let catalog = catalog(addr);
    let plan = install::plan_install(
        &catalog,
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        fixture.server.path.join(".nebula-lock"),
        format!(r#"{{"pid":4242,"started_at":{now}}}"#),
    )
    .unwrap();

    let error = install::apply(
        &catalog,
        &fixture.server,
        &plan,
        &InstallChoice::default(),
        Namespace::Required,
        &no_progress,
    )
    .unwrap_err();
    assert!(
        matches!(error, CatalogError::Locked { pid: 4242 }),
        "{error}"
    );
    assert!(!fixture.required("root.jar").exists());
}

#[test]
fn a_mutation_sweeps_staging_entries_older_than_a_day() {
    let fixture = Fixture::new();
    let staging = fixture.server.path.join(".nebula-tmp");
    std::fs::create_dir_all(&staging).unwrap();
    let stale = staging.join("1-stale.part");
    let fresh = staging.join("2-fresh.part");
    std::fs::write(&stale, b"old").unwrap();
    std::fs::write(&fresh, b"new").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::now() - Duration::from_secs(48 * 60 * 60)),
        )
        .unwrap();

    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap();

    assert!(!stale.exists(), "a day-old staging entry survived");
    assert!(fresh.exists(), "a fresh staging entry was swept");
}

// ---------------------------------------------------------------------------
// Replacing and updating
// ---------------------------------------------------------------------------

/// Installs `root-1.jar` and hands back the stub address and the catalog.
fn install_root(fixture: &Fixture, extra: Vec<(String, u16, Vec<u8>)>) -> (SocketAddr, Catalog) {
    let mut table = modrinth_mod("root", "root-1.jar", b"v1", "");
    table.extend(extra);
    let (addr, _) = serve(table);
    let catalog = catalog(addr);
    plan_and_apply(&catalog, fixture, "root", Namespace::Required).unwrap();
    (addr, catalog)
}

#[test]
fn replace_never_leaves_two_versions_of_one_mod_in_the_directory() {
    let fixture = Fixture::new();
    let (addr, catalog) = install_root(&fixture, vec![bytes_route("/files/root-2.jar", b"v2")]);
    assert!(fixture.required("root-1.jar").is_file());

    let newer = version_at(addr, "root", "root-2", "root-2.jar", b"v2");
    update::replace_file(
        &catalog,
        &fixture.server,
        "root-1.jar",
        &newer,
        &no_progress,
    )
    .unwrap();

    assert!(!fixture.required("root-1.jar").exists());
    assert!(fixture.required("root-2.jar").is_file());
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 1);

    let index = fixture.index();
    assert_eq!(index.mods.len(), 1);
    let entry = &index.mods["root-2.jar"];
    assert_eq!(entry.namespace, Namespace::Required);
    assert_eq!(entry.version_id, "root-2");
    assert_eq!(entry.hashes.sha1, sha1(b"v2"));
}

#[test]
fn a_replace_that_fails_after_moving_the_old_file_out_puts_it_back() {
    let fixture = Fixture::new();
    let (addr, catalog) = install_root(&fixture, vec![bytes_route("/files/root-2.jar", b"v2")]);
    // A directory exactly where the new file has to land: step 3 fails after
    // step 2 has already moved the old file out of the namespace directory.
    std::fs::create_dir_all(fixture.required("root-2.jar")).unwrap();

    let newer = version_at(addr, "root", "root-2", "root-2.jar", b"v2");
    let error = update::replace_file(
        &catalog,
        &fixture.server,
        "root-1.jar",
        &newer,
        &no_progress,
    )
    .unwrap_err();
    assert!(matches!(error, CatalogError::Io(_)), "{error}");

    // The old file came back and still owns its index entry; at no point was
    // there a second jar beside it.
    assert!(fixture.required("root-1.jar").is_file());
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 1);
    let index = fixture.index();
    assert_eq!(index.mods.len(), 1);
    assert!(index.mods.contains_key("root-1.jar"));
}

#[test]
fn a_replace_whose_download_fails_changes_nothing() {
    let fixture = Fixture::new();
    let (addr, catalog) = install_root(&fixture, Vec::new());
    // `/files/absent.jar` is not in the table, so the stub 404s it.
    let missing = version_at(addr, "root", "root-2", "absent.jar", b"v2");
    assert!(
        update::replace_file(
            &catalog,
            &fixture.server,
            "root-1.jar",
            &missing,
            &no_progress
        )
        .is_err()
    );

    assert!(fixture.required("root-1.jar").is_file());
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 1);
    assert!(fixture.index().mods.contains_key("root-1.jar"));
}

#[test]
fn replace_refuses_a_file_nebula_did_not_install() {
    let fixture = Fixture::new();
    let (addr, catalog) = install_root(&fixture, Vec::new());
    std::fs::write(fixture.required("hand.jar"), b"h").unwrap();
    let newer = version_at(addr, "root", "root-2", "root-2.jar", b"v2");
    let error = update::replace_file(&catalog, &fixture.server, "hand.jar", &newer, &no_progress)
        .unwrap_err();
    assert!(error.to_string().contains("hand.jar"), "{error}");
}

#[test]
fn check_updates_compares_hashes_for_modrinth_in_one_request() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("old.jar"), b"old").unwrap();
    std::fs::write(fixture.required("current.jar"), b"current").unwrap();
    fixture.write_index(&[
        ("old.jar", entry(ProviderId::Modrinth, "old", &sha1(b"old"))),
        (
            "current.jar",
            entry(ProviderId::Modrinth, "cur", &sha1(b"current")),
        ),
        // No file on disk: `파일 없음`, excluded from the check.
        (
            "gone.jar",
            entry(ProviderId::Modrinth, "gone", &sha1(b"gone")),
        ),
    ]);

    let newer = version_body("old", "old-2", "old-2.jar", &sha1(b"new"), "");
    let same = version_body("cur", "cur-1", "current.jar", &sha1(b"current"), "");
    let (addr, calls) = serve(vec![route(
        "/v2/version_files/update",
        format!(
            "{{\"{}\": {newer}, \"{}\": {same}}}",
            sha1(b"old"),
            sha1(b"current")
        ),
    )]);

    let candidates = update::check_updates(&catalog(addr), &fixture.server)
        .unwrap()
        .candidates;

    // Only the file whose returned hash differs (R12), never the missing one.
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].file_name, "old.jar");
    assert_eq!(candidates[0].to.version_id, "old-2");
    assert_eq!(candidates[0].from, "v1");

    let sent = requests(&calls);
    assert_eq!(sent.len(), 1, "one bulk request for the whole server");
    let body = sent[0].json();
    let hashes: Vec<&str> = body["hashes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    assert!(hashes.contains(&sha1(b"old").as_str()));
    assert!(!hashes.contains(&sha1(b"gone").as_str()));
    assert_eq!(body["loaders"][0], "fabric");
    assert_eq!(body["game_versions"][0], "1.20.1");
}

#[test]
fn check_updates_asks_curseforge_once_per_mod_with_the_documented_spacing() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("a.jar"), b"a").unwrap();
    std::fs::write(fixture.required("b.jar"), b"b").unwrap();
    fixture.write_index(&[
        ("a.jar", entry(ProviderId::CurseForge, "1", &sha1(b"a"))),
        ("b.jar", entry(ProviderId::CurseForge, "2", &sha1(b"b"))),
    ]);

    let files = |id: u64, name: &str, digest: &str| {
        route(
            &format!("/v1/mods/{id}/files"),
            format!(
                r#"{{"data":[{{"id":{id}0,"modId":{id},"fileName":"{name}","releaseType":1,
                   "fileDate":"2026-02-01T00:00:00Z","fileLength":1,
                   "downloadUrl":"{STUB}/files/{name}",
                   "hashes":[{{"value":"{digest}","algo":1}}],
                   "sortableGameVersions":[
                       {{"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125}},
                       {{"gameVersionName":"Fabric","gameVersionTypeId":68441}}],
                   "isServerPack":false}}]}}"#
            ),
        )
    };
    let (addr, calls) = serve(vec![
        files(1, "a-2.jar", &sha1(b"a2")),
        // The installed bytes come back unchanged, so this is not an update.
        files(2, "b.jar", &sha1(b"b")),
    ]);

    let candidates = update::check_updates(&catalog(addr), &fixture.server)
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].file_name, "a.jar");

    let recorded = calls.lock().unwrap();
    assert_eq!(
        recorded.len(),
        2,
        "one filtered /files call per managed CurseForge mod (D7)"
    );
    let gap = recorded[1].0.duration_since(recorded[0].0);
    assert!(gap >= Duration::from_millis(250), "{gap:?}");
    for (_, request) in recorded.iter() {
        assert_eq!(request.header("x-api-key"), Some("test-key"));
    }
}

#[test]
fn apply_updates_runs_one_mod_at_a_time_and_reports_each() {
    let fixture = Fixture::new();
    let (addr, catalog) = install_root(&fixture, vec![bytes_route("/files/root-2.jar", b"v2")]);
    let candidates = vec![
        UpdateCandidate {
            file_name: "root-1.jar".into(),
            title: "root".into(),
            from: "root-1".into(),
            to: version_at(addr, "root", "root-2", "root-2.jar", b"v2"),
        },
        // Nothing in the index claims this file, so only this row fails.
        UpdateCandidate {
            file_name: "unknown.jar".into(),
            title: "unknown".into(),
            from: "?".into(),
            to: version_at(addr, "root", "root-3", "root-2.jar", b"v2"),
        },
    ];

    let (seen, progress) = steps();
    let results = update::apply_updates(&catalog, &fixture.server, &candidates, &progress)
        .expect("the run itself succeeded; only one row failed");

    assert_eq!(results.len(), 2);
    assert!(results[0].1.is_ok(), "{:?}", results[0].1);
    assert!(results[1].1.is_err(), "an unmanaged row must fail alone");
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (1, 2, "root-2.jar".to_owned()),
            (2, 2, "root-2.jar".to_owned())
        ]
    );
    assert!(fixture.required("root-2.jar").is_file());
    assert!(fixture.index().mods.contains_key("root-2.jar"));
}

// ---------------------------------------------------------------------------
// Remove, move and list
// ---------------------------------------------------------------------------

#[test]
fn remove_and_move_keep_the_index_in_step_with_the_directories() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("a.jar"), b"a").unwrap();
    std::fs::write(fixture.required("b.jar"), b"b").unwrap();
    fixture.write_index(&[
        ("a.jar", entry(ProviderId::Modrinth, "a", &sha1(b"a"))),
        ("b.jar", entry(ProviderId::Modrinth, "b", &sha1(b"b"))),
    ]);

    manage::move_mod(&fixture.server, "a.jar", Namespace::OptionalOff).unwrap();
    assert!(!fixture.required("a.jar").exists());
    assert!(
        fixture
            .namespace(Namespace::OptionalOff)
            .join("a.jar")
            .is_file()
    );
    assert_eq!(
        fixture.index().mods["a.jar"].namespace,
        Namespace::OptionalOff
    );

    manage::remove_mod(&fixture.server, "b.jar").unwrap();
    assert!(!fixture.required("b.jar").exists());
    assert!(!fixture.index().mods.contains_key("b.jar"));
    assert_eq!(fixture.index().mods.len(), 1);

    // A hand-placed jar moves too, without gaining an index entry.
    std::fs::write(fixture.required("hand.jar"), b"h").unwrap();
    manage::move_mod(&fixture.server, "hand.jar", Namespace::OptionalOn).unwrap();
    assert!(
        fixture
            .namespace(Namespace::OptionalOn)
            .join("hand.jar")
            .is_file()
    );
    assert!(!fixture.index().mods.contains_key("hand.jar"));

    // A `배포 불가` file is not a jar, and deleting it is the fix §8.3 offers.
    std::fs::write(fixture.required("notes.txt"), b"n").unwrap();
    manage::remove_mod(&fixture.server, "notes.txt").unwrap();
    assert!(!fixture.required("notes.txt").exists());

    // A name that is not one entry of one namespace never reaches the disk.
    assert!(manage::remove_mod(&fixture.server, "../servermeta.json").is_err());
    assert!(manage::move_mod(&fixture.server, "..", Namespace::Required).is_err());
    assert!(fixture.server.server_meta_file().is_file());
}

#[test]
fn list_installed_flags_hand_placed_missing_and_unshippable_files() {
    let fixture = Fixture::new();
    write_jar(
        &fixture.required("managed.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"managed","version":"1","name":"Managed"}"#,
        )],
    );
    std::fs::write(fixture.required("notes.txt"), b"not a jar").unwrap();
    write_jar(
        &fixture.namespace(Namespace::OptionalOn).join("hand.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"hand","version":"2","name":"Hand"}"#,
        )],
    );
    fixture.write_index(&[
        (
            "managed.jar",
            entry(ProviderId::Modrinth, "managed", &sha1(b"m")),
        ),
        ("absent.jar", entry(ProviderId::Modrinth, "absent", "aa")),
    ]);

    let rows = manage::list_installed(&fixture.server).unwrap();
    let by_name: HashMap<&str, &InstalledMod> = rows
        .iter()
        .map(|row| (row.file_name.as_str(), row))
        .collect();

    assert_eq!(rows.len(), 4);
    assert!(!by_name["managed.jar"].is_unmanaged());
    assert!(by_name["hand.jar"].is_unmanaged());
    assert_eq!(by_name["hand.jar"].namespace, Namespace::OptionalOn);
    assert!(by_name["absent.jar"].missing);
    assert!(by_name["notes.txt"].unshippable.is_some());
    assert_eq!(by_name["hand.jar"].title(), "Hand");
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// A 1x1 lossless WebP. `image` 0.25 decodes WebP but cannot encode it, so the
/// bytes are a literal; the test decoding them is the check that they are a
/// real WebP and that the format is sniffed, never read off the URL.
const WEBP_1X1: [u8; 34] = [
    0x52, 0x49, 0x46, 0x46, 0x1A, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x4C,
    0x0D, 0x00, 0x00, 0x00, 0x2F, 0x00, 0x00, 0x00, 0x10, 0x07, 0x10, 0x11, 0x11, 0x88, 0x88, 0xFE,
    0x07, 0x00,
];

fn encoded(format: image::ImageFormat) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut out, format)
        .unwrap();
    out.into_inner()
}

#[test]
fn icons_decode_every_served_format_and_are_written_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());

    for (name, body) in [
        ("png", encoded(image::ImageFormat::Png)),
        ("jpeg", encoded(image::ImageFormat::Jpeg)),
        ("webp", WEBP_1X1.to_vec()),
    ] {
        // Named `.png` whatever it holds, so only the magic bytes can decide.
        let (addr, calls) = serve(vec![bytes_route("/icon.png", &body)]);
        let catalog = catalog(addr);
        let url = format!("http://{addr}/icon.png");

        let first = icons::fetch(&catalog, &root, ProviderId::Modrinth, "p", &url).unwrap();
        let IconRef::Path(path) = first else {
            panic!("{name}: a Modrinth icon is a cached path");
        };
        assert!(path.starts_with(root.icons_cache_dir()), "{name}");
        assert_eq!(path.extension().unwrap(), "png", "{name}");
        let decoded = image::open(&path).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (ICON_SIZE, ICON_SIZE),
            "{name}"
        );

        // The second call reuses the file and makes no request at all.
        let written = std::fs::read(&path).unwrap();
        let again = icons::fetch(&catalog, &root, ProviderId::Modrinth, "p", &url).unwrap();
        assert_eq!(again, IconRef::Path(path.clone()), "{name}");
        assert_eq!(std::fs::read(&path).unwrap(), written, "{name}");
        assert_eq!(requests(&calls).len(), 1, "{name}: one request only");
    }
}

#[test]
fn a_curseforge_icon_never_reaches_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
    let (addr, calls) = serve(vec![bytes_route(
        "/logo",
        &encoded(image::ImageFormat::Png),
    )]);
    let catalog = catalog(addr);
    let url = format!("http://{addr}/logo");

    let first = icons::fetch(&catalog, &root, ProviderId::CurseForge, "123", &url).unwrap();
    let IconRef::Rgba(bytes) = first else {
        panic!("a CurseForge icon is memory only (D2)");
    };
    assert_eq!((bytes.w, bytes.h), (ICON_SIZE, ICON_SIZE));
    assert_eq!(bytes.rgba.len() as u32, ICON_SIZE * ICON_SIZE * 4);
    assert!(!root.icons_cache_dir().exists(), "nothing may be cached");

    // The session cache answers the second call; the key is the project id.
    let again = icons::fetch(&catalog, &root, ProviderId::CurseForge, "123", &url).unwrap();
    assert_eq!(again, IconRef::Rgba(bytes));
    assert_eq!(requests(&calls).len(), 1);
    assert!(!root.icons_cache_dir().exists());
    // The key survives the round trip; it is what the GUI looks up.
    assert_eq!(requests(&calls)[0].header("x-api-key"), Some("test-key"));
}

#[test]
fn a_jar_icon_is_cached_under_the_jars_own_digest() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
    let jar = dir.path().join("mod.jar");
    std::fs::write(&jar, b"jar bytes").unwrap();

    let path = icons::local_icon(&root, &jar, &encoded(image::ImageFormat::Png)).unwrap();
    let expected = format!("local-{}.png", hash::sha1_hex_of_file(&jar).unwrap());
    assert_eq!(path.file_name().unwrap().to_string_lossy(), expected);
    let decoded = image::open(&path).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (ICON_SIZE, ICON_SIZE));
    assert_eq!(
        icons::local_icon(&root, &jar, &encoded(image::ImageFormat::Png)).unwrap(),
        path
    );
}

// ---------------------------------------------------------------------------
// Server management
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(not(windows))]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[test]
fn delete_refuses_everything_that_is_not_exactly_one_server_directory() {
    let fixture = Fixture::new();
    // A trailing space is the interesting one: Windows normalises it away
    // while resolving, so only the resolved file name catches it.
    for name in [".", "..", "", "../servers", "a/b", "Test-1.20.1 "] {
        let error = manage::delete_server(&fixture.root, name).unwrap_err();
        assert!(
            !matches!(error, CatalogError::Locked { .. }),
            "{name}: {error}"
        );
        assert!(fixture.server.path.is_dir(), "{name} deleted the server");
    }
    // The trailing-space refusal says what actually went wrong, rather than
    // reusing the containment message about a path that is in the right place.
    // Only Windows resolves "Test-1.20.1 " to the real directory; elsewhere it
    // names a directory that does not exist, which the loop above refuses.
    if cfg!(windows) {
        let error = manage::delete_server(&fixture.root, "Test-1.20.1 ").unwrap_err();
        assert!(
            error.to_string().contains("앞뒤 공백을 확인하세요"),
            "{error}"
        );
    }

    // A symlink pointing outside `servers` is refused by the resolved parent.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.txt"), b"k").unwrap();
    let link = fixture.root.servers_dir().join("Link-1.20.1");
    if symlink_dir(outside.path(), &link).is_ok() {
        let error = manage::delete_server(&fixture.root, "Link-1.20.1").unwrap_err();
        assert!(matches!(error, CatalogError::Other(_)), "{error}");
        assert!(
            outside.path().join("keep.txt").is_file(),
            "the symlink target was followed"
        );
        let _ = std::fs::remove_dir(&link);
    } else {
        println!("skipping the symlink case: this platform refuses symlink creation");
    }

    manage::delete_server(&fixture.root, "Test-1.20.1").unwrap();
    assert!(!fixture.server.path.exists());
    assert!(fixture.root.servers_dir().is_dir());
}

#[test]
fn duplicate_stages_then_renames_and_never_shows_a_partial_directory() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("a.jar"), b"a").unwrap();
    fixture.write_index(&[("a.jar", entry(ProviderId::Modrinth, "a", &sha1(b"a")))]);
    std::fs::create_dir_all(fixture.server.path.join(".nebula-tmp").join("junk")).unwrap();
    // A day-old leftover from an interrupted duplicate, removed on the next
    // attempt — unlike the fresh one beside it, which may well be another
    // process still copying into it.
    let stale = fixture.root.servers_dir().join(".Old-1.20.1.partial");
    let busy = fixture.root.servers_dir().join(".Busy-1.20.1.partial");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::create_dir_all(&busy).unwrap();
    backdate(&stale, Duration::from_secs(48 * 60 * 60));
    // A kill mid-copy leaves this run's own staging behind; copying over it
    // would merge the dead run's files into the new server.
    let mine = fixture.root.servers_dir().join(".Copy-1.20.1.partial");
    std::fs::create_dir_all(mine.join("fabricmods").join("required")).unwrap();
    std::fs::write(
        mine.join("fabricmods").join("required").join("dead.jar"),
        b"half copied",
    )
    .unwrap();

    let new_dir = manage::duplicate_server(&fixture.root, "Test-1.20.1", "Copy").unwrap();
    assert_eq!(new_dir, "Copy-1.20.1");
    assert!(!stale.exists());
    assert!(busy.is_dir(), "a fresh .partial was swept");
    std::fs::remove_dir_all(&busy).unwrap();

    let copy = ServerDir::new(fixture.root.clone(), &new_dir);
    assert!(copy.path.join("fabricmods/required/a.jar").is_file());
    assert!(
        !copy.path.join("fabricmods/required/dead.jar").exists(),
        "the interrupted copy's files were merged into the new server"
    );
    // `modindex.json` comes across as-is: the copy holds the same mods.
    assert_eq!(
        std::fs::read(copy.modindex_file()).unwrap(),
        std::fs::read(fixture.server.modindex_file()).unwrap()
    );
    // Neither the staging directory nor the live lock is copied.
    assert!(!copy.path.join(".nebula-tmp").exists());
    assert!(!copy.path.join(".nebula-lock").exists());

    let meta = nebula_core::server::read_server_meta(&copy).unwrap();
    assert_eq!(meta.meta.name, "Copy (Minecraft 1.20.1)");
    // The description carries the id too, and it ships in distribution.json,
    // so a copy that kept it would advertise the server it came from.
    assert!(
        meta.meta.description.starts_with("Copy Running Minecraft"),
        "{}",
        meta.meta.description
    );

    // Nothing half-built is left, and both servers list cleanly.
    let names: Vec<String> = list_servers(&fixture.root)
        .unwrap()
        .into_iter()
        .map(|summary| summary.dir_name)
        .collect();
    assert_eq!(names, ["Copy-1.20.1", "Test-1.20.1"]);
    assert!(
        !std::fs::read_dir(fixture.root.servers_dir())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".partial"))
    );

    // The same id twice is refused, with the 새 서버 message.
    let error = manage::duplicate_server(&fixture.root, "Test-1.20.1", "Copy").unwrap_err();
    assert!(error.to_string().contains("이미 있습니다"), "{error}");
    for bad in ["../evil", "a/b", ""] {
        assert!(
            manage::duplicate_server(&fixture.root, "Test-1.20.1", bad).is_err(),
            "{bad} was accepted"
        );
    }
}

// ---------------------------------------------------------------------------
// Name collisions, release preference and provenance adoption
// ---------------------------------------------------------------------------

#[test]
fn apply_never_writes_over_a_hand_placed_jar() {
    let fixture = Fixture::new();
    // Nothing in the index knows about this file; only the directory does.
    std::fs::write(fixture.required("root.jar"), b"by hand").unwrap();

    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let error = plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap_err();

    assert!(
        matches!(&error, CatalogError::NameTaken { file } if file == "root.jar"),
        "{error}"
    );
    // The bytes the user put there are untouched, and no rollback deleted them.
    assert_eq!(
        std::fs::read(fixture.required("root.jar")).unwrap(),
        b"by hand"
    );
    assert!(!fixture.server.modindex_file().exists());

    // The same name in another namespace is refused too: two files with one
    // name would hand the scanners two modules that cannot both ship.
    std::fs::remove_file(fixture.required("root.jar")).unwrap();
    let elsewhere = fixture.namespace(Namespace::OptionalOff).join("root.jar");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, b"by hand").unwrap();
    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let error = plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap_err();
    assert!(matches!(error, CatalogError::NameTaken { .. }), "{error}");
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"by hand");
}

#[test]
fn apply_refuses_two_planned_files_that_share_one_name() {
    let fixture = Fixture::new();
    // Two different projects whose primary file carries the same name; the
    // index cannot see the collision, because nothing is inserted yet.
    let mut table = modrinth_mod("root", "same.jar", b"root", &dependency("dep", "required"));
    table.extend(modrinth_mod("dep", "same.jar", b"dep", ""));
    let (addr, _) = serve(table);

    let error = plan_and_apply(&catalog(addr), &fixture, "root", Namespace::Required).unwrap_err();
    assert!(matches!(error, CatalogError::NameTaken { .. }), "{error}");
    assert_eq!(jars(&fixture.namespace(Namespace::Required)), 0);
}

#[test]
fn a_plan_carries_the_version_number_and_publication_date() {
    let fixture = Fixture::new();
    let (addr, _) = serve(modrinth_mod("root", "root.jar", b"root", ""));
    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();

    // §8.4's confirm line needs the number and the date, not the free-text
    // version name, which on Modrinth usually repeats the mod's title.
    assert_eq!(plan.root.version_number, "root-1");
    assert_eq!(plan.root.published, "2026-01-01T00:00:00Z");
    assert_eq!(plan.root.text_provider, ProviderId::Modrinth);
}

#[test]
fn an_incompatible_hand_placed_jar_is_matched_by_its_mod_id() {
    let fixture = Fixture::new();
    // No index entry at all: only the jar's own `fabric.mod.json` says it is
    // the conflicting project.
    write_jar(
        &fixture.required("sodium-hand.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"sodium","version":"0.5.13","name":"Sodium"}"#,
        )],
    );
    let mut table = modrinth_mod(
        "root",
        "root.jar",
        b"root",
        &dependency("sodium", "incompatible"),
    );
    table.push(route("/v2/project/sodium", project("sodium")));
    let (addr, _) = serve(table);

    let plan = install::plan_install(
        &catalog(addr),
        &fixture.server,
        ProviderId::Modrinth,
        "root",
        None,
    )
    .unwrap();
    assert_eq!(plan.incompatible, ["sodium-hand.jar"]);
}

/// One Modrinth version with an explicit release type.
fn typed_version(
    project_id: &str,
    version_id: &str,
    file_name: &str,
    digest: &str,
    kind: &str,
) -> String {
    format!(
        r#"{{"id":"{version_id}","project_id":"{project_id}","name":"{version_id}",
            "version_number":"{version_id}","version_type":"{kind}",
            "date_published":"2026-01-01T00:00:00Z","game_versions":["1.20.1"],
            "loaders":["fabric"],
            "files":[{{"filename":"{file_name}","url":"{STUB}/files/{file_name}",
                       "primary":true,"size":3,"hashes":{{"sha1":"{digest}"}}}}],
            "dependencies":[]}}"#
    )
}

/// A managed Modrinth mod installed at `release`, whose file holds `bytes`.
fn installed_at(fixture: &Fixture, bytes: &[u8], release: ReleaseType) {
    std::fs::write(fixture.required("mod.jar"), bytes).unwrap();
    let mut entry = entry(ProviderId::Modrinth, "proj", &sha1(bytes));
    entry.release = Some(release);
    fixture.write_index(&[("mod.jar", entry)]);
}

#[test]
fn an_installed_release_is_never_offered_a_beta() {
    let fixture = Fixture::new();
    installed_at(&fixture, b"v1", ReleaseType::Release);

    let beta = typed_version("proj", "v2", "mod-v2.jar", &sha1(b"v2"), "beta");
    let release = typed_version("proj", "v1", "mod.jar", &sha1(b"v1"), "release");
    let (addr, _) = serve(vec![
        route(
            "/v2/version_files/update",
            format!("{{\"{}\": {beta}}}", sha1(b"v1")),
        ),
        // D3 over the project's own list finds nothing newer than what is
        // installed, so the beta is not offered at all.
        route("/v2/project/proj/version", format!("[{beta},{release}]")),
    ]);

    let report = update::check_updates(&catalog(addr), &fixture.server).unwrap();
    assert!(report.candidates.is_empty(), "{:?}", report.candidates);
}

#[test]
fn a_release_hiding_behind_a_beta_is_offered_instead_of_the_beta() {
    let fixture = Fixture::new();
    installed_at(&fixture, b"v1", ReleaseType::Release);

    let beta = typed_version("proj", "v3", "mod-v3.jar", &sha1(b"v3"), "beta");
    let newer = typed_version("proj", "v2", "mod-v2.jar", &sha1(b"v2"), "release");
    let old = typed_version("proj", "v1", "mod.jar", &sha1(b"v1"), "release");
    let (addr, _) = serve(vec![
        route(
            "/v2/version_files/update",
            format!("{{\"{}\": {beta}}}", sha1(b"v1")),
        ),
        route(
            "/v2/project/proj/version",
            format!("[{beta},{newer},{old}]"),
        ),
    ]);

    let candidates = update::check_updates(&catalog(addr), &fixture.server)
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].to.version_id, "v2");
    assert_eq!(candidates[0].to.release, ReleaseType::Release);
}

#[test]
fn an_installed_beta_is_still_offered_a_newer_beta() {
    let fixture = Fixture::new();
    installed_at(&fixture, b"v1", ReleaseType::Beta);

    let beta = typed_version("proj", "v2", "mod-v2.jar", &sha1(b"v2"), "beta");
    // No `/v2/project/proj/version` route: a same-rank answer is taken
    // straight from the bulk request, without a second round trip.
    let (addr, _) = serve(vec![route(
        "/v2/version_files/update",
        format!("{{\"{}\": {beta}}}", sha1(b"v1")),
    )]);

    let candidates = update::check_updates(&catalog(addr), &fixture.server)
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].to.version_id, "v2");
}

#[test]
fn a_deleted_curseforge_project_does_not_cost_the_other_mods_their_check() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("gone.jar"), b"gone").unwrap();
    std::fs::write(fixture.required("live.jar"), b"live").unwrap();
    fixture.write_index(&[
        (
            "gone.jar",
            entry(ProviderId::CurseForge, "1", &sha1(b"gone")),
        ),
        (
            "live.jar",
            entry(ProviderId::CurseForge, "2", &sha1(b"live")),
        ),
    ]);

    // `/v1/mods/1/files` is absent, so the stub 404s it.
    let (addr, _) = serve(vec![route(
        "/v1/mods/2/files",
        format!(
            r#"{{"data":[{{"id":20,"modId":2,"fileName":"live-2.jar","releaseType":1,
               "fileDate":"2026-02-01T00:00:00Z","fileLength":1,
               "downloadUrl":"{STUB}/files/live-2.jar",
               "hashes":[{{"value":"{}","algo":1}}],
               "sortableGameVersions":[
                   {{"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125}},
                   {{"gameVersionName":"Fabric","gameVersionTypeId":68441}}],
               "isServerPack":false}}]}}"#,
            sha1(b"live2")
        ),
    )]);

    let candidates = update::check_updates(&catalog(addr), &fixture.server)
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].file_name, "live.jar");
}

#[test]
fn an_update_check_adopts_a_hand_placed_jar_modrinth_knows() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("hand.jar"), b"hand").unwrap();

    let identified = version_body("handproj", "hand-1", "hand.jar", &sha1(b"hand"), "");
    let (addr, _) = serve(vec![
        route(
            "/v2/version_files",
            format!("{{\"{}\": {identified}}}", sha1(b"hand")),
        ),
        // The adopted entry takes part in the very same comparison.
        route("/v2/version_files/update", "{}".to_owned()),
    ]);

    let report = update::check_updates(&catalog(addr), &fixture.server).unwrap();
    assert_eq!(report.adopted, ["hand.jar"]);
    assert!(report.candidates.is_empty());

    let entry = &fixture.index().mods["hand.jar"];
    assert_eq!(entry.provider, ProviderId::Modrinth);
    assert_eq!(entry.project_id, "handproj");
    assert_eq!(entry.version_id, "hand-1");
    assert_eq!(entry.release, Some(ReleaseType::Release));
    assert_eq!(entry.hashes.sha1, sha1(b"hand"));
    assert_eq!(entry.namespace, Namespace::Required);

    // A second check has nothing left to adopt.
    let again = update::check_updates(&catalog(addr), &fixture.server).unwrap();
    assert!(again.adopted.is_empty());
}

#[test]
fn an_update_check_adopts_a_hand_placed_jar_curseforge_fingerprints() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("cf.jar"), b"cf-hand").unwrap();
    let fingerprint = hash::curseforge_fingerprint(b"cf-hand");

    let (addr, _) = serve(vec![
        // Modrinth does not know these bytes.
        route("/v2/version_files", "{}".to_owned()),
        route(
            "/v1/fingerprints",
            format!(
                r#"{{"data":{{"exactMatches":[{{"id":77,"file":{{"id":9001,"modId":77,
                   "fileName":"cf.jar","releaseType":2,"fileFingerprint":{fingerprint},
                   "fileDate":"2026-01-01T00:00:00Z","fileLength":7,
                   "downloadUrl":"{STUB}/files/cf.jar","hashes":[],
                   "sortableGameVersions":[]}}}}],
                   "exactFingerprints":[{fingerprint}],"unmatchedFingerprints":null}}}}"#
            ),
        ),
        // No `/v1/mods/77/files`: the project answers nothing, which the
        // check skips rather than failing on.
        route("/v2/version_files/update", "{}".to_owned()),
    ]);

    let report = update::check_updates(&catalog(addr), &fixture.server).unwrap();
    assert_eq!(report.adopted, ["cf.jar"]);

    let entry = &fixture.index().mods["cf.jar"];
    assert_eq!(entry.provider, ProviderId::CurseForge);
    assert_eq!(entry.project_id, "77");
    assert_eq!(entry.release, Some(ReleaseType::Beta));
    // D2: a CurseForge row still carries no free text on disk.
    let raw = std::fs::read_to_string(fixture.server.modindex_file()).unwrap();
    assert!(!raw.contains("title"), "{raw}");
    assert!(!raw.contains("project_slug"), "{raw}");
}

#[test]
fn remove_refuses_while_the_mod_directory_cannot_be_read() {
    let fixture = Fixture::new();
    std::fs::write(fixture.required("a.jar"), b"a").unwrap();
    fixture.write_index(&[("a.jar", entry(ProviderId::Modrinth, "a", &sha1(b"a")))]);
    // The share the mods live on went away: every row would read `파일 없음`,
    // and dropping the entry would unmanage a mod whose jar is fine (D1).
    std::fs::remove_dir_all(&fixture.container).unwrap();

    let error = manage::remove_mod(&fixture.server, "a.jar").unwrap_err();
    assert!(error.to_string().contains("Test-1.20.1"), "{error}");
    assert!(fixture.index().mods.contains_key("a.jar"));
}

#[test]
fn remove_clears_the_last_stale_row_of_a_readable_empty_container() {
    let fixture = Fixture::new();
    // No jar anywhere, but the three namespace directories are right there:
    // the container is empty, not unreadable, so D1 does not refuse and the
    // user can finally drop the `파일 없음` row (F9's accepted ceiling, lifted).
    fixture.write_index(&[("a.jar", entry(ProviderId::Modrinth, "a", &sha1(b"a")))]);
    assert!(fixture.required("a.jar").parent().unwrap().is_dir());

    manage::remove_mod(&fixture.server, "a.jar").unwrap();
    assert!(fixture.index().mods.is_empty());
}

#[test]
fn the_index_names_the_mods_that_need_one_file() {
    let fixture = Fixture::new();
    let mut sodium = entry(ProviderId::Modrinth, "sodium", "aa");
    sodium.title = Some("Sodium".to_owned());
    sodium.dependencies = vec!["fabric-api".to_owned()];
    let mut extra = entry(ProviderId::Modrinth, "extra", "bb");
    extra.dependencies = vec!["fabric-api".to_owned()];
    fixture.write_index(&[
        (
            "fabric-api.jar",
            entry(ProviderId::Modrinth, "fabric-api", "cc"),
        ),
        ("sodium.jar", sodium),
        ("extra.jar", extra),
    ]);

    let index = fixture.index();
    // Titled where the provider allows one, file name where it does not.
    assert_eq!(
        manage::dependents_of(&index, "fabric-api.jar"),
        ["extra.jar", "Sodium"]
    );
    assert!(manage::dependents_of(&index, "sodium.jar").is_empty());
    assert!(manage::dependents_of(&index, "absent.jar").is_empty());
}

#[test]
fn server_context_reads_the_version_and_loader_of_one_server() {
    let fixture = Fixture::new();
    let ctx = catalog::server_context(&fixture.server).unwrap();
    assert_eq!(ctx.mc, mc());
    assert_eq!(ctx.loader, Loader::Fabric);
    assert!(ctx.container.ends_with("fabricmods"));

    // A server with no loader has nowhere to put a mod.
    let vanilla = create_server(
        &fixture.root,
        "Plain",
        &mc(),
        &CreateServerOptions::default(),
    )
    .unwrap();
    let error = catalog::server_context(&vanilla.dir).unwrap_err();
    assert!(
        error.to_string().contains("모드 로더가 없습니다"),
        "{error}"
    );
}
