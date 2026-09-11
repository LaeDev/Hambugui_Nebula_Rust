//! Mod catalog: provider mapping, the shared HTTP policy, the install index,
//! the server lock and the jar reader — all offline, against the in-process
//! stub in `common`. Nothing here touches api.modrinth.com or
//! api.curseforge.com; the one live check is gated by `NEBULA_NET_TESTS=1`.

mod common;

use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{requests, stub};
use nebula_core::catalog::index::{IndexEntry, IndexHashes, ModIndex, reconcile, reconcile_with};
use nebula_core::catalog::jarmeta::{self, LocalFile};
use nebula_core::catalog::lock::ServerLock;
use nebula_core::catalog::{
    Catalog, CatalogError, Endpoints, HashAlgo, Loader, ProviderId, SearchQuery, SortKey,
};
use nebula_core::hash;
use nebula_core::layout::{Root, ServerDir};
use nebula_core::scan::{Namespace, init_toggleable};
use nebula_core::version::MinecraftVersion;
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";

fn endpoints(addr: SocketAddr) -> Endpoints {
    Endpoints {
        modrinth: format!("http://{addr}"),
        curseforge: format!("http://{addr}/v1"),
    }
}

fn catalog(addr: SocketAddr) -> Catalog {
    Catalog::new(endpoints(addr), Some("test-key".into()))
}

fn mc() -> MinecraftVersion {
    "1.20.1".parse().unwrap()
}

fn search_query(sort: SortKey, loader: Loader) -> SearchQuery {
    SearchQuery {
        query: "jei".into(),
        sort,
        limit: 20,
        offset: 40,
        mc: mc(),
        loader,
    }
}

fn json(body: &'static str) -> (u16, &'static str, Vec<u8>) {
    (200, "application/json", body.as_bytes().to_vec())
}

fn server_dir(dir: &Path) -> ServerDir {
    ServerDir::new(
        Root::new(dir, Url::parse(BASE).unwrap()),
        "Test-1.20.1".to_owned(),
    )
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

// --- Modrinth -------------------------------------------------------------

const SEARCH_BODY: &str = r#"{
  "hits": [
    {"project_id":"AANobbMI","project_type":"mod","slug":"sodium","title":"Sodium",
     "author":"jellysquid3","description":"fast","downloads":42,
     "date_modified":"2026-01-02T03:04:05Z","icon_url":"https://cdn.invalid/i.webp",
     "client_side":"required","server_side":"unsupported","environment":["client_only"]},
    {"project_id":"exodus","project_type":"modpack","slug":"exodus","title":"Exodus",
     "description":"a pack","downloads":7,"all_project_types":["mod","modpack"]}
  ],
  "offset": 0, "limit": 20, "total_hits": 348
}"#;

#[test]
fn modrinth_search_facets_the_loader_and_drops_non_mod_hits() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v2/search" => json(SEARCH_BODY),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();

    let page = modrinth
        .search(&search_query(SortKey::Downloads, Loader::Forge))
        .unwrap();

    // The `project_type:mod` facet is rewritten server-side to the plural
    // all-versions field, so the modpack has to be dropped by its own type.
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].slug, "sodium");
    assert_eq!(page.hits[0].title, "Sodium");
    assert_eq!(page.hits[0].downloads, 42);
    assert_eq!(page.hits[0].provider, ProviderId::Modrinth);
    assert_eq!(
        page.hits[0].page_url.as_deref(),
        Some("https://modrinth.com/mod/sodium")
    );
    assert_eq!(page.total, 348);

    let sent = requests(&calls);
    let query = sent[0].query();
    assert_eq!(
        query["facets"],
        r#"[["project_type:mod"],["categories:forge"],["versions:1.20.1"]]"#
    );
    assert_eq!(query["query"], "jei");
    assert_eq!(query["limit"], "20");
    // Paging is `offset = n * limit`; the echoed offset is never read back.
    assert_eq!(query["offset"], "40");
    let agent = sent[0].header("user-agent").unwrap();
    assert!(agent.starts_with("nebula/"), "{agent}");
    assert!(agent.contains("github.com/LaeDev/nebularust"), "{agent}");
}

#[test]
fn modrinth_maps_every_sort_key_to_an_index() {
    let (addr, calls) = stub(|_| json(SEARCH_BODY));
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();
    for sort in [SortKey::Relevance, SortKey::Downloads, SortKey::Newest] {
        modrinth
            .search(&search_query(sort, Loader::Fabric))
            .unwrap();
    }
    let indexes: Vec<String> = requests(&calls)
        .iter()
        .map(|req| req.query()["index"].clone())
        .collect();
    assert_eq!(indexes, ["relevance", "downloads", "updated"]);
}

/// A version with no primary file, a sources jar first and a legacy `%2B`
/// name — the three traps of §A.4 in one payload.
const VERSIONS_BODY: &str = r#"[
  {"id":"OihdIimA","project_id":"P7dR8mSH","name":"0.29.5","version_number":"0.29.5+1.17",
   "version_type":"release","date_published":"2021-01-23T00:00:00Z",
   "game_versions":["1.20.1"],"loaders":["fabric","quilt"],
   "files":[
     {"hashes":{"sha1":"AAAA1111"},"url":"https://cdn.invalid/a-sources.jar",
      "filename":"fabric-api-0.29.5%2B1.17-sources.jar","primary":false,"size":10},
     {"hashes":{"sha1":"bbbb2222","sha512":"cccc3333"},
      "url":"https://cdn.modrinth.com/data/P7dR8mSH/versions/0.29.5%2B1.17/fabric-api-0.29.5%2B1.17.jar",
      "filename":"fabric-api-0.29.5%2B1.17.jar","primary":false,"size":20,"file_type":null}
   ],
   "dependencies":[{"project_id":"AANobbMI","version_id":null,"dependency_type":"required"},
                   {"project_id":null,"version_id":null,"file_name":"bundled.jar","dependency_type":"embedded"}]},
  {"id":"older","project_id":"P7dR8mSH","version_number":"0.28.0","version_type":"beta",
   "date_published":"2020-01-01T00:00:00Z","game_versions":["1.20.1"],"loaders":["fabric"],
   "files":[{"hashes":{"sha1":"dddd4444"},"url":"https://cdn.invalid/b.jar",
             "filename":"b.jar","primary":true,"size":30}]}
]"#;

#[test]
fn modrinth_picks_the_primary_file_and_writes_the_name_verbatim() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v2/project/fabric-api/version" => json(VERSIONS_BODY),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();

    let versions = modrinth
        .versions("fabric-api", &mc(), Loader::Fabric)
        .unwrap();
    assert_eq!(versions.len(), 2);
    // Newest first, whatever order the server used.
    assert_eq!(versions[0].version_id, "OihdIimA");

    let file = versions[0].primary_file().unwrap();
    assert_eq!(file.file_name, "fabric-api-0.29.5%2B1.17.jar");
    assert_eq!(
        file.url.as_deref(),
        Some(
            "https://cdn.modrinth.com/data/P7dR8mSH/versions/0.29.5%2B1.17/fabric-api-0.29.5%2B1.17.jar"
        )
    );
    assert_eq!(file.sha512.as_deref(), Some("cccc3333"));
    assert_eq!(versions[0].dependencies.len(), 2);
    // Quilt is not a loader we ship, so it is simply dropped.
    assert_eq!(versions[0].loaders, [Loader::Fabric]);

    // D3 prefers the release over the older beta.
    let latest = modrinth
        .latest_file("fabric-api", &mc(), Loader::Fabric)
        .unwrap()
        .unwrap();
    assert_eq!(latest.version_id, "OihdIimA");

    let query = requests(&calls)[0].query();
    assert_eq!(query["loaders"], r#"["fabric"]"#);
    assert_eq!(query["game_versions"], r#"["1.20.1"]"#);
    assert_eq!(query["include_changelog"], "false");
}

#[test]
fn modrinth_sends_the_algorithm_the_hash_length_implies() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v2/version_files" => json(
            r#"{"bbbb2222":{"id":"OihdIimA","project_id":"P7dR8mSH","version_type":"release",
                 "files":[{"hashes":{"sha1":"bbbb2222"},"url":"u","filename":"a.jar","primary":true}]}}"#,
        ),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();

    let sha512 = "AB".repeat(64);
    modrinth
        .versions_by_hashes(HashAlgo::Sha1, &[sha512])
        .unwrap();
    let sha1 = "CD".repeat(20);
    let found = modrinth
        .versions_by_hashes(HashAlgo::Sha512, &[sha1])
        .unwrap();
    assert!(found.contains_key("bbbb2222"));

    let sent = requests(&calls);
    // The hash length wins over the argument: the server does not validate
    // `algorithm`, so a wrong one looks exactly like "nothing installed".
    assert_eq!(sent[0].json()["algorithm"], "sha512");
    assert_eq!(sent[1].json()["algorithm"], "sha1");
    // Matching is case sensitive against lowercase hex.
    assert_eq!(sent[0].json()["hashes"][0], "ab".repeat(64));
    assert_eq!(sent[1].json()["hashes"][0], "cd".repeat(20));

    // An empty list never leaves the process.
    assert!(
        modrinth
            .versions_by_hashes(HashAlgo::Sha1, &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(requests(&calls).len(), 2);
}

#[test]
fn modrinth_update_check_compares_file_hashes_not_ids() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v2/version_files/update" => json(
            r#"{"aaaa1111":{"id":"vNEW","project_id":"P7dR8mSH","version_type":"release",
                 "files":[{"hashes":{"sha1":"ffff9999"},"url":"u","filename":"new.jar","primary":true}]},
                "bbbb2222":{"id":"vSAME","project_id":"Q","version_type":"release",
                 "files":[{"hashes":{"sha1":"bbbb2222"},"url":"u","filename":"same.jar","primary":true}]}}"#,
        ),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();

    let installed = ["aaaa1111".to_owned(), "bbbb2222".to_owned()];
    let answers = modrinth
        .updates_by_hashes(HashAlgo::Sha1, &installed, &mc(), Loader::Fabric)
        .unwrap();

    // One file changed; the other version id is new but ships the same jar,
    // which an id comparison would report as an update forever.
    assert!(!answers["aaaa1111"].has_file_hash("aaaa1111"));
    assert!(answers["bbbb2222"].has_file_hash("BBBB2222"));

    let body = requests(&calls)[0].json();
    // Omitting either filter silently widens the check instead of erroring.
    assert_eq!(body["loaders"][0], "fabric");
    assert_eq!(body["game_versions"][0], "1.20.1");
    assert_eq!(body["algorithm"], "sha1");
}

// --- CurseForge -----------------------------------------------------------

const CF_FILES_BODY: &str = r#"{"data":[
  {"id":8851009,"modId":238222,"displayName":"15.59 for Forge","fileName":"jei-1.20.1-forge.jar",
   "releaseType":2,"fileStatus":4,"fileDate":"2026-09-10T14:18:31.89Z","fileLength":1825651,
   "downloadUrl":"https://edge.forgecdn.net/files/8851/9/jei-1.20.1-forge.jar",
   "hashes":[{"value":"3BBB49D8","algo":1},{"value":"BDFE2C22","algo":2}],
   "gameVersions":["Client","1.20.1","Forge","Server"],
   "sortableGameVersions":[
     {"gameVersionName":"Client","gameVersion":"","gameVersionTypeId":75208},
     {"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125},
     {"gameVersionName":"Forge","gameVersion":"","gameVersionTypeId":68441}],
   "isServerPack":false,"fileFingerprint":2298786945,
   "dependencies":[{"modId":306612,"relationType":3}]},
  {"id":8851010,"modId":238222,"fileName":"jei-1.20.1-fabric.jar","releaseType":1,
   "fileDate":"2026-09-10T00:00:00Z","downloadUrl":"https://edge.forgecdn.net/f.jar",
   "sortableGameVersions":[
     {"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125},
     {"gameVersionName":"Fabric","gameVersion":"","gameVersionTypeId":68441}],
   "isServerPack":false},
  {"id":8851011,"modId":238222,"fileName":"jei-serverpack.zip","releaseType":1,
   "fileDate":"2026-09-09T00:00:00Z","downloadUrl":"https://edge.forgecdn.net/s.zip",
   "sortableGameVersions":[
     {"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125},
     {"gameVersionName":"Forge","gameVersion":"","gameVersionTypeId":68441}],
   "isServerPack":true},
  {"id":8851012,"modId":238222,"fileName":"jei-noloader.jar","releaseType":1,
   "fileDate":"2026-09-08T00:00:00Z","downloadUrl":"https://edge.forgecdn.net/n.jar",
   "sortableGameVersions":[
     {"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125}],
   "isServerPack":false}
]}"#;

#[test]
fn curseforge_search_sends_the_documented_query_and_the_key() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v1/mods/search" => json(
            r#"{"data":[{"id":238222,"name":"JEI","slug":"jei","summary":"items",
                 "downloadCount":9.0,"dateModified":"2026-01-01T00:00:00Z","status":4,
                 "logo":{"thumbnailUrl":"https://media.forgecdn.net/t.jpeg","url":"https://media.forgecdn.net/f.jpeg"},
                 "links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei","wikiUrl":null},
                 "authors":[{"name":"mezz"}]}],
               "pagination":{"index":0,"pageSize":20,"resultCount":1,"totalCount":151}}"#,
        ),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();

    let page = cf
        .search(&search_query(SortKey::Downloads, Loader::Forge))
        .unwrap();
    assert_eq!(page.total, 151);
    assert_eq!(page.hits[0].project_id, "238222");
    assert_eq!(page.hits[0].author.as_deref(), Some("mezz"));
    // The 256x256 thumbnail, never the full-size logo.
    assert_eq!(
        page.hits[0].icon_url.as_deref(),
        Some("https://media.forgecdn.net/t.jpeg")
    );

    let sent = requests(&calls);
    let query = sent[0].query();
    assert_eq!(query["gameId"], "432");
    assert_eq!(query["classId"], "6");
    assert_eq!(query["gameVersion"], "1.20.1");
    assert_eq!(query["modLoaderType"], "1");
    assert_eq!(query["sortField"], "6");
    assert_eq!(query["pageSize"], "20");
    assert_eq!(sent[0].header("x-api-key"), Some("test-key"));
}

#[test]
fn curseforge_reads_the_loader_off_sortable_game_versions() {
    let (addr, _) = stub(|req| match req.path() {
        "/v1/mods/238222/files" => json(CF_FILES_BODY),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();

    let versions = cf.versions("238222", &mc(), Loader::Forge).unwrap();
    let names: Vec<&str> = versions
        .iter()
        .map(|version| version.files[0].file_name.as_str())
        .collect();
    // The Fabric file is filtered out by its 68441 row, the server pack is
    // never a candidate, and the file with no loader row is accepted.
    assert_eq!(names, ["jei-1.20.1-forge.jar", "jei-noloader.jar"]);

    let forge = &versions[0];
    assert_eq!(forge.loaders, [Loader::Forge]);
    assert_eq!(forge.game_versions, ["1.20.1"]);
    assert_eq!(forge.release, nebula_core::catalog::ReleaseType::Beta);
    assert_eq!(forge.files[0].fingerprint, Some(2298786945));
    // 1 = SHA-1 and 2 = MD5, lowercased for the Modrinth-by-sha1 fallback.
    assert_eq!(forge.files[0].sha1.as_deref(), Some("3bbb49d8"));
    assert_eq!(forge.files[0].md5.as_deref(), Some("bdfe2c22"));
    assert_eq!(forge.dependencies.len(), 1);

    // D3 prefers the release, even though the beta is newer.
    let latest = cf
        .latest_file("238222", &mc(), Loader::Forge)
        .unwrap()
        .unwrap();
    assert_eq!(latest.files[0].file_name, "jei-noloader.jar");
}

/// One page of `/files`, all of them 1.20.1 Forge.
fn cf_files_page(ids: std::ops::Range<u64>, year: u32) -> Vec<u8> {
    let files: Vec<String> = ids
        .map(|id| {
            format!(
                r#"{{"id":{id},"modId":238222,"fileName":"jei-{id}.jar","releaseType":1,
                  "fileDate":"{year}-01-{:02}T00:00:00Z",
                  "downloadUrl":"https://edge.forgecdn.net/{id}.jar",
                  "sortableGameVersions":[
                    {{"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125}},
                    {{"gameVersionName":"Forge","gameVersion":"","gameVersionTypeId":68441}}],
                  "isServerPack":false}}"#,
                (id % 28) + 1
            )
        })
        .collect();
    format!(r#"{{"data":[{}]}}"#, files.join(",")).into_bytes()
}

#[test]
fn curseforge_pages_past_a_full_first_page_of_files() {
    let (addr, calls) = stub(|req| {
        let index = req.query().get("index").cloned().unwrap_or_default();
        match (req.path(), index.as_str()) {
            // A full page means there is more; nothing documents whether
            // `/files` is oldest- or newest-first, so both pages are read.
            ("/v1/mods/238222/files", "0") => (200, "application/json", cf_files_page(0..50, 2020)),
            ("/v1/mods/238222/files", "50") => {
                (200, "application/json", cf_files_page(50..52, 2026))
            }
            _ => (404, "text/plain", Vec::new()),
        }
    });
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();

    let versions = cf.versions("238222", &mc(), Loader::Forge).unwrap();
    assert_eq!(versions.len(), 52);
    assert_eq!(requests(&calls).len(), 2);
    // Newest first across the page boundary, so D3 cannot pick a 2020 file.
    assert_eq!(versions[0].files[0].file_name, "jei-51.jar");

    let latest = cf
        .latest_file("238222", &mc(), Loader::Forge)
        .unwrap()
        .unwrap();
    assert_eq!(latest.files[0].file_name, "jei-51.jar");
}

#[test]
fn curseforge_computes_the_unmatched_fingerprints_itself() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v1/fingerprints" => json(
            r#"{"data":{"isCacheBuilt":true,
                 "exactMatches":[{"id":238222,"file":{"id":8851009,"modId":238222,
                   "fileName":"jei.jar","releaseType":1,"fileFingerprint":2298786945,
                   "downloadUrl":"https://edge.forgecdn.net/x.jar",
                   "sortableGameVersions":[]}}],
                 "exactFingerprints":[2298786945],
                 "partialMatches":[],"partialMatchFingerprints":{},
                 "installedFingerprints":[2298786945,1],
                 "unmatchedFingerprints":null}}"#,
        ),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();

    let matched = cf
        .versions_by_hashes(HashAlgo::Sha1, &["2298786945".into(), "1".into()])
        .unwrap();
    // `unmatchedFingerprints` was null even though `1` matched nothing, so
    // the unmatched set is submitted minus exact.
    assert_eq!(matched.len(), 1);
    assert!(matched.contains_key("2298786945"));
    assert!(!matched.contains_key("1"));
    assert_eq!(matched["2298786945"].project_id, "238222");

    // Fingerprints go on the wire as JSON numbers.
    let body = requests(&calls)[0].json();
    assert_eq!(body["fingerprints"][0], 2298786945u64);
    assert_eq!(body["fingerprints"][1], 1);
}

#[test]
fn curseforge_blocked_file_has_no_url_and_keeps_its_hash() {
    let (addr, calls) = stub(|req| match req.path() {
        "/v1/mods/448233/files" => json(
            r#"{"data":[{"id":8287086,"modId":448233,"fileName":"entityculling.jar",
                 "releaseType":1,"fileStatus":4,"fileDate":"2026-01-01T00:00:00Z",
                 "fileLength":1590221,"downloadUrl":null,"isAvailable":true,
                 "sortableGameVersions":[
                   {"gameVersionName":"1.20.1","gameVersion":"1.20.1","gameVersionTypeId":75125},
                   {"gameVersionName":"Forge","gameVersion":"","gameVersionTypeId":68441}],
                 "isServerPack":false,"fileFingerprint":4018169999}]}"#,
        ),
        // 200 with an empty downloadUrl, where /download-url is a bare 403.
        "/v1/mods/448233/files/8287086/download-info" => json(
            r#"{"data":[{"type":1,"downloadUrl":"","fileName":"entityculling.jar",
                 "fileLength":1590221,
                 "hashes":[{"value":"B818F6D7","algo":1},{"value":"F54A5BAA","algo":2}]}]}"#,
        ),
        _ => (404, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();

    let versions = cf.versions("448233", &mc(), Loader::Forge).unwrap();
    let file = &versions[0].files[0];
    assert!(file.url.is_none(), "a blocked file must never carry a URL");
    assert_eq!(file.sha1.as_deref(), Some("b818f6d7"));
    assert_eq!(file.md5.as_deref(), Some("f54a5baa"));
    assert_eq!(file.fingerprint, Some(4018169999));

    let paths: Vec<String> = requests(&calls)
        .iter()
        .map(|req| req.path().to_owned())
        .collect();
    assert_eq!(paths[1], "/v1/mods/448233/files/8287086/download-info");
}

#[test]
fn curseforge_key_survives_a_redirect_to_another_port() {
    let (target, target_calls) =
        stub(|_| json(r#"{"data":{"id":238222,"name":"JEI","slug":"jei","summary":"items"}}"#));
    let location: &'static str = String::leak(format!("Location: http://{target}/v1/mods/238222"));
    let (addr, _) = stub(move |_| (302, location, Vec::new()));

    let catalog = catalog(addr);
    let hit = catalog
        .provider(ProviderId::CurseForge)
        .unwrap()
        .project("238222")
        .unwrap();
    assert_eq!(hit.title, "JEI");

    // `edge.forgecdn.net` redirects to `mediafilez.forgecdn.net`, and the key
    // has to survive that hop or the CDN starts answering 401 (R2).
    let followed = requests(&target_calls);
    assert_eq!(followed.len(), 1);
    assert_eq!(followed[0].header("x-api-key"), Some("test-key"));
}

#[test]
fn curseforge_requests_are_spaced_by_at_least_250ms() {
    let (addr, calls) = stub(|_| json(r#"{"data":{"id":1,"name":"a","slug":"a"}}"#));
    let catalog = catalog(addr);
    let cf = catalog.provider(ProviderId::CurseForge).unwrap();
    cf.project("1").unwrap();
    cf.project("2").unwrap();

    let log = calls.lock().unwrap();
    let gap = log[1].0.duration_since(log[0].0);
    assert!(
        gap >= Duration::from_millis(240),
        "{gap:?} between two calls"
    );
}

#[test]
fn curseforge_is_absent_without_a_key() {
    let catalog = Catalog::new(Endpoints::default(), None);
    assert!(catalog.provider(ProviderId::CurseForge).is_none());
    assert_eq!(catalog.providers().count(), 1);
    assert!(catalog.curseforge_key().is_none());
}

// --- HTTP policy ----------------------------------------------------------

#[test]
fn retries_a_server_error_and_then_succeeds() {
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    SEEN.store(0, Ordering::SeqCst);
    let (addr, calls) = stub(|_| match SEEN.fetch_add(1, Ordering::SeqCst) {
        0 => (500, "text/plain", b"boom".to_vec()),
        _ => json(r#"{"hits":[],"total_hits":0}"#),
    });
    let catalog = catalog(addr);
    let page = catalog
        .provider(ProviderId::Modrinth)
        .unwrap()
        .search(&search_query(SortKey::Relevance, Loader::Fabric))
        .unwrap();
    assert_eq!(page.total, 0);
    assert_eq!(requests(&calls).len(), 2);
}

#[test]
fn honours_a_small_retry_after_on_429() {
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    SEEN.store(0, Ordering::SeqCst);
    let (addr, calls) = stub(|_| match SEEN.fetch_add(1, Ordering::SeqCst) {
        0 => (
            429,
            "Retry-After: 1",
            br#"{"error":"ratelimit_error"}"#.to_vec(),
        ),
        _ => json(r#"{"hits":[],"total_hits":0}"#),
    });
    let catalog = catalog(addr);
    catalog
        .provider(ProviderId::Modrinth)
        .unwrap()
        .search(&search_query(SortKey::Relevance, Loader::Fabric))
        .unwrap();

    let log = calls.lock().unwrap();
    assert_eq!(log.len(), 2);
    let waited = log[1].0.duration_since(log[0].0);
    // The backoff schedule alone would have retried after 0.5 s.
    assert!(waited >= Duration::from_millis(900), "{waited:?}");
}

#[test]
fn builds_errors_without_a_body() {
    // A 404 body is zero bytes on Modrinth, and a 400 carries an undeclared
    // `details[]`; neither may be required to build an error (§E.1.1).
    let (addr, _) = stub(|req| match req.path() {
        "/v2/project/missing" => (404, "text/plain", Vec::new()),
        _ => (400, "text/plain", Vec::new()),
    });
    let catalog = catalog(addr);
    let modrinth = catalog.provider(ProviderId::Modrinth).unwrap();

    assert!(matches!(
        modrinth.project("missing").unwrap_err(),
        CatalogError::NotFound
    ));
    let other = modrinth.project("bad").unwrap_err();
    assert!(matches!(other, CatalogError::Network(_)), "{other}");
    assert!(other.to_string().contains("400"), "{other}");
}

// --- index ----------------------------------------------------------------

fn entry(provider: ProviderId, namespace: Namespace) -> IndexEntry {
    IndexEntry::new(
        provider,
        "AANobbMI",
        "OihdIimA",
        IndexHashes {
            sha1: "abc123".into(),
            sha512: Some("def456".into()),
            md5: None,
        },
        namespace,
    )
    .with_text(
        Some("sodium".into()),
        Some("Sodium".into()),
        Some("https://cdn.invalid/i.webp".into()),
    )
}

#[test]
fn index_round_trips_and_omits_curseforge_free_text() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_dir(dir.path());

    let mut index = ModIndex::default();
    index.mods.insert(
        "sodium.jar".into(),
        entry(ProviderId::Modrinth, Namespace::Required),
    );
    let mut cf = entry(ProviderId::CurseForge, Namespace::OptionalOff);
    cf.dependencies = vec!["306612".into()];
    index.mods.insert("jei.jar".into(), cf);
    index.save(&server).unwrap();

    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(server.modindex_file()).unwrap()).unwrap();
    assert_eq!(raw["version"], 1);
    assert_eq!(raw["mods"]["sodium.jar"]["title"], "Sodium");
    assert_eq!(raw["mods"]["sodium.jar"]["namespace"], "required");
    assert_eq!(raw["mods"]["jei.jar"]["namespace"], "optionaloff");
    // CurseForge's terms forbid caching its API data, so a round-tripped CF
    // entry carries no title, slug or icon url at all.
    assert!(raw["mods"]["jei.jar"].get("title").is_none());
    assert!(raw["mods"]["jei.jar"].get("project_slug").is_none());
    assert!(raw["mods"]["jei.jar"].get("icon_url").is_none());
    assert_eq!(raw["mods"]["jei.jar"]["dependencies"][0], "306612");

    assert_eq!(ModIndex::load(&server).unwrap(), index);
    // The temporary file the atomic save wrote through is gone.
    assert!(!server.path.join("modindex.json.tmp").exists());
    let leftovers: Vec<String> = std::fs::read_dir(&server.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["modindex.json"]);
}

#[test]
fn a_missing_index_is_empty_and_a_newer_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_dir(dir.path());
    assert!(ModIndex::load(&server).unwrap().mods.is_empty());

    std::fs::create_dir_all(&server.path).unwrap();
    std::fs::write(server.modindex_file(), br#"{"version":99,"mods":{}}"#).unwrap();
    assert!(ModIndex::load(&server).is_err());
}

#[test]
fn reconcile_flags_missing_files_and_hand_placed_jars() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_dir(dir.path());
    let container = server.path.join("fabricmods");
    write_jar(
        &container.join("required/hand.jar"),
        &[(
            "fabric.mod.json",
            br#"{"id":"hand","version":"1.0.0","name":"Hand Placed"}"#,
        )],
    );
    std::fs::create_dir_all(container.join("optionalon")).unwrap();
    std::fs::create_dir_all(container.join("optionaloff")).unwrap();

    let mut index = ModIndex::default();
    index.mods.insert(
        "gone.jar".into(),
        entry(ProviderId::Modrinth, Namespace::OptionalOn),
    );
    index.save(&server).unwrap();

    let seen = reconcile(&server, &container).unwrap();
    assert!(seen.persistable);
    assert_eq!(seen.rows.len(), 2);

    let hand = &seen.rows[0];
    assert_eq!(hand.file_name, "hand.jar");
    assert!(hand.is_unmanaged(), "no index entry claims it");
    assert!(!hand.missing);
    assert_eq!(hand.title(), "Hand Placed");
    assert_eq!(hand.local.as_ref().unwrap().mod_id, "hand");

    let gone = &seen.rows[1];
    assert!(gone.missing);
    assert_eq!(gone.namespace, Namespace::OptionalOn);
    assert_eq!(gone.title(), "Sodium");

    // Reconciliation never writes: the entry whose file vanished stays.
    assert!(
        ModIndex::load(&server)
            .unwrap()
            .mods
            .contains_key("gone.jar")
    );
}

#[test]
fn reconcile_refuses_to_persist_an_unreadable_container() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = ModIndex::default();
    index.mods.insert(
        "sodium.jar".into(),
        entry(ProviderId::Modrinth, Namespace::Required),
    );

    // The container directory is not there to be read, so nothing derived from
    // this pass may be written back (D1).
    let absent = dir.path().join("absent");
    let seen = reconcile_with(&index, &absent).unwrap();
    assert!(!seen.persistable);
    assert_eq!(seen.rows.len(), 1);
    assert!(seen.rows[0].missing);

    // The same holds when the container exists but its namespace directories
    // do not: there is still nothing that could have been read.
    std::fs::create_dir_all(&absent).unwrap();
    assert!(!reconcile_with(&index, &absent).unwrap().persistable);
}

#[test]
fn reconcile_persists_a_readable_but_empty_container() {
    let dir = tempfile::tempdir().unwrap();
    let container = dir.path().join("fabricmods");
    init_toggleable(&container).unwrap();

    let mut index = ModIndex::default();
    index.mods.insert(
        "gone.jar".into(),
        entry(ProviderId::Modrinth, Namespace::Required),
    );

    // Empty is not unreadable: the row is `파일 없음`, and the delete that
    // clears it must be allowed through (F9's accepted ceiling, lifted).
    let seen = reconcile_with(&index, &container).unwrap();
    assert!(seen.persistable);
    assert_eq!(seen.rows.len(), 1);
    assert!(seen.rows[0].missing);

    // An empty index over the same container is the same normal state.
    assert!(
        reconcile_with(&ModIndex::default(), &container)
            .unwrap()
            .persistable
    );
}

// --- lock -----------------------------------------------------------------

#[test]
fn the_server_lock_is_exclusive_and_released_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let server = server_dir(dir.path());
    std::fs::create_dir_all(&server.path).unwrap();

    let held = ServerLock::acquire(&server).unwrap();
    assert!(server.path.join(".nebula-lock").is_file());
    match ServerLock::acquire(&server) {
        Err(CatalogError::Locked { pid }) => assert_eq!(pid, std::process::id()),
        other => panic!("expected a clean refusal, got {other:?}"),
    }
    drop(held);
    assert!(!server.path.join(".nebula-lock").exists());
    ServerLock::acquire(&server).unwrap();
}

// --- jar metadata ---------------------------------------------------------

#[test]
fn reads_a_fabric_jar_with_a_sized_icon_map() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sodium.jar");
    write_jar(
        &path,
        &[
            (
                "fabric.mod.json",
                br#"{"id":"sodium","version":"0.5.13","name":"sodium",
                     "description":"Fast rendering",
                     "icon":{"16":"assets/small.png","128":"assets/big.png"}}"#,
            ),
            ("assets/small.png", b"small"),
            ("assets/big.png", b"big"),
        ],
    );

    let LocalFile::Mod(local) = jarmeta::read(&path).unwrap() else {
        panic!("expected a mod");
    };
    assert_eq!(local.mod_id, "sodium");
    assert_eq!(local.version, "0.5.13");
    // Capitalised exactly as the distribution scanners name it.
    assert_eq!(local.name, "Sodium");
    assert_eq!(local.description.as_deref(), Some("Fast rendering"));
    // The largest declared size wins, not the first key.
    assert_eq!(local.icon.as_deref(), Some(&b"big"[..]));
    // One pass over the jar: sha512 and the CurseForge fingerprint are asked
    // for per file by the two callers that need them, not on every list row.
    assert_eq!(local.sha1.len(), 40);
    assert_eq!(local.md5.len(), 32);
    assert_eq!(
        hash::curseforge_fingerprint_of_file(&path).unwrap(),
        hash::curseforge_fingerprint(&std::fs::read(&path).unwrap())
    );
}

#[test]
fn reads_a_mods_toml_jar_and_its_logo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jei-1.20.1-forge-15.59.0.210.jar");
    write_jar(
        &path,
        &[
            (
                "META-INF/mods.toml",
                br#"
modLoader = "javafml"
loaderVersion = "[47,)"
license = "MIT"
logoFile = "/pack.png"

[[mods]]
modId = "jei"
version = "${file.jarVersion}"
displayName = "Just Enough Items"
description = "Item and recipe viewing."
"#,
            ),
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nImplementation-Version: 15.59.0.210\r\n",
            ),
            ("pack.png", b"logo"),
        ],
    );

    let LocalFile::Mod(local) = jarmeta::read(&path).unwrap() else {
        panic!("expected a mod");
    };
    assert_eq!(local.mod_id, "jei");
    assert_eq!(local.name, "Just Enough Items");
    // `${file.jarVersion}` is resolved from the manifest, as the scanners do.
    assert_eq!(local.version, "15.59.0.210");
    assert_eq!(
        local.description.as_deref(),
        Some("Item and recipe viewing.")
    );
    // A pack-level `logoFile` with a leading slash still names a zip entry.
    assert_eq!(local.icon.as_deref(), Some(&b"logo"[..]));
}

#[test]
fn reads_a_jar_with_no_metadata_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mystery-1.2.3.jar");
    write_jar(&path, &[("a.class", b"x")]);

    let LocalFile::Mod(local) = jarmeta::read(&path).unwrap() else {
        panic!("expected a mod");
    };
    // Name parity with the scanners: crude inference, id lowercased.
    assert_eq!(local.mod_id, "mystery");
    assert_eq!(local.version, "1.2.3");
    assert_eq!(local.name, "Mystery");
    assert!(local.icon.is_none());
}

#[test]
fn classifies_files_the_distribution_scanners_would_choke_on() {
    let dir = tempfile::tempdir().unwrap();

    let corrupt = dir.path().join("broken.jar");
    std::fs::write(&corrupt, b"this is not a zip at all").unwrap();
    match jarmeta::read(&corrupt).unwrap() {
        LocalFile::Unshippable { file_name, reason } => {
            assert_eq!(file_name, "broken.jar");
            assert!(!reason.is_empty());
        }
        other => panic!("expected an unshippable file, got {other:?}"),
    }

    let text = dir.path().join("readme.txt");
    std::fs::write(&text, b"hello").unwrap();
    match jarmeta::read(&text).unwrap() {
        LocalFile::Unshippable { file_name, reason } => {
            assert_eq!(file_name, "readme.txt");
            assert!(reason.contains(".jar"), "{reason}");
        }
        other => panic!("expected an unshippable file, got {other:?}"),
    }
}

// --- hashing --------------------------------------------------------------

#[derive(serde::Deserialize)]
struct Murmur2Vectors {
    vectors: Vec<Murmur2Vector>,
}

#[derive(serde::Deserialize)]
struct Murmur2Vector {
    name: String,
    hex: String,
    filtered_len: usize,
    fingerprint: u32,
}

#[test]
fn matches_the_recorded_murmur2_vectors() {
    const RAW: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/catalog/murmur2.json"
    ));
    let dir = tempfile::tempdir().unwrap();
    let recorded: Murmur2Vectors = serde_json::from_str(RAW).unwrap();
    assert!(recorded.vectors.len() >= 5);

    for vector in &recorded.vectors {
        let bytes = hex::decode(&vector.hex).unwrap();
        assert_eq!(
            hash::curseforge_fingerprint(&bytes),
            vector.fingerprint,
            "{}",
            vector.name
        );
        // The seed mixes the *filtered* length, the one thing easy to get
        // wrong, so the fixture records it too.
        let filtered = bytes
            .iter()
            .filter(|byte| !matches!(byte, 0x09 | 0x0a | 0x0d | 0x20))
            .count();
        assert_eq!(filtered, vector.filtered_len, "{}", vector.name);

        let path = dir.path().join("vector.bin");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            hash::curseforge_fingerprint_of_file(&path).unwrap(),
            vector.fingerprint,
            "{} through the streaming reader",
            vector.name
        );
    }
}

#[test]
fn streams_sha512_like_the_one_shot_form() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("payload.bin");
    let payload: Vec<u8> = (0..300_000u32).map(|index| index as u8).collect();
    std::fs::write(&path, &payload).unwrap();
    assert_eq!(
        hash::sha512_hex_of_file(&path).unwrap(),
        hash::sha512_hex(&payload)
    );
    assert_eq!(hash::sha512_hex_of_file(&path).unwrap().len(), 128);
}

/// The one live check: the known-answer pair from the facts document. The URL
/// is quoted from there verbatim, not rebuilt from the file id (R4).
#[test]
fn fingerprints_the_recorded_jei_jar() {
    if std::env::var("NEBULA_NET_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping: set NEBULA_NET_TESTS=1 to run the live fingerprint check");
        return;
    }
    const URL: &str = "https://edge.forgecdn.net/files/8851/9/jei-1.20.1-forge-15.59.0.210.jar";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jei-1.20.1-forge-15.59.0.210.jar");
    nebula_core::remote::download_to(URL, &path).unwrap();
    assert_eq!(
        hash::curseforge_fingerprint_of_file(&path).unwrap(),
        2298786945
    );
}
