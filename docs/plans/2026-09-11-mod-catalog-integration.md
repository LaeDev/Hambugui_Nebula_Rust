# Mod catalog — integration map

Companion to `2026-09-11-mod-catalog.md` (plan v1.1). Synthesised from six subsystem maps taken on 2026-09-11. Paths are relative to `crates/`; line numbers are from that tree and shift after edits.

**Precedence:** where this map and DESIGN-v2 §8 (final) or the plan's *Core types and signatures* disagree, those win — several rows below predate them. Known re-synced points: `DialogKind` = the §8.1 list (`mod-deps`/`mod-menu` no longer exist); the server strip is a wrapping `PillStrip` (no Flickable); no Chip `outline`; busy = global `SheetData.busy` for mutating jobs + `ModsData.{search-busy, resolving, list-busy, checking}` for quiet ones; `InstalledModRowData` carries no description, `ModHitRowData` does; `GuiSettings` gains both `curseforge-api-key` and `last-mods-server`; `Ui` gains `search_seq`/`list_seq: Cell<u64>`; `Config` gains `curseforge_api_key`; `catalog` returns `CatalogError` (plan) rather than extending `Error::Http`; the stub signature is the one in plan rule 4; staging is `<server>/.nebula-tmp/`, not `root.temp_dir()`; `Namespace` gains serde derives (`lowercase`).

Line-reference corrections from the critic pass: `publish_hint` is at gui/mod.rs:1455 (not 2400); `apply_snapshot` at :428; `Debounce` at :184 (`tick` at :895); `with_ui` at :120; `optifine_meta` is already `pub(crate)`; `ManualDownload` fields are `{ name, file_name, url }`.

## 1. Reusable APIs

### 1.1 Jar metadata (→ `catalog/jarmeta.rs`)

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| `import::open` | nebula-core/src/import.rs | `pub(crate) fn open(path: &Path) -> Result<ZipArchive<File>>` | The one zip-open chokepoint. jarmeta opens each jar once through it and reads meta + icon from the same archive. |
| `entry_bytes` | nebula-core/src/scan/forgemod.rs:564 | `fn entry_bytes<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>>` | Read `fabric.mod.json`, `META-INF/*.toml`, `mcmod.info`, `META-INF/MANIFEST.MF`, icon entries. Private today (§4). |
| `parse_mods_toml` / `ModsToml` / `ModsTomlEntry` | scan/forgemod.rs:101-120 | `pub(crate) fn parse_mods_toml(bytes: &[u8]) -> Option<ModsToml>`; `ModsTomlEntry { mod_id, version: Option<String>, display_name: Option<String> }` | Forge 1.13+ and NeoForge (both toml names). Add `logo_file`. |
| `parse_mcmod_info` / `McModInfo` | scan/forgemod.rs | `pub(crate) fn parse_mcmod_info(bytes: &[u8]) -> Option<McModInfo>`; `{ modid, name, version: Option<String> }` | Forge 1.7-1.12; bare array or `{modList}`; first entry only. |
| `forge_meta_113` / `forge_meta_17` / `ForgeModMeta` | scan/forgemod.rs | `pub(crate) fn forge_meta_113(file_name: &str, parsed: Option<ModsToml>, claritas: Option<&ClaritasMeta>, manifest: Option<&[u8]>) -> ForgeModMeta`; `pub(crate) fn forge_meta_17(file_name, parsed: Option<McModInfo>, claritas) -> ForgeModMeta` | Pure resolvers; pass `claritas = None`. Handle `examplemod`, `${file.jarVersion}`, `@VERSION@`, crude fallback. Prefer over `read_forge_mod_meta` (logs an error per jar when claritas is None, opens the jar itself). |
| `implementation_version` | scan/forgemod.rs | `pub(crate) fn implementation_version(manifest: &[u8]) -> Option<String>` | Resolve `${file.jarVersion}`. |
| `wants_jar_version`, `is_optifine`, `optifine_meta` | scan/forgemod.rs:474, :560 | private | Inflate MANIFEST only when wanted; OptiFine → `changelog.txt`, else crude (do not fail). |
| `parse_fabric_mod_json` / `FabricModJson` | scan/fabricmod.rs:20-26 | `pub fn parse_fabric_mod_json(bytes: &[u8]) -> Option<FabricModJson>`; `{ id, version, name: Option<String>, entrypoints }` | Fabric id/version/name (use fields directly, not `fabric_module_id_and_name`). Add `icon`. |
| `crude_inference` / `capitalize` | scan/mod.rs | `pub fn crude_inference(file_name: &str) -> (String, String)` | Fallback; mod id = `name.to_lowercase()` so names match distribution.json. |
| `ModLoader::toml_entries` | scan/forgemod.rs | `pub enum ModLoader { Forge, NeoForge }; fn toml_entries(self) -> &'static [&'static str]` | NeoForge: `neoforge.mods.toml` then `mods.toml`. Needs `LoaderKind → ModLoader` match. |
| `forge_flavour_for` | scan/forgemod.rs | `pub fn forge_flavour_for(mc: &MinecraftVersion) -> Result<ForgeFlavour>` | Legacy17 (mcmod.info) vs Modern113 (mods.toml). NeoForge always Modern113. |
| `write_jar` | nebula-core/tests/scan.rs | `fn write_jar(path: &Path, entries: &[(&str, &[u8])])` | Copy (~9 lines) into jarmeta `#[cfg(test)]`; `zip` is a dev-dep. |

### 1.2 Directory model (→ `index.rs`, `install.rs`, server manage)

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| `discover` / `Candidate` / `FILE_NAME_BLACKLIST` | scan/mod.rs:42, :78 | `pub fn discover(dir: &Path) -> Result<Vec<Candidate { file_name, path, size }>>`; `[".gitkeep"]` | Non-recursive, name-sorted, skips symlinks/dirs, no extension filter (catalog filters `.jar` itself). |
| `Namespace` / `NAMESPACES` / `discover_toggleable` / `init_toggleable` | scan/toggle.rs:20-67 | `pub enum Namespace { Required, OptionalOn, OptionalOff }; dir_name() -> "required" \| "optionalon" \| "optionaloff"; pub fn discover_toggleable(container: &Path) -> Result<Vec<(Namespace, Candidate)>>` | Installed list; `move` = `fs::rename` between namespace dirs. |
| `server::namespace_dir` | server.rs:503 | `pub(crate) fn namespace_dir(container: &Path, ns: Namespace) -> PathBuf` | Install/move target dir. |
| `LoaderKind` | server.rs:39-70 | `pub enum LoaderKind { Forge, NeoForge, Fabric }; as_str(); mod_container(self, &ServerDir) -> PathBuf` | Provider loader facet + container path. |
| `list_servers` / `ServerSummary` | server.rs:189-292 | `pub fn list_servers(root: &Root) -> Result<Vec<ServerSummary { dir_name, id, mc, loader: Option<(LoaderKind, String)>, mods, files, libraries, has_icon, meta }>>` | Server selector; mc + loader for version filtering. No single-server lookup (§5). Not Serialize. |
| `create_server` / `CreatedServer` | server.rs:124-181 | `pub fn create_server(root, id, mc, opts) -> Result<CreatedServer>` | Template for `duplicate_server` guards and the Korean "exists" message (server.rs:141). |
| `effective_id` / `split_dir_name` | server.rs:120, :326 | `pub fn effective_id(id: &str, mc: &MinecraftVersion) -> String`; `fn split_dir_name(&str) -> Option<(&str, &str)>` (private, rsplit at last `-`) | duplicate: parse mc from source dir, build target dir name. |
| `read_server_meta` / `write_server_meta` | server.rs:183, :294 | `pub fn read_server_meta(dir: &ServerDir) -> Result<ServerMeta>`; `pub fn write_server_meta(dir, &ServerMeta) -> Result<()>` | Rewrite `meta.meta.name` after duplicate. |
| `Root` / `ServerDir` | layout.rs:9-158 | `Root { path, base_url }: servers_dir(), cache_dir(), temp_dir(), url()`; `ServerDir { root, dir_name, path }: server_meta_file(), forgemods_dir(), neoforgemods_dir(), fabricmods_dir()` | Single owner of paths; add `modindex_file()` / `icons_cache_dir()` (§3.1). `ServerDir::new` does not validate `dir_name`. |
| `paths::is_single_segment` / `assert_contained` / `clean_relative` | paths.rs:39, :45, :6 | `pub fn is_single_segment(id: &str) -> bool`; `pub fn assert_contained(root: &Path, candidate: &Path) -> Result<()>`; `pub fn clean_relative(path: &str) -> Result<String>` | id validation; containment before `remove_dir_all`; sanitise API `filename` before joining. |
| `hash::sha1_hex_of_file` / `digests_of_file` / `stream` | hash.rs:51-71 | `pub fn sha1_hex_of_file(&Path) -> Result<String>`; `Digests { size, md5, sha1, sha256 }` | Index hashes; extend `stream` with sha512 + murmur2. |

### 1.3 HTTP (→ `catalog/http.rs`, `modrinth.rs`, `curseforge.rs`)

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| `remote::client` | remote.rs:21-37 | `fn client() -> Result<&'static reqwest::blocking::Client>` (private, OnceLock, UA `nebula-rust/<ver>`, connect 30 s, reqwest default 30 s request timeout) | Make `pub(crate)`; catalog wraps `client()?.get(url).headers(..).send()` in its retry loop. Do not build a third client. |
| `remote::get_json_with_headers` | remote.rs:51-67 | `pub fn get_json_with_headers<T: DeserializeOwned>(url: &str, headers: &[(&str, &str)]) -> Result<T>` | Simple GETs with no status branching (`error_for_status` drops the body). |
| `remote::download_to_verified` | remote.rs | `pub fn download_to_verified(url: &str, dest: &Path, expected_sha1: Option<&str>) -> Result<()>` | `.part` + rename + sha1 check + delete on mismatch. sha512 sibling needed. |
| `publish::check` / `excerpt` / `http` | publish/mod.rs:423-440 | `pub(crate) fn check(response, url) -> Result<Response>`; `pub(crate) fn excerpt(body: &str) -> String`; `pub(crate) fn http(url, msg) -> Error` | Status + 200-char body errors; read `response.status()` (429/404) before `check`. |
| `publish::with_query` / `urlencode` | publish/mod.rs:446-455 | `pub(crate) fn with_query(endpoint: &str, pairs: &[(&str, &str)]) -> String` | Query strings (`RequestBuilder::query` not compiled). Modrinth `facets` JSON must go through this. |
| `publish::build_client` | publish/mod.rs | `pub(crate) fn build_client() -> Result<Client>` (`.timeout(None)`) | Existing second builder; do not add a third. |
| `curseforge::API_BASE` / `API_KEY` / `get` | curseforge.rs:18-20, :209-211 | `pub const API_BASE: &str`; `pub const API_KEY: &str`; `fn get<T>(url) -> Result<T>` (adds `X-API-KEY`) | Reuse constants and header; thread a base URL through. Existing `ModFileResponse`/`ModResponse` are private and minimal. |
| `import::DownloadFn` / `default_download` | import.rs:37-41 | `pub type DownloadFn = dyn Fn(&str, &Path, Option<&str>) -> Result<()> + Send + Sync` | Injection idiom (closure, not trait + mock server) for downloads and per-file progress. |
| `import::ManualDownload` + `cli::report_manual_downloads` | import.rs; nebula/src/cli.rs:376-408 | `ManualDownload { name, file_name, url }`; `fn report_manual_downloads(&[ManualDownload])` (stderr, bypasses `--quiet`) | CF `downloadUrl == null` reporting. |
| `Error` | error.rs | `Http { url, message }`, `Invalid`, `Config`, `UnsafePath`, `Io { context, source }`, `Json`, `Zip`, `Tool` | Stringly; no status field. |

### 1.4 GUI host (→ `nebula/src/gui/mods.rs`)

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| `spawn_job` | nebula/src/gui/mod.rs:659 | `fn spawn_job<T, W, A>(ui: &Rc<Ui>, label: &str, screen: Screen, work: W, apply: A) -> bool` | install / update / remove / move / delete / duplicate (global lock). Returns false if busy. |
| `spawn_quiet` | gui/mod.rs:693 | `fn spawn_quiet<T, W, A>(work: W, apply: A)` | search + installed-list read outside the lock; applier drops stale answers. |
| `Ui::set_busy` / `Screen` | gui/mod.rs:292, :142 | `fn set_busy(&self, busy: bool, label: &str, screen: Screen)`; `enum Screen { Sheet, NewServer, Lookup, Import, Publish, Settings }` | Add `Screen::Mods` + match arm. |
| `with_ui` / `invoke_from_event_loop` | gui/mod.rs:114 | `fn with_ui(body: impl FnOnce(&Rc<Ui>))` | Worker → UI hop for mid-job progress. |
| `Ui::toast` | gui/mod.rs:582 | `fn toast(&self, tone: Tone, message: &str)` (MAX 3, 4 s, by id) | One final success/failure toast. |
| `busy_pill` / `job_label` / `shown_secs` | gui/mod.rs:714 | `fn busy_pill(label, elapsed) -> String` | Progress text in the top-bar pill (`ui.job_label.replace(..); ui.shown_secs.set(u64::MAX)`). |
| `snapshot` / `apply_snapshot` / `spawn_refresh` | gui/mod.rs:1075, :446 | `fn snapshot(current: &GuiSettings, distro_name: &str) -> Snapshot`; `fn apply_snapshot(&self, snap)` | Fold into install `Ok` so server counts refresh under one busy window. |
| `server_rows` / `initials` / `hue_of` / `size_label` | gui/mod.rs:1159, :1302 | `fn initials(id: &str) -> String`; `fn hue_of(id: &str) -> i32`; `fn size_label(bytes: u64) -> String` | Row builders, fallback avatars, `12.3 MB`. |
| Icon decode idiom | gui/mod.rs:446-455 | `p.as_deref().and_then(\|p\| Image::load_from_path(p).ok()).unwrap_or_default()` into `Rc<VecModel<Image>>` | installed-icons / hit-icons parallel models. |
| `Debounce` + tick | gui/mod.rs:895 | `struct Debounce; fn arm/cancel/due(after)`; `SETTLE = 300 ms` | Optional search-as-you-settle. |
| `GuiSettings` / `config_dir` / `to_config` | gui/settings.rs:43-122 | `GuiSettings::to_config(&self) -> Result<Config>` | CurseForge key field. |
| `on_open_dialog` / `on_close_dialog` | gui/mod.rs:2005-2029 | refused while busy; close clears NewServerData errors | Extend close to clear mods dialog fields. |
| `gdrive_busy` pattern | gui/mod.rs:1954-1977 | `Cell<bool>` + `SettingsData.gdrive-busy` + `spawn_quiet` | Precedent for `search_busy`. |

### 1.5 Slint kit (→ `nebula/ui/screens/mods.slint`)

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| ListRow | ui/lib/list-row.slint | `title, subtitle, avatar, initials, hue, icon, enabled, selectable, selected; activated(); @children` (trailing) | installed / result rows; initials fallback when `avatar.width == 0`. |
| Chip | ui/lib/chip.slint | `text, tone, icon, tone-glyph` (presentational only) | 필수 / 선택 켜짐 / 선택 꺼짐 / 업데이트 / 설치됨 / 베타. |
| SegmentedControl | ui/lib/segmented.slint | `options, selected (in), enabled; changed(int)` | 설치됨/찾기, Modrinth/CurseForge, sort. |
| TextField | ui/lib/text-field.slint | `text (in-out), action-icon, action-label; edited, accepted(string), action()` — trailing icon only | search field (Enter = `accepted`). |
| Dialog | ui/lib/dialog.slint | `title, a11y-id, closable; closed(); @children` — no scroll | mod detail / deps / row menu / delete / duplicate. |
| Switch | ui/lib/switch.slint | `text, checked (in-out); toggled(bool)` | pre-checked dependency list. |
| ProgressBar | ui/lib/progress.slint | indeterminate, host-driven `Theme.progress-phase` | loading under the search field. |
| EmptyState / Callout | ui/lib/empty-state.slint, callout.slint | `icon, title, body; @children` / `tone, title, body; @children` | no-server / error + 다시 시도. |
| Card / CardHeader / KeyValue | ui/lib/card.slint, key-value.slint | `CardHeader { title, subtitle; @children }`; `KeyValue { key, value, dim }` | `N개 · 업데이트 M개`; detail dialog lines. |
| Flickable idiom | ui/lib/log-view.slint | `Flickable { viewport-height: n*h; for row[index] { y: index*h } }` | transpose (`viewport-width`, `x`) for the server chip strip. |
| ServersScreen rows / NewServerForm / import selection | ui/screens/servers.slint:74-96, 155-315; import.slint:40-53 | `for row[index] in SheetData.servers : ListRow { avatar: index < SheetData.server-icons.length ? ... : root.no-icon }`; `export component NewServerForm { callback submitted(); forward-focus }`; `selected: ImportData.selected-zip == index` | row composition, dialog body, index-owned selection. |
| Icons | nebula/assets/icons | package, search, chevron-right, external-link, trash-2, refresh-cw, check, plus, x | No new SVGs. |

### 1.6 CLI / tests

| Item | Path | Signature | Catalog use |
|---|---|---|---|
| `Command` / `dispatch` / `root_from_env` | nebula/src/cli.rs:44, :201, :225 | `enum Command { .. }`; `fn dispatch(Command) -> Result<()>`; `fn root_from_env() -> Result<(Config, Root)>` | `Mods` / `Server` families. |
| `PublishTargetKind` | cli.rs | `#[derive(ValueEnum)]` | Pattern for `ProviderKind`, `NamespaceArg`, `SortArg`. |
| `validate_command` | cli.rs:428-450 | print report, then `Err(Error::Invalid)` | "findings are fatal" / stdout-data pattern for `mods update`. |
| `Cli::command().debug_assert()` | cli.rs:527-569 | | Auto-covers new subcommands; add `try_parse_from` per conflict. |
| `stub_webdav` / `serve` / `Calls` | nebula-core/tests/publish.rs:374-438 | `fn stub_webdav() -> (SocketAddr, Calls)`; fixed `201`, no routing | Lift to `tests/common/mod.rs`, add route table + header capture. |
| `Fixture` / `zip_at` / `server` / `read` | nebula-core/tests/import.rs:18-57 | `fn fixture() -> Fixture { _dir, root }` | catalog test files. |
| `enabled()` / `java_executable()` | nebula-core/tests/resolver_net.rs:26-51 | `NEBULA_NET_TESTS=1` gate, prints skip + returns | `catalog_net.rs`. |
| `Recorder` | tests/publish.rs:59-74 | trait-object double for `Target` | model for a `Provider` double. |
| DownloadFn recorder closure | tests/import.rs:217-236 | `Arc<Mutex<Vec<String>>>` call log | install tests. |

## 2. Hard constraints

Scan / distribution
- `FILE_NAME_BLACKLIST = [".gitkeep"]` matches basenames only, inside `files/`, `libraries/`, and the three namespace dirs; it does not skip directories. `<server>/modindex.json` (top level) and `<root>/cache/icons/` are never scanned — leave the blacklist untouched (plan rule 5 mentions it; placement alone suffices). Never write under `<server>/files`, `libraries/`, the namespace dirs, or `<root>/modpacks` (import walks it recursively).
- Distribution output must not change: mods land only in `<container>/{required,optionalon,optionaloff}/`; `resolve_icon` must not read `cache/icons` (its URL becomes published content); `publish` uploads only URLs referenced by distribution.json.
- `import::open` is the only zip-open site in the crate; no `ZipArchive::new` in catalog. nebula-core has no image/png dependency — jar icon extraction returns raw bytes.
- Forge parsers are deliberately `pub(crate)` (scan/mod.rs:19-21); the catalog stays inside nebula-core.
- Name parity with scanners: fallback = `crude_inference`, mod id = lowercased crude name; `${file.jarVersion}` needs MANIFEST bytes; OptiFine detected by file name; NeoForge tries `neoforge.mods.toml` before `mods.toml`; `mcmod.info` first entry only; `logoFile` may start with `/`.
- Namespaces: enable/disable is a move between `required/optionalon/optionaloff`, never a `.disabled` rename.

HTTP
- Blocking only (no tokio). reqwest `blocking + json + native-tls`, default-features off: no `query`/`form`/`gzip`/`stream` — `RequestBuilder::query()` does not compile; use `publish::with_query`.
- `remote::client()` is a private OnceLock with reqwest's default 30 s whole-request timeout (remote.rs:29-31 comment is wrong); `publish::build_client` sets `.timeout(None)`. No retry, backoff, 429 handling, or POST helper anywhere.
- `Error::Http { url, message }` has no status code; branch on `Response::status()` before conversion.
- Double-implementation watchlist: third client builder; second `API_BASE`/`API_KEY`/`X-API-KEY` wrapper; third `fn http(url, msg)`; second status+body checker; second query builder; new hashing path.
- UA is `nebula-rust/<ver>` (no contact); Modrinth wants an identifying UA — override per request or extend the shared string once.
- D2: CurseForge API data not cached on disk; CF icons memory-only; user's own key (`NEBULA_CURSEFORGE_API_KEY` / settings); upstream key only for the import path. Tests never hit api.curseforge.com / api.modrinth.com.

Server / paths
- Dir name `<id>-<mc>`, split at the LAST `-`; `is_single_segment` rejects empty, `..`, `/ \ : NUL` but accepts `.` — reject `.` explicitly before `remove_dir_all`.
- `assert_contained` canonicalises both sides: candidate must exist; check `is_dir()` first for a clean `Invalid`. Run containment on the source before delete/copy; guard the destination with `is_single_segment + !exists` (create_server:139).
- `list_servers` fails the whole listing on one unreadable `servermeta.json`; duplicate must land `servermeta.json` first or copy to a temp name and rename last.
- `init_root` does not create `<root>/cache`; create `cache/icons` on write. `ServerSummary` is not Serialize.

GUI
- Slint models, globals, and `slint::Image` are touched on the event-loop thread only; workers carry `PathBuf`/`String`/ints; `Image` is not `Send`.
- One job at a time, globally: `spawn_job` refuses when `ui.busy`; `SheetData.busy` disables every control via `AppData.page-live`, refuses `open-dialog`, disables Esc. An install under `spawn_job` blocks 생성/가져오기/저장. Only `gdrive_busy` runs outside the lock.
- `set_busy(false)` only after `apply`; never from a worker. Never write `strict_ok`, `clear_result`, `clear_verdict` from mods (build-sheet claims).
- Toasts: max 3, 4 s, by id, no in-place update. Tick (100 ms) must never touch fs/network.
- Slint 1.17.1 decodes png + jpeg + svg only (no webp/gif). `Image::load_from_path` LRU keyed by (path, mtime whole seconds); decode is synchronous on the UI thread and stats the file each call — keep icons ≤128 px.
- No `image` inside a Slint struct (`--load-data` ceiling); icon lists are parallel `[image]` models. Every new global/property needs a `--load-data` state file; unknown names are silently ignored.
- Design tokens only (no hex in components); no opacity fade, no rotation spinners; `animate` loops render one frame — indeterminate motion via `Theme.progress-phase`.
- Dialogs inside the `keys` FocusScope; `closed => { SheetActions.close-dialog(); keys.focus(); }`; one `DialogKind` open at a time (replace, never stack).
- `SegmentedControl.selected` / `ListRow.selected` are strictly `in`; caller owns state. Selection survives refresh by value (dir_name / file name), never by index.
- Job-starting callbacks take no arguments and read `in-out` globals. `AppData.go()` is the only page switch; `on_navigate` is where a page's disk re-read is triggered.
- Every exported enum/struct/global goes through `app.slint:27-46`. `a11y-id` on every control (UIA handles); IconButton requires `a11y-label`.
- Copy: UI errors Korean cause + fix; core log lines English; Pretendard has no 700 weight.

CLI / build
- Exit codes: main.rs `Err → "error: {err}" + exit 1`; clap → 2. Handlers never `process::exit`, never prompt (stdin may not be a TTY) — `--yes` or `Err(Invalid)`.
- `Config::from_env()` mutates env via dotenvy on first call: main thread, before workers. One `log::Log` per process. stdout = data, stderr = logs. Injected seams `Send + Sync + 'static`; test doubles share via `Arc<Mutex<_>>`.
- Rust 1.97, edition 2024; CI: fmt, clippy `-D warnings`, `cargo test --workspace` offline on ubuntu + windows. cargo-deny license allowlist: new crates (`image`) are policy decisions (D5, R8, R10).
- Command names/aliases follow upstream; new commands documented in README §5.

## 3. Integration points

### 3.1 nebula-core

| File | Exact insertion |
|---|---|
| `src/lib.rs:3-21` | `pub mod catalog;` in the `pub mod` list. Re-export `delete_server, duplicate_server, find_server` beside `create_server, list_servers` (~line 30). |
| `src/catalog/mod.rs` (new) | `Provider` trait (`search`, `project`, `versions`, `versions_by_hashes`), `Loader { Forge, NeoForge, Fabric }` + `From<LoaderKind>`, `ProjectHit`, `Version`, `VersionFile`, `Dependency`, `Catalog { providers }`, `Endpoints { modrinth: String, curseforge: String }` with `Default` = production consts (no `format!("{API_BASE}/..")` at call sites). All response fields `Option`/defaulted (R5). |
| `src/catalog/http.rs` (new) | `fn get_json<T>(url, headers) -> Result<T>` / `fn post_json<B, T>(..)` wrapping `remote::client()`: 3 attempts, 0.5/1/2 s backoff on connect/timeout/5xx; 429 → `Retry-After` / `X-Ratelimit-Reset` sleep; then `publish::check` for the error. Per-request `User-Agent` for Modrinth. CF ≥250 ms throttle (R7). |
| `src/catalog/modrinth.rs`, `curseforge.rs` (new) | v2 / Core v1 mappings per facts §A/§B; CF key from `NEBULA_CURSEFORGE_API_KEY` / settings, else hidden (D2). |
| `src/catalog/jarmeta.rs` (new) | One loop: `import::open` → probe `fabric.mod.json` → `META-INF/neoforge.mods.toml` → `META-INF/mods.toml` → `mcmod.info` (or use `LoaderKind`) → `forge_meta_113/17(.., claritas = None, manifest if wants_jar_version)` / `FabricModJson` → icon path (`logoFile` per-mod then top-level; fabric string or largest-size map; strip leading `/`) → `entry_bytes`. Per-file `Result` (R9). |
| `src/catalog/index.rs`, `install.rs`, `update.rs`, `icons.rs` (new) | Per plan. `index.rs` reads `dir.modindex_file()`, writes `.tmp` + rename, reconciles with `discover_toggleable(loader.mod_container(&dir))`. `install.rs` downloads to `root.temp_dir()`, verifies, renames into `namespace_dir(container, ns)`. `icons.rs` writes `root.icons_cache_dir()` (create_dir_all). |
| `src/scan/forgemod.rs:564` | `fn entry_bytes` → `pub(crate) fn entry_bytes`. |
| `src/scan/forgemod.rs:101-120` | Add `#[serde(rename = "logoFile")] pub logo_file: Option<String>` to `ModsToml`, `ModsTomlEntry`, `McModInfo` (no `deny_unknown_fields`; serde is the only constructor). |
| `src/scan/forgemod.rs:474, :560` | `wants_jar_version`, `is_optifine` (and `optifine_meta` if private) → `pub(crate)`. |
| `src/scan/fabricmod.rs:20-26` | Add `pub icon: Option<FabricIcon>`; `#[derive(Deserialize)] #[serde(untagged)] pub enum FabricIcon { Path(String), Sized(BTreeMap<String, String>) }`; add `icon: None` to the literal at :97-102 (grep `tests/scan.rs` for other literals). |
| `src/remote.rs:21-37` | `fn client()` → `pub(crate) fn client()`; add `.timeout(None)` so the comment is true. `fn http` → `pub(crate)` (or catalog uses `publish::http`). |
| `src/remote.rs:51-67` | Add `pub fn post_json<B: Serialize, T: DeserializeOwned>(url: &str, headers: &[(&str, &str)], body: &B) -> Result<T>` next to `get_json_with_headers` (Modrinth `version_files`, `version_files/update` are POST). |
| `src/curseforge.rs:18-20, :209-211` | Catalog imports `curseforge::{API_BASE, API_KEY}`. Change `fn get` to `pub(crate) fn get_at<T>(base: &str, path: &str, key: &str) -> Result<T>` (or route through `catalog::http`) so import and catalog share one CF GET. Keep `ModFileResponse`/`ModResponse` shapes; `paths::clean_relative(file_name)` before joining. |
| `src/publish/mod.rs:423-455` | No change required (`check`, `excerpt`, `http`, `urlencode`, `with_query` are `pub(crate)`). Optional: move them to `remote.rs` so remote/publish/curseforge/catalog share one set (touches gdrive.rs, webdav.rs imports). |
| `src/error.rs:15-16` | Keep `Error::Http` unchanged; 429 handled inside the retry loop. Only if callers must distinguish 404: add `status: Option<u16>` — touches remote.rs:40, publish/mod.rs:423, nebula/src/gui/mod.rs:2400 (`publish_hint`). |
| `src/hash.rs:51-71` | Add `pub fn sha512_hex_of_file(path: &Path) -> Result<String>` via the existing `stream` helper (separate fn, not a fourth `Digests` field — avoids a hasher pass on every resolver read); add CF murmur2 fingerprint with the KAT (R11). Add `remote::download_to_verified` sibling taking `(Algo, &str)` or a `sha512` variant. |
| `src/layout.rs` (~46 Root impl, ~127 ServerDir impl) | `pub fn icons_cache_dir(&self) -> PathBuf { self.cache_dir().join("icons") }` next to `forge_cache_dir`; `pub fn modindex_file(&self) -> PathBuf { self.path.join("modindex.json") }` next to `server_meta_file`. |
| `src/server.rs` (after `create_server`, ~181) | `pub fn find_server(root: &Root, dir_name: &str) -> Result<ServerSummary>` (single-server: `is_single_segment && != "."`, `split_dir_name`, `read_server_meta`, loader). `pub fn delete_server(root: &Root, dir_name: &str) -> Result<()>`: guards → `ServerDir::new` → `is_dir()` else `Invalid` → `paths::assert_contained(&root.servers_dir(), &dir.path)` → `remove_dir_all` mapped with `io(format!("removing {}", ..))`. `pub fn duplicate_server(root: &Root, dir_name: &str, new_id: &str) -> Result<ServerDir>`: same guards + `is_single_segment(new_id)` → mc via `split_dir_name` → dest `ServerDir::new(root, effective_id(new_id, &mc))`, `Invalid` if exists (message style server.rs:141) → std recursive copy (skip symlinks, `UnsafePath` under files/ is consistent with scan_files) → copy to temp name + rename or `servermeta.json` first → `read_server_meta`/`write_server_meta` rewriting `meta.meta.name`. Plan's `catalog/manage.rs` → put here (owns `split_dir_name`/`server_dirs`). |
| `src/server.rs:40-64` | Add a `LoaderKind → Option<scan::ModLoader>` match (Forge, NeoForge, Fabric → None) for `toml_entries()`. |
| `src/import.rs` | No change; `open`, `read_json`, `create_parent` reused as `pub(crate)`. |
| `tests/common/mod.rs` (new) | Lift `stub_webdav`/`serve` from tests/publish.rs:374-438; generalise to `fn stub(routes: impl Fn(&str, &str, &[u8]) -> (u16, &'static str, Vec<u8>) + Send + Sync + 'static) -> (SocketAddr, Calls)`; record headers in `Calls`; route on path prefix (query arrives percent-encoded), 404 default. Share `Fixture`, `BASE`, `read`, `server`, `zip_at`. Include with `mod common;`. |
| `tests/catalog.rs`, `tests/server_manage.rs`, `tests/catalog_net.rs` (new) | Fixtures under `fixtures/catalog/` recorded in the facts doc; net file gated exactly like resolver_net.rs. |

### 3.2 nebula — CLI (`src/cli.rs`)

| Where | Exact insertion |
|---|---|
| `enum Command` (line 44) | `Mods { server: String, #[command(subcommand)] command: ModsCommand }`, `#[command(subcommand)] Server(ServerCommand)`. |
| new enums | `#[derive(Subcommand)] enum ModsCommand { Search { query: String, #[arg(long, value_enum)] provider: Option<ProviderKind>, #[arg(long, value_enum)] sort: Option<SortArg>, #[arg(long)] limit: Option<usize> }, Install { project: String, #[arg(long)] version: Option<String>, #[arg(long, value_enum)] namespace: Option<NamespaceArg>, #[arg(long)] no_deps: bool, #[arg(long, value_enum)] provider: Option<ProviderKind> }, List, Update { #[arg(long)] apply: bool }, Remove { file: String }, Move { file: String, #[arg(value_enum)] namespace: NamespaceArg } }`; `enum ServerCommand { Delete { dir: String, #[arg(long)] yes: bool }, Duplicate { dir: String, new_id: String } }`; `#[derive(ValueEnum)] ProviderKind, SortArg, NamespaceArg` like `PublishTargetKind`. |
| `fn dispatch` (line 201) | `Command::Mods { server, command } => mods(&server, command)`, `Command::Server(cmd) => server_command(cmd)`; each `fn` opens with `let (cfg, root) = root_from_env()?;` then `server::find_server(&root, dir)`. `Delete` without `--yes` → `Err(Error::Invalid(..))`. |
| `report_manual_downloads` (line 376) | `mods install` pushes CF blocked files as `ManualDownload` and calls it; `println!("installed N, skipped M, manual K")`. `mods update` without `--apply` prints candidates to stdout. |
| tests (line 527) | `try_parse_from` for `--no-deps`, `delete` without `--yes` parse-ok, namespace values. |
| `README.md §5`, `docs/verification/2026-09-10-cli-qa.md` | Add the two families; `--help` command count (13) changes. |

### 3.3 nebula — GUI host (`src/gui/`)

| Where | Exact insertion |
|---|---|
| `mod.rs:13` | `mod mods;` next to `mod logsink;`; new `src/gui/mods.rs` (child module sees private `Ui`, `spawn_job`, `spawn_quiet`, `with_ui`, `initials`, `hue_of`, `size_label`). |
| `mod.rs:142 enum Screen` | Add `Mods`. |
| `mod.rs:292-318 set_busy` | Arm `app.global::<ModsData>().set_busy(busy && screen == Screen::Mods)`. Add `search_busy: Cell<bool>` + `fn set_search_busy` mirroring `gdrive_busy` (mod.rs:1954-1977). |
| `mod.rs:207-287 Ui / Ui::new` | Fields: `installed: Rc<VecModel<InstalledModRowData>>`, `installed_icons: Rc<VecModel<Image>>`, `hits: Rc<VecModel<ModHitRowData>>`, `hit_icons: Rc<VecModel<Image>>`, `installed_paths: RefCell<Vec<PathBuf>>`, `hit_ids: RefCell<Vec<(ProviderKind, String)>>`, `mods_server: RefCell<Option<String>>` (dir_name, by value). |
| `mod.rs:1489 bind_models` | `let mods = app.global::<ModsData>(); mods.set_installed(ModelRc::from(ui.installed.clone()));` etc. for the four models. |
| `mod.rs:1549 wire` | `mods::wire(&ui, &app);`. Closures in the `on_import_modpack` (1739-1799) shape: clear `ModsData.error-text` → read `in-out` globals → resolve row index against the parallel Vec captured now → `let current = ui.settings.borrow().clone(); let cfg = current.to_config()?;` → `spawn_job(&ui, "모드 설치", Screen::Mods, work, apply)`; `work` builds `Root::new(cfg.root, cfg.base_url)`, calls `catalog`, returns `(rows, Vec<Option<PathBuf>>, snapshot(&current, &name))`; `apply` → `ui.apply_snapshot(snap)`, `set_vec`, decode icons into the parallel model, set `result-line`, one toast. Errors → `ModsData.error-text` + `Tone::Danger` toast; never `Ui::fail`/`clear_result`. Search → `spawn_quiet` with staleness check on (query, provider, sort, selected server). |
| `mod.rs:1991-1998 on_navigate` | `AppPage::Mods` → rebuild the installed list for `mods_server` (spawn_quiet + stale check), not `spawn_refresh`. |
| `mod.rs:2017-2029 on_close_dialog` | Also clear mods dialog fields (delete-confirm text, duplicate id error). |
| progress (inside `work`) | Pass core a per-file callback (`DownloadFn` shape or `&(dyn Fn(usize, usize, &str) + Send + Sync)`): `log::info!("다운로드 중 {i}/{n} · {file}")` + `slint::invoke_from_event_loop(move \|\| with_ui(\|ui\| { ui.job_label.replace(..); ui.shown_secs.set(u64::MAX); if let Some(app) = ui.app.upgrade() { app.global::<ModsData>().set_progress_line(..) } }))`. |
| `settings.rs:43-62` + `mod.rs:1506-1547` | `#[serde(default)] curseforge_api_key: String` (kebab-case) on `GuiSettings`; one `in-out` on `SettingsData`; read/write in `apply_settings_to_ui` / `form_from_ui`. |
| `mod.rs:2400 publish_hint` | Only if `Error::Http` gains a status field. |

### 3.4 nebula — Slint (`ui/`)

| Where | Exact insertion |
|---|---|
| `lib/data.slint:7` | `export enum AppPage { build, servers, mods, import, publish, settings }`. |
| `lib/data.slint:10` | `export enum DialogKind { none, new-server, mod-detail, install-confirm, version-picker, update-preview, mod-delete, server-menu, server-delete, server-duplicate }` (§8.1). |
| `lib/data.slint` structs | `export struct InstalledModRowData { file-name: string, title: string, version: string, source-label: string, namespace: string, initials: string, hue: int, update-available: bool }`; `export struct ModHitRowData { id: string, title: string, author: string, description: string, downloads-label: string, initials: string, hue: int, installed: bool }`. No `image` fields. |
| `lib/data.slint` globals | `export global ModsData { in property <[InstalledModRowData]> installed; in property <[image]> installed-icons; in property <[ModHitRowData]> hits; in property <[image]> hit-icons; in property <bool> busy; in property <bool> search-busy; in property <string> error-text; in property <string> result-line; in property <string> progress-line; in property <bool> has-more; in property <int> update-count; in property <bool> has-cf-key; in-out property <string> selected-server; in-out property <int> tab; in-out property <int> provider; in-out property <int> sort; in-out property <string> query; }`. Callbacks (on `SheetActions` or a new `ModsActions`): `select-mod-server(string)`, `search-mods()`, `load-more-mods()`, `install-mod(int)`, `remove-mod(int)`, `move-mod(int, string)`, `check-mod-updates()`, `update-all-mods()`, `open-mod-project(int)`, `delete-server()`, `duplicate-server()`. |
| `app.slint:14-19` | `import { ModsScreen, ModDetailBody, ModDepsBody, ServerDeleteBody, ServerDuplicateBody } from "screens/mods.slint";`. |
| `app.slint:27-46` | Add `ModsData`, `InstalledModRowData`, `ModHitRowData` (and `ModsActions`) to the `export { ... } from "lib/data.slint"` block. |
| `app.slint:94-116` | Ctrl/Cmd: `"1"` build, `"2"` servers, `"3"` mods, `"4"` import, `"5"` publish, `"6"` settings. Update comments at app.slint:125 and nav-rail.slint:107-109. |
| `app.slint:163-169` | `AppData.page == AppPage.mods ? "모드" :` in the title chain. |
| `app.slint:198-202` | `if AppData.page == AppPage.mods : ModsScreen { }`. |
| `app.slint:216-266` | Per new kind, inside the `keys` FocusScope: `if AppData.dialog == DialogKind.install-confirm : Dialog { title: ModsData.confirm-title; a11y-id: "dialog-install-confirm"; closable: !SheetData.busy; init => body.focus(); closed => { SheetActions.close-dialog(); keys.focus(); } body := ModDepsBody { } HorizontalLayout { alignment: end; Button { text: "취소"; variant: ghost; } Button { text: "설치"; variant: primary; size: lg; loading: ModsData.busy; } } }`. |
| `lib/nav-rail.slint:142-150` | After 서버: `NavItem { icon: @image-url("../../assets/icons/package.svg"); text: "모드"; a11y-id: "nav-mods"; selected: root.page == AppPage.mods; activated => { root.navigate(AppPage.mods); } }`. |
| `screens/mods.slint` (new) | `export component ModsScreen inherits VerticalLayout { private property <bool> live: AppData.page-live; private property <image> no-icon; spacing: Theme.card-gap; ... }`: server pill strip (new selectable pill, `accessible-checkable/checked`, ring, in a horizontal `Flickable` with Left/Right on a FocusScope writing `viewport-x`), `SegmentedControl { options: ["설치됨", "찾기"]; selected: ModsData.tab; changed(i) => { ModsData.tab = i; } }`, one `Card` per tab, rows `ListRow { avatar: index < ModsData.installed-icons.length ? ModsData.installed-icons[index] : root.no-icon; ... Chip {} IconButton { icon: chevron-right } }`, `ProgressBar` under the search field while `search-busy`, EmptyState (no server / no root / no results), Callout error + 다시 시도. Exported dialog bodies with `submitted()` + `forward-focus`. |
| `screens/servers.slint:87-95` | Trailing `IconButton` → row menu: record row id into a global, then `SheetActions.open-dialog(DialogKind.server-menu)`; or add a second `IconButton { icon: package.svg }` that selects + `AppData.go(AppPage.mods)`. Keep `server-row-<id>` / `server-open-<id>` a11y-ids. |
| `lib/chip.slint`, `lib/text-field.slint` (optional) | `in property <bool> outline` on Chip (border 1px `Theme.border`, transparent fill) for 선택 꺼짐; `in property <image> leading-icon` on TextField (~10 lines, mirrors suffix). |
| `docs/plans/gui-v2/states/`, `README.md` contract table | `mods.json`, `mods-search.json`, `mods-empty.json`, `dark-mods.json`; document `ModsData` properties. |

## 4. Visibility changes

| Symbol | File:line | Now | Needed | Why |
|---|---|---|---|---|
| `entry_bytes` | nebula-core/src/scan/forgemod.rs:564 | private | `pub(crate)` | jarmeta reads meta + icon entries from one opened archive. |
| `wants_jar_version` | scan/forgemod.rs:474 | private | `pub(crate)` | inflate MANIFEST only when the toml version asks for it. |
| `is_optifine`, `optifine_meta` | scan/forgemod.rs:560 | private | `pub(crate)` | OptiFine detection without `read_forge_mod_meta`'s per-jar error log / second open. |
| `remote::client` | nebula-core/src/remote.rs:21 | private | `pub(crate)` | one client; catalog retry loop sends its own requests. |
| `remote::http` | remote.rs | private | `pub(crate)` (or use `publish::http`) | avoid a third `fn http(url, msg)`. |
| `curseforge::get` | nebula-core/src/curseforge.rs:209 | private | `pub(crate)` with base/key params | shared CF GET for import + catalog. |
| `server::split_dir_name` | nebula-core/src/server.rs:326 | private | `pub(crate)` | only if delete/duplicate/find live outside server.rs. |

Already sufficient as `pub(crate)` (catalog is in-crate): `import::{open, read_json, create_parent}`, `parse_mods_toml`, `parse_mcmod_info`, `forge_meta_113`, `forge_meta_17`, `ForgeModMeta`, `implementation_version`, `publish::{build_client, check, excerpt, http, urlencode, with_query}`, `server::namespace_dir`. Nothing needs widening to `pub` — do not re-export the forge parsers from `scan::` (mod.rs:19-21 comment).

## 5. Gaps to build

nebula-core
- Entire `catalog/` module: nothing exists (no Provider trait, no Modrinth client/structs/base URL, no CF search/file/hash structs, no index, no install/update, no icon cache, no jar-icon extraction).
- `http.rs`: retry/backoff, 429 (`Retry-After`, `X-Ratelimit-Reset`), CF 250 ms throttle, Modrinth UA, POST helper, injectable `Endpoints`.
- `jarmeta.rs`: single open-once loop; loader sniff order; icon path → bytes (`logoFile` top/per-mod, fabric string-or-map largest key, leading `/` strip); `quilt.mod.json` parser (none exists); per-file `Result`; OptiFine degrade to crude.
- `hash.rs`: sha512, CF murmur2 fingerprint + KAT; `download_to_verified` variant for sha512.
- `remote.rs`: `.timeout(None)` on the shared client (pre-existing bug); `download_to` `.part` name collision / no cleanup on failure (pre-existing; flag, not catalog scope).
- `server.rs`: `find_server`, `delete_server`, `duplicate_server`, std recursive copy (no walkdir/fs_extra).
- `layout.rs`: `modindex_file`, `icons_cache_dir`.
- Loader mapping: `LoaderKind` ↔ `scan::ModLoader` ↔ `catalog::Loader` ↔ Slint `Loader`.
- Icon decode: `image` crate (png/jpeg/webp, magic-byte sniff, re-encode to PNG ≤128 px) — new dependency, cargo-deny + rav1e ban (D5, R8).
- Tests: `tests/common/mod.rs` routed stub with header capture; `fixtures/catalog/`; `catalog.rs`, `server_manage.rs`, `catalog_net.rs`; jarmeta corrupt/truncated jar and `$`-key toml cases (R9); Sodium `%2B` filename case (R6).

nebula
- CLI: `mods` and `server` families, `ProviderKind`/`SortArg`/`NamespaceArg`, first `--yes` flag; README §5 + QA doc.
- GUI host: `gui/mods.rs`; `Screen::Mods`; four models + parallel Vecs; `search_busy`; staleness check for search answers; progress-line + pill route; settings key field; `on_navigate` / `on_close_dialog` arms.
- Slint: `screens/mods.slint`; selectable pill + horizontal Flickable strip (no such component; a11y ceiling for off-screen pills); dialogs (detail with own Flickable or 더 보기 since Dialog has no scroll; deps checklist; server menu; delete-by-typing-id; duplicate); optional Chip outline / TextField leading icon; sixth nav item; Ctrl+6; `--load-data` state files.

## 6. Risks

Plan-vs-map conflicts to settle before coding
- Plan `http.rs` builds its own `reqwest::blocking::Client` — maps: reuse `remote::client()` (a third builder is a double implementation).
- Plan `catalog/manage.rs` — maps: `server.rs` owns directory lifecycle and the private `split_dir_name`/`server_dirs`.
- Plan rule 5 adds `modindex.json` to `FILE_NAME_BLACKLIST` — unnecessary (top level is never scanned) and the blacklist cannot exclude `cache/icons` (dirs are not matched).
- Plan "progress in a toast per file" — MAX_TOASTS=3 / 4 s evicts a stream; use pill + `ModsData.progress-line` + one final toast. Byte-level progress is impossible without changing `remote::download_to` (`copy_to` streams the whole body).
- Plan "one catalog job at a time per server" — the GUI lock is global: an install blocks 생성/가져오기/저장 and vice versa; search under `spawn_job` locks every page during a network round-trip → search must use `spawn_quiet` + `search_busy`.
- Plan `icons.rs` cache name: D5 says `<sha1(url)>.png`, GUI map assumes `<provider>-<project_id>.png`; D2 forbids CF icons on disk — pick one and keep CF memory-only.
- Plan `jarmeta` reads `quilt.mod.json` — no parser exists; scanners do not know Quilt.

Code-level
- `Error::Http` shape change touches remote.rs:40, publish/mod.rs:423, gui/mod.rs:2400.
- Routing `curseforge::get` through the retrying/keyed path changes `g server-curseforge` behaviour under rate limits; D2 key fallback changes import behaviour.
- Adding `.timeout(None)` to `remote::client()` changes every resolver download (intended direction).
- `assert_contained` on a missing dir yields `Io`, not `Invalid`; `is_single_segment(".")` passes and resolves to `<root>/servers` — reject explicitly.
- An interrupted `duplicate_server` without `servermeta.json` breaks `list_servers` for every server.
- `FabricModJson` literal sites (fabricmod.rs:97-102, tests) must gain `icon: None` or fail to compile.
- Modrinth `facets` and CF `searchFilter` must be percent-encoded by hand; stub routes must parse the query, not string-match the target.
- CurseForge: HEAD returns 404 on the API; `downloadUrl == null` files → Modrinth sha1 lookup then `ManualDownload`; CDN URL reconstruction forbidden (R3/R4).
- `snapshot()` must not gain a jar scan (it runs on every page change); read the installed list only for the selected server on the 모드 page.
- Row index resolved against the parallel Vec captured with the model, or a refresh mid-click installs into the wrong server.
- Slint: pills scrolled off the strip vanish from the UIA tree; Dialog has no scroll; `ListRow { selectable: true }` with trailing children is untested (row TouchArea sits above IconButtons); `--load-data` silently ignores unknown names; png/jpeg only (webp must be re-encoded); rewriting an icon within the same mtime second serves the LRU'd bitmap; N synchronous decodes per apply on the UI thread.
- No CLI end-to-end harness; exit codes verified only in the manual QA doc. Adding one needs a dev-dependency (deny policy) or `env!("CARGO_BIN_EXE_nebula")`.
- `deny.toml`: `image` crate licences, rav1e ban (R8), GPL-3.0-only removal + Slint royalty-free licence (R10) are gate changes in the same pass.
- a11y-id renames on server rows (`server-row-<id>`, `server-open-<id>`) and Ctrl+3 renumbering break existing QA scripts/docs.
- Four loader enums coexist (GUI Slint `Loader`, `server::LoaderKind`, `scan::ModLoader`, `catalog::Loader`); facet/version filtering must map explicitly (NeoForge on 1.20.1 not widened, D4).
- Korean cause + fix on every user-facing error; core log lines stay English.
