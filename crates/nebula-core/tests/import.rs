//! Modpack import across the three formats, offline.
//!
//! Synthetic archives are built with the `zip` crate and the Modrinth branch's
//! downloads are served by a `DownloadFn` closure that copies from a local
//! directory and asserts the SHA-1 it was handed.

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use nebula_core::hash::sha1_hex_of_file;
use nebula_core::import::{ImportFormat, detect_format, import_modpack};
use nebula_core::layout::{Root, ServerDir};
use nebula_core::server::read_server_meta;
use url::Url;

const BASE: &str = "https://cdn.example.invalid/";

struct Fixture {
    _dir: tempfile::TempDir,
    root: Root,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path(), Url::parse(BASE).unwrap());
    nebula_core::distribution::init_root(&root).unwrap();
    Fixture { _dir: dir, root }
}

fn zip_at(path: &Path, entries: &[(&str, &str)]) -> PathBuf {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, body) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path.to_path_buf()
}

/// Every modpack lands in `<root>/modpacks`, which is also how the CLI resolves
/// a bare file name.
fn modpack(root: &Root, name: &str, entries: &[(&str, &str)]) -> PathBuf {
    let dir = root.modpacks_dir().join("prism");
    fs::create_dir_all(&dir).unwrap();
    zip_at(&dir.join(name), entries)
}

fn server(root: &Root, dir_name: &str) -> ServerDir {
    ServerDir::new(root.clone(), dir_name)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

// ---------------------------------------------------------------------------
// CurseForge
// ---------------------------------------------------------------------------

const CURSEFORGE_MANIFEST: &str = r#"{
  "minecraft": { "version": "1.20.1", "modLoaders": [{ "id": "forge-47.2.0", "primary": true }] },
  "manifestType": "minecraftModpack",
  "manifestVersion": 1,
  "name": "CF Pack",
  "version": "2.1",
  "author": "someone",
  "files": [],
  "overrides": "overrides"
}"#;

#[test]
fn imports_a_curseforge_pack() {
    let fixture = fixture();
    let zip = modpack(
        &fixture.root,
        "cf.zip",
        &[
            ("manifest.json", CURSEFORGE_MANIFEST),
            ("modlist.html", "<html></html>"),
            ("overrides/config/a.cfg", "a"),
            ("overrides/options.txt", "fov:70"),
            ("overrides/mods/inline.jar", "jar"),
            // Zip slip: must be skipped, not written above the server root.
            ("overrides/../../evil.cfg", "pwned"),
        ],
    );

    assert_eq!(detect_format(&zip).unwrap(), ImportFormat::CurseForge);

    // Resolved by bare file name below <root>/modpacks.
    let report = import_modpack(
        &fixture.root,
        "CF",
        Path::new("cf.zip"),
        None,
        &nebula_core::import::default_download,
    )
    .unwrap();

    assert_eq!(report.format, ImportFormat::CurseForge);
    assert_eq!(report.downloaded, 0);
    assert_eq!(report.copied, 3);
    assert!(report.manual.is_empty());

    let server = server(&fixture.root, "CF-1.20.1");
    assert_eq!(report.created.dir.dir_name, "CF-1.20.1");
    assert_eq!(read(&server.files_dir().join("config/a.cfg")), "a");
    assert_eq!(read(&server.files_dir().join("options.txt")), "fov:70");
    assert_eq!(
        read(&server.forgemods_dir().join("required/inline.jar")),
        "jar"
    );
    assert!(!fixture.root.path.join("evil.cfg").exists());
    assert!(!server.path.join("evil.cfg").exists());

    let meta = read_server_meta(&server).unwrap();
    assert_eq!(meta.forge.unwrap().version, "47.2.0");
    assert_eq!(meta.meta.version, "2.1");
    assert_eq!(meta.meta.name, "CF Pack");
}

#[test]
fn rejects_a_legacy_neoforge_curseforge_pack() {
    let fixture = fixture();
    let zip = modpack(
        &fixture.root,
        "legacy.zip",
        &[(
            "manifest.json",
            r#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"neoforge-1.20.1-47.1.106"}]},"version":"1"}"#,
        )],
    );
    let error = import_modpack(
        &fixture.root,
        "Legacy",
        &zip,
        None,
        &nebula_core::import::default_download,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Legacy NeoForge"), "{error}");
}

// ---------------------------------------------------------------------------
// Modrinth
// ---------------------------------------------------------------------------

const MODRINTH_INDEX: &str = r#"{
  "formatVersion": 1,
  "game": "minecraft",
  "versionId": "3.0",
  "name": "Mr Pack",
  "files": [
    {
      "path": "mods/required.jar",
      "hashes": { "sha1": "SHA1_REQUIRED" },
      "env": { "client": "required", "server": "required" },
      "downloads": ["https://cdn.example.invalid/required.jar"],
      "fileSize": 3
    },
    {
      "path": "mods/optional.jar",
      "hashes": { "sha1": "SHA1_OPTIONAL" },
      "env": { "client": "optional", "server": "unsupported" },
      "downloads": ["https://cdn.example.invalid/optional.jar"],
      "fileSize": 3
    },
    {
      "path": "mods/server-only.jar",
      "hashes": { "sha1": "0000000000000000000000000000000000000000" },
      "env": { "client": "unsupported", "server": "required" },
      "downloads": ["https://cdn.example.invalid/server-only.jar"],
      "fileSize": 3
    },
    {
      "path": "config/extra.cfg",
      "hashes": {},
      "downloads": ["https://cdn.example.invalid/extra.cfg"],
      "fileSize": 5
    }
  ],
  "dependencies": { "minecraft": "1.21.1", "neoforge": "21.1.77" }
}"#;

#[test]
fn imports_a_modrinth_pack() {
    let fixture = fixture();
    let sources = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("required.jar", "req"),
        ("optional.jar", "opt"),
        ("server-only.jar", "srv"),
        ("extra.cfg", "extra"),
    ] {
        fs::write(sources.path().join(name), body).unwrap();
    }
    let sha1 = |name: &str| sha1_hex_of_file(&sources.path().join(name)).unwrap();

    let index = MODRINTH_INDEX
        .replace("SHA1_REQUIRED", &sha1("required.jar"))
        .replace("SHA1_OPTIONAL", &sha1("optional.jar"));

    let zip = modpack(
        &fixture.root,
        "pack.mrpack",
        &[
            ("modrinth.index.json", &index),
            ("overrides/config/a.cfg", "from overrides"),
            ("overrides/options.txt", "fov:80"),
            ("client-overrides/config/a.cfg", "from client-overrides"),
            ("server-overrides/never.cfg", "server only"),
        ],
    );
    assert_eq!(detect_format(&zip).unwrap(), ImportFormat::Modrinth);

    // `DownloadFn` is `dyn Fn + 'static`, so the closure owns what it needs.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let download = {
        let (sources, calls) = (sources.path().to_path_buf(), Rc::clone(&calls));
        move |url: &str, dest: &Path, expected: Option<&str>| -> nebula_core::Result<()> {
            let name = url.rsplit('/').next().unwrap().to_owned();
            let source = sources.join(&name);
            // Only files that declare a sha1 are asked to verify one.
            let want = match name.as_str() {
                "extra.cfg" => None,
                _ => Some(sha1_hex_of_file(&source).unwrap()),
            };
            assert_eq!(expected.map(str::to_owned), want, "sha1 handed for {name}");
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::copy(&source, dest).unwrap();
            calls.borrow_mut().push(name);
            Ok(())
        }
    };

    let report = import_modpack(&fixture.root, "Mr", &zip, None, &download).unwrap();
    assert_eq!(report.format, ImportFormat::Modrinth);
    assert_eq!(report.downloaded, 3);
    assert_eq!(report.skipped_unsupported, 1);
    assert_eq!(report.copied, 3);

    assert_eq!(
        *calls.borrow(),
        ["required.jar", "optional.jar", "extra.cfg"]
    );

    let server = server(&fixture.root, "Mr-1.21.1");
    let mods = server.neoforgemods_dir();
    assert_eq!(read(&mods.join("required/required.jar")), "req");
    assert_eq!(read(&mods.join("optionalon/optional.jar")), "opt");
    assert!(!mods.join("required/server-only.jar").exists());
    assert!(!mods.join("optionalon/server-only.jar").exists());
    assert_eq!(read(&server.files_dir().join("config/extra.cfg")), "extra");

    // client-overrides is applied after overrides, so it wins.
    assert_eq!(
        read(&server.files_dir().join("config/a.cfg")),
        "from client-overrides"
    );
    assert_eq!(read(&server.files_dir().join("options.txt")), "fov:80");
    assert!(!server.files_dir().join("never.cfg").exists());

    let meta = read_server_meta(&server).unwrap();
    assert_eq!(meta.neoforge.unwrap().version, "21.1.77");
    assert_eq!(meta.meta.version, "3.0");
    assert_eq!(meta.meta.name, "Mr Pack");
}

#[test]
fn rejects_a_quilt_modrinth_pack() {
    let fixture = fixture();
    let zip = modpack(
        &fixture.root,
        "quilt.mrpack",
        &[(
            "modrinth.index.json",
            r#"{"versionId":"1","files":[],"dependencies":{"minecraft":"1.20.1","quilt-loader":"0.20.0"}}"#,
        )],
    );
    let error = import_modpack(
        &fixture.root,
        "Quilt",
        &zip,
        None,
        &nebula_core::import::default_download,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Quilt"), "{error}");
}

// ---------------------------------------------------------------------------
// Prism / MultiMC
// ---------------------------------------------------------------------------

const MMC_PACK: &str = r#"{
  "formatVersion": 1,
  "components": [
    { "uid": "net.minecraft", "version": "1.20.1", "cachedName": "Minecraft" },
    { "uid": "net.fabricmc.fabric-loader", "version": "0.15.0", "cachedName": "Fabric Loader" }
  ]
}"#;

fn prism_entries(game_dir: &str) -> Vec<(String, &'static str)> {
    [
        ("mods/enabled.jar", "on"),
        ("mods/disabled.jar.disabled", "off"),
        ("mods/notes.txt", "notes"),
        ("options.txt", "fov:90"),
        ("config/x.cfg", "x"),
        ("servers.dat", "servers"),
        ("logs/latest.log", "log"),
        ("crash-reports/crash.txt", "crash"),
        ("saves/world/level.dat", "world"),
        ("screenshots/a.png", "png"),
        (".fabric/cache", "cache"),
        ("usercache.json", "users"),
    ]
    .into_iter()
    .map(|(name, body)| (format!("{game_dir}{name}"), body))
    .collect()
}

#[test]
fn imports_a_prism_instance() {
    let fixture = fixture();
    let mut entries: Vec<(String, &str)> = vec![
        ("mmc-pack.json".to_owned(), MMC_PACK),
        (
            "instance.cfg".to_owned(),
            "InstanceType=OneSix\nname=Prism Pack\n",
        ),
    ];
    entries.extend(prism_entries(".minecraft/"));
    let borrowed: Vec<(&str, &str)> = entries
        .iter()
        .map(|(name, body)| (name.as_str(), *body))
        .collect();
    let zip = modpack(&fixture.root, "prism.zip", &borrowed);

    assert_eq!(detect_format(&zip).unwrap(), ImportFormat::PrismInstance);
    let report = import_modpack(
        &fixture.root,
        "Prism",
        &zip,
        None,
        &nebula_core::import::default_download,
    )
    .unwrap();

    assert_eq!(report.format, ImportFormat::PrismInstance);
    assert_eq!(report.downloaded, 0);
    // enabled.jar, disabled.jar, notes.txt, options.txt, x.cfg, servers.dat
    assert_eq!(report.copied, 6);

    let server = server(&fixture.root, "Prism-1.20.1");
    let mods = server.fabricmods_dir();
    assert_eq!(read(&mods.join("required/enabled.jar")), "on");
    assert_eq!(read(&mods.join("optionaloff/disabled.jar")), "off");
    assert!(!mods.join("required/disabled.jar.disabled").exists());
    assert_eq!(read(&server.files_dir().join("mods/notes.txt")), "notes");

    // options.txt is a config pack owners ship; the scratch dirs are not.
    assert_eq!(read(&server.files_dir().join("options.txt")), "fov:90");
    assert_eq!(read(&server.files_dir().join("config/x.cfg")), "x");
    assert_eq!(read(&server.files_dir().join("servers.dat")), "servers");
    for excluded in [
        "logs/latest.log",
        "crash-reports/crash.txt",
        "saves/world/level.dat",
        "screenshots/a.png",
        ".fabric/cache",
        "usercache.json",
    ] {
        assert!(
            !server.files_dir().join(excluded).exists(),
            "{excluded} should not have been imported"
        );
    }

    let meta = read_server_meta(&server).unwrap();
    assert_eq!(meta.fabric.unwrap().version, "0.15.0");
    assert_eq!(meta.meta.name, "Prism Pack");
    // Prism instances carry no pack version.
    assert_eq!(meta.meta.version, "1.0.0");
}

#[test]
fn accepts_a_multimc_game_directory() {
    let fixture = fixture();
    let mut entries: Vec<(String, &str)> = vec![("mmc-pack.json".to_owned(), MMC_PACK)];
    entries.extend(prism_entries("minecraft/"));
    let borrowed: Vec<(&str, &str)> = entries
        .iter()
        .map(|(name, body)| (name.as_str(), *body))
        .collect();
    let zip = modpack(&fixture.root, "multimc.zip", &borrowed);

    let report = import_modpack(
        &fixture.root,
        "Old",
        &zip,
        None,
        &nebula_core::import::default_download,
    )
    .unwrap();
    assert_eq!(report.copied, 6);

    let server = server(&fixture.root, "Old-1.20.1");
    assert!(
        server
            .fabricmods_dir()
            .join("required/enabled.jar")
            .is_file()
    );
    // Without instance.cfg the generated name stands.
    assert_eq!(
        read_server_meta(&server).unwrap().meta.name,
        "Old (Minecraft 1.20.1)"
    );
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

#[test]
fn rejects_an_archive_that_is_not_a_modpack() {
    let fixture = fixture();
    let zip = modpack(&fixture.root, "random.zip", &[("readme.txt", "hello")]);
    let error = detect_format(&zip).unwrap_err();
    assert!(error.to_string().contains("not a CurseForge"), "{error}");
    assert!(
        import_modpack(
            &fixture.root,
            "Random",
            &zip,
            None,
            &nebula_core::import::default_download
        )
        .is_err()
    );
}

#[test]
fn reports_a_missing_modpack_file() {
    let fixture = fixture();
    let error = import_modpack(
        &fixture.root,
        "Missing",
        Path::new("nope.zip"),
        None,
        &nebula_core::import::default_download,
    )
    .unwrap_err();
    assert!(error.to_string().contains("nope.zip"), "{error}");
}
