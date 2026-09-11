# Mod catalog and modpack management (Prism-style) — plan v1.1 (2026-09-11, after the critic panel)

Requested 2026-09-11: manage several modpacks like Prism Launcher and download mods directly, built "very stably". Prism Launcher (GPL-3.0) is a behavioural reference only — endpoints, parameters, data formats, edge cases — never copied code. Verified facts live in `2026-09-11-mod-catalog-facts.md` (adversarially verified; corrections folded into the Decisions and Provider-quirks sections below). The integration map (`2026-09-11-mod-catalog-integration.md`) lists the exact insertion points and visibility changes; the GUI spec is DESIGN-v2 §8; the design-panel record is `2026-09-11-mod-catalog-design-review.md`.

## Scope

In: mod search/install/update/remove for a selected server (modpack) from **Modrinth** (primary) and **CurseForge** (secondary); version selection filtered by the server's Minecraft version and loader; required-dependency resolution; hash verification; per-server install index with provenance; icon cache; installed-mod list from the jar metadata even for mods placed by hand; server delete/duplicate; CLI parity.
Out (later): exporting a server as `.mrpack`/CurseForge zip; resource packs/shaders; Quilt.

## Architecture (`crates/nebula-core/src/catalog/`)

| File | Responsibility |
|---|---|
| `mod.rs` | `Provider` trait and the provider-neutral types (see **Core types and signatures** below); `Catalog { providers, endpoints, cf_key, icon_memory }` façade built by `Catalog::new(Endpoints, Option<String>)`; `Endpoints { modrinth, curseforge }` with `Default` = production constants so tests inject the in-process stub; `CatalogError`. Every entry point in the other files takes `&Catalog` first. |
| `http.rs` | One `reqwest::blocking::Client` with `User-Agent: nebula/<version> (+repo url)`, connect 30 s / request 120 s, per-request retries (3, exponential 0.5/1/2 s) on 5xx/timeouts/connection errors, `429` honoured via `Retry-After` when present else `X-Ratelimit-Reset` (clamped 1–60 s) else the backoff schedule; JSON helpers with typed errors carrying the URL, status and a body excerpt (bodies may be empty). The numbers are our choice, not provider guidance — keep them as named constants. |
| `modrinth.rs` | API v2 mapping (facts §A). |
| `curseforge.rs` | Core API v1 mapping (facts §B). The key comes from `Catalog` (filled from `Config.curseforge_api_key`, which `Config::from_env` reads from `NEBULA_CURSEFORGE_API_KEY` and `GuiSettings::to_config` fills from the settings field); this module never reads the environment. Without a key the provider is absent from `Catalog.providers`. The upstream `curseforge::API_KEY` const stays reachable only from `import.rs`. `downloadUrl == null` → blocked file (R3). |
| `jarmeta.rs` | Local jar → `LocalMod { name, version, mod_id, description: Option<String>, icon: Option<Vec<u8>> }` from `fabric.mod.json` / `quilt.mod.json` (read-only) / `mods.toml` / `neoforge.mods.toml` / `mcmod.info` (reuse `scan::forgemod`/`fabricmod` parsers; add icon path extraction); sha1/sha512/md5 + CurseForge fingerprint of the file. Per-file `Result` (R9); also classifies a namespace-dir entry as `Unshippable` when its name does not end in `.jar` or `zip::ZipArchive::new` fails, because the distribution scanners abort the whole build on such a file (`scan/forgemod.rs` `read_forge_mod_meta`). |
| `index.rs` | `<server>/modindex.json`: `{ "version": 1, "mods": { "<file name>": { provider, project_id, project_slug?, version_id, file_id?, title?, icon_url?, hashes { sha1, sha512?, md5? }, installed_at, namespace, dependencies: [project_id…] } } }` — `namespace` serialises via `#[serde(rename_all = "lowercase")]` on `scan::toggle::Namespace` (`required`/`optionalon`/`optionaloff`, = `dir_name()`); `dependencies` = the required deps resolved at install time (delete warning). **CurseForge entries omit `project_slug`, `title` and `icon_url`** (serde skip when `provider == CurseForge`; test asserts a round-tripped CF entry carries no free text) — their title comes from `jarmeta`, their icon from the session memory cache (D2). Load/save atomically (`.tmp` + rename) **inside the server lock** (rule 6). Reconciliation is **read-only**: entries whose file is absent are marked `missing` in memory (`파일 없음`, excluded from update checks) and never dropped from the file; entries leave the file only through an explicit user mutation. If `discover_toggleable` saw zero files while the index holds entries, the container is missing/unreadable and nothing is persisted. Hand-placed jars appear as `직접 추가`. |
| `install.rs` | `resolve_version(&Catalog, project, mc, loader, prefer) → Result<Option<Version>>` (D3 ladder; `None` = no version for mc+loader); `plan_install(&Catalog, &ServerDir, project, version) → Result<InstallPlan>` (deps per D6; blocked CurseForge files are moved from `files` to `blocked` here, before any download); `apply(&Catalog, &ServerDir, plan: &InstallPlan, chosen: &InstallChoice, ns: Namespace, progress: &(dyn Fn(usize, usize, &str) + Send + Sync)) → Result<InstallReport>` (progress called once before each file with 1-based index, total, file name; byte-level progress is out of scope because `remote::download_to` streams through `copy_to`). `apply` takes the server lock, re-checks `plan.server`, `plan.index_sha1` and `plan.resolved_at` (older than 10 min → `CatalogError::Stale`), validates every provider file name (R6), stages in `<server>/.nebula-tmp/<pid>-<rand>/`, hash-verifies, renames into `<container>/<ns>/`, rewrites the index. Any failure other than a blocked file aborts with **best-effort rollback**: files already renamed in are removed and their index entries dropped; a file that cannot be removed goes to `left_behind` (absolute path, no index entry — the next reconcile shows it as `직접 추가`). A project already installed (D6 matching rule) is never added twice: installing it again is a replace (`update.rs` ordering). |
| `update.rs` | `check_updates(&Catalog, &ServerDir) → Result<Vec<UpdateCandidate>>` via hash lookup (D7) filtered by mc+loader; `replace_file(&Catalog, &ServerDir, old: &str, new: &Version, progress) → Result<()>` is the ONLY way a managed file changes version (used by update apply, `교체` and downgrade): (1) download + verify the new file into `<server>/.nebula-tmp/`; (2) rename the old file OUT of the namespace dir into `.nebula-tmp/`; (3) rename the new file in, rewrite the index entry (new key), delete the old file. The namespace dir never holds two versions of one mod at any instant; a crash between (2) and (3) leaves the mod absent with its index entry intact (visible as `파일 없음`, re-installable). `apply_updates(&Catalog, &ServerDir, candidates, progress) → Vec<(String, Result<()>)>` runs them one at a time — not atomic across mods, matching §8.3's per-mod failure list. |
| `icons.rs` | `fetch(&Catalog, &Root, hit) → Result<IconRef>` on the worker: `image::load_from_memory` (sniffs magic bytes; crate `image = { version = "0.25", default-features = false, features = ["png","jpeg","webp"] }` added to `crates/nebula-core/Cargo.toml`; cargo unifies it with i-slint-core's build, no new lockfile packages) → resize 72×72. Modrinth: PNG written **once** to `<root>/cache/icons/<sha1(url)>.png` (never overwritten: an existing file is used without a request, which also avoids Slint's (path, mtime-second) image cache serving a stale bitmap) → `IconRef::Path`. CurseForge: never touches disk — `IconRef::Rgba(IconBytes { rgba, w, h })` from `Catalog.icon_memory: HashMap<String, IconBytes>`; the GUI builds `slint::Image::from_rgba8` on the event-loop thread. Jar icons → `<root>/cache/icons/local-<sha1(jar)>.png`. The GUI only ever loads `.png` paths. Never enable `slint/image-default-formats` (R8). |
| `manage.rs` | `delete_server(root, dir_name)` guards in order: `paths::is_single_segment(dir_name)` AND not `.`/`..`; `ServerDir::new(..).path.is_dir()` (else `Error::Invalid`); after canonicalising, `real.parent() == real_path(root.servers_dir())` AND `real.file_name() == dir_name` — containment alone is not sufficient because `<root>/servers/.` canonicalises to the servers dir and passes `assert_contained`; takes the server lock; then `remove_dir_all`. Tests: `.`, `..`, `""`, `"Test-1.20.1 "`, a symlink. `duplicate_server(root, dir_name, new_id)` validates `new_id` with the 새 서버 rules, copies into `<root>/servers/.<new dir>.partial/` (its suffix is not a Minecraft version, so `server_dirs` skips it with a warning), rewrites `servermeta.json` (`meta.meta.name`) in the staging dir, skips `.nebula-tmp`/`.nebula-lock`, refuses symlinked entries (`Error::UnsafePath`), and renames the finished directory into place as the last step; a leftover `.*.partial` is deleted at the start of the next duplicate. `modindex.json` is copied as-is. |

Namespaces map to existing dirs: `required/` ↔ Prism "enabled required", `optionalon/`, `optionaloff/` ↔ disabled. Enable/disable = move between namespaces (no `.disabled` rename; keeps Nebula's contract).

## Core types and signatures (`catalog/mod.rs`)

```rust
pub enum ProviderId { Modrinth, CurseForge }
pub enum Loader { Forge, NeoForge, Fabric }          // from server::LoaderKind
pub enum ReleaseType { Release, Beta, Alpha }        // Modrinth "release"/"beta"/"alpha"; CurseForge 1/2/3
pub enum DependencyKind { Required, Optional, Incompatible, Embedded }
pub enum HashAlgo { Sha1, Sha512 }                    // chosen from the hash length, never guessed
pub enum SortKey { Relevance, Downloads, Newest }     // Modrinth relevance/downloads/updated; CurseForge popularity(2)/downloads(6)/last-updated(3)

pub struct SearchQuery { pub query: String, pub sort: SortKey, pub limit: u32, pub offset: u32, pub mc: MinecraftVersion, pub loader: Loader }
pub struct SearchPage { pub hits: Vec<ProjectHit>, pub total: u64 }
pub struct ProjectHit { pub provider: ProviderId, pub project_id: String, pub slug: String, pub title: String, pub author: Option<String>, pub description: String, pub downloads: u64, pub updated: Option<String>, pub icon_url: Option<String>, pub page_url: Option<String> }
pub struct Version { pub project_id: String, pub version_id: String, pub name: String, pub number: String, pub release: ReleaseType, pub published: String, pub game_versions: Vec<String>, pub loaders: Vec<Loader>, pub files: Vec<VersionFile>, pub dependencies: Vec<Dependency> }
pub struct VersionFile { pub file_name: String, pub url: Option<String> /* None = blocked */, pub size: u64, pub primary: bool, pub sha1: Option<String>, pub sha512: Option<String>, pub md5: Option<String>, pub fingerprint: Option<u32>, pub file_id: Option<String> }
pub struct Dependency { pub project_id: Option<String>, pub version_id: Option<String>, pub kind: DependencyKind }

pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn search(&self, q: &SearchQuery) -> Result<SearchPage, CatalogError>;
    fn project(&self, id: &str) -> Result<ProjectHit, CatalogError>;
    fn versions(&self, project_id: &str, mc: &MinecraftVersion, loader: Loader) -> Result<Vec<Version>, CatalogError>;
    fn versions_by_hashes(&self, algo: HashAlgo, hashes: &[String]) -> Result<BTreeMap<String, Version>, CatalogError>; // CurseForge: fingerprints (identification only, D7)
    fn latest_file(&self, project_id: &str, mc: &MinecraftVersion, loader: Loader) -> Result<Option<Version>, CatalogError>; // CurseForge update candidate (D7); Modrinth = versions()[0] after D3
}

pub struct PlannedFile { pub provider: ProviderId, pub project_id: String, pub slug: Option<String>, pub title: String, pub version_id: String, pub version_name: String, pub release: ReleaseType, pub file: VersionFile }
pub struct PlannedDep { pub file: PlannedFile, pub kind: DependencyKind, pub already_installed: Option<String> /* installed file name */ }
pub struct InstallPlan { pub server: String, pub index_sha1: String, pub resolved_at: SystemTime, pub root: PlannedFile, pub required: Vec<PlannedDep>, pub optional: Vec<PlannedDep>, pub blocked: Vec<ManualDownload>, pub incompatible: Vec<String>, pub unresolvable: Vec<String> }
pub struct InstallChoice { pub deps: BTreeSet<String> /* project ids the user kept; CLI --no-deps = empty */ }
pub struct InstallReport { pub installed: Vec<String>, pub skipped_already_installed: Vec<String>, pub manual: Vec<ManualDownload>, pub rolled_back: bool, pub left_behind: Vec<PathBuf> }
pub struct ManualDownload { pub name: String, pub file_name: String, pub url: String }
pub struct UpdateCandidate { pub file_name: String, pub title: String, pub from: String, pub to: Version }

pub enum CatalogError { Network(String), RateLimited { retry_after_secs: u32 }, NotFound, HashMismatch { file: String, expected: String, got: String }, Blocked(ManualDownload), Unresolvable { dependency: String }, Incompatible { a: String, b: String }, BadFileName(String), Locked { pid: u32 }, Stale, Io(std::io::Error), Other(Error) }
impl From<CatalogError> for Error { /* flattens to Http/Invalid/Io for the CLI exit codes */ }
```

The GUI matches on `CatalogError` for its copy (§8.4) and never on message text.

## GUI

The single source is **DESIGN-v2 §8** (final, 2026-09-11). Summary for the implementer:
- Nav rail gains **모드**; shortcuts Ctrl/Cmd+1..6. New `DialogKind`s: `mod-detail`, `install-confirm`, `version-picker`, `update-preview`, `mod-delete`, `server-menu`, `server-delete`, `server-duplicate`.
- Server selection through `PillStrip` (radio-group, roving focus, Enter/Space selects), persisted in `GuiSettings.last-mods-server`; a `SegmentedControl` 설치됨 / 찾기 shows one card at a time; the scope line (`<id> · Minecraft <mc> · <loader> <version>`) sits in the visible card header; every dialog title, progress text and toast names the server.
- Busy model: mutating jobs (install apply, update apply, remove, move, server delete/duplicate) take the global job lock and report through the top-bar pill + one `ProgressRow` in the card that owns the job; an open dialog stays open with disabled controls until the job ends; search / resolve / list reload / update check are quiet, single-flight jobs whose stale results are dropped by sequence number.
- Install always confirms through `install-confirm` (resolved version + type warning, namespace `SegmentedControl` default 필수, required deps pre-checked, optional collapsed, incompatible/unresolvable block the primary). Delete/duplicate/mod-delete use dialogs (no timed inline controls).
- New components: `PillStrip`, `ProgressRow` (live region), `ListRow.description`, `TextField.leading-icon`, `Button.danger`; dialog initial focus via body `forward-focus`; no new tokens. Icons reach Slint through parallel `[image]` models (paths for Modrinth/jar, `from_rgba8` for CurseForge). Settings: `GuiSettings` gains `curseforge-api-key` (form field) and `last-mods-server` (not a form field); `Config` gains `curseforge_api_key`. Copy formats in §8.8.

## CLI

`nebula mods <server-dir> search <query> [--provider modrinth|curseforge] [--sort relevance|downloads|newest] [--limit N] [--offset N]`, `… install <project id|slug> [--version <id>] [--namespace required|optionalon|optionaloff] [--no-deps] [--provider …]`, `… list`, `… update [--apply]`, `… remove <file> [--yes]` (required only when other managed mods depend on it), `… move <file> <namespace>`; `nebula server delete <dir> --yes`, `nebula server duplicate <dir> <new-id>`.

## Reliability rules (the "매우 안정적" contract)

1. Every download is staged in `<server>/.nebula-tmp/<pid>-<random>/` — never in `<root>/temp`, which `resolver/fg2.rs` deletes wholesale during any 1.7–1.12 Forge build, possibly from a second process. Staging inside the server directory keeps the final `fs::rename` same-volume and invisible to every scanner (`discover_toggleable` reads only the three namespace dirs; `count_files` walks `files/`). The file is hash-verified (sha512 preferred, sha1, md5/fingerprint for CF; no hash → refuse) and only then renamed into `<container>/<namespace>/`. A blocked CurseForge file is not a failure (it never enters the download set); any other failure aborts the `apply` with best-effort rollback and an honest report (`left_behind`). Stale `.nebula-tmp` entries older than one day are removed at the start of the next mutation.
2. Every provider call has a timeout, retries with backoff, and rate-limit handling. Rate limiters are process-global and shared by quiet and mutating jobs: one mutex per provider holding the last-request instant (CurseForge ≥ 250 ms spacing) and the parsed `X-Ratelimit-*` budget (Modrinth). A 429 wait is capped at 30 s and retried once; longer waits surface as `RateLimited`. Quiet jobs are single-flight per kind (search / resolve / list reload / update check) so rapid input cannot fan out.
3. The index is rewritten atomically (`.tmp` + rename) inside the server lock; reconciliation on read is read-only (D1).
4. Offline tests cover every provider mapping with recorded fixtures served by the in-process stub (`tests/common/mod.rs`: `fn stub(routes: impl Fn(&Req) -> (u16, &'static str /* content-type */, Vec<u8>)) -> (SocketAddr, Calls)`, `Req { method, target, headers, body }`, `Calls = Arc<Mutex<Vec<(Instant, Req)>>>` so the 250 ms spacing and the `x-api-key`-survives-redirect rule are assertable); live tests are gated by `NEBULA_NET_TESTS=1`.
5. Nothing in this feature changes the distribution output. `<server>/modindex.json`, `.nebula-tmp/` and `.nebula-lock` sit at the server-directory top level, which no scanner reads (`scan::discover` is non-recursive; `discover_toggleable` reads only `required/`, `optionalon/`, `optionaloff/`; `count_files` walks `files/`; `resolve_icon` reads only `.png`/`.jpg`); `<root>/cache/icons` is outside every scanned root. `FILE_NAME_BLACKLIST` (`[".gitkeep"]`, basenames inside scanned dirs) is irrelevant and must not be touched. A regression test builds a fixture root with all of these present and asserts the rebuilt `distribution.json` is byte-identical.
6. Every mutation of a server (mod install/update/remove/move, server delete/duplicate) is serialised **across processes** by an exclusive lock file `<server>/.nebula-lock` created with `create_new(true)` and deleted on completion, containing pid + start time. A lock younger than 30 minutes whose pid is alive → clean refusal (`다른 Nebula 작업이 이 서버를 사용 중입니다`, `CatalogError::Locked`); an older or orphaned lock is broken with a log line. The index is read-modify-written entirely inside the lock. (The GUI's in-process `busy` flag is a UX lock, not the correctness lock.)

## Open items

All items of v0.9 are resolved in the facts document and the tables below (endpoints/params/fields → facts §A/§B and "Provider quirks"; CurseForge key policy → D2; fingerprint → D8; webp → D5; dependency semantics → D6). Remaining risks are tracked in facts `## E. Gaps`.

## Decisions (2026-09-11, from `2026-09-11-mod-catalog-facts.md` D1–D8 and R1–R12)

| # | Decision | Why |
|---|---|---|
| D1 | One JSON index per server, `<server>/modindex.json`, written atomically (`.tmp` + rename) and reconciled **read-only** against the mod directories on every load (absent files are flagged `missing`, never dropped from the file; entries leave only through an explicit user mutation; a reconcile that sees zero files against a non-empty index is never persisted); hand-placed jars appear as `직접 추가`. | Prism/packwiz interop is not a goal (the mods folder is consumed through the distribution, not by Prism); O(1) lookups; losing the file degrades to "unmanaged", never to data loss. |
| D2 | CurseForge browsing/installing requires the **user's own API key** (`NEBULA_CURSEFORGE_API_KEY` or the settings field `CurseForge API 키`); without a key every CurseForge surface is hidden and a Callout explains how to get one (console.curseforge.com). The inherited upstream key stays only as the fallback for the existing modpack-import path (upstream behaviour). No CurseForge API data is cached on disk: CF icons live in a per-session memory cache; the index stores only the ids/hashes of the user's own installs — for CurseForge entries `title`, `project_slug` and `icon_url` are omitted (see `index.rs`). `x-api-key` is sent on CDN downloads too and must survive the `edge.forgecdn.net` → `mediafilez.forgecdn.net` redirect (R2); the stub test routes A → 302 → B on another port and B asserts the header. README carries the CurseForge 3rd-party terms note. | ToS forbids sharing keys and caching API data; Modrinth is the primary, fully-featured provider. |
| D3 | Version selection: newest **release** for the server's Minecraft version + loader; if none, newest beta, then alpha, each labelled (`베타`/`알파` chip and a warning line). Users can pick any version in the version dialog. | A provisioning tool should not silently ship alphas (Prism does). |
| D4 | No Forge/NeoForge widening on 1.20.1 (Nebula does not support legacy NeoForge 1.20.1). | Fewer surprises. |
| D5 | Icons: decode with `image = { default-features = false, features = ["png","jpeg","webp"] }` sniffing magic bytes (never the extension); Modrinth icons cached at `<root>/cache/icons/<sha1(url)>.png`; jar icons at `<root>/cache/icons/local-<sha1(jar)>.png`; CurseForge icons memory-only (D2). A CI step asserts that `cargo tree -p nebula -e normal,no-proc-macro -i rav1e` prints no tree (it prints `warning: nothing to print.` today, so assert on the output, not the exit code), so nobody enables `slint/image-default-formats` (R8) — note `-e normal` or `-e no-proc-macro` alone are **not** empty, because rav1e is reachable through `slint-macros` (proc-macro) and `slint-build` (build-dep) — a `deny.toml` `bans.deny` entry cannot express this (cargo-deny cannot scope a ban to the normal graph) and a plain `Cargo.lock` grep would fire today, since `rav1e` is already in the lockfile via the host/proc-macro graph. | |
| D6 | Dependencies: `required` resolved recursively (depth ≤ 6, cycle-safe, deduplicated, shown pre-checked in the dialog); a project counts as **already installed** when the index holds its `(provider, project_id)` in any namespace, OR when a jar in any namespace reports the same mod id via `jarmeta` (best-effort slug match for `직접 추가`) — such a dependency is listed disabled with `설치됨`, never hidden, and `apply` re-checks under the lock; `optional` listed unchecked; `incompatible` blocks the install when the conflicting project is installed (error names both); `embedded` ignored; an unresolvable required dependency **fails the whole install** with the dependency named — never dropped silently. | A dropped required dep produces a broken server. |
| D7 | Update check — Modrinth: one `POST /v2/version_files/update` request per server (single loader, all sha1s, lowercase hex; `update_many` is the documented successor — keep the path a constant). CurseForge: `POST /v1/fingerprints` identifies the installed files only (its `exactMatches[].latestFiles` is **unfiltered** across every MC version/loader — verified live with JEI: 11 entries, none for the installed 1.20.1); the update candidate then comes from **one `GET /v1/mods/{id}/files?gameVersion=<mc>&modLoaderType=<loader>&pageSize=50` per managed CurseForge mod**, throttled by R7 (this is what Prism does). Results filtered by mc+loader; "is this an update" (returned hash ≠ installed hash) and "which file to download" are computed independently (R12). | A single CF request cannot answer the question; N throttled requests are the correct cost. |
| D8 | All logic in `nebula-core::catalog`; Slint views in `nebula`. `hash.rs` gains sha512 and the CurseForge MurmurHash2 variant. Offline known-answer vectors are generated once from the reference implementation into `fixtures/catalog/murmur2.json` (empty, whitespace-only, 1-byte, 3-byte tail, > 4 KiB buffer exercising the filtered-length path); the `jei-1.20.1-forge-15.59.0.210.jar` → `2298786945` pair runs behind `NEBULA_NET_TESTS=1` (R11). | |
| R3/R4 | Blocked CurseForge files (`downloadUrl == null`): call `GET /v1/mods/{modId}/files/{fileId}/download-info` (returns 200 with `downloadUrl: ""` plus sha1/md5 for blocked files, where `/download-url` is a bare 403), then try Modrinth by that sha1 (`POST /v2/version_files`), else return `ManualDownload { name, file, url: links.websiteUrl + "/files/" + fileId }` and the UI shows the link; **CDN URL reconstruction is forbidden** (code comment + CONTRIBUTING line). Never report success without bytes on disk. | |
| R5 | Every provider struct field is `Option` or defaulted; unknown fields ignored; base URLs and API version constants injectable for tests and for the v2→v3 move. | |
| R6 | Modrinth `url` is requested verbatim; the provider `filename` is written verbatim **only after** a single-segment check stricter than `paths::is_single_segment`: non-empty, not `.`/`..`, no `/ \\ : \0`, no leading/trailing space or dot, not a Windows reserved device name, not in `FILE_NAME_BLACKLIST`, ends with `.jar`. A failing name is a hard failure `CatalogError::BadFileName` (`제공자가 보낸 파일 이름을 쓸 수 없습니다: {name}`) — never sanitised, because the name is the index key. Unit tests: the Sodium `%2B`/`+` pair, `../../../servermeta.json`, `a/b.jar`, `CON.jar`, `""`. | |
| R7 | CurseForge client-side throttle: ≥ 250 ms between requests, search results reuse `latestFilesIndexes` instead of per-mod `/files` calls; Modrinth honours `X-Ratelimit-*` and sleeps on 429. | |
| R9 | Every jar parse is a per-file `Result`; a corrupt jar becomes a row `메타데이터 없음` with the file name, never a crash; tests include corrupt/truncated jars and a toml with `$` in a key. This hardens `jarmeta.rs` only: the distribution scanners still abort the whole build on a non-jar in a namespace dir, so the installed list flags such files as `배포 불가` with the fix (§8.3) and the manual-download callout repeats the rule (§8.4). | |
| D9 | GUI busy model as in DESIGN-v2 §8.6: mutating jobs take the global lock, read-only catalog calls are quiet jobs, single-flight per kind, with stale-result dropping by sequence number (`Ui.search_seq` / `list_seq`, incremented at spawn and compared in the applier). A dialog that is open when a mutating job starts stays open with its controls disabled until the job ends; dialog controls therefore stage changes and a primary applies them (§8.6). | One mutating job at a time keeps the index and the directories consistent; read-only calls must never block each other or the UI. |
| D10 | Destructive actions (mod delete, server delete, server duplicate) confirm through dialogs; server delete requires typing the exact directory name; no timed inline confirmations. | Consistent with the existing dialog kit; screen-reader and keyboard predictable. |
| D11 | CLI parity is mandatory for every GUI mutation (search/install/list/update/remove/move, server delete/duplicate); `--yes` replaces prompts; exit codes follow the existing convention. | Scripted provisioning and QA. |
| R10 | `deny.toml` `[licenses] allow`: drop `GPL-3.0-only`, add `LicenseRef-Slint-Royalty-free-2.0` (Slint is tri-licensed; the binary is distributed under the royalty-free licence), so an accidental GPL dependency fails the gate. Run `cargo deny check licenses bans` in the same commit; if any crate outside the Slint family then resolves to GPL-3.0-only, stop and report instead of re-adding the allow entry. (R8 is enforced by the `cargo tree` check in D5, not by `deny.toml`.) | |

## Provider quirks the implementation must honour (from the adversarial verification, 2026-09-11)

Modrinth v2
- Filter loaders with `categories:<loader>` (documented, project-level: the project has any version for that loader). `loaders:` is a different, version-level filter (fewer hits); never combine them.
- The `project_type:mod` facet is rewritten server-side to the plural `project_types`, so a modpack that ever shipped a mod version matches — also check the hit's own `project_type == "mod"` and drop the rest.
- `X-Ratelimit-Reset` can read `0` on a fresh window — clamp the sleep to ≥ 1 s; Modrinth sends no `Retry-After`. 404 bodies are empty; 400 carries an undeclared `details[]` — never require a body to build an error.
- Prefer `environment` over the deprecated `client_side`/`server_side`; deserialise all three as `Option`.
- `GET /v2/project/{id}/version` accepts `loaders`, `game_versions`, and real `limit`/`offset`; results are sorted by `date_published` desc — we still apply D3 ourselves.
- `files[].hashes.sha1`/`.sha512` are individually optional → `Option<String>`; verify with sha512 when present, else sha1, else refuse to install ("해시 없는 파일은 설치하지 않습니다").
- File choice: the `primary: true` file; if none is primary, the first file whose name ends with `.jar` and does not end with `-sources.jar`/`-dev.jar`/`-javadoc.jar`; `file_type` is uploader-supplied and null proves nothing.
- Request `url` verbatim; write `filename` verbatim (legacy uploads carry literal `%2B` — do not decode); unit test on the Sodium pair.
- `POST /v2/version_files` / `/version_files/update`: hashes must be lowercase hex; unknown hashes are simply absent from the response map; `algorithm` is not validated by the server (a wrong algorithm looks like "no matches") — always send the algorithm matching the hash length; "an update exists" = returned file hash ≠ installed hash (never compare version ids alone). There is no `/version_files/latest`.
- Rate limit 300/min with `X-Ratelimit-*` headers; 429 body `{"error":"ratelimit_error",...}`; User-Agent is policy, we send `nebula/<version> (https://github.com/LaeDev/nebularust)` anyway.

CurseForge v1
- `modLoaderType=0` returns nothing (not a wildcard) — omit the parameter instead.
- `gameVersions`/`modLoaderTypes`/`categoryIds` arrays combine as unions (AND for categories); documented max sizes are not enforced — we cap ourselves.
- `latestFilesIndexes[].modLoader` and the `links.*` fields are nullable → `Option`. When a file's `modLoader` is null, derive loaders from `sortableGameVersions` entries with `gameVersionTypeId == 68441` ("Modloader"; `75208` = Environment) instead of string-matching `gameVersions[]`.
- Files with `isServerPack == true` are never install candidates. `ModStatus` and `fileStatus` both use `4 = Approved` but are different enums — two Rust types.
- `pageSize` on `/mods/{id}/files` is honoured even at 10000; we still page at 50 and stop at the first page that satisfies the mc+loader filter.
- Blocked files: `downloadUrl == null`; `/download-url` returns 403 with an empty body; **never** reconstruct `edge.forgecdn.net` URLs (R4).
- `POST /v1/fingerprints`: fingerprints may be sent as JSON numbers; `unmatchedFingerprints` may be `null` → compute unmatched = submitted − exact; `partialMatchFingerprints` is `map<string, i64[]>`.
- No rate-limit headers and no declared 429: fixed ≥ 250 ms spacing and request coalescing (R7). Batch endpoints (`/mods`, `/fingerprints`) silently drop unknown ids — compute "missing" client-side.

Prism reference notes
- Prism's Modrinth-by-sha1 fallback for blocked files exists only in its modpack-import path; its browser drops versions with an empty URL. Nebula applies the fallback in both paths (superset), then the manual link.
- Prism's release-type strings are `Release/Beta/Alpha/Unknown` (irrelevant to us: JSON index, D1).

## Implementation notes (phase A)

Phase A shipped `catalog/{mod,http,modrinth,curseforge,jarmeta,index,lock}.rs`, the `hash.rs` additions, `Config.curseforge_api_key`, the `Namespace` serde derives, the `image` dependency and the R10 `deny.toml` change. `install.rs`, `update.rs`, `icons.rs` and `manage.rs`, the CLI and the GUI are untouched. Deviations from this plan, all deliberate:

**Types and the trait**
- `ManualDownload` is **re-exported from `crate::import`**, not redefined: the fields are already `{ name, file_name, url }` and a second same-named type would clash with the existing `lib.rs` re-export.
- `Provider` gains one method with a default body, `updates_by_hashes(algo, hashes, mc, loader)`. D7's Modrinth endpoint has no home in the plan's trait, and `update.rs` only ever holds a `&dyn Provider`. CurseForge keeps the default (an empty map) because its fingerprint matches are unfiltered by version (facts §E.3.1); it answers through `latest_file`.
- Three helpers carry rules the plan states in prose: `Version::primary_file()` (§A.4 primary-file rule), `Version::has_file_hash()` (R12: an update is a hash difference, never an id one) and the free `preferred_version()` (the D3 ladder, which `install.rs::resolve_version` will call).
- `Catalog` has **no `icon_memory` field yet** — `IconBytes` belongs to `icons.rs`. `Catalog::new(Endpoints, Option<String>)` does not change when it is added.
- `CatalogError` also implements `From<Error>`, so in-crate helpers work with `?`. `From<CatalogError> for Error` unwraps `Other(e)` rather than re-wrapping it, so the round trip is lossless.
- Modrinth's `environment` / `client_side` / `server_side` are **not deserialised**: no plan type carries a side, so the fields would be dead. The preference order is recorded in a comment at the mapping site for whoever adds the side chip.
- An unrecognised Modrinth `version_type` or CurseForge `releaseType` becomes `Alpha`, so D3 only reaches it when there is nothing else.
- CurseForge `versions_by_hashes` takes **decimal fingerprints** in the `hashes` slot and ignores `algo` (D8); `ModStatus` and `FileStatus` are two Rust enums whose only consumer today is a debug log when the value is not Approved.
- `/mods/{id}/files` pages **to the end of the list** (cap 5 pages of 50), not "to the first page that satisfies the filter" as the `curseforge.rs` row says. Nothing documents the order `/files` returns; if it is oldest-first, stopping at the first page with a match hands D3 an ancient release as "the latest". The server already filters by version and loader, so this is one request for almost every project.
- A CurseForge file with **no** `68441` row is accepted for any loader (Prism's behaviour), so a loader-less file can outrank a correctly tagged one under D3. `tests/catalog.rs` asserts exactly that, which looks wrong at first glance and is not.

**HTTP**
- `remote::client()` is now `pub(crate)` but its **builder is unchanged**: catalog sets `.timeout(120 s)` per request instead of the `.timeout(None)` the integration map proposed, which would have altered every resolver download. One client, two policies.
- The 429 wait is computed before the body is touched (`publish::check` consumes it): `Retry-After`, else `X-Ratelimit-Reset` clamped to 1–60 s, else the backoff schedule; over 30 s it is `RateLimited` without sleeping.
- The CurseForge limiter is `Mutex<Option<Instant>>` so the first request of a process is not delayed.
- `x-api-key` **does** survive a cross-origin 302 under reqwest's default redirect policy (facts §E.2.7 left this OPEN); `tests/catalog.rs` pins it with two stubs on different ports, so a reqwest upgrade that starts stripping it fails the suite.

**jarmeta / index / lock**
- `jarmeta::read(path)` takes no `LoaderKind` or `ForgeFlavour`: a hand-placed jar may be anything, so it probes `fabric.mod.json` → `quilt.mod.json` → `neoforge.mods.toml` → `mods.toml` → `mcmod.info` → crude inference. Consequence: an OptiFine jar is always named the `Modern113` way (`OptiFine`), never the 1.7–1.12 changelog first line.
- `LocalMod` also carries `file_name` and the four digests; a non-`.jar` name or a zip that will not open is `LocalFile::Unshippable { file_name, reason }`, not an error.
- Each jar is read four times (zip, md5+sha1, sha512, fingerprint). The fingerprint alone needs two passes (the seed mixes the filtered length), and widening `Digests` was ruled out by the facts document; if the installed list ever feels slow, one combined pass in `hash.rs` is the fix.
- `IndexEntry::installed_at` is unix seconds (`u64`) — no date dependency.
- The CurseForge free-text rule is enforced **in the constructor** (`IndexEntry::with_text` silently keeps nothing for CurseForge) plus plain `skip_serializing_if = "Option::is_none"`, because `skip_serializing_if` cannot see a sibling field.
- `reconcile` returns `Reconciled { rows, persistable }`. It never writes, so the zero-files-vs-non-empty guard is a flag the caller must check before persisting anything derived from the pass.
- A `modindex.json` whose `version` is newer than 1 is an error rather than a silent downgrade that would drop fields on the next save (facts §E.3.3 left the migration rule OPEN).
- `ServerLock` has **no pid liveness probe**: `std` has none and a recycled Windows pid would make a naive one lie. A lock is broken only when it is older than 30 minutes or its body is unparsable, so a crash can refuse a concurrent run for at most half an hour.
- `layout.rs` gained `ServerDir::modindex_file()` only; `Root::icons_cache_dir()` lands with `icons.rs`.

**Tests and build**
- `tests/common/mod.rs` keeps the rule-4 signature. Extension: when the response's second element contains a colon it is written out as raw header lines instead of a `Content-Type`, which is how a route sets `Location` on a 302 and `Retry-After` on a 429.
- `tests/fixtures/catalog/murmur2.json` was generated from an **independent JavaScript** implementation of the public-domain MurmurHash2 reference, not from the Rust under test. The empty-input answer (`0x5bd15e36`) is additionally derived by hand in a `hash.rs` unit test, so the fixture is anchored by something neither implementation produced.
- `golden.rs` gained the rule-5 regression: the same root built with `modindex.json`, `.nebula-lock`, `.nebula-tmp/` and `cache/icons/` present produces a byte-identical `distribution.json`.
- `image` is in `crates/nebula-core/Cargo.toml` but unused until `icons.rs`. `cargo build` adds only the `"image"` line to nebula-core's `Cargo.lock` entry — no new package (669 before and after).
- `cargo deny check` passes with `GPL-3.0-only` dropped and `LicenseRef-Slint-Royalty-free-2.0` added (R10); no non-Slint crate resolved to GPL.

**Seams phase B calls into:** `Catalog::provider`, `Provider::{search, project, versions, versions_by_hashes, latest_file, updates_by_hashes}`, `preferred_version`, `Version::{primary_file, has_file_hash}`, `catalog::index::{ModIndex::load, ModIndex::save, IndexEntry::new, IndexEntry::with_text, reconcile}`, `catalog::jarmeta::read`, `catalog::lock::ServerLock::acquire`, `hash::{sha512_hex_of_file, curseforge_fingerprint_of_file}`, `layout::ServerDir::modindex_file`.

## Implementation notes (phase B)

Phase B shipped `catalog/{install,update,icons,manage}.rs`, the `catalog::http` download path, `Root::icons_cache_dir`, `Catalog.icon_memory`, and the `nebula mods` / `nebula server` CLI. The GUI is still untouched. Deviations from this plan, all deliberate:

**Signatures phase C calls**
- `install::resolve_version(&Catalog, ProviderId, project_id: &str, &MinecraftVersion, Loader) -> Result<Option<Version>, CatalogError>` — the plan's row wrote `(project, mc, loader, prefer)`; the provider has to be named because `Catalog` holds two, and `prefer` is D3 itself, which `preferred_version` already encodes.
- `install::plan_install(&Catalog, &ServerDir, ProviderId, project_id: &str, version: Option<&str>) -> Result<InstallPlan, CatalogError>`.
- `install::apply(&Catalog, &ServerDir, &InstallPlan, &InstallChoice, Namespace, &(dyn Fn(usize, usize, &str) + Send + Sync)) -> Result<InstallReport, CatalogError>`.
- `update::{check_updates(&Catalog, &ServerDir), replace_file(&Catalog, &ServerDir, old_file_name: &str, new: &Version, progress), apply_updates(&Catalog, &ServerDir, &[UpdateCandidate], progress)}`.
- `icons::{fetch(&Catalog, &Root, ProviderId, key: &str, icon_url: &str) -> Result<IconRef, CatalogError>, local_icon(&Root, jar_path: &Path, bytes: &[u8]) -> Result<PathBuf, CatalogError>}`.
- `manage::{delete_server(&Root, dir_name), duplicate_server(&Root, dir_name, new_id) -> Result<String>, list_installed(&ServerDir) -> Result<Vec<InstalledMod>>, remove_mod(&ServerDir, file_name), move_mod(&ServerDir, file_name, Namespace), server_names(&Root) -> Vec<String>}`. **`remove_mod` and `move_mod` live in `manage.rs`**, next to `list_installed`, rather than in `install.rs`: all three are "one file, under the lock, index in step".
- New shared helper `catalog::server_context(&ServerDir) -> Result<ServerContext { mc, loader, container }, CatalogError>`; every entry point needs the same three facts and nothing else read them off `servermeta.json` before. `server::server_loader` became `pub` for it.

**Install**
- `CatalogError` gains **`RolledBack { source: Box<CatalogError>, left_behind: Vec<PathBuf> }`**. `left_behind` can only be produced by a rollback, so returning it inside `Ok(InstallReport)` would have made it dead on every path it can appear on, while §8.4 needs the cause *and* the leftovers at the same moment. `apply` therefore returns `Err(cause)` when the rollback was clean and `Err(RolledBack { .. })` when it was not. **`InstallReport.rolled_back` and `left_behind` are therefore always default in a returned report**: on the failure path the GUI has no report at all and must read both off `CatalogError::RolledBack`. The two fields stay in the type because the plan froze it. `From<CatalogError> for Error` unwraps the new variant first, so a rolled-back `HashMismatch` keeps its `Error::Http` exit code.
- `install::check_name_free(&ModIndex, file_name, replacing)` closes the one collision `(provider, project id)` cannot see: two different projects whose primary file carries the same name. The index is keyed by file name, so installing the second would overwrite the first jar **and** drop its entry in one silent step. `apply` checks it before any download and `replace_file_locked` again under the lock; a replace may only take over the entry it is replacing.
- The dependency walk is breadth first (`VecDeque`), so `seen` records every project at its shortest depth and D6's "depth ≤ 6" means what it says: a diamond whose long arm is deeper than the bound is still resolved through its short one.
- `apply` orders the work **dependencies first, root last**, because the root is the only file that can already be installed and therefore the only one that goes through `update::replace_file`. A failure during the replace rolls back the fresh files; a failure after it cannot happen.
- `replace_file` is split into a public form that takes the lock and saves, and `pub(crate) replace_file_locked(.., &mut ModIndex, ..)` that does neither. `ServerLock::acquire` uses `create_new(true)`, so a re-entrant acquire from `apply` would return `Locked { pid: <our own pid> }` — a self-deadlock that reads as contention. `apply` loads the index once, mutates it across fresh installs and the replace, and saves exactly once, still inside the lock.
- `InstallChoice.deps` gates **required and optional** dependencies alike, which is what `--no-deps = empty` means. The CLI pre-fills it with every required dependency's project id; a caller that forgets to would install the root alone.
- The index `dependencies` of the **root** entry list every transitive required dependency the plan resolved; a dependency's own entry gets an empty list, because `PlannedDep`/`PlannedFile` carry no parent link and this plan freezes those types. The delete warning of §8.3 only reads the root direction, so nothing is lost there.
- `plan_install` records the `index_sha1` through `install::index_sha1(&ServerDir)`, the sha1 of `modindex.json`'s bytes, or of nothing when the file does not exist yet. `apply` re-checks it with the same function, so there is one convention.
- The R6 reserved-name test compares the segment **before the first dot**, so `CON.jar` is refused as `CON` is; `%2B` and `+` both pass and neither is ever decoded into the other.
- `plan_install` reads jar mod ids for the D6 slug match **only on an index miss**, and once per plan. `jarmeta::read` hashes each jar four times; a `mod_id`-only reader is the noted upgrade if planning ever feels slow.
- `Installed::find` for a dependency whose `project_id` is absent (a Modrinth dependency that names only a `version_id`) pushes the version id onto `unresolvable` when it is required — no endpoint of ours resolves one, and silently dropping it is what D6 forbids.
- `fetch_verified` hashes the downloaded file once with `digests_of_file` and writes those digests into the index, so an entry always describes the bytes on disk rather than what the provider claimed.

**Update**
- `check_updates` excludes `파일 없음` entries by listing the namespace directories, not by running a full `reconcile`: reconciling would open and hash every jar for an answer that only needs file names.
- `UpdateCandidate.title` falls back to the file name for CurseForge entries, which carry no cached title (D2). The installed list already has the jar's own name.
- `apply_updates` takes the lock once and saves the index after each mod. A lock or context failure has nowhere to go in its `Vec<(String, Result<…>)>` signature, so every row carries the same cause flattened to `Other(Invalid)`.

**Icons**
- `IconRef` / `IconBytes` live in `icons.rs`; `Catalog.icon_memory: Mutex<HashMap<String, IconBytes>>` was added as the plan said, with `Catalog::new` unchanged.
- `fetch` takes the provider and the project-id key explicitly rather than a `ProjectHit`, because the installed list has an index entry and a jar, never a hit.
- Resizing is `resize_exact` with Lanczos3 to exactly 72x72; `image` 0.25 has no WebP **encoder**, so the offline WebP test uses a 34-byte lossless literal that the test itself decodes.

**Manage**
- `duplicate_server` holds the **source server's lock across the whole copy**, not only across the final rename, so a concurrent install cannot be caught half-written; `modindex.json` is copied as-is because the copy holds the same mods.
- `duplicate_server` writes `meta.meta.name = "{new_id} (Minecraft {mc})"`, the same shape `default_server_meta` produces. A custom source name is deliberately not carried over: two servers sharing one display name is worse than a predictable one.
- `delete_server`'s trailing-space case is caught by the resolved `file_name` check, not by containment — Windows normalises `"Test-1.20.1 "` away while canonicalising, so the path check alone would pass.
- `ServerLock`'s `Drop` now ignores `NotFound`, so deleting a server while holding its lock does not log a warning about a file that went with the directory.
- `remove_mod` / `move_mod` check the file name for path escapes but **not** against R6: R6 governs what a provider may name a file we write, and a `배포 불가` `notes.txt` already in the directory has to be deletable (§8.3).

**CLI**
- `nebula mods <server> {search,install,list,update,remove,move}` and `nebula server {delete,duplicate}` as specified. `--namespace` and the `move` argument take `required|optionalon|optionaloff` (clap's kebab-case default would have produced `optional-on`). `server delete` without `--yes` prints the absolute path and the distribution warning, then fails with the `Invalid` exit code.
- Progress is written to stderr as `다운로드 중 2/5 · <file>`; tables and summaries go to stdout.
- `mods_command(&Config, &Root, server, ModsCommand)` is split out of `dispatch` so the end-to-end test drives it against a temporary root without touching the process environment.

**Tests and build**
- `tests/catalog_install.rs` adds 31 offline tests; `install.rs` adds 2 unit tests; `cli.rs` adds 2. Workspace total 262 → 297.
- The rule-5 distribution regression was **already shipped in phase A** (`golden.rs::the_catalogs_own_files_do_not_change_the_distribution`); no second one was written.
- The failure injected between `replace_file`'s steps is a directory occupying the new file's path (rename-in fails after the old file has moved out), and a 404 rather than a 500 where only "the download failed" matters — a 500 costs the retry schedule's 3.5 s and proves nothing extra, and `tests/catalog.rs` already covers the 5xx retry.
- `image` was added to `nebula-core`'s `[dev-dependencies]` and `tempfile` to `nebula`'s, both at the versions already in the graph: `Cargo.lock` still holds 669 packages, `cargo deny check` is clean and `cargo tree -p nebula -e normal,no-proc-macro -i rav1e` still prints `warning: nothing to print.` (D5/R8).

## Implementation notes (phase C)

Phase C shipped the Rust host of the 모드 page: `crates/nebula/src/gui/mods.rs`, the `GuiSettings` additions, `Screen::Mods`, and the navigation/tick/snapshot hooks in `gui/mod.rs`. No new dependency; no `.slint` change was needed. Deviations from DESIGN-v2 §8, all deliberate:

**State and structure**
- One `ModsState` struct, held as a single `Ui.mods` field, with free functions `fn name(ui: &Rc<Ui>)` in `mods.rs`. No `ModsHost` with its own weak reference back to `Ui`: a second ownership story for the same window buys nothing.
- `Ui::apply_snapshot` now takes `self: &Rc<Self>`, because the 모드 page's pill strip is fed from the same rows and its applier needs an `Rc`. Every caller already held one.
- `ModsData.servers` is derived in `apply_snapshot` from the rows `server_rows` already produced; `Snapshot` gained no field and server-list validation stays in one place.
- `form_from_ui(app, stored)` carries `last_mods_server` over from what is stored. It is not a form field, so a 저장 that rebuilt the struct from the form alone would wipe the 모드 page's selection on every save — and the same function is what computes `SettingsData.dirty`.
- `select-server` persists through `next.save()` directly, the way `appearance-changed` does, not through `commit_settings`: a pill click is not a form validation, a global job and a 설정을 저장했습니다 toast.
- `ModsData.manual` has its own `VecModel`. `Ui::manual` is bound to `ImportData.manual-downloads`; sharing it would let an install rewrite the 가져오기 screen's list.

**Copy (§8.8) and the formats the spec does not name**
- `install-confirm`'s resolved-version line stops at the size (`fabric-api 0.92.0 · 정식 · 2.1 MB`). §8.4 ends it with the publication date, but `PlannedFile` carries no date and this plan freezes the type; the date is one field away if the freeze is ever lifted.
- Relative time gains the two ends §8.8 does not name: under a minute is `방금 전`, and one hour to one day is `N시간 전`. `1440분 전` is not a sentence anyone reads.
- Counts are **truncated**, never rounded, so 99,999,999 reads `9999만` rather than `1억`. Under 만 the plain number is printed.
- A hit with no author reads `작성자 미상`; with no `updated` the caption is author + downloads alone.
- The installed row's version comes from `LocalMod.version` (the jar's own), because the index stores a provider version **id** — a slug like `xK9Za2`, not a number — with `version_id` as the fallback and `버전 미상` as the last resort.
- `이동` toasts are three written-out sentences (`필수로` / `선택 켜짐으로` / `선택 꺼짐으로`): the labels take two different particles and a computed batchim rule would be three lines of code to get one of them wrong.
- `준비 중` is a new string: the `ProgressRow` goes up the moment a mutating job starts, and §8's progress line only exists once the first file is announced. It is replaced by `다운로드 중 1/N · …` as soon as the core calls back.

**Behaviour**
- The §8.5 delete gate is enforced **in the host**, on the trimmed input, before anything happens. The dialog's own binding compares untrimmed (Slint 1.17 has no `trim`), so it is the stricter of the two and only ever refuses early.
- `open-project-page` asks `Provider::project(project_id)` quietly and opens the returned `page_url`. The index keeps no URL, and a CurseForge entry deliberately keeps no slug either (D2), so one request answers for both providers.
- The 429 retry is exactly one, waits `min(retry_after, 30 s)` on the worker and sets its message once — no ticking countdown. The message is written by the **retry**, not by the attempt that failed, because starting a job clears the Callout; the same line also stands in for the progress text while the worker sleeps. Note that `catalog::http` already absorbs sub-30 s 429 waits, so a `RateLimited` reaching the GUI is one core already declined to wait for; the retry is cheap insurance rather than the common path.
- `mods.slint` routes the failure Callout's 다시 시도 to one callback for both owners, so the host keeps a `Retry` slot. A **quiet** failure always rewrites that slot — `Reload` for the installed list, `Check` for the update check, `None` for everything else — so 다시 시도 under a failed search can never re-run the last 삭제.
- A downgrade in `version-picker` is a two-click confirmation: the first click writes `현재 X → Y로 내립니다` into the dialog, the same row clicked again applies it.
- The installed list reloads on entering 모드, and from a snapshot only while that page is on screen: reading a two-hundred-mod list hashes every jar four times (`jarmeta::read`), and a snapshot fires on every visit to 서버 and 가져오기.
- Icons: a bounded queue of four, pumped on the event loop, keyed by file name / project id rather than row index, and already-decoded images are carried across a re-render so an update check does not re-hash every jar. `IconRef::Path` → `Image::load_from_path`, `IconRef::Rgba` → `Image::from_rgba8`, both on the event-loop thread.
- `open_folder` and the new `open_external` share one spawn helper. They are kept apart because `open_folder` starts with `create_dir_all`, which on a URL would make a directory named after the address.
- `show-all-installed` is a log line: the eight-row cap is the screen's own and the host already feeds it the whole list.
- §8.6's "an open dialog stays open with its controls disabled and Esc refused" needed **no** `.slint` change: every 모드 dialog already carries `closable: !SheetData.busy`, every body gates its controls on the same flag, and `app.slint`'s Esc handler is guarded by `&& !SheetData.busy`.

**Core changes**
- `update::apply_updates` now returns `Result<Vec<UpdateOutcome>, CatalogError>` (`UpdateOutcome = (String, Result<(), CatalogError>)`, a named alias because clippy refuses the nested type). A refused lock, an unreadable `servermeta.json` or a broken index is nobody's per-mod failure, and flattening it into every row cost the variant the GUI picks its copy by. `fail_all` is gone; `cli.rs::mods_update` adds one `?` and the offline test one `expect`.
- No accessor was added to `InstalledMod` or `IndexEntry`: `title()`, `is_unmanaged()`, `entry.{provider, project_id, version_id, namespace, hashes, dependencies}` and `local.{name, version, description, icon, sha1}` already covered every field the page shows.

**Tests and verification**
- `gui/mods.rs` adds 14 unit tests for the pure formatters and the plan-to-rows mapping; `gui/settings.rs` adds 2 (an old settings file without either new key loads; both persist under their kebab-case names and reach `Config`). Workspace total 297 → 314.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- The GUI was launched once against a temporary `NEBULA_CONFIG_DIR` and a two-server fixture root with one hand-placed jar, driven to 모드 with Ctrl+3 and closed from its own window: it read the root, listed both servers and logged `Neo-1.21.1: 설치된 모드 1개 (관리 0 · 직접 추가 1)`, exit code 0.

## Fix notes (core)

The review, CLI-QA and GUI-QA lanes' core findings, applied 2026-09-11. Only `nebula-core`, `cli.rs` and two compile fixes in `gui/mods.rs`; the GUI findings (F8, F11, D2, D4-D7) and the GUI halves of F12/D1/D3 are the GUI lane's.

**Types that moved**
- `PlannedFile` gains `text_provider: ProviderId`, `version_number: String` and `published: String`. `text_provider` is where the *free text* came from, which is not always `provider`: the R3 mirror branch downloads from Modrinth while its title is still CurseForge's, and `entry_for` now drops slug and title on `text_provider == CurseForge` rather than relying on `IndexEntry::with_text`, whose guard reads the rewritten download provider (F2).
- `IndexEntry` gains `release: Option<ReleaseType>` (serde-skipped when absent; an older entry counts as `Release`), written at install and replace time. `ReleaseType` therefore derives `Serialize`/`Deserialize`, lowercase (CLI-QA-01).
- `CatalogError` gains `ServerChanged` (the server or index moved under an open plan — `apply` returns it instead of `Stale`, which now only means age, F6) and `NameTaken { file }` (F1).
- `check_updates` returns `CheckReport { candidates, adopted }` (F7).
- `install::check_name_free(&ModIndex, &Path, &BTreeSet<String>, file_name, replacing)` is the one name-collision helper `apply` and `replace_file_locked` share.
- `manage::dependents_of(&ModIndex, file_name) -> Vec<String>` (CLI-QA-03); `manage::rename_copy` takes the source id (CLI-QA-04).
- `LocalMod` loses `sha512` and `fingerprint`: `jarmeta::read` now makes **one** pass over each jar (`digests_of_file`) instead of four. The two callers that need the other digests ask per file through `hash::sha512_hex_of_file` / `hash::curseforge_fingerprint_of_file` (F7).

**Behaviour**
- A file name is refused when the index holds it, when this apply already claimed it, or when a *file* of that name sits in any of the three namespace directories — so an install can no longer overwrite a hand-placed jar and then delete it in the rollback. `is_file`, not `exists`: anything else on that path is not a mod anybody would lose, and the rename refuses it anyway (which is what the "replace fails after moving the old file out" test injects).
- The `Incompatible` dependency arm resolves the project for its slug through `Provider::project` alone, **not** `resolve_dependency`: the latter also returns `None` when the project has no build for this Minecraft version and loader, which is precisely the state a stale hand-placed conflicting jar is in (F10). One extra request per declared incompatibility.
- `check_updates` adopts `직접 추가` jars: one `POST /v2/version_files` by sha1, then one `POST /v1/fingerprints` for whatever Modrinth did not recognise, both bulk. Matches are written as managed entries **under the server lock**, and the comparison then runs against the reloaded index, so an adopted mod's update is reported in the same run. A refused lock or a failed lookup only postpones the adoption; it never fails the check the user asked for (F7).
- Modrinth's bulk update endpoint answers with the newest file of any release type, so D3 is applied afterwards: a candidate ranked worse than the installed entry sends one `versions` request and offers `preferred_version` instead, and only when its file hash differs (CLI-QA-01). A same-or-better rank is still taken straight from the bulk answer, so the common path stays at one request.
- A CurseForge project that 404s is skipped with a log line instead of failing the whole check (F5).
- `throttle` computes its wait under the limiter and sleeps after dropping it, so a rate-limit window can no longer make another process see `Locked` while a mutating job holds the server lock. The CurseForge static became `CURSEFORGE_NEXT`, a reservation of the next allowed instant, because "sleep outside the mutex" and "last request's instant" together would lose the 250 ms spacing (F4).
- `sweep_partials` skips the directory the current duplicate is about to create and only removes leftovers older than `STALE_TMP`, the gate `sweep_staging` already used (F3). Because the sweep no longer clears a young leftover, `duplicate_server` clears **its own** reserved staging path before creating it: a kill mid-`copy_tree` would otherwise leave files that the retry copies over and renames into the new server.
- `manage::remove_mod` reconciles first and refuses when `!persistable`, naming the container — `Reconciled::persistable`'s first production reader (F9). A delete therefore walks and reads every jar once; that is the price of the D1 guard, and F7's single-pass `jarmeta::read` is what keeps it cheap.
- `delete_server`'s name mismatch has its own sentence (`서버 폴더 이름이 정확히 일치하지 않습니다: "{name}" — 앞뒤 공백을 확인하세요`), separate from the containment refusal (CLI-QA-07). `rename_copy` also rewrites a description that starts with the generated `"{source id} Running Minecraft"` prefix — the whole prefix, never the bare id, so a hand-written description is left alone (CLI-QA-04).

- `reconcile_with`'s D1 gate asks whether the container could be **read**, not whether it held anything: `persistable` is false only when the container directory and all three namespace directories are absent. `discover` turns a missing directory into an empty list and propagates every other read error, so directory existence is the whole test. A readable, empty container with stale index rows is persistable, which is what lets a user remove the last `파일 없음` row (the GUI lane's refinement, 2026-09-11; it replaces the accepted ceiling the core pass recorded).

**Accepted ceilings**
- `check_updates` still asks CurseForge once per managed CurseForge mod (D7); adoption adds at most one `/v1/fingerprints` call and two extra passes over the unmanaged jars it did not recognise on Modrinth.
- `From<CatalogError> for Error` now produces complete Korean sentences for every user-visible variant, and `Error::Http` falls back to `{message}` when `url` is empty. `CatalogError::Network` carries `"<url>: <what failed>"` in one string and has no separate url, so it maps to `Error::Invalid("네트워크 오류: …")` rather than an `Error::Http` with an empty url (CLI-QA-02). Every catalog failure is now `Error::Invalid`; `main` exits 1 for every `Error`, so no exit code moved.
- CLI: `mods remove` gained `--yes`, demanded only when other managed mods list the target's project as a dependency, which it prints first (CLI-QA-03); `mods install --no-deps` writes the omitted dependency titles and §8.4's warning to stderr (CLI-QA-05); search prints only the first line of a description (CLI-QA-06); `mods update` prints `직접 추가 N개를 관리 목록에 연결했습니다` when the check adopted anything (F7).

**GUI edits (compile only)**
- `gui/mods.rs:2223` reads `report.candidates`; the adopted names are dropped with a `ponytail:` note for the GUI lane.
- `gui/mods.rs:2986` (unit test) fills the three new `PlannedFile` fields.

**Tests**
- Workspace 314 → 329. New: hand-placed-jar collision (both namespaces), two planned files sharing a name, `version_number`/`published` on a plan, an incompatible hand-placed jar matched by mod id, the three CLI-QA-01 release-preference cases, a deleted CurseForge project not costing the other mods their check, both adoption paths, `remove_mod` refusing an unreadable container, `dependents_of`, the flattened-error sentences, and the CurseForge limiter being free while a request waits its turn.
- Changed: the `Stale` assertions split into `ServerChanged` + `Stale`; the mirror test now asserts `!raw.contains("JEI")`; the duplicate test backdates its stale `.partial` and keeps a fresh one; the jarmeta test asks for the fingerprint per file.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (329 passed, 1 ignored), `cargo deny check` (advisories/bans/licenses/sources ok) and `cargo tree -p nebula -e normal,no-proc-macro -i rav1e` (`warning: nothing to print.`) are all clean. No dependency was added.
