//! The 모드 page's host half (DESIGN-v2 §8) and the 서버 row menu (§8.5).
//!
//! Three rules hold this module together, on top of the two in [`super`]:
//!
//! * **Every string the screen shows is formatted here.** `mods.slint` decides
//!   layout and tone and never wording that depends on data it cannot see
//!   (§8.8 is the format list, and the pure functions at the bottom of this
//!   file are the only place those formats live).
//! * **Mutating jobs take the global lock, read-only calls never do** (§8.6).
//!   The quiet ones are single-flight per kind: the applier drops a result
//!   whose captured sequence number, or whose server, has moved on.
//! * **`slint::Image` is built on the event-loop thread only.** Workers carry
//!   an [`IconRef`] — a cached PNG path, or raw RGBA — and the decode happens
//!   in the applier, like `SheetData.server-icons` already does.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nebula_core::catalog::icons::{self, IconRef};
use nebula_core::catalog::index::InstalledMod;
use nebula_core::catalog::{
    self, Catalog, CatalogError, Endpoints, InstallChoice, InstallPlan, InstallReport, PlannedFile,
    ProjectHit, ProviderId, ReleaseType, SearchQuery, SortKey, UpdateCandidate, Version,
};
use nebula_core::scan::Namespace;
use nebula_core::{Root, ServerDir};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, SharedString, VecModel};

use super::settings::GuiSettings;
use super::{
    AppData, AppPage, DepRowData, DialogKind, InstalledModRowData, ManualDownloadData,
    ModHitRowData, ModsActions, ModsData, Screen, SheetData, Tone, Ui, UpdateRowData,
    VersionRowData, logsink, size_label, with_ui,
};

/// One page of search results. Also what decides 더 보기: a full page means
/// there may be another one.
const SEARCH_LIMIT: u32 = 20;

/// How many icon fetches may be in flight at once. Four keeps a forty-row list
/// from opening forty sockets while still filling the visible rows quickly.
const MAX_ICON_JOBS: usize = 4;

/// The cap §8.4 puts on an automatic 429 wait.
const MAX_RATE_WAIT: Duration = Duration::from_secs(30);

/// The three namespace labels, in the order the `SegmentedControl` lists them.
const NAMESPACE_LABELS: [&str; 3] = ["필수", "선택 켜짐", "선택 꺼짐"];

/// Modrinth 관련도 / 다운로드 / 최신 (§8.4).
const MODRINTH_SORTS: [&str; 3] = ["관련도", "다운로드", "최신"];
/// CurseForge 인기 / 다운로드 / 최신 (§8.4).
const CURSEFORGE_SORTS: [&str; 3] = ["인기", "다운로드", "최신"];

// ── State ───────────────────────────────────────────────────────────────────

/// Which list a version pick is about, and what it is being compared against.
enum VersionTarget {
    /// A managed file on the server: the pick replaces it (`update::replace_file`).
    Installed {
        file_name: String,
        title: String,
        /// The installed version's id and publication date, which is what
        /// decides 교체 from 이전 버전으로 교체.
        current_id: String,
        current_published: String,
        current_number: String,
    },
    /// A search hit that is not installed yet: the pick re-plans the install.
    Hit {
        provider: ProviderId,
        project_id: String,
    },
}

/// What 다시 시도 in the failure Callout re-runs (§8.4). Only the jobs that
/// report through that Callout are here; 복제 and 서버 삭제 report inside their
/// own dialogs.
#[derive(Clone)]
enum Retry {
    Install,
    Updates,
    Remove(String),
    Move(String, Namespace),
    Replace(String, Box<Version>),
    /// The two quiet jobs whose failure is worth offering again. Every other
    /// quiet failure clears the slot, so 다시 시도 can never run the last
    /// *mutating* job under a Callout that is about something else.
    Reload,
    Check,
}

/// One queued icon fetch. `key` is the row's identity — a file name or a
/// project id — never its index, so a list that reordered while the fetch was
/// out cannot give a row somebody else's avatar.
struct IconJob {
    installed: bool,
    key: String,
    /// `list_seq` or `search_seq` at spawn.
    seq: u64,
    source: IconSource,
}

enum IconSource {
    /// A provider icon: cached as PNG for Modrinth, memory-only for CurseForge.
    Remote {
        provider: ProviderId,
        project_id: String,
        url: String,
    },
    /// The jar's own declared icon, already read by `jarmeta`.
    Local { jar: PathBuf, bytes: Vec<u8> },
}

pub(super) struct ModsState {
    // The models bound to `ModsData`.
    servers: Rc<VecModel<SharedString>>,
    installed: Rc<VecModel<InstalledModRowData>>,
    installed_icons: Rc<VecModel<Image>>,
    hits: Rc<VecModel<ModHitRowData>>,
    hit_icons: Rc<VecModel<Image>>,
    sort_labels: Rc<VecModel<SharedString>>,
    deps: Rc<VecModel<DepRowData>>,
    versions: Rc<VecModel<VersionRowData>>,
    updates: Rc<VecModel<UpdateRowData>>,
    /// Its own model, not `Ui::manual`: that one is the 가져오기 screen's, and
    /// an install must not rewrite a modpack import's list.
    manual: Rc<VecModel<ManualDownloadData>>,

    /// What every job runs against. Rebuilt when the CurseForge key changes,
    /// which also drops the session icon cache D2 keeps in it.
    catalog: RefCell<Arc<Catalog>>,
    catalog_key: RefCell<Option<String>>,

    /// The selected server's directory name; empty when there is none.
    server: RefCell<String>,
    /// Which server `installed_rows` describes, so entering the page only
    /// reloads when the subject changed.
    loaded: RefCell<String>,
    /// `<server>/<loader>mods`: where the three namespace directories live.
    container: RefCell<PathBuf>,

    installed_rows: RefCell<Vec<InstalledMod>>,
    /// `(provider, project id)` of everything the index claims, which is what
    /// marks a search hit 설치됨 (D6).
    installed_keys: RefCell<BTreeSet<(ProviderId, String)>>,
    hit_rows: RefCell<Vec<ProjectHit>>,
    candidates: RefCell<Vec<UpdateCandidate>>,
    version_rows: RefCell<Vec<Version>>,
    version_target: RefCell<Option<VersionTarget>>,
    /// The row a downgrade was asked about; a second click on it applies (§8.3).
    downgrade: Cell<i32>,

    plan: RefCell<Option<InstallPlan>>,
    /// What produced the plan, so a `Stale` refusal can resolve it again.
    plan_source: RefCell<Option<(ProviderId, String, Option<String>)>>,
    choice: RefCell<InstallChoice>,
    /// Parallel to the `deps` model: the project id each row toggles.
    dep_ids: RefCell<Vec<String>>,
    /// One automatic 429 retry per install attempt (§8.4).
    rate_retried: Cell<bool>,
    retry: RefCell<Option<Retry>>,

    /// The file `mod-detail`, `mod-delete` and 다른 버전… are about.
    detail: RefCell<String>,
    /// The 서버 row menu's subject, which is not the pill strip's selection.
    menu_server: RefCell<String>,

    search_seq: Cell<u64>,
    list_seq: Cell<u64>,
    /// §8.6's single-flight token for 업데이트 확인: the applier that still owns
    /// it clears `checking`, a superseded one drops its result instead.
    check_seq: Cell<u64>,
    offset: Cell<u32>,

    /// The running job's last `(done, total, file)`, so the tick can refresh
    /// the elapsed in the progress line without the worker saying anything.
    progress: RefCell<Option<(usize, usize, String)>>,
    shown_progress: Cell<u64>,

    /// `관리 12 · 직접 추가 3`, without the check clause the tick appends.
    counts_prefix: RefCell<String>,
    last_check: Cell<Option<Instant>>,
    shown_check: Cell<u64>,

    icon_queue: RefCell<VecDeque<IconJob>>,
    icon_flight: Cell<usize>,
}

impl ModsState {
    pub(super) fn new(settings: &GuiSettings) -> Self {
        let key = cf_key(settings);
        Self {
            servers: Rc::new(VecModel::default()),
            installed: Rc::new(VecModel::default()),
            installed_icons: Rc::new(VecModel::default()),
            hits: Rc::new(VecModel::default()),
            hit_icons: Rc::new(VecModel::default()),
            sort_labels: Rc::new(VecModel::default()),
            deps: Rc::new(VecModel::default()),
            versions: Rc::new(VecModel::default()),
            updates: Rc::new(VecModel::default()),
            manual: Rc::new(VecModel::default()),
            catalog: RefCell::new(Arc::new(Catalog::new(Endpoints::default(), key.clone()))),
            catalog_key: RefCell::new(key),
            server: RefCell::new(settings.last_mods_server.clone()),
            loaded: RefCell::new(String::new()),
            container: RefCell::new(PathBuf::new()),
            installed_rows: RefCell::new(Vec::new()),
            installed_keys: RefCell::new(BTreeSet::new()),
            hit_rows: RefCell::new(Vec::new()),
            candidates: RefCell::new(Vec::new()),
            version_rows: RefCell::new(Vec::new()),
            version_target: RefCell::new(None),
            downgrade: Cell::new(-1),
            plan: RefCell::new(None),
            plan_source: RefCell::new(None),
            choice: RefCell::new(InstallChoice::default()),
            dep_ids: RefCell::new(Vec::new()),
            rate_retried: Cell::new(false),
            retry: RefCell::new(None),
            detail: RefCell::new(String::new()),
            menu_server: RefCell::new(String::new()),
            search_seq: Cell::new(0),
            list_seq: Cell::new(0),
            check_seq: Cell::new(0),
            offset: Cell::new(0),
            progress: RefCell::new(None),
            shown_progress: Cell::new(u64::MAX),
            counts_prefix: RefCell::new(String::new()),
            last_check: Cell::new(None),
            shown_check: Cell::new(u64::MAX),
            icon_queue: RefCell::new(VecDeque::new()),
            icon_flight: Cell::new(0),
        }
    }
}

/// The key the catalog is built with: the environment wins over the settings
/// field, which is what the CurseForge helper in §4 promises. One rule, one
/// place — `GuiSettings::to_config` deliberately reports only the field.
fn cf_key(settings: &GuiSettings) -> Option<String> {
    std::env::var("NEBULA_CURSEFORGE_API_KEY")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            Some(settings.curseforge_api_key.trim().to_owned()).filter(|value| !value.is_empty())
        })
}

// ── Startup wiring ──────────────────────────────────────────────────────────

pub(super) fn bind_models(ui: &Rc<Ui>, app: &super::App) {
    let data = app.global::<ModsData>();
    let mods = &ui.mods;
    data.set_servers(ModelRc::from(mods.servers.clone()));
    data.set_installed(ModelRc::from(mods.installed.clone()));
    data.set_installed_icons(ModelRc::from(mods.installed_icons.clone()));
    data.set_hits(ModelRc::from(mods.hits.clone()));
    data.set_hit_icons(ModelRc::from(mods.hit_icons.clone()));
    data.set_sort_labels(ModelRc::from(mods.sort_labels.clone()));
    data.set_deps(ModelRc::from(mods.deps.clone()));
    data.set_versions(ModelRc::from(mods.versions.clone()));
    data.set_updates(ModelRc::from(mods.updates.clone()));
    data.set_manual(ModelRc::from(mods.manual.clone()));
    mods.sort_labels.set_vec(labels(&MODRINTH_SORTS));
    data.set_has_cf_key(mods.catalog_key.borrow().is_some());
}

fn labels(values: &[&str]) -> Vec<SharedString> {
    values
        .iter()
        .map(|value| SharedString::from(*value))
        .collect()
}

/// Rebuilds the catalog after a settings save, but only when the key actually
/// changed: `Catalog` owns the session-only CurseForge icon cache (D2), and
/// throwing it away on every save would re-fetch every avatar.
pub(super) fn settings_saved(ui: &Rc<Ui>) {
    let key = cf_key(&ui.settings.borrow());
    if *ui.mods.catalog_key.borrow() == key {
        return;
    }
    ui.mods
        .catalog
        .replace(Arc::new(Catalog::new(Endpoints::default(), key.clone())));
    ui.mods.catalog_key.replace(key.clone());
    if let Some(app) = ui.app.upgrade() {
        app.global::<ModsData>().set_has_cf_key(key.is_some());
        // A provider that just lost its key cannot stay selected.
        if key.is_none() {
            set_provider(ui, 0);
        }
    }
}

/// The server list, straight off the snapshot every page refresh produces.
///
/// This is also where §8.2's fallback lives: a stored `last-mods-server` that
/// no longer names a server hands the selection to the first one and says so.
pub(super) fn apply_servers(ui: &Rc<Ui>, names: Vec<String>) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    ui.mods.servers.set_vec(
        names
            .iter()
            .map(|name| SharedString::from(name.as_str()))
            .collect::<Vec<_>>(),
    );
    data.set_has_servers(!names.is_empty());

    let wanted = ui.mods.server.borrow().clone();
    let chosen = match names.iter().position(|name| *name == wanted) {
        Some(index) => index,
        None => {
            if !wanted.is_empty() && !names.is_empty() {
                ui.toast(
                    Tone::Warning,
                    &format!(
                        "서버 {wanted}을(를) 찾을 수 없어 {}을(를) 선택했습니다",
                        names[0]
                    ),
                );
            }
            0
        }
    };
    let selected = names.get(chosen).cloned().unwrap_or_default();
    data.set_server_index(match names.is_empty() {
        true => -1,
        false => i32::try_from(chosen).unwrap_or(0),
    });
    data.set_scope_line(scope_line(ui, &selected).into());

    let changed = *ui.mods.server.borrow() != selected;
    if changed {
        ui.mods.server.replace(selected.clone());
        persist_server(ui, &selected);
        clear_for_server_change(ui);
    }
    // Only while the page is on screen: reading a two-hundred-mod list opens
    // and hashes every jar, and a snapshot fires on every visit to 서버 or
    // 가져오기. Entering 모드 is what loads it otherwise ([`navigated`]).
    let stale = *ui.mods.loaded.borrow() != selected;
    let showing = app.global::<AppData>().get_page() == AppPage::Mods;
    if showing && !selected.is_empty() && stale {
        reload_installed(ui);
    }
}

/// `Neo-1.21.1 · Minecraft 1.21.1 · NeoForge 21.1.72` (§8.2). Both halves come
/// off the snapshot's own server row, so nothing here reads the disk.
fn scope_line(ui: &Rc<Ui>, dir_name: &str) -> String {
    if dir_name.is_empty() {
        return String::new();
    }
    let loader = ui
        .servers
        .iter()
        .find(|row| row.id == dir_name)
        .map(|row| row.loader.to_string())
        .unwrap_or_default();
    let mc = dir_name.rsplit_once('-').map(|(_, mc)| mc).unwrap_or("");
    let loader = match loader.as_str() {
        "" | "없음" => "로더 없음".to_owned(),
        other => other.to_owned(),
    };
    format!("{dir_name} · Minecraft {mc} · {loader}")
}

/// Writes the selection through. Not `commit_settings`: that validates the
/// whole form, takes the global lock and toasts — three things a pill click is
/// not. `appearance-changed` sets the precedent.
fn persist_server(ui: &Rc<Ui>, name: &str) {
    let mut next = ui.settings.borrow().clone();
    if next.last_mods_server == name {
        return;
    }
    next.last_mods_server = name.to_owned();
    if let Err(error) = next.save() {
        log::error!("설정을 저장하지 못했습니다: {error}");
    }
    ui.settings.replace(next);
}

/// Changing the server clears the results but keeps the query (§8.2), and
/// drops everything the old server's lists said.
fn clear_for_server_change(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    ui.mods.search_seq.set(ui.mods.search_seq.get() + 1);
    ui.mods.list_seq.set(ui.mods.list_seq.get() + 1);
    // The check in flight is about the server that is leaving; superseding it
    // here is also what releases `checking`, so the new server can be checked
    // without waiting for a result nobody wants.
    ui.mods.check_seq.set(ui.mods.check_seq.get() + 1);
    data.set_checking(false);
    ui.mods.hits.set_vec(Vec::new());
    ui.mods.hit_icons.set_vec(Vec::new());
    ui.mods.hit_rows.replace(Vec::new());
    ui.mods.candidates.replace(Vec::new());
    ui.mods.offset.set(0);
    ui.mods.last_check.set(None);
    ui.mods.shown_check.set(u64::MAX);
    ui.mods.manual.set_vec(Vec::new());
    data.set_search_state("idle".into());
    data.set_has_more(false);
    data.set_updates_available(0);
    clear_failure(ui);
}

/// Entering the page: refresh the list when its subject changed since the last
/// load. The pill strip itself is refreshed by the snapshot job [`super`] runs
/// for every list page.
pub(super) fn navigated(ui: &Rc<Ui>) {
    let server = ui.mods.server.borrow().clone();
    let stale = *ui.mods.loaded.borrow() != server;
    if !server.is_empty() && stale {
        reload_installed(ui);
    }
}

/// Both time-dependent strings on the page: the progress line's elapsed and
/// the header's `마지막 업데이트 확인 N분 전`. Written only when the second (or
/// the minute) actually changes, because Slint marks a property dirty on every
/// `set`.
pub(super) fn tick(ui: &Rc<Ui>, app: &super::App) {
    let data = app.global::<ModsData>();
    let progress = ui.mods.progress.borrow().clone();
    if let (Some(started), Some((done, total, file))) = (ui.job_started.get(), progress) {
        let elapsed = started.elapsed();
        if ui.mods.shown_progress.replace(elapsed.as_secs()) != elapsed.as_secs() {
            data.set_progress_text(progress_line(done, total, &file, elapsed).into());
        }
    }
    if let Some(checked) = ui.mods.last_check.get() {
        let minutes = checked.elapsed().as_secs() / 60;
        if ui.mods.shown_check.replace(minutes) != minutes {
            data.set_check_caption(
                format!(
                    "{} · 마지막 업데이트 확인 {}",
                    ui.mods.counts_prefix.borrow(),
                    since_label(checked.elapsed())
                )
                .into(),
            );
        }
    }
}

// ── Callbacks ───────────────────────────────────────────────────────────────

pub(super) fn wire(ui: &Rc<Ui>, app: &super::App) {
    let actions = app.global::<ModsActions>();

    actions.on_select_server({
        let ui = ui.clone();
        move |index| {
            let Some(name) = ui.mods.servers.row_data(index.max(0) as usize) else {
                return;
            };
            let name = name.to_string();
            if *ui.mods.server.borrow() == name {
                return;
            }
            ui.mods.server.replace(name.clone());
            persist_server(&ui, &name);
            clear_for_server_change(&ui);
            if let Some(app) = ui.app.upgrade() {
                app.global::<ModsData>()
                    .set_scope_line(scope_line(&ui, &name).into());
            }
            reload_installed(&ui);
        }
    });

    actions.on_set_view({
        let ui = ui.clone();
        move |view| {
            // The view switch itself is Slint state; the host only has to make
            // sure the list the 설치됨 card shows is the current server's.
            if view == 0 {
                navigated(&ui);
            }
        }
    });

    actions.on_search({
        let ui = ui.clone();
        move || start_search(&ui, false)
    });
    actions.on_retry_search({
        let ui = ui.clone();
        move || start_search(&ui, false)
    });
    actions.on_search_more({
        let ui = ui.clone();
        move || start_search(&ui, true)
    });

    actions.on_set_provider({
        let ui = ui.clone();
        move |index| set_provider(&ui, index)
    });

    actions.on_set_sort({
        let ui = ui.clone();
        move |_| {
            // §8.4: a sort change re-searches immediately when there is a
            // query, and the offset resets.
            if !current_query(&ui).is_empty() {
                start_search(&ui, false);
            }
        }
    });

    actions.on_install_hit({
        let ui = ui.clone();
        move |index| resolve_hit(&ui, index)
    });

    actions.on_toggle_dep({
        let ui = ui.clone();
        move |index, on| toggle_dep(&ui, index, on)
    });

    actions.on_confirm_install({
        let ui = ui.clone();
        move || {
            ui.mods.rate_retried.set(false);
            start_install(&ui, Duration::ZERO);
        }
    });

    actions.on_retry_install({
        let ui = ui.clone();
        move || retry_last(&ui)
    });

    actions.on_open_manual_folder({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let folder = app.global::<ModsData>().get_manual_folder().to_string();
            super::open_folder(std::path::Path::new(&folder));
        }
    });

    actions.on_open_failure_folder({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let folder = app.global::<ModsData>().get_failure_folder().to_string();
            super::open_folder(std::path::Path::new(&folder));
        }
    });

    actions.on_open_detail({
        let ui = ui.clone();
        move |index| open_detail(&ui, index)
    });

    actions.on_apply_namespace({
        let ui = ui.clone();
        move || apply_namespace(&ui)
    });

    actions.on_open_project_page({
        let ui = ui.clone();
        move || open_project_page(&ui)
    });

    actions.on_open_version_picker({
        let ui = ui.clone();
        move || open_version_picker(&ui)
    });

    actions.on_pick_version({
        let ui = ui.clone();
        move |index| pick_version(&ui, index)
    });

    actions.on_request_delete({
        let ui = ui.clone();
        move || request_delete(&ui)
    });

    actions.on_confirm_delete({
        let ui = ui.clone();
        move || confirm_delete(&ui)
    });

    actions.on_check_updates({
        let ui = ui.clone();
        move || check_updates(&ui)
    });

    actions.on_open_update_preview({
        let ui = ui.clone();
        move || open_dialog(&ui, DialogKind::UpdatePreview)
    });

    actions.on_apply_updates({
        let ui = ui.clone();
        move || start_updates(&ui)
    });

    actions.on_show_all_installed(|| {
        // The eight-row cap is the screen's own, and the host already feeds it
        // the whole list; there is nothing to do but say it happened.
        log::info!("설치된 모드를 모두 표시합니다.");
    });

    // ── 서버 row menu (§8.5) ────────────────────────────────────────────────

    actions.on_open_server_menu({
        let ui = ui.clone();
        move |id| {
            ui.mods.menu_server.replace(id.to_string());
            if let Some(app) = ui.app.upgrade() {
                app.global::<ModsData>().set_server_menu_id(id.clone());
            }
            open_dialog(&ui, DialogKind::ServerMenu);
        }
    });

    actions.on_server_menu_mods({
        let ui = ui.clone();
        move || {
            let id = ui.mods.menu_server.borrow().clone();
            close_dialog(&ui);
            let Some(app) = ui.app.upgrade() else { return };
            if *ui.mods.server.borrow() != id {
                ui.mods.server.replace(id.clone());
                persist_server(&ui, &id);
                clear_for_server_change(&ui);
                let index = ui
                    .mods
                    .servers
                    .iter()
                    .position(|name| name == id)
                    .and_then(|index| i32::try_from(index).ok())
                    .unwrap_or(-1);
                let data = app.global::<ModsData>();
                data.set_server_index(index);
                data.set_scope_line(scope_line(&ui, &id).into());
            }
            app.global::<AppData>().invoke_go(AppPage::Mods);
        }
    });

    actions.on_server_menu_open_folder({
        let ui = ui.clone();
        move || {
            let id = ui.mods.menu_server.borrow().clone();
            close_dialog(&ui);
            if let Some(path) = ui
                .servers
                .iter()
                .find(|row| row.id == id)
                .map(|row| row.path.to_string())
            {
                super::open_folder(std::path::Path::new(&path));
            }
        }
    });

    actions.on_server_menu_duplicate({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<ModsData>();
            data.set_duplicate_id(SharedString::new());
            data.set_duplicate_error(SharedString::new());
            open_dialog(&ui, DialogKind::ServerDuplicate);
        }
    });

    actions.on_server_menu_delete({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let id = ui.mods.menu_server.borrow().clone();
            let path = ui
                .servers
                .iter()
                .find(|row| row.id == id)
                .map(|row| row.path.to_string())
                .unwrap_or_default();
            let data = app.global::<ModsData>();
            data.set_delete_server_id(id.as_str().into());
            data.set_delete_server_path(path.into());
            data.set_delete_server_input(SharedString::new());
            open_dialog(&ui, DialogKind::ServerDelete);
        }
    });

    actions.on_confirm_duplicate({
        let ui = ui.clone();
        move || confirm_duplicate(&ui)
    });

    actions.on_confirm_server_delete({
        let ui = ui.clone();
        move || confirm_server_delete(&ui)
    });
}

fn open_dialog(ui: &Rc<Ui>, kind: DialogKind) {
    // §8.6: no new dialog opens while a mutating job runs.
    if ui.busy.get() {
        return;
    }
    if let Some(app) = ui.app.upgrade() {
        app.global::<AppData>().set_dialog(kind);
    }
}

fn close_dialog(ui: &Rc<Ui>) {
    if let Some(app) = ui.app.upgrade() {
        app.global::<AppData>().set_dialog(DialogKind::None);
    }
}

fn current_query(ui: &Rc<Ui>) -> String {
    ui.app
        .upgrade()
        .map(|app| app.global::<ModsData>().get_query().trim().to_owned())
        .unwrap_or_default()
}

/// The server the page is about, or `None` with the reason already reported.
fn selected(ui: &Rc<Ui>) -> Option<(Arc<Catalog>, Root, ServerDir)> {
    let name = ui.mods.server.borrow().clone();
    if name.is_empty() {
        return None;
    }
    let cfg = match ui.settings.borrow().to_config() {
        Ok(cfg) => cfg,
        Err(error) => {
            show_quiet_failure(
                ui,
                "설정이 완성되지 않았습니다",
                &error.to_string(),
                "search",
                None,
            );
            return None;
        }
    };
    let root = Root::new(cfg.root, cfg.base_url);
    let server = ServerDir::new(root.clone(), &name);
    Some((ui.mods.catalog.borrow().clone(), root, server))
}

// ── Quiet jobs (§8.6) ───────────────────────────────────────────────────────

/// Re-reads the installed list. Quiet, single-flight, and the applier drops a
/// result whose sequence number or server has moved on.
fn reload_installed(ui: &Rc<Ui>) {
    let Some((_, root, server)) = selected(ui) else {
        return;
    };
    let Some(app) = ui.app.upgrade() else { return };
    let seq = ui.mods.list_seq.get() + 1;
    ui.mods.list_seq.set(seq);
    app.global::<ModsData>().set_list_busy(true);

    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || {
            let container = catalog::server_context(&server)
                .map(|ctx| ctx.container)
                .unwrap_or_default();
            (
                seq,
                name,
                container,
                root,
                catalog::manage::list_installed(&server),
            )
        },
        |ui, (seq, name, container, root, outcome)| {
            if seq != ui.mods.list_seq.get() || *ui.mods.server.borrow() != name {
                return;
            }
            let Some(app) = ui.app.upgrade() else { return };
            app.global::<ModsData>().set_list_busy(false);
            match outcome {
                Ok(rows) => {
                    ui.mods.loaded.replace(name);
                    ui.mods.container.replace(container);
                    apply_installed(ui, rows, &root, seq);
                }
                Err(error) => {
                    // The list is the page's whole subject: a failure here is
                    // not a quiet one.
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "installed", Some(Retry::Reload));
                }
            }
        },
    );
}

/// Fills the 설치됨 model and queues one icon fetch per row.
fn apply_installed(ui: &Rc<Ui>, rows: Vec<InstalledMod>, root: &Root, seq: u64) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    let updated: BTreeSet<String> = ui
        .mods
        .candidates
        .borrow()
        .iter()
        .map(|candidate| candidate.file_name.clone())
        .collect();

    // 업데이트 가능 first, then name (§8.3).
    let mut rows = rows;
    rows.sort_by(|a, b| {
        updated
            .contains(&b.file_name)
            .cmp(&updated.contains(&a.file_name))
            .then_with(|| a.file_name.cmp(&b.file_name))
    });

    // An icon already decoded keeps its row across a re-render: rebuilding the
    // list after an update check must not re-hash every jar on disk.
    let mut have: HashMap<String, Image> = HashMap::new();
    for (index, row) in ui.mods.installed_rows.borrow().iter().enumerate() {
        if let Some(image) = ui.mods.installed_icons.row_data(index)
            && image.size().width > 0
        {
            have.insert(row.file_name.clone(), image);
        }
    }

    ui.mods.installed.set_vec(
        rows.iter()
            .map(|row| installed_row(row, updated.contains(&row.file_name)))
            .collect::<Vec<_>>(),
    );
    ui.mods.installed_icons.set_vec(
        rows.iter()
            .map(|row| have.get(&row.file_name).cloned().unwrap_or_default())
            .collect::<Vec<_>>(),
    );
    ui.mods.installed_keys.replace(
        rows.iter()
            .filter_map(|row| {
                row.entry
                    .as_ref()
                    .map(|entry| (entry.provider, entry.project_id.clone()))
            })
            .collect(),
    );

    let managed = rows.iter().filter(|row| !row.is_unmanaged()).count();
    let manual = rows.len() - managed;
    ui.mods
        .counts_prefix
        .replace(format!("관리 {managed} · 직접 추가 {manual}"));
    data.set_installed_count_line(format!("설치된 모드 {}", rows.len()).into());
    let caption = match ui.mods.last_check.get() {
        Some(at) => format!(
            "{} · 마지막 업데이트 확인 {}",
            ui.mods.counts_prefix.borrow(),
            since_label(at.elapsed())
        ),
        None => format!("{} · 확인 전", ui.mods.counts_prefix.borrow()),
    };
    data.set_check_caption(caption.into());
    let available = rows
        .iter()
        .filter(|row| updated.contains(&row.file_name))
        .count();
    data.set_updates_available(i32::try_from(available).unwrap_or(0));

    let container = ui.mods.container.borrow().clone();
    for row in &rows {
        if have.contains_key(&row.file_name) {
            continue;
        }
        let source = match row.entry.as_ref().and_then(|entry| {
            entry
                .icon_url
                .as_ref()
                .map(|url| (entry.provider, entry.project_id.clone(), url.clone()))
        }) {
            Some((provider, project_id, url)) => IconSource::Remote {
                provider,
                project_id,
                url,
            },
            None => match row.local.as_ref().and_then(|local| local.icon.clone()) {
                Some(bytes) => IconSource::Local {
                    jar: container
                        .join(row.namespace.dir_name())
                        .join(&row.file_name),
                    bytes,
                },
                None => continue,
            },
        };
        queue_icon(
            ui,
            IconJob {
                installed: true,
                key: row.file_name.clone(),
                seq,
                source,
            },
            root,
        );
    }
    log::info!(
        "{}: 설치된 모드 {}개 (관리 {managed} · 직접 추가 {manual})",
        ui.mods.loaded.borrow(),
        rows.len()
    );
    ui.mods.installed_rows.replace(rows);
    // Cached hits keep their 설치됨 markers honest across a server change.
    refresh_hit_markers(ui);
}

fn installed_row(row: &InstalledMod, update_available: bool) -> InstalledModRowData {
    let title = row.title().to_owned();
    let provider = match &row.entry {
        Some(entry) => match entry.provider {
            ProviderId::Modrinth => "Modrinth",
            ProviderId::CurseForge => "CurseForge",
        },
        None => "직접 추가",
    };
    let version = installed_version(row);
    InstalledModRowData {
        file_name: row.file_name.as_str().into(),
        title: title.as_str().into(),
        subtitle: format!("{version} · {} · {provider}", row.file_name).into(),
        initials: super::initials(&title).into(),
        hue: super::hue_of(&title),
        namespace: row.namespace.dir_name().into(),
        namespace_label: namespace_label(row.namespace).into(),
        update_available,
        missing: row.missing,
        unshippable: row.unshippable.is_some(),
    }
}

/// The jar's own version is the honest answer for both a managed and a
/// hand-placed file; the index stores a provider version **id**, which is a
/// slug like `xK9Za2`, not a number.
fn installed_version(row: &InstalledMod) -> String {
    row.local
        .as_ref()
        .map(|local| local.version.clone())
        .filter(|version| !version.trim().is_empty())
        .or_else(|| row.entry.as_ref().map(|entry| entry.version_id.clone()))
        .unwrap_or_else(|| "버전 미상".to_owned())
}

fn namespace_label(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Required => NAMESPACE_LABELS[0],
        Namespace::OptionalOn => NAMESPACE_LABELS[1],
        Namespace::OptionalOff => NAMESPACE_LABELS[2],
    }
}

fn namespace_of(index: i32) -> Namespace {
    match index {
        1 => Namespace::OptionalOn,
        2 => Namespace::OptionalOff,
        _ => Namespace::Required,
    }
}

fn namespace_index(namespace: Namespace) -> i32 {
    match namespace {
        Namespace::Required => 0,
        Namespace::OptionalOn => 1,
        Namespace::OptionalOff => 2,
    }
}

fn set_provider(ui: &Rc<Ui>, index: i32) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    let curseforge = index == 1;
    if curseforge && ui.mods.catalog_key.borrow().is_none() {
        data.set_provider_index(0);
        return;
    }
    data.set_provider_index(index);
    ui.mods.sort_labels.set_vec(labels(match curseforge {
        true => &CURSEFORGE_SORTS,
        false => &MODRINTH_SORTS,
    }));
    if !current_query(ui).is_empty() {
        start_search(ui, false);
    }
}

fn provider_of(index: i32) -> ProviderId {
    match index {
        1 => ProviderId::CurseForge,
        _ => ProviderId::Modrinth,
    }
}

fn sort_of(index: i32) -> SortKey {
    match index {
        1 => SortKey::Downloads,
        2 => SortKey::Newest,
        _ => SortKey::Relevance,
    }
}

/// One page of search. `more` keeps the rows already on screen and asks for the
/// next offset; anything else starts at zero.
fn start_search(ui: &Rc<Ui>, more: bool) {
    let query = current_query(ui);
    let Some((catalog, root, server)) = selected(ui) else {
        return;
    };
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    if query.is_empty() {
        data.set_search_state("idle".into());
        return;
    }
    let provider = provider_of(data.get_provider_index());
    let sort = sort_of(data.get_sort_index());
    let offset = match more {
        true => ui.mods.offset.get() + SEARCH_LIMIT,
        false => 0,
    };
    let seq = ui.mods.search_seq.get() + 1;
    ui.mods.search_seq.set(seq);
    ui.mods.offset.set(offset);
    data.set_search_busy(true);
    data.set_loading_more(more);
    if !more {
        data.set_search_state("loading".into());
    }

    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || {
            let page = catalog::server_context(&server).and_then(|ctx| {
                catalog::install::require_provider(&catalog, provider)?.search(&SearchQuery {
                    query,
                    sort,
                    limit: SEARCH_LIMIT,
                    offset,
                    mc: ctx.mc,
                    loader: ctx.loader,
                })
            });
            (seq, name, provider, more, root, page)
        },
        |ui, (seq, name, provider, more, root, outcome)| {
            if seq != ui.mods.search_seq.get() || *ui.mods.server.borrow() != name {
                return;
            }
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<ModsData>();
            data.set_search_busy(false);
            data.set_loading_more(false);
            match outcome {
                Ok(page) => {
                    let full = page.hits.len() as u32 >= SEARCH_LIMIT;
                    let mut rows = match more {
                        true => ui.mods.hit_rows.borrow().clone(),
                        false => Vec::new(),
                    };
                    let first = rows.len();
                    rows.extend(page.hits);
                    ui.mods.hit_rows.replace(rows);
                    if !more {
                        // A fresh page is different projects at the same
                        // indices; keeping the old images would hand row 0
                        // somebody else's avatar until its own arrived.
                        ui.mods.hit_icons.set_vec(Vec::new());
                    }
                    rebuild_hits(ui, data.get_sort_index() == 2);
                    data.set_has_more(full);
                    data.set_search_state(
                        match ui.mods.hit_rows.borrow().is_empty() {
                            true => "empty",
                            false => "results",
                        }
                        .into(),
                    );
                    queue_hit_icons(ui, &root, first, seq);
                }
                Err(error) => {
                    // The page that did not arrive is not behind us: leaving
                    // the offset advanced would make 더 보기 skip it.
                    if more {
                        ui.mods
                            .offset
                            .set(ui.mods.offset.get().saturating_sub(SEARCH_LIMIT));
                    }
                    let (title, body) = search_error(&error, provider);
                    data.set_search_error_title(title.into());
                    data.set_search_error_body(body.into());
                    data.set_search_state("error".into());
                }
            }
        },
    );
}

fn rebuild_hits(ui: &Rc<Ui>, newest_first: bool) {
    let keys = ui.mods.installed_keys.borrow();
    let now = now_secs();
    let rows = ui
        .mods
        .hit_rows
        .borrow()
        .iter()
        .map(|hit| {
            hit_row(
                hit,
                keys.contains(&(hit.provider, hit.project_id.clone())),
                newest_first,
                now,
                "",
            )
        })
        .collect::<Vec<_>>();
    let len = rows.len();
    ui.mods.hits.set_vec(rows);
    // Keep the parallel icon model the same length, preserving what has
    // already arrived for the rows that did not move.
    let mut icons: Vec<Image> = ui.mods.hit_icons.iter().collect();
    icons.resize(len, Image::default());
    ui.mods.hit_icons.set_vec(icons);
}

/// Re-marks cached results after a server change (§8.2), keeping every other
/// field — including a `버전 없음` caption — as it was.
fn refresh_hit_markers(ui: &Rc<Ui>) {
    let keys = ui.mods.installed_keys.borrow();
    for (index, hit) in ui.mods.hit_rows.borrow().iter().enumerate() {
        let Some(mut row) = ui.mods.hits.row_data(index) else {
            continue;
        };
        let installed = keys.contains(&(hit.provider, hit.project_id.clone()));
        if row.installed != installed {
            row.installed = installed;
            ui.mods.hits.set_row_data(index, row);
        }
    }
}

fn hit_row(
    hit: &ProjectHit,
    installed: bool,
    newest_first: bool,
    now: i64,
    no_version_suffix: &str,
) -> ModHitRowData {
    ModHitRowData {
        provider: hit.provider.as_str().into(),
        project_id: hit.project_id.as_str().into(),
        title: hit.title.as_str().into(),
        caption: format!("{}{no_version_suffix}", hit_caption(hit, newest_first, now)).into(),
        description: hit.description.replace('\n', " ").trim().into(),
        initials: super::initials(&hit.title).into(),
        hue: super::hue_of(&hit.project_id),
        installed,
        resolving: false,
        no_version: !no_version_suffix.is_empty(),
        install_a11y_id: install_a11y_id(hit.provider, &hit.project_id).into(),
    }
}

// ── Icons ───────────────────────────────────────────────────────────────────

fn queue_hit_icons(ui: &Rc<Ui>, root: &Root, from: usize, seq: u64) {
    let jobs: Vec<IconJob> = ui.mods.hit_rows.borrow()[from..]
        .iter()
        .filter_map(|hit| {
            hit.icon_url.as_ref().map(|url| IconJob {
                installed: false,
                key: hit.project_id.clone(),
                seq,
                source: IconSource::Remote {
                    provider: hit.provider,
                    project_id: hit.project_id.clone(),
                    url: url.clone(),
                },
            })
        })
        .collect();
    for job in jobs {
        queue_icon(ui, job, root);
    }
}

/// Queues one fetch and starts as many as the bound allows. The queue and the
/// counter live on the event loop, so no lock is needed to keep them honest.
fn queue_icon(ui: &Rc<Ui>, job: IconJob, root: &Root) {
    ui.mods.icon_queue.borrow_mut().push_back(job);
    pump_icons(ui, root);
}

fn pump_icons(ui: &Rc<Ui>, root: &Root) {
    while ui.mods.icon_flight.get() < MAX_ICON_JOBS {
        let Some(job) = ui.mods.icon_queue.borrow_mut().pop_front() else {
            return;
        };
        // A job for a list that has already been replaced is dropped before it
        // costs a request.
        let current = match job.installed {
            true => ui.mods.list_seq.get(),
            false => ui.mods.search_seq.get(),
        };
        if job.seq != current {
            continue;
        }
        ui.mods.icon_flight.set(ui.mods.icon_flight.get() + 1);
        let catalog = ui.mods.catalog.borrow().clone();
        let root = root.clone();
        let IconJob {
            installed,
            key,
            seq,
            source,
        } = job;
        super::spawn_quiet(
            move || {
                let found = match source {
                    IconSource::Remote {
                        provider,
                        project_id,
                        url,
                    } => icons::fetch(&catalog, &root, provider, &project_id, &url),
                    IconSource::Local { jar, bytes } => {
                        icons::local_icon(&root, &jar, &bytes).map(IconRef::Path)
                    }
                };
                (installed, key, seq, root, found)
            },
            |ui, (installed, key, seq, root, found)| {
                ui.mods
                    .icon_flight
                    .set(ui.mods.icon_flight.get().saturating_sub(1));
                apply_icon(ui, installed, &key, seq, found);
                pump_icons(ui, &root);
            },
        );
    }
}

/// Decodes one icon **on the event-loop thread** and drops it into the row it
/// belongs to, matched by key so a reordered list cannot get the wrong avatar.
fn apply_icon(
    ui: &Rc<Ui>,
    installed: bool,
    key: &str,
    seq: u64,
    found: Result<IconRef, CatalogError>,
) {
    let current = match installed {
        true => ui.mods.list_seq.get(),
        false => ui.mods.search_seq.get(),
    };
    if seq != current {
        return;
    }
    let reference = match found {
        Ok(reference) => reference,
        Err(error) => return log::debug!("아이콘을 가져오지 못했습니다 ({key}): {error}"),
    };
    let image = match reference {
        IconRef::Path(path) => match Image::load_from_path(&path) {
            Ok(image) => image,
            Err(error) => {
                return log::debug!("아이콘을 읽지 못했습니다 ({}): {error}", path.display());
            }
        },
        IconRef::Rgba(bytes) => Image::from_rgba8(SharedPixelBuffer::clone_from_slice(
            &bytes.rgba,
            bytes.w,
            bytes.h,
        )),
    };
    let index = match installed {
        true => ui
            .mods
            .installed
            .iter()
            .position(|row| row.file_name == key),
        false => ui.mods.hits.iter().position(|row| row.project_id == key),
    };
    let Some(index) = index else { return };
    let model = match installed {
        true => &ui.mods.installed_icons,
        false => &ui.mods.hit_icons,
    };
    if index < model.row_count() {
        model.set_row_data(index, image);
    }
}

// ── Install (§8.4) ──────────────────────────────────────────────────────────

/// The quiet half of 설치: resolve the version, then plan. No version for the
/// server's mc + loader opens no dialog — the row says so instead.
fn resolve_hit(ui: &Rc<Ui>, index: i32) {
    let Some(app) = ui.app.upgrade() else { return };
    if app.global::<ModsData>().get_resolving() {
        return;
    }
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(hit) = ui.mods.hit_rows.borrow().get(index).cloned() else {
        return;
    };
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    app.global::<ModsData>().set_resolving(true);
    set_row_resolving(ui, index, true);

    let name = server.dir_name.clone();
    let provider = hit.provider;
    let project_id = hit.project_id.clone();
    // The row index is the worker's, not the applier's: a search that lands
    // meanwhile replaces every row. The applier re-finds the row by project id
    // the way `apply_icon` does, and drops the result when it is gone (F11).
    let key = project_id.clone();
    let seq = ui.mods.search_seq.get();
    super::spawn_quiet(
        move || {
            let planned = (|| {
                let ctx = catalog::server_context(&server)?;
                let scope = format!("{} {}", ctx.mc, loader_name(ctx.loader));
                let Some(version) = catalog::install::resolve_version(
                    &catalog,
                    provider,
                    &project_id,
                    &ctx.mc,
                    ctx.loader,
                )?
                else {
                    return Ok((scope, None));
                };
                let plan = catalog::install::plan_install(
                    &catalog,
                    &server,
                    provider,
                    &project_id,
                    Some(&version.version_id),
                )?;
                Ok((scope, Some(plan)))
            })();
            (seq, key, name, planned)
        },
        |ui, (seq, key, name, outcome)| {
            let Some(app) = ui.app.upgrade() else { return };
            // Always released: the button would otherwise stay in `loading`
            // for the rest of the session.
            app.global::<ModsData>().set_resolving(false);
            if *ui.mods.server.borrow() != name {
                return;
            }
            // Stale page: the row this was about is gone, so nothing of it may
            // be written onto whoever now sits at that index (F11).
            let row = match seq == ui.mods.search_seq.get() {
                true => ui.mods.hits.iter().position(|row| row.project_id == key),
                false => None,
            };
            let Some(index) = row else { return };
            set_row_resolving(ui, index, false);
            match outcome {
                Ok((_, Some(plan))) => {
                    ui.mods.plan_source.replace(Some((
                        plan.root.provider,
                        plan.root.project_id.clone(),
                        Some(plan.root.version_id.clone()),
                    )));
                    open_confirm(ui, plan);
                }
                Ok((scope, None)) => mark_no_version(ui, index, &scope),
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "search", None);
                }
            }
        },
    );
}

fn set_row_resolving(ui: &Rc<Ui>, index: usize, resolving: bool) {
    if let Some(mut row) = ui.mods.hits.row_data(index) {
        row.resolving = resolving;
        ui.mods.hits.set_row_data(index, row);
    }
}

/// §8.4: no dialog, the row's 설치 goes disabled with the reason in its caption
/// and one neutral toast says the same thing.
fn mark_no_version(ui: &Rc<Ui>, index: usize, scope: &str) {
    let Some(mut row) = ui.mods.hits.row_data(index) else {
        return;
    };
    if !row.no_version {
        row.caption = format!("{} · {scope}용 버전 없음", row.caption).into();
    }
    row.no_version = true;
    let title = row.title.to_string();
    ui.mods.hits.set_row_data(index, row);
    ui.toast(
        Tone::Neutral,
        &format!("{title}: {scope}용 버전이 없습니다"),
    );
}

/// Fills `install-confirm` from a resolved plan and opens it (§8.4).
fn open_confirm(ui: &Rc<Ui>, plan: InstallPlan) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    let server = ui.mods.server.borrow().clone();

    data.set_confirm_title(format!("{server}에 설치").into());
    data.set_confirm_version_line(confirm_version_line(&plan.root).into());
    data.set_confirm_warning(
        match plan.root.release {
            ReleaseType::Release => String::new(),
            other => format!("정식 버전이 없어 {}를 선택했습니다", release_label(other)),
        }
        .into(),
    );
    data.set_confirm_namespace_index(0);

    let (rows, ids) = dep_rows(&plan);
    let choice = InstallChoice {
        deps: rows
            .iter()
            .zip(&ids)
            .filter(|(row, _)| row.checked)
            .map(|(_, id)| id.clone())
            .collect(),
    };
    ui.mods.deps.set_vec(rows);
    ui.mods.dep_ids.replace(ids);
    ui.mods.choice.replace(choice);

    let (blocked_title, blocked_body) = blocked_copy(&plan);
    data.set_confirm_blocked_title(blocked_title.as_str().into());
    data.set_confirm_blocked_body(blocked_body.as_str().into());
    data.set_confirm_can_install(blocked_title.is_empty());
    ui.mods.plan.replace(Some(plan));
    refresh_confirm_footer(ui);
    clear_failure(ui);
    open_dialog(ui, DialogKind::InstallConfirm);
}

/// Required first, then optional — the ordering `install-confirm` relies on to
/// know where its 선택 의존성 toggle belongs.
fn dep_rows(plan: &InstallPlan) -> (Vec<DepRowData>, Vec<String>) {
    let mut rows = Vec::new();
    let mut ids = Vec::new();
    for (dep, required) in plan
        .required
        .iter()
        .map(|dep| (dep, true))
        .chain(plan.optional.iter().map(|dep| (dep, false)))
    {
        let already = dep.already_installed.is_some();
        rows.push(DepRowData {
            project_id: dep.file.project_id.as_str().into(),
            title: dep.file.title.as_str().into(),
            // The number, never the provider's version *name*: on Modrinth
            // that name usually already starts with the title (D1).
            version: version_text(&dep.file).into(),
            size_label: match already {
                true => SharedString::new(),
                false => size_label(dep.file.file.size).into(),
            },
            checked: required || already,
            already_installed: already,
            required,
            helper: match (already, required) {
                (true, _) => dep
                    .already_installed
                    .clone()
                    .unwrap_or_default()
                    .as_str()
                    .into(),
                (false, true) => "필수 의존성을 빼면 게임이 시작되지 않을 수 있습니다".into(),
                (false, false) => SharedString::new(),
            },
        });
        ids.push(dep.file.project_id.clone());
    }
    (rows, ids)
}

/// The two states that refuse the primary (§8.4).
fn blocked_copy(plan: &InstallPlan) -> (String, String) {
    if !plan.incompatible.is_empty() {
        return (
            "함께 설치할 수 없는 모드가 있습니다".to_owned(),
            format!(
                "{}와(과) {}은(는) 함께 설치할 수 없습니다 — 먼저 한쪽을 지우세요",
                plan.root.title,
                plan.incompatible.join(", ")
            ),
        );
    }
    if !plan.unresolvable.is_empty() {
        return (
            "의존성을 찾을 수 없습니다".to_owned(),
            format!(
                "{} — 이 의존성 없이는 설치할 수 없습니다",
                plan.unresolvable.join(", ")
            ),
        );
    }
    (String::new(), String::new())
}

fn toggle_dep(ui: &Rc<Ui>, index: i32, on: bool) {
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(id) = ui.mods.dep_ids.borrow().get(index).cloned() else {
        return;
    };
    if let Some(mut row) = ui.mods.deps.row_data(index) {
        row.checked = on;
        ui.mods.deps.set_row_data(index, row);
    }
    let mut choice = ui.mods.choice.borrow_mut();
    match on {
        true => choice.deps.insert(id),
        false => choice.deps.remove(&id),
    };
    drop(choice);
    refresh_confirm_footer(ui);
}

/// `{N}개 · {size}` and `{N}개 설치` — counting only what will be downloaded.
fn refresh_confirm_footer(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let plan = ui.mods.plan.borrow();
    let Some(plan) = plan.as_ref() else { return };
    let choice = ui.mods.choice.borrow();
    let mut count = 1;
    let mut bytes = plan.root.file.size;
    for dep in plan.required.iter().chain(&plan.optional) {
        if dep.already_installed.is_some() || !choice.deps.contains(&dep.file.project_id) {
            continue;
        }
        count += 1;
        bytes += dep.file.file.size;
    }
    let data = app.global::<ModsData>();
    data.set_confirm_footer(format!("{count}개 · {}", size_label(bytes)).into());
    data.set_confirm_primary_label(format!("{count}개 설치").into());
}

/// Applies the staged plan. `wait` is the 429 back-off the one automatic retry
/// sleeps through on the worker before it tries again (§8.4).
fn start_install(ui: &Rc<Ui>, wait: Duration) {
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    let Some(app) = ui.app.upgrade() else { return };
    let Some(plan) = ui.mods.plan.borrow().clone() else {
        return;
    };
    let choice = ui.mods.choice.borrow().clone();
    let namespace = namespace_of(app.global::<ModsData>().get_confirm_namespace_index());
    ui.mods.retry.replace(Some(Retry::Install));
    // The manual list belongs to the install that produced it, and this is a
    // different one.
    ui.mods.manual.set_vec(Vec::new());
    begin_progress(ui, "search");
    if !wait.is_zero() {
        // §8.4's 429 line has to survive the retry it announces, so it is
        // written here rather than by the attempt that failed: `begin_progress`
        // clears the Callout, and the progress row would otherwise read
        // 준비 중 for up to thirty silent seconds.
        let line = rate_limit_line(wait);
        show_failure(ui, "요청이 너무 많습니다", &line, "search");
        app.global::<ModsData>().set_progress_text(line.into());
    }

    let progress = progress_sink("search");
    spawn_catalog_job(
        ui,
        "모드 설치",
        move || {
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            let container = catalog::server_context(&server).map(|ctx| ctx.container);
            let report =
                catalog::install::apply(&catalog, &server, &plan, &choice, namespace, &progress);
            report.map(|report| (report, container.unwrap_or_default(), namespace))
        },
        |ui, outcome| {
            end_progress(ui);
            match outcome {
                Ok((report, container, namespace)) => {
                    finish_install(ui, report, &container, namespace);
                }
                Err(error) => install_failed(ui, error),
            }
        },
    );
}

fn finish_install(ui: &Rc<Ui>, report: InstallReport, container: &std::path::Path, ns: Namespace) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    close_dialog(ui);
    ui.mods.manual.set_vec(
        report
            .manual
            .iter()
            .map(|item| ManualDownloadData {
                name: item.name.as_str().into(),
                file: item.file_name.as_str().into(),
                url: item.url.as_str().into(),
            })
            .collect::<Vec<_>>(),
    );
    data.set_manual_folder(container.join(ns.dir_name()).display().to_string().into());
    let server = ui.mods.server.borrow().clone();
    ui.toast(
        Tone::Success,
        &format!("{server}: 모드 {}개를 설치했습니다", report.installed.len()),
    );
    reload_installed(ui);
}

/// §8.4's three special endings: a rate limit retries once, a stale plan is
/// resolved again and re-confirmed, everything else is a danger Callout with
/// the cause, the fix and — after a rollback — what was left behind.
fn install_failed(ui: &Rc<Ui>, error: CatalogError) {
    if let CatalogError::RateLimited { retry_after_secs } = error
        && !ui.mods.rate_retried.replace(true)
    {
        let wait = Duration::from_secs(u64::from(retry_after_secs)).min(MAX_RATE_WAIT);
        // The message is `start_install`'s to write: the retry below clears
        // the Callout on its way in, so saying it here would show it for the
        // 50 ms until the timer fires.
        //
        // The job lock is still held by the job that just failed; a one-shot
        // timer starts the retry on the next turn of the loop, after it clears.
        slint::Timer::single_shot(Duration::from_millis(50), move || {
            with_ui(|ui| start_install(ui, wait));
        });
        return;
    }
    if matches!(error, CatalogError::Stale) {
        return reresolve_plan(ui);
    }
    if matches!(error, CatalogError::ServerChanged) {
        // §8.4 refuses this one out loud — it is never the silent re-resolve.
        // The plan is dead, so the dialog it belongs to goes with it and
        // 다시 시도 is left with nothing to repeat rather than a refusal loop.
        close_dialog(ui);
        ui.mods.retry.replace(None);
    }
    let (title, body) = failure_copy(&error);
    show_failure(ui, &title, &body, "search");
    set_failure_folder(ui, &error);
    ui.toast(Tone::Danger, "설치 실패");
}

/// The rollback Callout's 폴더 열기 target (§8.4): the directory holding the
/// first file the rollback could not remove. Every other failure leaves the
/// property empty, which is what hides the button.
fn set_failure_folder(ui: &Rc<Ui>, error: &CatalogError) {
    let CatalogError::RolledBack { left_behind, .. } = error else {
        return;
    };
    // The report lists files; `open_folder` creates what it is handed, so the
    // parent is the only safe thing to give it.
    let Some(folder) = left_behind.first().and_then(|path| path.parent()) else {
        return;
    };
    if let Some(app) = ui.app.upgrade() {
        app.global::<ModsData>()
            .set_failure_folder(folder.display().to_string().into());
    }
}

/// A plan older than ten minutes, or one made against a different index, is
/// resolved again and `install-confirm` re-opened on the fresh one (§8.4).
fn reresolve_plan(ui: &Rc<Ui>) {
    let Some((provider, project_id, version)) = ui.mods.plan_source.borrow().clone() else {
        return;
    };
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || {
            let plan = catalog::install::plan_install(
                &catalog,
                &server,
                provider,
                &project_id,
                version.as_deref(),
            );
            (name, plan)
        },
        |ui, (name, outcome)| {
            if *ui.mods.server.borrow() != name {
                return;
            }
            match outcome {
                Ok(plan) => {
                    ui.toast(Tone::Warning, "설치 계획이 오래되어 다시 확인합니다");
                    open_confirm(ui, plan);
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "search", None);
                }
            }
        },
    );
}

fn retry_last(ui: &Rc<Ui>) {
    let retry = ui.mods.retry.borrow().clone();
    match retry {
        Some(Retry::Install) => {
            ui.mods.rate_retried.set(false);
            start_install(ui, Duration::ZERO);
        }
        Some(Retry::Updates) => start_updates(ui),
        Some(Retry::Remove(file)) => start_remove(ui, &file),
        Some(Retry::Move(file, namespace)) => start_move(ui, &file, namespace),
        Some(Retry::Replace(file, version)) => start_replace(ui, &file, *version),
        Some(Retry::Reload) => reload_installed(ui),
        Some(Retry::Check) => check_updates(ui),
        // Nothing to repeat: the Callout is about a job that either has no
        // second attempt of its own, or was never started.
        None => clear_failure(ui),
    }
}

// ── 설치됨 row actions (§8.3) ───────────────────────────────────────────────

fn open_detail(ui: &Rc<Ui>, index: i32) {
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(app) = ui.app.upgrade() else { return };
    let rows = ui.mods.installed_rows.borrow();
    let Some(row) = rows.get(index) else { return };
    let data = app.global::<ModsData>();
    let server = ui.mods.server.borrow().clone();
    let title = row.title().to_owned();

    data.set_detail_title(format!("{title} — {server}").into());
    data.set_detail_description(
        row.local
            .as_ref()
            .and_then(|local| local.description.clone())
            .unwrap_or_default()
            .replace('\n', " ")
            .trim()
            .into(),
    );
    data.set_detail_version(installed_version(row).into());
    data.set_detail_file(row.file_name.as_str().into());
    data.set_detail_provider(
        match &row.entry {
            Some(entry) => match entry.provider {
                ProviderId::Modrinth => "Modrinth",
                ProviderId::CurseForge => "CurseForge",
            },
            None => "직접 추가",
        }
        .into(),
    );
    data.set_detail_hash(short_hash(row).into());
    data.set_detail_namespace_index(namespace_index(row.namespace));
    data.set_detail_has_provider(row.entry.is_some());
    data.set_detail_is_managed(row.entry.is_some());
    let file_name = row.file_name.clone();
    drop(rows);
    ui.mods.detail.replace(file_name);
    open_dialog(ui, DialogKind::ModDetail);
}

/// SHA-1, first twelve characters (§8.3).
fn short_hash(row: &InstalledMod) -> String {
    let full = row
        .entry
        .as_ref()
        .map(|entry| entry.hashes.sha1.clone())
        .or_else(|| row.local.as_ref().map(|local| local.sha1.clone()))
        .unwrap_or_default();
    full.chars().take(12).collect()
}

fn find_installed(ui: &Rc<Ui>) -> Option<InstalledMod> {
    let file = ui.mods.detail.borrow().clone();
    ui.mods
        .installed_rows
        .borrow()
        .iter()
        .find(|row| row.file_name == file)
        .cloned()
}

fn apply_namespace(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let Some(row) = find_installed(ui) else {
        return;
    };
    let namespace = namespace_of(app.global::<ModsData>().get_detail_namespace_index());
    if namespace == row.namespace {
        return close_dialog(ui);
    }
    start_move(ui, &row.file_name, namespace);
}

fn start_move(ui: &Rc<Ui>, file: &str, namespace: Namespace) {
    let Some((_, _, server)) = selected(ui) else {
        return;
    };
    ui.mods
        .retry
        .replace(Some(Retry::Move(file.to_owned(), namespace)));
    begin_progress(ui, "installed");
    let file = file.to_owned();
    spawn_catalog_job(
        ui,
        "모드 이동",
        move || catalog::manage::move_mod(&server, &file, namespace),
        move |ui, outcome| {
            end_progress(ui);
            match outcome {
                Ok(()) => {
                    close_dialog(ui);
                    ui.toast(Tone::Success, moved_toast(namespace));
                    reload_installed(ui);
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_failure(ui, &title, &body, "installed");
                    ui.toast(Tone::Danger, "이동 실패");
                }
            }
        },
    );
}

/// The three labels take two different particles (로 / 으로), so the sentences
/// are written out rather than assembled.
fn moved_toast(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Required => "필수로 이동했습니다",
        Namespace::OptionalOn => "선택 켜짐으로 이동했습니다",
        Namespace::OptionalOff => "선택 꺼짐으로 이동했습니다",
    }
}

/// The project page, asked for by id because the index keeps no URL and a
/// CurseForge entry deliberately keeps no slug either (D2).
fn open_project_page(ui: &Rc<Ui>) {
    let Some(row) = find_installed(ui) else {
        return;
    };
    let Some(entry) = row.entry.clone() else {
        return;
    };
    let catalog = ui.mods.catalog.borrow().clone();
    super::spawn_quiet(
        move || {
            catalog::install::require_provider(&catalog, entry.provider)
                .and_then(|provider| provider.project(&entry.project_id))
        },
        |ui, outcome| match outcome {
            Ok(hit) => match hit.page_url {
                Some(url) => super::open_external(&url),
                None => ui.toast(Tone::Warning, "프로젝트 페이지 주소가 없습니다"),
            },
            Err(error) => {
                let (title, body) = failure_copy(&error);
                show_quiet_failure(ui, &title, &body, "installed", None);
            }
        },
    );
}

fn open_version_picker(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let (provider, project_id, target) = match app.global::<AppData>().get_dialog() {
        // From `install-confirm`: the pick re-plans the install.
        DialogKind::InstallConfirm => {
            let plan = ui.mods.plan.borrow();
            let Some(plan) = plan.as_ref() else { return };
            (
                plan.root.provider,
                plan.root.project_id.clone(),
                VersionTarget::Hit {
                    provider: plan.root.provider,
                    project_id: plan.root.project_id.clone(),
                },
            )
        }
        // From `mod-detail`: the pick replaces the installed file.
        _ => {
            let Some(row) = find_installed(ui) else {
                return;
            };
            let Some(entry) = row.entry.clone() else {
                return;
            };
            (
                entry.provider,
                entry.project_id.clone(),
                VersionTarget::Installed {
                    file_name: row.file_name.clone(),
                    title: row.title().to_owned(),
                    current_id: entry.version_id.clone(),
                    current_published: String::new(),
                    current_number: installed_version(&row),
                },
            )
        }
    };

    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    let title = match &target {
        VersionTarget::Installed { title, .. } => title.clone(),
        VersionTarget::Hit { .. } => ui
            .mods
            .plan
            .borrow()
            .as_ref()
            .map(|plan| plan.root.title.clone())
            .unwrap_or_default(),
    };
    ui.mods.version_target.replace(Some(target));
    ui.mods.downgrade.set(-1);
    let server_name = ui.mods.server.borrow().clone();
    app.global::<ModsData>()
        .set_version_dialog_title(format!("{title} 버전 — {server_name}").into());
    app.global::<ModsData>()
        .set_downgrade_confirm_line(SharedString::new());
    ui.mods.versions.set_vec(Vec::new());
    open_dialog(ui, DialogKind::VersionPicker);

    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || {
            let versions = catalog::server_context(&server).and_then(|ctx| {
                catalog::install::require_provider(&catalog, provider)?.versions(
                    &project_id,
                    &ctx.mc,
                    ctx.loader,
                )
            });
            (name, versions)
        },
        |ui, (name, outcome)| {
            if *ui.mods.server.borrow() != name {
                return;
            }
            match outcome {
                Ok(versions) => apply_versions(ui, versions),
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "installed", None);
                }
            }
        },
    );
}

fn apply_versions(ui: &Rc<Ui>, versions: Vec<Version>) {
    let current = match ui.mods.version_target.borrow().as_ref() {
        Some(VersionTarget::Installed {
            current_id,
            current_published,
            ..
        }) => Some((current_id.clone(), current_published.clone())),
        _ => None,
    };
    // The installed entry stores no publication date, so the installed
    // version's own row supplies it: whichever row carries the current id is
    // the anchor every other row is compared against.
    let anchor = current.as_ref().and_then(|(id, _)| {
        versions
            .iter()
            .find(|version| version.version_id == *id)
            .map(|version| version.published.clone())
    });
    let current = current.map(|(id, published)| (id, anchor.unwrap_or(published)));

    ui.mods.versions.set_vec(
        versions
            .iter()
            .map(|version| {
                let size = version.primary_file().map(|file| file.size).unwrap_or(0);
                let (action, enabled) = version_action(
                    &version.version_id,
                    &version.published,
                    current.as_ref().map(|(id, at)| (id.as_str(), at.as_str())),
                );
                VersionRowData {
                    version_id: version.version_id.as_str().into(),
                    name: version.name.as_str().into(),
                    release_label: release_label(version.release).into(),
                    game_versions_label: game_versions_label(&version.game_versions).into(),
                    date: date_label(&version.published).unwrap_or_default().into(),
                    size_label: size_label(size).into(),
                    action: action.into(),
                    enabled,
                }
            })
            .collect::<Vec<_>>(),
    );
    ui.mods.version_rows.replace(versions);
}

fn pick_version(ui: &Rc<Ui>, index: i32) {
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let Some(version) = ui.mods.version_rows.borrow().get(index).cloned() else {
        return;
    };
    // Read the target out as owned values first: everything below either
    // spawns a job or re-enters the state this `RefCell` holds.
    let picked = match ui.mods.version_target.borrow().as_ref() {
        Some(VersionTarget::Hit {
            provider,
            project_id,
        }) => Some(Picked::Hit(*provider, project_id.clone())),
        Some(VersionTarget::Installed {
            file_name,
            current_number,
            ..
        }) => Some(Picked::Installed(file_name.clone(), current_number.clone())),
        None => None,
    };
    match picked {
        Some(Picked::Hit(provider, project_id)) => {
            replan(ui, provider, &project_id, &version.version_id);
        }
        Some(Picked::Installed(file_name, current_number)) => {
            let downgrade = ui
                .mods
                .versions
                .row_data(index)
                .is_some_and(|row| row.action == "이전 버전으로 교체");
            // §8.3: a downgrade asks a one-line confirmation inside the dialog
            // first; the same click again is what applies it.
            let index = i32::try_from(index).unwrap_or(-1);
            if downgrade && ui.mods.downgrade.replace(index) != index {
                if let Some(app) = ui.app.upgrade() {
                    app.global::<ModsData>().set_downgrade_confirm_line(
                        format!("현재 {current_number} → {}로 내립니다", version.number).into(),
                    );
                }
                return;
            }
            start_replace(ui, &file_name, version);
        }
        None => {}
    }
}

/// What [`pick_version`] read off `version_target` before letting go of it.
enum Picked {
    Hit(ProviderId, String),
    /// The file being replaced, and the version number the downgrade line
    /// names as "current".
    Installed(String, String),
}

/// A version picked before the install: the plan is rebuilt around it and
/// `install-confirm` re-opens on the new one.
fn replan(ui: &Rc<Ui>, provider: ProviderId, project_id: &str, version_id: &str) {
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    ui.mods.plan_source.replace(Some((
        provider,
        project_id.to_owned(),
        Some(version_id.to_owned()),
    )));
    let (project_id, version_id) = (project_id.to_owned(), version_id.to_owned());
    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || {
            let plan = catalog::install::plan_install(
                &catalog,
                &server,
                provider,
                &project_id,
                Some(&version_id),
            );
            (name, plan)
        },
        |ui, (name, outcome)| {
            if *ui.mods.server.borrow() != name {
                return;
            }
            match outcome {
                Ok(plan) => open_confirm(ui, plan),
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "search", None);
                }
            }
        },
    );
}

fn start_replace(ui: &Rc<Ui>, file: &str, version: Version) {
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    ui.mods.retry.replace(Some(Retry::Replace(
        file.to_owned(),
        Box::new(version.clone()),
    )));
    begin_progress(ui, "installed");
    let file = file.to_owned();
    let progress = progress_sink("installed");
    spawn_catalog_job(
        ui,
        "모드 업데이트",
        move || catalog::update::replace_file(&catalog, &server, &file, &version, &progress),
        |ui, outcome| {
            end_progress(ui);
            match outcome {
                Ok(()) => {
                    close_dialog(ui);
                    ui.toast(Tone::Success, "모드를 교체했습니다");
                    reload_installed(ui);
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_failure(ui, &title, &body, "installed");
                    ui.toast(Tone::Danger, "교체 실패");
                }
            }
        },
    );
}

fn request_delete(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let Some(row) = find_installed(ui) else {
        return;
    };
    let data = app.global::<ModsData>();
    data.set_delete_title(format!("모드 삭제 — {}", row.file_name).into());
    data.set_delete_path(
        ui.mods
            .container
            .borrow()
            .join(row.namespace.dir_name())
            .join(&row.file_name)
            .display()
            .to_string()
            .into(),
    );
    data.set_delete_dependents(dependents_line(ui, &row).into());
    open_dialog(ui, DialogKind::ModDelete);
}

/// `fabric-api를 필요로 하는 모드: Sodium, Lithium` (§8.3), read off the index
/// entries that recorded this project as a required dependency at install time.
fn dependents_line(ui: &Rc<Ui>, row: &InstalledMod) -> String {
    let Some(entry) = row.entry.as_ref() else {
        return String::new();
    };
    let names: Vec<String> = ui
        .mods
        .installed_rows
        .borrow()
        .iter()
        .filter(|other| other.file_name != row.file_name)
        .filter(|other| {
            other
                .entry
                .as_ref()
                .is_some_and(|other| other.dependencies.contains(&entry.project_id))
        })
        .map(|other| other.title().to_owned())
        .collect();
    match names.is_empty() {
        true => String::new(),
        false => format!("{}를 필요로 하는 모드: {}", row.title(), names.join(", ")),
    }
}

fn confirm_delete(ui: &Rc<Ui>) {
    let Some(row) = find_installed(ui) else {
        return;
    };
    start_remove(ui, &row.file_name);
}

fn start_remove(ui: &Rc<Ui>, file: &str) {
    let Some((_, _, server)) = selected(ui) else {
        return;
    };
    ui.mods.retry.replace(Some(Retry::Remove(file.to_owned())));
    begin_progress(ui, "installed");
    let file = file.to_owned();
    spawn_catalog_job(
        ui,
        "모드 삭제",
        move || catalog::manage::remove_mod(&server, &file),
        |ui, outcome| {
            end_progress(ui);
            match outcome {
                Ok(()) => {
                    close_dialog(ui);
                    ui.toast(Tone::Success, "모드를 삭제했습니다");
                    reload_installed(ui);
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_failure(ui, &title, &body, "installed");
                    ui.toast(Tone::Danger, "삭제 실패");
                }
            }
        },
    );
}

// ── Updates (§8.3) ──────────────────────────────────────────────────────────

fn check_updates(ui: &Rc<Ui>) {
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    let Some(app) = ui.app.upgrade() else { return };
    // §8.6 is single-flight per kind, and this kind writes: the adoption pass
    // rewrites the index under the server lock, so a second check would race
    // the first one for it.
    if app.global::<ModsData>().get_checking() {
        return;
    }
    let seq = ui.mods.check_seq.get() + 1;
    ui.mods.check_seq.set(seq);
    app.global::<ModsData>().set_checking(true);
    let name = server.dir_name.clone();
    super::spawn_quiet(
        move || (seq, name, catalog::update::check_updates(&catalog, &server)),
        |ui, (seq, name, outcome)| {
            // The sequence is the ownership token: a superseded result leaves
            // the spinner to whoever owns it now, and only then does the
            // server guard drop the data (F11).
            if seq != ui.mods.check_seq.get() {
                return;
            }
            let Some(app) = ui.app.upgrade() else { return };
            app.global::<ModsData>().set_checking(false);
            if *ui.mods.server.borrow() != name {
                return;
            }
            match outcome {
                Ok(report) => {
                    let count = report.candidates.len();
                    ui.mods.last_check.set(Some(Instant::now()));
                    ui.mods.shown_check.set(u64::MAX);
                    apply_candidates(ui, report.candidates);
                    for line in check_toasts(report.adopted.len(), count, ui.toasts.row_count()) {
                        ui.toast(Tone::Success, &line);
                    }
                    if !report.adopted.is_empty() {
                        // The adoption wrote managed rows for jars the list
                        // still calls 직접 추가 (§8.4).
                        reload_installed(ui);
                    }
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_quiet_failure(ui, &title, &body, "installed", Some(Retry::Check));
                }
            }
        },
    );
}

/// What 업데이트 확인 says afterwards: the adoption line first, then §8.3's
/// verdict — folded into one line when two more toasts would push an older one
/// off the three-deep stack.
fn check_toasts(adopted: usize, updates: usize, standing: usize) -> Vec<String> {
    let verdict = match updates {
        0 => "모두 최신입니다".to_owned(),
        n => format!("업데이트 {n}개 있음"),
    };
    let Some(adopted) =
        (adopted > 0).then(|| format!("직접 추가 {adopted}개를 관리 목록에 연결했습니다"))
    else {
        return vec![verdict];
    };
    match standing + 2 > super::MAX_TOASTS {
        true => vec![format!("{adopted} · {verdict}")],
        false => vec![adopted, verdict],
    }
}

fn apply_candidates(ui: &Rc<Ui>, candidates: Vec<UpdateCandidate>) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    let installed = ui.mods.installed_rows.borrow();
    ui.mods.updates.set_vec(
        candidates
            .iter()
            .map(|candidate| UpdateRowData {
                file_name: candidate.file_name.as_str().into(),
                title: candidate.title.as_str().into(),
                // §8.3 wants `0.91.0 → 0.92.0`, and `candidate.from` is the
                // provider's opaque version id. The jar the list already read
                // carries the number (D3).
                from: installed
                    .iter()
                    .find(|row| row.file_name == candidate.file_name)
                    .map(installed_version)
                    .unwrap_or_else(|| candidate.from.clone())
                    .into(),
                to: candidate.to.number.as_str().into(),
                release_label: release_label(candidate.to.release).into(),
            })
            .collect::<Vec<_>>(),
    );
    drop(installed);
    data.set_update_count(i32::try_from(candidates.len()).unwrap_or(0));
    ui.mods.candidates.replace(candidates);
    // The rows re-sort (업데이트 가능 first) and re-chip off the same list.
    let rows = ui.mods.installed_rows.borrow().clone();
    let root = ui
        .settings
        .borrow()
        .to_config()
        .ok()
        .map(|cfg| Root::new(cfg.root, cfg.base_url));
    if let Some(root) = root {
        let seq = ui.mods.list_seq.get();
        apply_installed(ui, rows, &root, seq);
    }
}

fn start_updates(ui: &Rc<Ui>) {
    let Some((catalog, _, server)) = selected(ui) else {
        return;
    };
    let candidates = ui.mods.candidates.borrow().clone();
    if candidates.is_empty() {
        return;
    }
    ui.mods.retry.replace(Some(Retry::Updates));
    begin_progress(ui, "installed");
    let progress = progress_sink("installed");
    spawn_catalog_job(
        ui,
        "모드 업데이트",
        move || catalog::update::apply_updates(&catalog, &server, &candidates, &progress),
        |ui, outcome| {
            end_progress(ui);
            close_dialog(ui);
            match outcome {
                Ok(results) => {
                    let names = failed_names(&results);
                    let failed: Vec<String> = results
                        .iter()
                        .filter_map(|(file, result)| {
                            result
                                .as_ref()
                                .err()
                                .map(|error| format!("{file}: {error}"))
                        })
                        .collect();
                    let done = results.len() - failed.len();
                    if failed.is_empty() {
                        ui.toast(Tone::Success, &format!("모드 {done}개를 업데이트했습니다"));
                    } else {
                        show_failure(
                            ui,
                            "일부 모드를 업데이트하지 못했습니다",
                            &format!(
                                "{done}개는 끝났습니다. 실패한 모드를 확인한 뒤 다시 시도하세요 — {}",
                                failed.join(" · ")
                            ),
                            "installed",
                        );
                        ui.toast(Tone::Danger, "업데이트 실패");
                    }
                    // Only what failed survives, so the Callout's 다시 시도
                    // re-runs exactly those; a clean run clears the list (F8).
                    let mut left = ui.mods.candidates.borrow().clone();
                    left.retain(|candidate| names.contains(&candidate.file_name));
                    apply_candidates(ui, left);
                    reload_installed(ui);
                }
                Err(error) => {
                    let (title, body) = failure_copy(&error);
                    show_failure(ui, &title, &body, "installed");
                    ui.toast(Tone::Danger, "업데이트 실패");
                }
            }
        },
    );
}

// ── 서버 duplicate / delete (§8.5) ──────────────────────────────────────────

fn confirm_duplicate(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    data.set_duplicate_error(SharedString::new());
    let new_id = data.get_duplicate_id().trim().to_owned();
    if new_id.is_empty() {
        return data.set_duplicate_error("새 서버 ID가 비어 있습니다 — 예: Test".into());
    }
    let source = ui.mods.menu_server.borrow().clone();
    let cfg = match ui.settings.borrow().to_config() {
        Ok(cfg) => cfg,
        Err(error) => return data.set_duplicate_error(error.to_string().into()),
    };
    let root = Root::new(cfg.root, cfg.base_url);
    // The refresh rides inside the same job: the lock is still held when the
    // applier runs, so a second `spawn_job` there would simply be refused.
    let current = ui.settings.borrow().clone();
    let distro = app.global::<SheetData>().get_distro_name().to_string();
    spawn_catalog_job(
        ui,
        "서버 복제",
        move || {
            let dir_name = catalog::manage::duplicate_server(&root, &source, &new_id)?;
            Ok((dir_name, super::snapshot(&current, &distro)))
        },
        |ui, outcome| match outcome {
            Ok((dir_name, snap)) => {
                close_dialog(ui);
                ui.apply_snapshot(snap);
                ui.toast(Tone::Success, &format!("서버를 복제했습니다: {dir_name}"));
            }
            Err(error) => {
                let message = error.to_string();
                log::error!("{message}");
                if let Some(app) = ui.app.upgrade() {
                    app.global::<ModsData>()
                        .set_duplicate_error(message.as_str().into());
                }
                ui.toast(Tone::Danger, "복제 실패");
            }
        },
    );
}

fn confirm_server_delete(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    let dir_name = data.get_delete_server_id().to_string();
    // The real gate: §8.5 compares the **trimmed** input, and Slint 1.17 has no
    // `trim`, so the dialog's own binding is only the first of the two checks.
    if !delete_confirmed(&data.get_delete_server_input(), &dir_name) {
        return;
    }
    let cfg = match ui.settings.borrow().to_config() {
        Ok(cfg) => cfg,
        Err(error) => {
            ui.toast(Tone::Danger, &error.to_string());
            return;
        }
    };
    let root = Root::new(cfg.root, cfg.base_url);
    let deleted = dir_name.clone();
    let current = ui.settings.borrow().clone();
    let distro = app.global::<SheetData>().get_distro_name().to_string();
    spawn_catalog_job(
        ui,
        "서버 삭제",
        move || {
            catalog::manage::delete_server(&root, &dir_name)?;
            Ok(super::snapshot(&current, &distro))
        },
        move |ui, outcome| match outcome {
            Ok(snap) => {
                close_dialog(ui);
                let was_selected = *ui.mods.server.borrow() == deleted;
                if was_selected {
                    // The page's subject just stopped existing; the snapshot
                    // below picks the first server that still does.
                    ui.mods.server.replace(String::new());
                    ui.mods.loaded.replace(String::new());
                    ui.mods.installed.set_vec(Vec::new());
                    ui.mods.installed_icons.set_vec(Vec::new());
                    ui.mods.installed_rows.replace(Vec::new());
                }
                ui.apply_snapshot(snap);
                ui.toast(Tone::Success, &format!("서버를 삭제했습니다: {deleted}"));
            }
            Err(error) => {
                log::error!("{error}");
                ui.toast(Tone::Danger, &format!("서버 삭제 실패: {error}"));
            }
        },
    );
}

// ── Progress, failure and job plumbing ──────────────────────────────────────

/// `spawn_job` speaks `nebula_core::Result`, but §8.4 chooses its copy by
/// `CatalogError` variant, so the catalog's own result rides inside the success
/// arm and the outer `Result` is never `Err`.
fn spawn_catalog_job<T, W, A>(ui: &Rc<Ui>, label: &str, work: W, apply: A) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> Result<T, CatalogError> + Send + 'static,
    A: FnOnce(&Rc<Ui>, Result<T, CatalogError>) + Send + 'static,
{
    super::spawn_job(
        ui,
        label,
        Screen::Mods,
        move || Ok(work()),
        move |ui, outcome| {
            if let Ok(inner) = outcome {
                apply(ui, inner);
            }
        },
    )
}

/// The progress callback every core entry point takes. It runs **on the
/// worker**, so it carries plain data back to the loop and touches nothing.
fn progress_sink(owner: &'static str) -> impl Fn(usize, usize, &str) + Send + Sync {
    move |done, total, name| {
        let name = name.to_owned();
        let _ = slint::invoke_from_event_loop(move || {
            with_ui(move |ui| {
                let Some(app) = ui.app.upgrade() else { return };
                let elapsed = ui
                    .job_started
                    .get()
                    .map(|at| at.elapsed())
                    .unwrap_or_default();
                let data = app.global::<ModsData>();
                data.set_progress_owner(owner.into());
                data.set_progress_visible(true);
                data.set_progress_fraction(match total {
                    0 => -1.0,
                    total => done as f32 / total as f32,
                });
                data.set_progress_text(progress_line(done, total, &name, elapsed).into());
                ui.mods.shown_progress.set(elapsed.as_secs());
                ui.mods.progress.replace(Some((done, total, name)));
            });
        });
    }
}

/// The row goes up the moment the job starts, indeterminate until the first
/// file is announced.
fn begin_progress(ui: &Rc<Ui>, owner: &str) {
    clear_failure(ui);
    ui.mods.progress.replace(None);
    ui.mods.shown_progress.set(u64::MAX);
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    data.set_progress_owner(owner.into());
    data.set_progress_visible(true);
    data.set_progress_fraction(-1.0);
    data.set_progress_text("준비 중".into());
}

fn end_progress(ui: &Rc<Ui>) {
    ui.mods.progress.replace(None);
    if let Some(app) = ui.app.upgrade() {
        let data = app.global::<ModsData>();
        data.set_progress_visible(false);
        data.set_progress_text(SharedString::new());
        data.set_progress_fraction(-1.0);
    }
}

/// A quiet job's failure, which also says what 다시 시도 should run. Mutating
/// jobs set their own [`Retry`] before they start and use [`show_failure`].
fn show_quiet_failure(ui: &Rc<Ui>, title: &str, body: &str, owner: &str, retry: Option<Retry>) {
    ui.mods.retry.replace(retry);
    show_failure(ui, title, body, owner);
}

fn show_failure(ui: &Rc<Ui>, title: &str, body: &str, owner: &str) {
    log::error!("{title}: {body}");
    let Some(app) = ui.app.upgrade() else { return };
    let data = app.global::<ModsData>();
    data.set_progress_owner(owner.into());
    data.set_failure_title(title.into());
    data.set_failure_body(body.into());
    // 다시 시도 appears only when the retry slot actually holds a job. Without
    // this it is a button that erases the message it sits on and does nothing
    // else — which is what a `ServerChanged` refusal and every quiet failure
    // that cleared the slot would otherwise show.
    data.set_failure_can_retry(ui.mods.retry.borrow().is_some());
    // Whoever wants the 폴더 열기 button sets it after this call: a later
    // failure must never inherit an earlier rollback's folder.
    data.set_failure_folder(SharedString::new());
    data.set_failure_visible(true);
}

fn clear_failure(ui: &Rc<Ui>) {
    if let Some(app) = ui.app.upgrade() {
        let data = app.global::<ModsData>();
        data.set_failure_visible(false);
        data.set_failure_title(SharedString::new());
        data.set_failure_body(SharedString::new());
        data.set_failure_folder(SharedString::new());
        data.set_failure_can_retry(true);
    }
}

// ── Pure formatting (§8.8) ──────────────────────────────────────────────────

/// `mods-install-{provider}-{project id}` (§8.4). Both halves are
/// provider-scoped and re-assignable, which is why the host builds it.
fn install_a11y_id(provider: ProviderId, project_id: &str) -> String {
    format!("mods-install-{}-{project_id}", provider.as_str())
}

/// The version a dialog shows for a planned file: the provider's version
/// *number*, because its version *name* is free text that usually repeats the
/// mod title (D1). CurseForge numbers its files by id and can leave it empty,
/// which is the only case the name is still the better answer.
fn version_text(file: &PlannedFile) -> &str {
    match file.version_number.trim().is_empty() {
        true => &file.version_name,
        false => &file.version_number,
    }
}

/// §8.4's install-confirm line: `fabric-api 0.92.0 · 정식 · 2.1 MB ·
/// 2025-03-03`. A provider that sent no publication date loses the last
/// segment rather than leaving a separator with nothing after it.
fn confirm_version_line(file: &PlannedFile) -> String {
    let mut line = format!(
        "{} {} · {} · {}",
        file.title,
        version_text(file),
        release_label(file.release),
        size_label(file.file.size)
    );
    if let Some(date) = date_label(&file.published) {
        line.push_str(" · ");
        line.push_str(&date);
    }
    line
}

/// The file names whose update failed, which is what 다시 시도 must re-run and
/// all that may stay in the candidate list afterwards (F8).
fn failed_names(results: &[catalog::update::UpdateOutcome]) -> BTreeSet<String> {
    results
        .iter()
        .filter(|(_, result)| result.is_err())
        .map(|(file, _)| file.clone())
        .collect()
}

/// `작성자 · 다운로드 120만 · 수정 3일 전`; sorted by 최신 the date leads (§8.4).
fn hit_caption(hit: &ProjectHit, newest_first: bool, now: i64) -> String {
    let author = hit
        .author
        .clone()
        .unwrap_or_else(|| "작성자 미상".to_owned());
    let downloads = format!("다운로드 {}", count_label(hit.downloads));
    let updated = hit
        .updated
        .as_deref()
        .and_then(epoch_secs)
        .map(|at| format!("수정 {}", relative_label(now - at)));
    match (newest_first, updated) {
        (true, Some(updated)) => format!("{updated} · {author} · {downloads}"),
        (false, Some(updated)) => format!("{author} · {downloads} · {updated}"),
        (_, None) => format!("{author} · {downloads}"),
    }
}

/// `120만` / `3.2억` (§8.8). Under 만 the plain number is the honest answer.
fn count_label(count: u64) -> String {
    const MAN: u64 = 10_000;
    const EOK: u64 = 100_000_000;
    if count < MAN {
        return count.to_string();
    }
    let (value, unit) = match count < EOK {
        true => (count as f64 / MAN as f64, "만"),
        false => (count as f64 / EOK as f64, "억"),
    };
    // Truncated, never rounded: `9999만` must not read as `1억`.
    let rendered = match value < 10.0 {
        true => format!("{value:.1}"),
        false => format!("{}", value as u64),
    };
    format!("{}{unit}", rendered.strip_suffix(".0").unwrap_or(&rendered))
}

/// `N분 전` / `N일 전` (§8.8), with the two ends §8.8 does not name: under a
/// minute is `방금 전` and the hours between them are `N시간 전`, because
/// `1440분 전` is not a sentence anyone reads.
fn relative_label(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        "방금 전".to_owned()
    } else if seconds < 3_600 {
        format!("{}분 전", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}시간 전", seconds / 3_600)
    } else {
        format!("{}일 전", seconds / 86_400)
    }
}

/// The same scale for a `Duration`, which is what the session-relative
/// `마지막 업데이트 확인` caption counts in.
fn since_label(elapsed: Duration) -> String {
    relative_label(elapsed.as_secs() as i64)
}

/// `YYYY-MM-DD` out of an RFC 3339 timestamp (§8.8); `None` when the leading
/// ten characters are not a date.
fn date_label(stamp: &str) -> Option<String> {
    let head: String = stamp.chars().take(10).collect();
    let bytes = head.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    shaped.then_some(head)
}

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SS…`.
///
/// A trailing offset is **ignored, not applied**: providers send UTC, and an
/// hour of error could only ever move `3일 전` by an hour.
fn epoch_secs(stamp: &str) -> Option<i64> {
    let (date, rest) = stamp.split_once(['T', 't', ' '])?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut clock = rest.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let second: i64 = clock
        .next()
        .map(|part| part.trim_end_matches(|c: char| !c.is_ascii_digit()))
        .and_then(|part| part.split('.').next())
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days between 1970-01-01 and this civil date (Howard Hinnant's algorithm,
/// public domain). A calendar is the one thing a date crate would buy us, and
/// twelve lines is cheaper than a dependency.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// `1.21.1, 1.21, 1.20.6 외 4개` (§8.8).
fn game_versions_label(versions: &[String]) -> String {
    let head = versions
        .iter()
        .take(3)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    match versions.len().saturating_sub(3) {
        0 => head,
        rest => format!("{head} 외 {rest}개"),
    }
}

fn release_label(release: ReleaseType) -> &'static str {
    match release {
        ReleaseType::Release => "정식",
        ReleaseType::Beta => "베타",
        ReleaseType::Alpha => "알파",
    }
}

fn loader_name(loader: catalog::Loader) -> &'static str {
    match loader {
        catalog::Loader::Forge => "Forge",
        catalog::Loader::NeoForge => "NeoForge",
        catalog::Loader::Fabric => "Fabric",
    }
}

/// The trailing button of one `version-picker` row (§8.3). `current` is the
/// installed version's `(id, published)`; `None` means nothing is installed, so
/// every row simply installs.
fn version_action(
    version_id: &str,
    published: &str,
    current: Option<(&str, &str)>,
) -> (&'static str, bool) {
    let Some((current_id, current_published)) = current else {
        return ("설치", true);
    };
    if version_id == current_id {
        return ("설치됨", false);
    }
    // RFC 3339 in UTC sorts lexicographically, which is the whole reason the
    // providers send it that way.
    match published > current_published {
        true => ("교체", true),
        false => ("이전 버전으로 교체", true),
    }
}

/// §8.5's gate: the **trimmed** input must equal the directory name exactly,
/// case included.
fn delete_confirmed(input: &str, dir_name: &str) -> bool {
    !dir_name.is_empty() && input.trim() == dir_name
}

/// §8.4's automatic-retry line, which is both the Callout body and what the
/// progress row reads while the worker sleeps through the back-off.
fn rate_limit_line(wait: Duration) -> String {
    format!(
        "요청이 너무 많습니다 — {}초 후 자동으로 다시 시도합니다",
        wait.as_secs()
    )
}

/// `다운로드 중 2/5 · fabric-api-0.92.jar · 0:12` (§8.4).
fn progress_line(done: usize, total: usize, file: &str, elapsed: Duration) -> String {
    format!(
        "다운로드 중 {done}/{total} · {file} · {}",
        logsink::elapsed_mono(elapsed)
    )
}

/// The 찾기 error Callout, chosen by variant and never by message text (§8.4).
fn search_error(error: &CatalogError, provider: ProviderId) -> (String, String) {
    let name = match provider {
        ProviderId::Modrinth => "Modrinth",
        ProviderId::CurseForge => "CurseForge",
    };
    match error {
        CatalogError::RateLimited { retry_after_secs } => (
            format!("요청이 너무 많습니다 — {retry_after_secs}초 후 다시 시도하세요"),
            format!("{name}가 요청 수를 제한했습니다. 잠시 기다린 뒤 다시 시도하세요."),
        ),
        CatalogError::Network(message) if offline(message) => (
            "네트워크에 연결되어 있지 않습니다".to_owned(),
            "인터넷 연결을 확인한 뒤 다시 시도하세요.".to_owned(),
        ),
        CatalogError::Network(message) => (
            format!("{name}에 연결하지 못했습니다"),
            format!("{message} — 잠시 뒤 다시 시도하거나 다른 제공자를 고르세요"),
        ),
        CatalogError::NotFound => (
            "검색 결과가 없습니다".to_owned(),
            "다른 검색어나 제공자를 시도하세요.".to_owned(),
        ),
        other => failure_copy(other),
    }
}

/// Whether a transport failure reads as "there is no network at all". The
/// variant carries the only thing reqwest tells us, so this is a hint for the
/// copy and never a branch anything acts on.
fn offline(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["dns", "resolve", "could not resolve", "connection refused"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// The rollback Callout's file list: names only (the full paths are 폴더
/// 열기's job, not the card's — §8.4), capped at 3 with a `외 N개` overflow
/// tag so a long rollback never wraps mid-path in the 560 px card.
fn left_behind_summary(left_behind: &[PathBuf]) -> String {
    const SHOWN: usize = 3;
    let names = left_behind
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy());
    let shown = names.clone().take(SHOWN).collect::<Vec<_>>().join(", ");
    match names.count().saturating_sub(SHOWN) {
        0 => shown,
        extra => format!("{shown} 외 {extra}개"),
    }
}

/// Cause then fix, for every variant that is not the search box's business
/// (§5). A rolled-back install also says what, if anything, survived (§8.4).
fn failure_copy(error: &CatalogError) -> (String, String) {
    match error {
        CatalogError::RolledBack {
            source,
            left_behind,
        } => {
            let (_, body) = failure_copy(source);
            let tail = match left_behind.is_empty() {
                true => "설치된 파일 없음 — 변경 사항이 되돌려졌습니다".to_owned(),
                false => format!(
                    "일부 파일을 되돌리지 못했습니다: {}",
                    left_behind_summary(left_behind)
                ),
            };
            ("설치 실패".to_owned(), format!("{body} — {tail}"))
        }
        CatalogError::Locked { pid } => (
            "다른 작업이 이 서버를 사용 중입니다".to_owned(),
            format!("다른 Nebula 작업(pid {pid})이 끝난 뒤 다시 시도하세요."),
        ),
        CatalogError::HashMismatch { file, .. } => (
            "내려받은 파일이 손상되었습니다".to_owned(),
            format!("{file}의 해시가 맞지 않습니다 — 다시 시도하세요."),
        ),
        CatalogError::BadFileName(name) => (
            "파일 이름을 쓸 수 없습니다".to_owned(),
            format!("제공자가 보낸 이름 {name}은(는) 쓸 수 없습니다 — 다른 버전을 고르세요."),
        ),
        CatalogError::Unresolvable { dependency } => (
            "의존성을 찾을 수 없습니다".to_owned(),
            format!("{dependency} — 이 의존성 없이는 설치할 수 없습니다."),
        ),
        CatalogError::RateLimited { retry_after_secs } => (
            "요청이 너무 많습니다".to_owned(),
            format!("{retry_after_secs}초 후 다시 시도하세요."),
        ),
        CatalogError::Stale => (
            "설치 계획이 오래되었습니다".to_owned(),
            "다시 시도를 눌러 새로 확인하세요.".to_owned(),
        ),
        // §8.4's own sentence: another process wrote to this server while the
        // dialog was open, and the answer is a fresh search — not a re-plan.
        CatalogError::ServerChanged => (
            "서버가 바뀌었습니다 — 다시 검색하세요".to_owned(),
            "설치를 준비하는 동안 다른 작업이 이 서버의 모드를 바꿨습니다.".to_owned(),
        ),
        other => ("작업에 실패했습니다".to_owned(), other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_in_man_and_eok() {
        assert_eq!(count_label(0), "0");
        assert_eq!(count_label(9_999), "9999");
        assert_eq!(count_label(10_000), "1만");
        assert_eq!(count_label(12_000), "1.2만");
        assert_eq!(count_label(1_200_000), "120만");
        // Truncated, so the last number below 억 still says 만.
        assert_eq!(count_label(99_999_999), "9999만");
        assert_eq!(count_label(320_000_000), "3.2억");
        assert_eq!(count_label(1_200_000_000), "12억");
    }

    #[test]
    fn relative_time_covers_every_scale() {
        assert_eq!(relative_label(0), "방금 전");
        assert_eq!(relative_label(59), "방금 전");
        assert_eq!(relative_label(60), "1분 전");
        assert_eq!(relative_label(5 * 60), "5분 전");
        assert_eq!(relative_label(3_599), "59분 전");
        assert_eq!(relative_label(3_600), "1시간 전");
        assert_eq!(relative_label(86_399), "23시간 전");
        assert_eq!(relative_label(3 * 86_400), "3일 전");
        // A clock that moved backwards reads as "just now", never as a
        // negative age.
        assert_eq!(relative_label(-5), "방금 전");
    }

    #[test]
    fn dates_come_off_the_front_of_an_rfc3339_stamp() {
        assert_eq!(
            date_label("2025-03-03T12:34:56Z").as_deref(),
            Some("2025-03-03")
        );
        assert_eq!(date_label("2025-03-03").as_deref(), Some("2025-03-03"));
        assert_eq!(date_label(""), None);
        assert_eq!(date_label("not a date at all"), None);
        assert_eq!(date_label("2025/03/03T00:00:00Z"), None);
    }

    #[test]
    fn epoch_seconds_agree_with_known_instants() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_secs("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(epoch_secs("2025-03-03T12:34:56Z"), Some(1_741_005_296));
        // Fractional seconds and an offset are both tolerated.
        assert_eq!(
            epoch_secs("2025-03-03T12:34:56.789+00:00"),
            Some(1_741_005_296)
        );
        assert_eq!(epoch_secs("nonsense"), None);

        // …and that is what turns a stamp into `3일 전`.
        let now = epoch_secs("2025-03-06T12:34:56Z").unwrap();
        let then = epoch_secs("2025-03-03T12:34:56Z").unwrap();
        assert_eq!(relative_label(now - then), "3일 전");
    }

    #[test]
    fn sizes_reuse_the_sheets_own_formatter() {
        // §8.8's `12.3 MB`, from the one size formatter the app has.
        assert_eq!(size_label(12_897_485), "12.3 MB");
        assert_eq!(size_label(2_202_010), "2.1 MB");
    }

    #[test]
    fn game_versions_truncate_after_three() {
        let versions: Vec<String> = ["1.21.1", "1.21", "1.20.6", "1.20.4", "1.20.1", "1.19.2"]
            .iter()
            .map(|v| (*v).to_owned())
            .collect();
        assert_eq!(
            game_versions_label(&versions),
            "1.21.1, 1.21, 1.20.6 외 3개"
        );
        assert_eq!(game_versions_label(&versions[..3]), "1.21.1, 1.21, 1.20.6");
        assert_eq!(game_versions_label(&versions[..1]), "1.21.1");
        assert_eq!(game_versions_label(&[]), "");
    }

    #[test]
    fn the_version_row_action_follows_the_publication_date() {
        // Nothing installed: every row simply installs.
        assert_eq!(
            version_action("v2", "2025-03-03T00:00:00Z", None),
            ("설치", true)
        );

        let current = Some(("v1", "2025-02-01T00:00:00Z"));
        assert_eq!(
            version_action("v1", "2025-02-01T00:00:00Z", current),
            ("설치됨", false)
        );
        assert_eq!(
            version_action("v2", "2025-03-03T00:00:00Z", current),
            ("교체", true)
        );
        assert_eq!(
            version_action("v0", "2025-01-01T00:00:00Z", current),
            ("이전 버전으로 교체", true)
        );
        // Same instant, different id: not an upgrade, so the honest label is
        // the downgrade one rather than a silent 교체.
        assert_eq!(
            version_action("v9", "2025-02-01T00:00:00Z", current),
            ("이전 버전으로 교체", true)
        );
    }

    #[test]
    fn the_delete_gate_trims_and_stays_exact() {
        assert!(delete_confirmed("Test-1.20.1", "Test-1.20.1"));
        assert!(delete_confirmed("  Test-1.20.1\t", "Test-1.20.1"));
        assert!(!delete_confirmed("test-1.20.1", "Test-1.20.1"));
        assert!(!delete_confirmed("Test-1.20", "Test-1.20.1"));
        assert!(!delete_confirmed("", "Test-1.20.1"));
        // Nothing to confirm against is never a confirmation.
        assert!(!delete_confirmed("", ""));
        assert!(!delete_confirmed("   ", ""));
    }

    #[test]
    fn the_install_button_id_is_provider_scoped() {
        assert_eq!(
            install_a11y_id(ProviderId::Modrinth, "AANobbMI"),
            "mods-install-modrinth-AANobbMI"
        );
        assert_eq!(
            install_a11y_id(ProviderId::CurseForge, "394468"),
            "mods-install-curseforge-394468"
        );
    }

    #[test]
    fn the_progress_line_names_the_file_and_the_elapsed() {
        assert_eq!(
            progress_line(2, 5, "fabric-api-0.92.jar", Duration::from_secs(12)),
            "다운로드 중 2/5 · fabric-api-0.92.jar · 0:12"
        );
    }

    #[test]
    fn namespaces_round_trip_through_their_control_index() {
        for namespace in nebula_core::scan::NAMESPACES {
            assert_eq!(namespace_of(namespace_index(namespace)), namespace);
        }
        assert_eq!(namespace_label(Namespace::Required), "필수");
        assert_eq!(namespace_label(Namespace::OptionalOn), "선택 켜짐");
        assert_eq!(namespace_label(Namespace::OptionalOff), "선택 꺼짐");
        // An index the control can never produce still lands somewhere safe.
        assert_eq!(namespace_of(-1), Namespace::Required);
        assert_eq!(namespace_of(9), Namespace::Required);
    }

    #[test]
    fn dependencies_arrive_required_first() {
        use nebula_core::catalog::{DependencyKind, PlannedDep, PlannedFile, VersionFile};

        fn dep(id: &str, size: u64, kind: DependencyKind, installed: Option<&str>) -> PlannedDep {
            PlannedDep {
                file: PlannedFile {
                    provider: ProviderId::Modrinth,
                    project_id: id.to_owned(),
                    slug: None,
                    title: id.to_owned(),
                    text_provider: ProviderId::Modrinth,
                    version_id: "v".into(),
                    version_name: "1.0".into(),
                    version_number: "1.0".into(),
                    published: "2026-01-01T00:00:00Z".into(),
                    release: ReleaseType::Release,
                    file: VersionFile {
                        file_name: format!("{id}.jar"),
                        url: Some("https://cdn.invalid/a.jar".into()),
                        size,
                        primary: true,
                        sha1: None,
                        sha512: None,
                        md5: None,
                        fingerprint: None,
                        file_id: None,
                    },
                },
                kind,
                already_installed: installed.map(str::to_owned),
            }
        }

        let plan = InstallPlan {
            server: "Test-1.20.1".into(),
            index_sha1: String::new(),
            resolved_at: SystemTime::now(),
            root: dep("root", 10, DependencyKind::Required, None).file,
            required: vec![
                dep("fabric-api", 2_202_010, DependencyKind::Required, None),
                dep("cloth", 1, DependencyKind::Required, Some("cloth-1.jar")),
            ],
            optional: vec![dep("modmenu", 5, DependencyKind::Optional, None)],
            blocked: Vec::new(),
            incompatible: Vec::new(),
            unresolvable: Vec::new(),
        };

        let (rows, ids) = dep_rows(&plan);
        assert_eq!(ids, ["fabric-api", "cloth", "modmenu"]);
        // Required first, so the first row whose predecessor is required is
        // where the screen puts its 선택 의존성 toggle.
        assert_eq!(
            rows.iter().map(|row| row.required).collect::<Vec<_>>(),
            [true, true, false]
        );
        // A required dependency is pre-checked; an optional one is not.
        assert_eq!(
            rows.iter().map(|row| row.checked).collect::<Vec<_>>(),
            [true, true, false]
        );
        assert!(rows[1].already_installed);
        assert_eq!(rows[1].size_label, "", "an installed dep downloads nothing");
        assert_eq!(rows[0].size_label, "2.1 MB");
        assert!(rows[0].helper.contains("필수 의존성"));

        // And the two blocking states name what they refuse.
        let mut blocked = plan.clone();
        blocked.incompatible = vec!["Sodium".into()];
        let (title, body) = blocked_copy(&blocked);
        assert_eq!(title, "함께 설치할 수 없는 모드가 있습니다");
        assert!(body.contains("Sodium"), "{body}");

        let mut missing = plan;
        missing.unresolvable = vec!["fabric-api".into()];
        let (title, body) = blocked_copy(&missing);
        assert_eq!(title, "의존성을 찾을 수 없습니다");
        assert!(body.contains("fabric-api"), "{body}");
    }

    #[test]
    fn the_confirm_line_reads_number_type_size_date() {
        use nebula_core::catalog::VersionFile;

        let mut file = PlannedFile {
            provider: ProviderId::Modrinth,
            project_id: "P7dR8mSH".into(),
            slug: Some("fabric-api".into()),
            title: "fabric-api".into(),
            text_provider: ProviderId::Modrinth,
            version_id: "xhLT3C5f".into(),
            // What D1 is about: Modrinth's version *name* repeats the title.
            version_name: "fabric-api 0.92.0 for Minecraft 1.20.1".into(),
            version_number: "0.92.0".into(),
            published: "2025-03-03T12:00:00Z".into(),
            release: ReleaseType::Release,
            file: VersionFile {
                file_name: "fabric-api-0.92.0.jar".into(),
                url: Some("https://cdn.invalid/a.jar".into()),
                size: 2_202_010,
                primary: true,
                sha1: None,
                sha512: None,
                md5: None,
                fingerprint: None,
                file_id: None,
            },
        };
        assert_eq!(
            confirm_version_line(&file),
            "fabric-api 0.92.0 · 정식 · 2.1 MB · 2025-03-03"
        );

        // No date from the provider: the line stops at the size rather than
        // ending on a separator.
        file.published = String::new();
        assert_eq!(
            confirm_version_line(&file),
            "fabric-api 0.92.0 · 정식 · 2.1 MB"
        );

        // A CurseForge file with no id numbers itself with nothing; the name
        // is then the only version text there is.
        file.version_number = String::new();
        assert_eq!(
            version_text(&file),
            "fabric-api 0.92.0 for Minecraft 1.20.1"
        );
    }

    #[test]
    fn a_partial_update_keeps_only_what_failed() {
        let results = vec![
            ("sodium.jar".to_owned(), Ok(())),
            (
                "lithium.jar".to_owned(),
                Err(CatalogError::HashMismatch {
                    file: "lithium.jar".into(),
                    expected: "a".into(),
                    got: "b".into(),
                }),
            ),
        ];
        let names = failed_names(&results);
        assert_eq!(names.len(), 1);
        assert!(names.contains("lithium.jar"));

        // A clean run leaves nothing to retain, which is what clears the list.
        let clean = vec![("sodium.jar".to_owned(), Ok(()))];
        assert!(failed_names(&clean).is_empty());
    }

    #[test]
    fn the_check_toasts_fold_when_the_stack_is_full() {
        assert_eq!(check_toasts(0, 0, 0), ["모두 최신입니다"]);
        assert_eq!(check_toasts(0, 3, 0), ["업데이트 3개 있음"]);
        // Room for both: the adoption line leads (§8.4).
        assert_eq!(
            check_toasts(2, 3, 1),
            [
                "직접 추가 2개를 관리 목록에 연결했습니다",
                "업데이트 3개 있음"
            ]
        );
        // Two more would push an older toast off the three-deep stack, so the
        // two messages share one line instead of one of them being lost.
        assert_eq!(
            check_toasts(2, 0, 2),
            ["직접 추가 2개를 관리 목록에 연결했습니다 · 모두 최신입니다"]
        );
    }

    #[test]
    fn the_hit_caption_puts_the_date_first_when_sorting_by_newest() {
        let hit = ProjectHit {
            provider: ProviderId::Modrinth,
            project_id: "AANobbMI".into(),
            slug: "sodium".into(),
            title: "Sodium".into(),
            author: Some("jellysquid3".into()),
            description: "렌더링 최적화".into(),
            downloads: 1_200_000,
            updated: Some("2025-03-03T00:00:00Z".into()),
            icon_url: None,
            page_url: None,
        };
        let now = epoch_secs("2025-03-06T00:00:00Z").unwrap();
        assert_eq!(
            hit_caption(&hit, false, now),
            "jellysquid3 · 다운로드 120만 · 수정 3일 전"
        );
        assert_eq!(
            hit_caption(&hit, true, now),
            "수정 3일 전 · jellysquid3 · 다운로드 120만"
        );

        // No author and no date: the caption still says something true.
        let bare = ProjectHit {
            author: None,
            updated: None,
            downloads: 42,
            ..hit
        };
        assert_eq!(hit_caption(&bare, false, now), "작성자 미상 · 다운로드 42");
    }

    #[test]
    fn the_search_error_is_chosen_by_variant() {
        let (title, _) = search_error(
            &CatalogError::RateLimited {
                retry_after_secs: 12,
            },
            ProviderId::Modrinth,
        );
        assert_eq!(title, "요청이 너무 많습니다 — 12초 후 다시 시도하세요");

        let (title, _) = search_error(
            &CatalogError::Network("failed to lookup address information".into()),
            ProviderId::Modrinth,
        );
        assert_eq!(title, "Modrinth에 연결하지 못했습니다");

        let (title, _) = search_error(
            &CatalogError::Network("could not resolve host".into()),
            ProviderId::CurseForge,
        );
        assert_eq!(title, "네트워크에 연결되어 있지 않습니다");
    }

    #[test]
    fn a_rollback_says_whether_anything_survived() {
        let clean = CatalogError::RolledBack {
            source: Box::new(CatalogError::HashMismatch {
                file: "sodium.jar".into(),
                expected: "a".into(),
                got: "b".into(),
            }),
            left_behind: Vec::new(),
        };
        let (title, body) = failure_copy(&clean);
        assert_eq!(title, "설치 실패");
        assert!(body.contains("설치된 파일 없음"), "{body}");
        assert!(body.contains("sodium.jar"), "{body}");

        let dirty = CatalogError::RolledBack {
            source: Box::new(CatalogError::Stale),
            left_behind: vec![PathBuf::from("D:/root/servers/Test-1.20.1/mods/a.jar")],
        };
        let (_, body) = failure_copy(&dirty);
        assert!(body.contains("되돌리지 못했습니다"), "{body}");
        assert!(body.contains("a.jar"), "{body}");
    }

    #[test]
    fn left_behind_summary_shows_names_not_paths_capped_at_three() {
        let few = vec![
            PathBuf::from("D:/root/servers/Test/mods/a.jar"),
            PathBuf::from("D:/root/servers/Test/mods/b.jar"),
        ];
        assert_eq!(left_behind_summary(&few), "a.jar, b.jar");

        let many = vec![
            PathBuf::from("D:/root/servers/Test/mods/a.jar"),
            PathBuf::from("D:/root/servers/Test/mods/b.jar"),
            PathBuf::from("D:/root/servers/Test/mods/c.jar"),
            PathBuf::from("D:/root/servers/Test/mods/d.jar"),
            PathBuf::from("D:/root/servers/Test/mods/e.jar"),
        ];
        assert_eq!(left_behind_summary(&many), "a.jar, b.jar, c.jar 외 2개");
    }
}
