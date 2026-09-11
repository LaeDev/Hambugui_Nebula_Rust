//! The GUI, wired to `nebula-core`.
//!
//! Two rules hold the whole module together:
//!
//! * **Slint models are touched on the event-loop thread only.** Every job runs
//!   on a `std::thread` and comes back through [`slint::invoke_from_event_loop`],
//!   which is also why the shared [`Ui`] lives in a thread-local instead of
//!   being captured by worker closures.
//! * **The app never claims more than it knows.** The verdict banner comes from
//!   a real [`validate::Report`], the counts from a real scan, and `busy`
//!   clears only after the worker's result has been applied.

mod logsink;
mod mods;
pub(crate) mod settings;

/// The CLI's per-user state lives here too: one definition, one directory, one
/// `NEBULA_CONFIG_DIR` override.
pub(crate) use settings::config_dir;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use nebula_core::publish::{PublishOptions, Target, gdrive};
use nebula_core::validate::{Report, Severity};
use nebula_core::version::{MinecraftVersion, Promotion};
use nebula_core::{
    Config, CreateServerOptions, Error, GenerateOptions, ImportFormat, Result, Root, ServerSummary,
    distribution, hash, import, publish, remote, server, validate,
};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};

use settings::GuiSettings;

slint::include_modules!();

const LOG_DRAIN_INTERVAL: Duration = Duration::from_millis(100);

/// How long a toast stays up (DESIGN-v2 §3), and how many are stacked.
const TOAST_LIFETIME: Duration = Duration::from_secs(4);
const MAX_TOASTS: usize = 3;

/// How far the indeterminate progress segment travels per tick: a full sweep
/// in 1.5 s. The bar is driven from here rather than by a Slint `animate` loop
/// because the software renderer only ever draws a loop's first frame into a
/// screenshot, which would make every busy state unreviewable.
const PHASE_STEP: f32 = 1.0 / 15.0;

/// Where the segment is parked when motion is reduced: visible, but still.
const PHASE_STILL: f32 = 0.35;

/// How long a field must stand still before the tick acts on it. Long enough
/// that typing a file name does not read the disk once per keystroke.
const SETTLE: Duration = Duration::from_millis(300);

// The three field errors both the debounced pass and 저장 can raise. Written
// once: two copies of a sentence drift, and the user sees the drift as two
// different problems.
const ROOT_MISSING: &str = "폴더가 없습니다 — 폴더 만들기를 누르거나 경로를 고치세요";
const DIR_MISSING: &str = "폴더가 없습니다 — 경로를 확인하세요";
const FILE_MISSING: &str = "파일이 없습니다 — 경로를 확인하세요";

pub fn run(level: log::LevelFilter) -> Result<()> {
    // `Config::from_env` loads `.env` into the process environment on its first
    // call, which is only sound while this is still the one thread. Every later
    // call is a no-op, `GuiSettings::load`'s seeding included.
    let _ = Config::from_env();

    let log_rx = logsink::install(level);

    let stored = GuiSettings::load();
    let app =
        App::new().map_err(|error| Error::Config(format!("GUI를 시작하지 못했습니다: {error}")))?;

    let ui = Rc::new(Ui::new(&app, stored));
    UI.with(|slot| slot.replace(Some(ui.clone())));

    bind_models(&ui, &app);
    apply_settings_to_ui(&app, &ui.settings.borrow());
    // **No scan before the window.** Reading a root with a few hundred mods
    // takes long enough to be a black screen, and a scan that blocks here also
    // has nowhere to report a failure to. The sheet opens saying 불러오는 중 and
    // the first tick runs the scan as an ordinary job; `apply_snapshot` is what
    // turns 불러오는 중 off.
    ui.apply_loading();
    wire(&ui, &app);

    // DESIGN-v2 §1: 720×960 by default, 640×800 at the smallest. Only the
    // minimum reaches the window's constraints — a `Window`'s own
    // `preferred-width` does not — so the default size is set here, once,
    // before the window is shown. The window stays resizable, which a literal
    // `width` in the .slint would not allow.
    app.window().set_size(slint::LogicalSize::new(720.0, 960.0));

    // Dropping the timer stops the log band with no diagnostic, so it outlives
    // the event loop.
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, LOG_DRAIN_INTERVAL, {
        let ui = ui.clone();
        move || tick(&ui, &log_rx)
    });

    log::info!("창을 띄웁니다: {} ms", ui.launched.elapsed().as_millis());
    app.run()
        .map_err(|error| Error::Config(format!("GUI가 중단되었습니다: {error}")))
}

// ── Shared UI state ─────────────────────────────────────────────────────────

thread_local! {
    /// Reachable from an `invoke_from_event_loop` closure, which cannot carry an
    /// `Rc`. Only ever written once, at startup.
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

fn with_ui(body: impl FnOnce(&Rc<Ui>)) {
    UI.with(|slot| {
        if let Some(ui) = slot.borrow().as_ref() {
            body(ui);
        }
    });
}

/// What the primary action and the upload button are allowed to be.
#[derive(Clone, Copy, Default)]
struct Gates {
    root_ok: bool,
    base_ok: bool,
    dist_exists: bool,
}

/// Which screen's primary shows the spinner. `SheetData.busy` stays the global
/// job lock in every case: it is what disables the actions that start a job.
/// Google 계정 연결 is deliberately absent: it waits on a browser rather than on
/// this machine, so it runs outside the global lock and gates only its own
/// button (`SettingsData.gdrive-busy`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Sheet,
    NewServer,
    Lookup,
    Import,
    Publish,
    Settings,
    /// The 모드 page's own mutating jobs (DESIGN-v2 §8.6). It has no per-screen
    /// spinner — the top-bar pill and one `ProgressRow` carry every one of them
    /// — so this arm only keeps a mods job from lighting another page up.
    Mods,
}

/// A job the sheet can re-run from its failure box with the current field
/// values. Uploads are not here: the 배포 band reports its own failures next to
/// the 업로드 button that repeats them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Generate,
    Validate,
    InitRoot,
}

impl JobKind {
    /// `SheetData.error-kind`; the build page turns it into the failure prefix.
    fn tag(self) -> &'static str {
        match self {
            JobKind::Generate => "generate",
            JobKind::Validate => "validate",
            JobKind::InitRoot => "init-root",
        }
    }

    /// What the toast says when this job fails (DESIGN-v2 §5).
    fn failure_toast(self) -> &'static str {
        match self {
            JobKind::Generate => "생성 실패",
            JobKind::Validate => "검증 실패",
            JobKind::InitRoot => "초기화 실패",
        }
    }
}

/// A one-shot delay driven by the 100 ms tick, so nothing here needs a second
/// timer or a thread.
#[derive(Default)]
struct Debounce(Cell<Option<Instant>>);

impl Debounce {
    fn arm(&self) {
        self.0.set(Some(Instant::now()));
    }

    fn cancel(&self) {
        self.0.set(None);
    }

    /// True once, `after` has passed since the last `arm`.
    fn due(&self, after: Duration) -> bool {
        match self.0.get() {
            Some(armed) if armed.elapsed() >= after => {
                self.0.set(None);
                true
            }
            _ => false,
        }
    }
}

struct Ui {
    app: slint::Weak<App>,
    settings: RefCell<GuiSettings>,
    busy: Cell<bool>,
    /// Google 계정 연결's own lock. Separate from `busy` so waiting on a browser
    /// does not stop a build, and still enough to refuse a second connect.
    gdrive_busy: Cell<bool>,
    /// Set until the first tick has started the opening scan.
    first_scan: Cell<bool>,
    /// When the process reached [`run`]; the startup lines are measured off it.
    launched: Instant,
    gates: Cell<Gates>,
    logs: Rc<VecModel<LogRowData>>,
    readiness: Rc<VecModel<ReadinessRowData>>,
    servers: Rc<VecModel<ServerRowData>>,
    /// Parallel to `servers`: each row's own icon, or an empty image. Separate
    /// from `ServerRowData` so the state files can still be rendered by
    /// `slint-viewer --load-data`, which cannot write an empty image.
    server_icons: Rc<VecModel<Image>>,
    toasts: Rc<VecModel<ToastData>>,
    /// Toasts are addressed by identity, never by position: the dismissal timer
    /// fires four seconds late, by which time an index means another toast.
    next_toast: Cell<i32>,
    findings: Rc<VecModel<FindingRowData>>,
    imports: Rc<VecModel<ImportRowData>>,
    manual: Rc<VecModel<ManualDownloadData>>,
    /// Parallel to `imports`: the row index the UI reports is an index here.
    import_paths: RefCell<Vec<PathBuf>>,
    /// The last `distro-name` a `dist_exists` check was made for.
    checked_name: RefCell<String>,
    /// Whether the **strict** validation of `<name>.json` passed, per file
    /// name. This, not the displayed report, is what the stamp and the upload
    /// button are gated on.
    strict_ok: RefCell<HashMap<String, bool>>,
    /// When the running job started, and what to call it: the busy eyebrow
    /// counts from here.
    job_started: Cell<Option<Instant>>,
    job_label: RefCell<String>,
    /// Last elapsed second written to `busy-label`; Slint marks a property
    /// dirty on every `set`, so the tick only writes when the second changes.
    shown_secs: Cell<u64>,
    /// The last job that failed, so 다시 시도 re-runs that one.
    job: Cell<Option<JobKind>>,
    name_check: Debounce,
    settings_check: Debounce,
    /// Slint marks a property dirty on every `set`, so the 100 ms tick only
    /// writes this one when it actually flips.
    forge_warning: Cell<bool>,
    /// Everything the 모드 page owns (DESIGN-v2 §8), in one field so the
    /// catalog's models and sequence counters stay out of the sheet's way.
    mods: mods::ModsState,
}

impl Ui {
    fn new(app: &App, stored: GuiSettings) -> Self {
        let mods = mods::ModsState::new(&stored);
        Self {
            app: app.as_weak(),
            settings: RefCell::new(stored),
            busy: Cell::new(false),
            gdrive_busy: Cell::new(false),
            first_scan: Cell::new(true),
            launched: Instant::now(),
            gates: Cell::new(Gates::default()),
            logs: Rc::new(VecModel::default()),
            readiness: Rc::new(VecModel::default()),
            servers: Rc::new(VecModel::default()),
            server_icons: Rc::new(VecModel::default()),
            toasts: Rc::new(VecModel::default()),
            next_toast: Cell::new(1),
            findings: Rc::new(VecModel::default()),
            imports: Rc::new(VecModel::default()),
            manual: Rc::new(VecModel::default()),
            import_paths: RefCell::new(Vec::new()),
            checked_name: RefCell::new(String::new()),
            strict_ok: RefCell::new(HashMap::new()),
            job_started: Cell::new(None),
            job_label: RefCell::new(String::new()),
            shown_secs: Cell::new(u64::MAX),
            job: Cell::new(None),
            name_check: Debounce::default(),
            settings_check: Debounce::default(),
            forge_warning: Cell::new(false),
            mods,
        }
    }

    /// `SheetData.busy` is the lock every screen reads to disable the actions
    /// that start a job; the per-screen flag only says which primary spins, so
    /// a build running on the sheet cannot make the import screen look busy.
    fn set_busy(&self, busy: bool, label: &str, screen: Screen) {
        self.busy.set(busy);
        self.job_started.set(busy.then(Instant::now));
        self.job_label.replace(label.to_owned());
        // Forces the next tick to write the pill.
        self.shown_secs.set(u64::MAX);
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sheet = app.global::<SheetData>();
        sheet.set_busy(busy);
        sheet.set_busy_label(match busy {
            true => busy_pill(label, Duration::ZERO).into(),
            false => SharedString::new(),
        });
        let new_server = app.global::<NewServerData>();
        new_server.set_busy(busy && screen == Screen::NewServer);
        new_server.set_lookup_busy(busy && screen == Screen::Lookup);
        app.global::<ImportData>()
            .set_busy(busy && screen == Screen::Import);
        app.global::<PublishData>()
            .set_busy(busy && screen == Screen::Publish);
        // Not `gdrive-busy`: that flag belongs to a job this lock never held.
        app.global::<SettingsData>()
            .set_busy(busy && screen == Screen::Settings);
        self.apply_gates();
    }

    /// Google 계정 연결's own lock, raised and lowered on the event-loop thread
    /// like every other property write.
    fn set_gdrive_busy(&self, busy: bool) {
        self.gdrive_busy.set(busy);
        if let Some(app) = self.app.upgrade() {
            app.global::<SettingsData>().set_gdrive_busy(busy);
        }
    }

    fn apply_gates(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let gates = self.gates.get();
        let live = !self.busy.get();
        app.global::<SheetData>()
            .set_can_generate(gates.root_ok && gates.base_ok && live);

        // A file the launcher would reject is not publishable, however clean
        // the displayed report looks. The band's own binding adds `target`.
        let strict = self.strict_for(&self.checked_name.borrow());
        let publish = app.global::<PublishData>();
        publish.set_can_publish(gates.dist_exists && strict && live);
        publish.set_blocked_reason(match (gates.dist_exists, strict) {
            (false, _) => "먼저 배포본을 생성하세요".into(),
            (true, false) => {
                "http:// URL 또는 SHA-256 누락이 있어 런처가 거부합니다. 오류 목록을 확인하세요"
                    .into()
            }
            (true, true) => SharedString::new(),
        });
    }

    fn strict_for(&self, name: &str) -> bool {
        self.strict_ok
            .borrow()
            .get(name.trim())
            .copied()
            .unwrap_or(false)
    }

    /// Re-reads the launcher gate for the file on disk, on a thread of its own.
    /// Silent: no busy window, no log line, and never on a keystroke — the tick
    /// debounces it, so this only runs when a file name has settled. Reading
    /// and validating a large `distribution.json` is tens of milliseconds, and
    /// the event loop cannot afford them on every settle.
    ///
    /// The answer is dropped unless the field still names the same file, in the
    /// same root, at the same mtime it had when the read started: three ways a
    /// slow probe could otherwise stamp a verdict on a file nobody validated.
    fn refresh_strict(&self, name: &str) {
        let root = self.settings.borrow().root.clone();
        let name = name.trim().to_owned();
        let Some(path) = distribution_path(&root, &name) else {
            self.strict_ok.borrow_mut().insert(name, false);
            self.apply_gates();
            return;
        };
        let before = modified(&path);
        spawn_quiet(
            move || {
                let ok = std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| validate::validate_json(&bytes, true).ok())
                    .is_some_and(|report| report.is_ok());
                (root, name, ok, before, path)
            },
            |ui, (root, name, ok, before, path)| {
                let stale = *ui.checked_name.borrow() != name
                    || ui.settings.borrow().root != root
                    || modified(&path) != before;
                if stale {
                    return;
                }
                ui.strict_ok.borrow_mut().insert(name, ok);
                ui.apply_gates();
            },
        );
    }

    /// The honest opening state: the settings are known, what is on disk is
    /// not. Every list is empty and says so, rather than claiming there is
    /// nothing there.
    fn apply_loading(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let current = self.settings.borrow();
        self.readiness.set_vec(
            [
                ("루트", &current.root),
                ("기본 URL", &current.base_url),
                ("Java", &current.java_executable),
                ("Launcher 폴더", &current.helios_data_folder),
            ]
            .map(|(label, value)| ReadinessRowData {
                label: label.into(),
                value: match value.trim().is_empty() {
                    true => "미설정".into(),
                    false => value.as_str().into(),
                },
                state: "loading".into(),
            })
            .to_vec(),
        );
        app.global::<SheetData>().set_loading(true);
    }

    /// Takes `Rc<Self>` because the 모드 page's own applier needs one; every
    /// caller already holds it.
    fn apply_snapshot(self: &Rc<Self>, snap: Snapshot) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sheet = app.global::<SheetData>();
        let told_before = sheet.get_servers_error();
        // The 모드 page's pill strip is fed from the same rows, but only when
        // they are real: a failed scan knows nothing about the servers.
        let mut servers_changed = false;
        self.readiness.set_vec(snap.readiness);
        match &snap.servers_error {
            // A scan that failed knows nothing about the servers, so the rows
            // already on screen — from the last scan that did read — are still
            // the best answer there is. Replacing them with an empty list is
            // how "the scan broke" becomes "you have no servers".
            Some(error) => sheet.set_servers_error(error.as_str().into()),
            None => {
                sheet.set_servers_error(SharedString::new());
                servers_changed = true;
                self.servers.set_vec(snap.servers);
                // `Image` is not `Send`, so the worker thread only carries paths
                // and the decode happens here, on the event-loop thread.
                self.server_icons.set_vec(
                    snap.server_icons
                        .iter()
                        .map(|path| {
                            path.as_deref()
                                .and_then(|path| Image::load_from_path(path).ok())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }

        // The row index means nothing across a refresh — a new file sorts in
        // above the selected one and the selection would silently become a
        // different modpack — so the selection travels as a path.
        let import_data = app.global::<ImportData>();
        let selected = usize::try_from(import_data.get_selected_zip())
            .ok()
            .and_then(|index| self.import_paths.borrow().get(index).cloned());
        self.imports.set_vec(snap.imports);
        import_data.set_selected_zip(
            selected
                .and_then(|path| snap.import_paths.iter().position(|other| *other == path))
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
        );
        self.import_paths.replace(snap.import_paths);

        sheet.set_has_root(snap.root_ok);
        sheet.set_root_initialised(snap.root_initialised);
        sheet.set_install_local_enabled(snap.install_local_enabled);
        if !snap.install_local_enabled {
            // A checked-but-disabled toggle would promise an install that
            // `generate_distribution` refuses.
            sheet.set_install_local(false);
        }
        import_data.set_folder_path(snap.modpacks_dir.into());
        app.global::<SettingsData>()
            .set_gdrive_connected(snap.gdrive_connected);

        self.gates.set(Gates {
            root_ok: snap.root_ok,
            base_ok: snap.base_ok,
            dist_exists: snap.dist_exists,
        });
        self.checked_name.replace(snap.checked_name);
        // A new root means a different file under the same name.
        let name = self.checked_name.borrow().clone();
        self.refresh_strict(&name);
        self.apply_gates();

        if sheet.get_loading() {
            sheet.set_loading(false);
            // Read back off the bound models, so the first line says what the
            // sheet is actually showing rather than what was computed for it.
            log::info!(
                "루트 {} · 서버 {}개 · 모드팩 {}개",
                match snap.root_ok {
                    true => self.settings.borrow().root.clone(),
                    false => "미설정".to_owned(),
                },
                self.servers.row_count(),
                self.imports.row_count()
            );
            log::info!("첫 스캔 반영: {} ms", self.launched.elapsed().as_millis());
        }
        // The callout stays up for as long as the scan keeps failing; the toast
        // is for the moment it starts failing. Every page change refreshes, so
        // repeating it would put the same notice up four times a minute.
        if let Some(error) = snap.servers_error
            && told_before != error
        {
            log::error!("{error}");
            self.toast(Tone::Danger, "서버 목록을 읽지 못했습니다");
        }
        // Last, so the 모드 page's fallback toast (§8.2) lands after the
        // scan's own and the pill strip agrees with the 서버 list beside it.
        if servers_changed {
            let names = self.servers.iter().map(|row| row.id.to_string()).collect();
            mods::apply_servers(self, names);
        }
    }

    /// Wipes every claim about the previous run. Called before a job starts, so
    /// a failure can never leave the old verdict on screen.
    fn clear_result(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sheet = app.global::<SheetData>();
        sheet.set_summary(SharedString::new());
        sheet.set_error_text(SharedString::new());
        sheet.set_error_kind(SharedString::new());
        sheet.set_error_hint(SharedString::new());
        self.clear_verdict();
        self.findings.set_vec(Vec::new());
        // Six findings from a different file are a different list; leaving it
        // expanded would show the new one already unfolded for no reason.
        sheet.set_findings_expanded(false);
        // An upload result describes the file that was on disk when it ran; a
        // new build makes it a claim about a file nobody uploaded.
        let publish = app.global::<PublishData>();
        publish.set_result_line(SharedString::new());
        publish.set_distribution_url(SharedString::new());
        publish.set_result_error(false);
    }

    /// The verdict banner names one file; a different file name makes it a
    /// claim about a file nobody validated. An empty `verdict-title` is what
    /// hides the banner.
    fn clear_verdict(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sheet = app.global::<SheetData>();
        sheet.set_verdict_title(SharedString::new());
        sheet.set_verdict_detail(SharedString::new());
        sheet.set_verdict_tone(Tone::Success);
        sheet.set_artifacts(0);
        sheet.set_sha256_count(0);
        sheet.set_errors(0);
        sheet.set_warnings(0);
    }

    /// `build.slint` renders the prefix from `error-kind`, so it stays out of
    /// the message, and 다시 시도 re-runs `kind`.
    fn fail(&self, kind: JobKind, message: &str) {
        log::error!("{message}");
        self.clear_result();
        self.job.set(Some(kind));
        if let Some(app) = self.app.upgrade() {
            let sheet = app.global::<SheetData>();
            sheet.set_error_kind(kind.tag().into());
            sheet.set_error_text(message.into());
            sheet.set_error_hint(cache_hint(message).into());
        }
        self.toast(Tone::Danger, kind.failure_toast());
    }

    /// Raises a toast and schedules its own dismissal. The timer closes over the
    /// toast's id, never its index: four seconds later the row may have moved.
    fn toast(&self, tone: Tone, message: &str) {
        let id = self.next_toast.get();
        self.next_toast.set(id + 1);
        self.toasts.push(ToastData {
            id,
            tone,
            message: message.into(),
        });
        while self.toasts.row_count() > MAX_TOASTS {
            self.toasts.remove(0);
        }
        slint::Timer::single_shot(TOAST_LIFETIME, move || {
            with_ui(|ui| ui.dismiss_toast(id));
        });
    }

    fn dismiss_toast(&self, id: i32) {
        if let Some(index) = self.toasts.iter().position(|toast| toast.id == id) {
            self.toasts.remove(index);
        }
    }

    fn apply_verdict(&self, verdict: Verdict) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let sheet = app.global::<SheetData>();
        sheet.set_error_text(SharedString::new());
        sheet.set_error_kind(SharedString::new());
        sheet.set_error_hint(SharedString::new());
        sheet.set_summary(summary_line(&verdict.report, verdict.strict_ok).into());
        self.findings
            .set_vec(finding_rows(&verdict.report, &verdict.server_ids));

        let tone = verdict_tone(&verdict.report, verdict.strict_ok);
        sheet.set_verdict_tone(tone);
        sheet.set_verdict_title(verdict_title(tone).into());
        sheet.set_verdict_detail(
            verdict_detail(&verdict.path, &verdict.sha256, verdict.report.artifacts).into(),
        );
        sheet.set_artifacts(verdict.report.artifacts as i32);
        sheet.set_sha256_count(verdict.report.sha256 as i32);
        sheet.set_errors(verdict.report.errors() as i32);
        sheet.set_warnings(verdict.report.warnings() as i32);
        self.toast(tone, verdict_toast(tone));

        // Keyed by the field value the run was started with, which is what
        // every lookup uses. Deriving it back out of the path instead would be
        // a second, subtly different, key derivation.
        self.strict_ok
            .borrow_mut()
            .insert(verdict.name.clone(), verdict.strict_ok);
        let mut gates = self.gates.get();
        gates.dist_exists = verdict.path.is_file();
        self.gates.set(gates);
        self.apply_gates();
    }

    fn publish_result(&self, line: String, url: String, failed: bool) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let data = app.global::<PublishData>();
        data.set_result_line(line.into());
        data.set_distribution_url(url.into());
        data.set_result_error(failed);
    }
}

// ── Jobs ────────────────────────────────────────────────────────────────────

/// Runs `work` off the event loop and hands the outcome to `apply` back on it.
///
/// One job at a time: a second request while `busy` is dropped with a log line
/// rather than queued, which matches what the disabled controls promise.
/// Returns whether the job actually started, so a caller can only raise its own
/// spinner for a job that exists.
fn spawn_job<T, W, A>(ui: &Rc<Ui>, label: &str, screen: Screen, work: W, apply: A) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> Result<T> + Send + 'static,
    A: FnOnce(&Rc<Ui>, Result<T>) + Send + 'static,
{
    if ui.busy.get() {
        log::info!("이미 다른 작업이 실행 중입니다. 끝난 뒤에 다시 시도하세요.");
        return false;
    }
    ui.set_busy(true, label, screen);
    std::thread::spawn(move || {
        let outcome = work();
        // If the window is gone there is nobody to report to, and the process
        // is on its way out anyway.
        let _ = slint::invoke_from_event_loop(move || {
            with_ui(|ui| {
                apply(ui, outcome);
                // Never before the result is on screen.
                ui.set_busy(false, "", screen);
            });
        });
    });
    true
}

/// Runs `work` off the event loop with no busy window, no lock and no log
/// line, and hands the outcome back on it.
///
/// For the two probes that must never look like a job: the strict
/// re-validation and the settings form's filesystem checks. Both are triggered
/// by a debounce rather than by a click, so a spinner would flicker on typing,
/// and both are cheap enough that several in flight cost nothing — the applier
/// is what decides whether a late answer is still true.
fn spawn_quiet<T, W, A>(work: W, apply: A)
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    A: FnOnce(&Rc<Ui>, T) + Send + 'static,
{
    std::thread::spawn(move || {
        let outcome = work();
        let _ = slint::invoke_from_event_loop(move || with_ui(|ui| apply(ui, outcome)));
    });
}

/// The file's modification time, or `None` when it cannot be read. Used only to
/// notice that a file changed under a probe.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// The busy pill (DESIGN-v2 §5): `배포본 생성 중 · 1:23`.
fn busy_pill(label: &str, elapsed: Duration) -> String {
    format!("{label} 중 · {}", logsink::elapsed_mono(elapsed))
}

/// A finished validation, whether it came from a fresh build or a stored file.
struct Verdict {
    /// The trimmed file-name field the run started with — the key `strict_ok`
    /// is stored under, and the one every gate looks it up by.
    name: String,
    sha256: String,
    /// What the findings list shows: a plain `http://` URL and a missing
    /// SHA-256 are warnings here, because the file is still readable.
    report: Report,
    /// The launcher's own contract, where those two are errors. The stamp and
    /// the upload button are gated on this and never on `report`.
    strict_ok: bool,
    path: PathBuf,
    /// So a finding can name a server instead of an index.
    server_ids: Vec<String>,
}

fn apply_verdict_outcome(ui: &Rc<Ui>, kind: JobKind, outcome: Result<Verdict>) {
    match outcome {
        Ok(verdict) => ui.apply_verdict(verdict),
        Err(error) => ui.fail(kind, &error.to_string()),
    }
}

/// Re-reads the root. Used on its own for `refresh`, and folded into every job
/// that changes what is on disk so there is only ever one busy window.
fn spawn_refresh(ui: &Rc<Ui>, label: &str) {
    let Some(app) = ui.app.upgrade() else { return };
    let current = ui.settings.borrow().clone();
    let name = app.global::<SheetData>().get_distro_name().to_string();
    spawn_job(
        ui,
        label,
        Screen::Sheet,
        move || Ok(snapshot(&current, &name)),
        |ui, outcome| {
            if let Ok(snap) = outcome {
                ui.apply_snapshot(snap);
            }
        },
    );
}

// ── Job starters ────────────────────────────────────────────────────────────
//
// Each reads the current field values off the globals, so 다시 시도 re-runs the
// same job with whatever is on screen now rather than replaying the old
// arguments.

fn start_generate(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let sheet = app.global::<SheetData>();
    let cfg = match ui.settings.borrow().to_config() {
        Ok(cfg) => cfg,
        Err(error) => return ui.fail(JobKind::Generate, &error.to_string()),
    };
    let name = sheet.get_distro_name().trim().to_owned();
    let opts = GenerateOptions {
        name: name.clone(),
        install_local: sheet.get_install_local(),
        discard_output: sheet.get_discard_output(),
        invalidate_cache: sheet.get_invalidate_cache(),
        helios_untracked: false,
        // There is a window on screen, so the installer may open its own.
        gui_fallback: true,
    };
    ui.clear_result();
    spawn_job(
        ui,
        "배포본 생성",
        Screen::Sheet,
        move || {
            let result = distribution::generate_distribution(&cfg, &opts)?;
            Ok(Verdict {
                name,
                sha256: result.sha256,
                strict_ok: validate::validate(&result.distribution, true)?.is_ok(),
                report: validate::validate(&result.distribution, false)?,
                server_ids: result
                    .distribution
                    .servers
                    .iter()
                    .map(|server| server.id.clone())
                    .collect(),
                path: result.path,
            })
        },
        |ui, outcome| apply_verdict_outcome(ui, JobKind::Generate, outcome),
    );
}

fn start_validate(ui: &Rc<Ui>) {
    let Some(app) = ui.app.upgrade() else { return };
    let name = app
        .global::<SheetData>()
        .get_distro_name()
        .trim()
        .to_owned();
    let root = ui.settings.borrow().root.clone();
    let Some(path) = distribution_path(&root, &name) else {
        return ui.fail(JobKind::Validate, "루트 폴더와 파일 이름을 먼저 설정하세요");
    };
    ui.clear_result();
    spawn_job(
        ui,
        "기존 파일 검증",
        Screen::Sheet,
        move || {
            let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                context: format!("reading {}", path.display()),
                source,
            })?;
            Ok(Verdict {
                name,
                sha256: hash::digests_of_bytes(&bytes).sha256,
                strict_ok: validate::validate_json(&bytes, true)?.is_ok(),
                report: validate::validate_json(&bytes, false)?,
                server_ids: server_ids_of(&bytes),
                path,
            })
        },
        |ui, outcome| apply_verdict_outcome(ui, JobKind::Validate, outcome),
    );
}

fn start_init_root(ui: &Rc<Ui>) {
    let current = ui.settings.borrow().clone();
    let cfg = match current.to_config() {
        Ok(cfg) => cfg,
        Err(error) => return ui.fail(JobKind::InitRoot, &error.to_string()),
    };
    let name = match ui.app.upgrade() {
        Some(app) => app.global::<SheetData>().get_distro_name().to_string(),
        None => return,
    };
    ui.clear_result();
    spawn_job(
        ui,
        "루트 초기화",
        Screen::Sheet,
        move || {
            distribution::init_root(&Root::new(cfg.root, cfg.base_url))?;
            Ok(snapshot(&current, &name))
        },
        |ui, outcome| match outcome {
            Ok(snap) => ui.apply_snapshot(snap),
            Err(error) => ui.fail(JobKind::InitRoot, &error.to_string()),
        },
    );
}

/// The ids the written file declares, in the order `validate` walks them. A
/// file that does not parse has no findings to label.
fn server_ids_of(bytes: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    value
        .get("servers")
        .and_then(|servers| servers.as_array())
        .map(|servers| {
            servers
                .iter()
                .map(|server| {
                    server
                        .get("id")
                        .and_then(|id| id.as_str())
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── The 100 ms tick ─────────────────────────────────────────────────────────

fn tick(ui: &Rc<Ui>, rx: &Receiver<logsink::LogLine>) {
    let Some(app) = ui.app.upgrade() else { return };
    drain_logs(&ui.logs, rx);
    tick_busy_label(ui, &app);
    tick_progress(ui, &app);
    mods::tick(ui, &app);

    // The opening scan: an ordinary job, started once the window exists, so a
    // slow root delays the first numbers rather than the first frame.
    if ui.first_scan.replace(false) {
        spawn_refresh(ui, "불러오는");
    }

    // Both checks read `in-out` properties the UI owns; only a settled file
    // name reaches the disk.
    let new_server = app.global::<NewServerData>();
    let warn = forge_mod_lists_warning(new_server.get_loader(), &new_server.get_mc_version());
    if ui.forge_warning.replace(warn) != warn {
        new_server.set_forge_mod_lists_warning(warn);
    }

    let name = app.global::<SheetData>().get_distro_name();
    if *ui.checked_name.borrow() != name.as_str() {
        ui.checked_name.replace(name.to_string());
        // The verdict names one file; the moment the field says another one it
        // is a claim about a file nobody validated.
        ui.clear_verdict();
        ui.name_check.arm();
    }
    if ui.name_check.due(SETTLE) {
        let name = ui.checked_name.borrow().clone();
        let root = ui.settings.borrow().root.clone();
        let mut gates = ui.gates.get();
        gates.dist_exists = distribution_path(&root, &name).is_some_and(|p| p.is_file());
        ui.gates.set(gates);
        ui.refresh_strict(&name);
        ui.apply_gates();
    }
    if ui.settings_check.due(SETTLE) {
        validate_settings_form(&app);
        let stored = ui.settings.borrow().clone();
        let dirty = form_from_ui(&app, &stored) != stored;
        app.global::<SettingsData>().set_dirty(dirty);
    }
}

/// Advances the busy pill. Slint marks a property dirty on every `set`, so it
/// is only written when the second changes.
fn tick_busy_label(ui: &Rc<Ui>, app: &App) {
    let Some(started) = ui.job_started.get() else {
        return;
    };
    let elapsed = started.elapsed();
    if ui.shown_secs.replace(elapsed.as_secs()) == elapsed.as_secs() {
        return;
    }
    app.global::<SheetData>()
        .set_busy_label(busy_pill(&ui.job_label.borrow(), elapsed).into());
}

/// Advances the one clock every indeterminate bar reads. Reduced motion parks
/// the segment where it can still be seen rather than hiding it.
fn tick_progress(ui: &Rc<Ui>, app: &App) {
    let theme = app.global::<Theme>();
    let phase = match (ui.busy.get(), ui.settings.borrow().reduced_motion) {
        (false, _) => 0.0,
        (true, true) => PHASE_STILL,
        (true, false) => (theme.get_progress_phase() + PHASE_STEP) % 1.0,
    };
    // Slint marks a property dirty on every `set`, so a parked bar is not
    // repainted ten times a second.
    if (theme.get_progress_phase() - phase).abs() > f32::EPSILON {
        theme.set_progress_phase(phase);
    }
}

/// The debounced settings pass: it says whether what has been typed so far is
/// usable, and clears each error the moment the value becomes valid. An empty
/// field is not an error yet — that is 저장's verdict, not this one's.
///
/// The four filesystem probes run on a worker: `is_dir` on a path that turns
/// out to be a dead network drive blocks for seconds, and this runs from the
/// tick. One probe per settle, and the answer is dropped if the fields moved
/// on while it was out.
fn validate_settings_form(app: &App) {
    let data = app.global::<SettingsData>();
    let root = data.get_root().trim().to_owned();
    let base = data.get_base_url().trim().to_owned();
    let java = data.get_java().trim().to_owned();
    let helios = data.get_helios().trim().to_owned();

    spawn_quiet(
        move || {
            let root_missing = !root.is_empty() && !Path::new(&root).is_dir();
            let base_error = match Config::normalize_base_url(&base) {
                Err(error) if !base.is_empty() => error.to_string(),
                _ => String::new(),
            };
            let java_missing = !java.is_empty() && !settings::is_file(&java);
            let helios_missing = !helios.is_empty() && !settings::is_dir(&helios);
            (
                (root, base, java, helios),
                (root_missing, base_error, java_missing, helios_missing),
            )
        },
        |ui, ((root, base, java, helios), (root_missing, base_error, java_missing, helios_missing))| {
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<SettingsData>();
            // A verdict about values that are no longer in the fields would
            // put an error under a path the user has already fixed.
            if data.get_root().trim() != root
                || data.get_base_url().trim() != base
                || data.get_java().trim() != java
                || data.get_helios().trim() != helios
            {
                return;
            }
            data.set_root_missing(root_missing);
            data.set_root_error(match root_missing {
                true => ROOT_MISSING.into(),
                false => SharedString::new(),
            });
            data.set_base_url_error(base_error.into());
            data.set_java_error(match java_missing {
                true => FILE_MISSING.into(),
                false => SharedString::new(),
            });
            data.set_helios_error(match helios_missing {
                true => DIR_MISSING.into(),
                false => SharedString::new(),
            });
        },
    );
}

/// Drains the whole channel, then trims once. Taking only `MAX_ROWS` per tick
/// left a build that logs faster than ten batches a second permanently behind,
/// and the band would show rows from a minute ago while the job ran.
fn drain_logs(model: &Rc<VecModel<LogRowData>>, rx: &Receiver<logsink::LogLine>) {
    for line in rx.try_iter() {
        model.push(LogRowData {
            time: line.time.into(),
            target: line.target.into(),
            message: line.message.into(),
            level: line.level.into(),
        });
    }
    // One pass, from the front: `remove(0)` is O(n), so trimming a large burst
    // one row at a time is quadratic.
    let extra = model.row_count().saturating_sub(logsink::MAX_ROWS);
    if extra > 0 {
        let kept: Vec<LogRowData> = model.iter().skip(extra).collect();
        model.set_vec(kept);
    }
}

// ── Snapshot: everything the sheet knows about a root ───────────────────────

/// A pure read of the root, safe to compute on a worker thread.
struct Snapshot {
    readiness: Vec<ReadinessRowData>,
    servers: Vec<ServerRowData>,
    /// Parallel to `servers`: the server's own `icon.png`/`icon.jpg` when it has
    /// one. Only the path crosses the thread boundary; `slint::Image` is not
    /// `Send` and the decode belongs on the event-loop thread anyway.
    server_icons: Vec<Option<PathBuf>>,
    /// Why the listing could not be read, with the file that broke it and what
    /// to do. `Some` means `servers`/`server_icons` are empty and meaningless —
    /// the caller keeps whatever it had.
    servers_error: Option<String>,
    imports: Vec<ImportRowData>,
    import_paths: Vec<PathBuf>,
    modpacks_dir: String,
    root_ok: bool,
    base_ok: bool,
    root_initialised: bool,
    install_local_enabled: bool,
    dist_exists: bool,
    gdrive_connected: bool,
    checked_name: String,
}

fn snapshot(current: &GuiSettings, distro_name: &str) -> Snapshot {
    let root_ok = settings::is_dir(&current.root);
    let base = Config::normalize_base_url(current.base_url.trim()).ok();
    let java_ok = settings::is_file(&current.java_executable);
    let helios_ok = settings::is_dir(&current.helios_data_folder);

    // Counts and paths do not depend on the base URL, so a root can still be
    // listed while the URL is being fixed; nothing here reads the placeholder.
    let root = Root::new(
        current.root.trim(),
        base.clone().unwrap_or_else(|| {
            Config::normalize_base_url("https://base-url.invalid/").expect("a literal URL")
        }),
    );
    let (servers, server_icons, servers_error) = match root_ok {
        true => server_rows(&root),
        false => (Vec::new(), Vec::new(), None),
    };
    let (imports, import_paths) = match root_ok {
        true => import_rows(&root.modpacks_dir()),
        false => (Vec::new(), Vec::new()),
    };

    // 확인됨 on 루트 means the root was read, not that the path exists: a root
    // whose listing failed is exactly the case the chip is read for.
    let readiness = vec![
        match (root_ok, servers_error.is_some()) {
            (true, true) => ReadinessRowData {
                label: "루트".into(),
                value: current.root.as_str().into(),
                state: "error".into(),
            },
            _ => readiness_row("루트", &current.root, root_ok),
        },
        readiness_row(
            "기본 URL",
            &base
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| current.base_url.clone()),
            base.is_some(),
        ),
        readiness_row("Java", &current.java_executable, java_ok),
        readiness_row("Launcher 폴더", &current.helios_data_folder, helios_ok),
    ];

    Snapshot {
        readiness,
        servers,
        server_icons,
        servers_error,
        imports,
        import_paths,
        modpacks_dir: root.modpacks_dir().display().to_string(),
        root_ok,
        base_ok: base.is_some(),
        root_initialised: !root_ok || root.distro_meta_file().is_file(),
        install_local_enabled: helios_ok,
        dist_exists: root_ok
            && distribution_path(&current.root, distro_name).is_some_and(|p| p.is_file()),
        gdrive_connected: settings::gdrive_token_path().is_file(),
        checked_name: distro_name.to_owned(),
    }
}

fn readiness_row(label: &str, value: &str, ok: bool) -> ReadinessRowData {
    ReadinessRowData {
        label: label.into(),
        // A blank value column next to a 미설정 chip reads as a rendering bug.
        value: match value.trim().is_empty() {
            true => "미설정".into(),
            false => value.into(),
        },
        state: match ok {
            true => "ok".into(),
            false => "missing".into(),
        },
    }
}

/// The rows, their icons, and — when the scan failed — the reason, which names
/// the file that broke it and says what to do about it. The rows are empty in
/// that case and the caller keeps the ones it already had: a failed scan knows
/// nothing about the servers, least of all that there are none.
fn server_rows(root: &Root) -> (Vec<ServerRowData>, Vec<Option<PathBuf>>, Option<String>) {
    let summaries = match server::list_servers(root) {
        Ok(summaries) => summaries,
        Err(error) => {
            return (
                Vec::new(),
                Vec::new(),
                Some(format!(
                    "{error} — 해당 servermeta.json을 고치거나 지운 뒤 새로 고침을 누르세요"
                )),
            );
        }
    };
    let (rows, icons) = summaries
        .iter()
        .map(|summary| {
            let dir = root.servers_dir().join(&summary.dir_name);
            let row = ServerRowData {
                id: summary.dir_name.as_str().into(),
                loader: loader_label(summary).into(),
                counts: counts_label(summary).into(),
                path: dir.display().to_string().into(),
                initials: initials(&summary.dir_name).into(),
                hue: hue_of(&summary.dir_name),
            };
            (row, server_icon(&dir))
        })
        .unzip();
    (rows, icons, None)
}

/// A server's own icon, if it put one next to its files. The launcher's own
/// convention: `icon.png`, else `icon.jpg`.
fn server_icon(dir: &Path) -> Option<PathBuf> {
    ["icon.png", "icon.jpg"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

/// The fallback avatar: one CJK glyph, or two Latin letters. A server named in
/// Hangul gets one character, because two fill the square twice over.
fn initials(id: &str) -> String {
    let mut letters = id.chars().filter(|ch| ch.is_alphanumeric());
    let Some(first) = letters.next() else {
        return String::new();
    };
    if !first.is_ascii() {
        return first.to_string();
    }
    let mut out: String = first.to_uppercase().collect();
    if let Some(second) = letters.next().filter(char::is_ascii) {
        out.extend(second.to_uppercase());
    }
    out
}

/// A stable hue per id (FNV-1a), so a server keeps its avatar colour between
/// runs and two servers in a list are told apart at a glance.
fn hue_of(id: &str) -> i32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    (hash % 360) as i32
}

/// Every modpack archive below `<root>/modpacks`, with the format read out of
/// the archive itself rather than guessed from the extension.
fn import_rows(modpacks: &Path) -> (Vec<ImportRowData>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut stack = vec![modpacks.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_modpack_archive(&path) {
                files.push(path);
            }
        }
    }
    files.sort();

    let rows = files
        .iter()
        .map(|path| {
            let (format, selectable) = match import::detect_format(path) {
                Ok(ImportFormat::CurseForge) => ("CurseForge", true),
                Ok(ImportFormat::Modrinth) => ("Modrinth", true),
                Ok(ImportFormat::PrismInstance) => ("Prism 인스턴스", true),
                Err(_) => ("알 수 없음", false),
            };
            ImportRowData {
                file_name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref()
                    .into(),
                format: format.into(),
                size_label: size_label(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0))
                    .into(),
                selectable,
            }
        })
        .collect();
    (rows, files)
}

fn is_modpack_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("mrpack"))
}

fn distribution_path(root: &str, name: &str) -> Option<PathBuf> {
    let root = root.trim();
    let name = name.trim();
    (!root.is_empty() && !name.is_empty()).then(|| Path::new(root).join(format!("{name}.json")))
}

// ── Label formatting ────────────────────────────────────────────────────────

fn loader_label(summary: &ServerSummary) -> String {
    match &summary.loader {
        Some((kind, version)) => format!("{kind} {version}"),
        None => "없음".into(),
    }
}

fn counts_label(summary: &ServerSummary) -> String {
    format!(
        "모드 {} · 파일 {} · 라이브러리 {}",
        summary.mods, summary.files, summary.libraries
    )
}

/// `412 MB`, `12.3 MB`, `1.2 GB`: one decimal, and a bare `.0` is noise.
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let rendered = format!("{value:.1}");
    format!(
        "{} {}",
        rendered.strip_suffix(".0").unwrap_or(&rendered),
        UNITS[unit]
    )
}

/// `strict_ok` is the launcher's verdict, not the reader's: a file with no
/// displayed errors that the launcher would still refuse says so here, because
/// the summary is the only line that gets read before the upload button.
fn summary_line(report: &Report, strict_ok: bool) -> String {
    let line = format!(
        "파일 {} · SHA-256 {} · 오류 {} · 경고 {}",
        report.artifacts,
        report.sha256,
        report.errors(),
        report.warnings()
    );
    match report.is_ok() && !strict_ok {
        true => format!("{line} · 런처 배포 불가"),
        false => line,
    }
}

/// The banner's colour, and with it the whole page's verdict (DESIGN-v2 §5):
/// errors in the displayed report are `danger`; a clean report the launcher
/// still refuses is `warning`; only a clean strict pass is `success`.
fn verdict_tone(report: &Report, strict_ok: bool) -> Tone {
    if !report.is_ok() {
        Tone::Danger
    } else if strict_ok {
        Tone::Success
    } else {
        Tone::Warning
    }
}

fn verdict_title(tone: Tone) -> &'static str {
    match tone {
        Tone::Success => "런처 검증 통과",
        Tone::Warning => "경고와 함께 통과",
        _ => "검증 실패",
    }
}

fn verdict_toast(tone: Tone) -> &'static str {
    match tone {
        Tone::Success => "배포본을 검증했습니다",
        Tone::Warning => "경고와 함께 통과했습니다",
        _ => "검증에 실패했습니다",
    }
}

/// `distribution.json · SHA-256 a91f3c02 · 파일 484`: what the file is, what it
/// hashes to, and how much of it there is.
fn verdict_detail(path: &Path, sha256: &str, artifacts: usize) -> String {
    format!(
        "{} · SHA-256 {} · 파일 {artifacts}",
        file_name(path),
        stamp_hash(sha256)
    )
}

fn finding_rows(report: &Report, server_ids: &[String]) -> Vec<FindingRowData> {
    report
        .findings
        .iter()
        .map(|finding| {
            let is_error = finding.severity == Severity::Error;
            FindingRowData {
                severity: match is_error {
                    true => "오류".into(),
                    false => "경고".into(),
                },
                location: finding_location(&finding.location, server_ids).into(),
                message: finding.message.as_str().into(),
                is_error,
            }
        })
        .collect()
}

/// `servers[0].modules[7].artifact.url` reads as
/// `Test-1.12.2 · modules[7].artifact.url`. The index is the validator's
/// business; the person looking at the list knows the servers by name.
fn finding_location(location: &str, server_ids: &[String]) -> String {
    let Some((index, rest)) = location
        .strip_prefix("servers[")
        .and_then(|rest| rest.split_once(']'))
    else {
        // `servers` itself is an error about the array, not about a server.
        return location.to_owned();
    };
    let Some(id) = index.parse::<usize>().ok().and_then(|i| server_ids.get(i)) else {
        return location.to_owned();
    };
    match rest.strip_prefix('.') {
        Some(tail) => format!("{id} · {tail}"),
        None => id.clone(),
    }
}

/// The verdict banner shows the first 8 hex characters of the file's SHA-256.
fn stamp_hash(sha256: &str) -> &str {
    &sha256[..sha256.len().min(8)]
}

/// `distribution.json`, the file the verdict is about.
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The one fix the sheet can offer without guessing: the installer and the
/// cached-install errors both name the cache, and the toggle is on this screen.
fn cache_hint(message: &str) -> &'static str {
    match message.contains("캐시") || message.contains("cache") {
        true => "Forge 설치 캐시 무효화를 켜고 다시 시도하세요",
        false => "",
    }
}

fn target_label(target: PublishTarget) -> &'static str {
    match target {
        PublishTarget::Webdav => "WebDAV",
        PublishTarget::Gdrive => "Google Drive",
        PublishTarget::None => "",
    }
}

fn forge_mod_lists_warning(loader: Loader, mc: &str) -> bool {
    loader == Loader::Forge
        && mc
            .trim()
            .parse::<MinecraftVersion>()
            .is_ok_and(|version| version >= MinecraftVersion::new(1, 20, Some(3)))
}

/// DESIGN §9: errors state a cause and a fix.
fn publish_hint(error: &Error) -> String {
    let message = error.to_string();
    let fix = if message.contains("401") {
        "설정에서 WebDAV 사용자와 비밀번호를 확인하세요"
    } else if message.contains("403") {
        "계정에 쓰기 권한이 있는지, Google Drive 폴더 ID가 맞는지 확인하세요"
    } else if message.contains("404") {
        "업로드 URL의 공유 폴더 경로가 맞는지 확인하세요"
    } else if message.contains("gdrive-token.json") {
        "설정에서 Google 계정 연결을 먼저 누르세요"
    } else {
        return message;
    };
    format!("{message} — {fix}")
}

fn theme_preference(name: &str) -> ThemePreference {
    match name {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

fn theme_name(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
        ThemePreference::System => "system",
    }
}

// ── Startup wiring ──────────────────────────────────────────────────────────

fn bind_models(ui: &Rc<Ui>, app: &App) {
    let sheet = app.global::<SheetData>();
    sheet.set_readiness(ModelRc::from(ui.readiness.clone()));
    sheet.set_servers(ModelRc::from(ui.servers.clone()));
    sheet.set_findings(ModelRc::from(ui.findings.clone()));
    sheet.set_log(ModelRc::from(ui.logs.clone()));

    sheet.set_server_icons(ModelRc::from(ui.server_icons.clone()));

    let import_data = app.global::<ImportData>();
    import_data.set_zips(ModelRc::from(ui.imports.clone()));
    import_data.set_manual_downloads(ModelRc::from(ui.manual.clone()));

    app.global::<AppData>()
        .set_toasts(ModelRc::from(ui.toasts.clone()));

    mods::bind_models(ui, app);
}

fn apply_settings_to_ui(app: &App, current: &GuiSettings) {
    let data = app.global::<SettingsData>();
    data.set_root(current.root.as_str().into());
    data.set_base_url(current.base_url.as_str().into());
    data.set_java(current.java_executable.as_str().into());
    data.set_helios(current.helios_data_folder.as_str().into());
    data.set_theme(theme_preference(&current.theme));
    data.set_reduced_motion(current.reduced_motion);
    data.set_webdav_url(current.webdav_url.as_str().into());
    data.set_webdav_user(current.webdav_user.as_str().into());
    data.set_webdav_password(current.webdav_password.as_str().into());
    data.set_webdav_public_url(current.webdav_public_url.as_str().into());
    data.set_gdrive_client_id(current.gdrive_client_id.as_str().into());
    data.set_gdrive_client_secret(current.gdrive_client_secret.as_str().into());
    data.set_gdrive_folder_id(current.gdrive_folder_id.as_str().into());
    data.set_curseforge_api_key(current.curseforge_api_key.as_str().into());

    let theme = app.global::<Theme>();
    theme.set_preference(theme_preference(&current.theme));
    theme.set_reduced_motion(current.reduced_motion);
}

/// The settings form as it currently stands. `save-settings` passes none of
/// the values; every one of them is `in-out`, so they are read from one place
/// instead of two.
///
/// `last_mods_server` is **not** a form field: it is carried over from what is
/// already stored, because a 저장 that dropped it would move the 모드 page back
/// to the first server on the next launch.
fn form_from_ui(app: &App, stored: &GuiSettings) -> GuiSettings {
    let data = app.global::<SettingsData>();
    GuiSettings {
        root: data.get_root().trim().to_owned(),
        base_url: data.get_base_url().trim().to_owned(),
        java_executable: data.get_java().trim().to_owned(),
        helios_data_folder: data.get_helios().trim().to_owned(),
        theme: theme_name(data.get_theme()).to_owned(),
        reduced_motion: data.get_reduced_motion(),
        webdav_url: data.get_webdav_url().trim().to_owned(),
        webdav_user: data.get_webdav_user().trim().to_owned(),
        webdav_password: data.get_webdav_password().to_owned().to_string(),
        webdav_public_url: data.get_webdav_public_url().trim().to_owned(),
        gdrive_client_id: data.get_gdrive_client_id().trim().to_owned(),
        gdrive_client_secret: data.get_gdrive_client_secret().to_owned().to_string(),
        gdrive_folder_id: data.get_gdrive_folder_id().trim().to_owned(),
        curseforge_api_key: data.get_curseforge_api_key().trim().to_owned(),
        last_mods_server: stored.last_mods_server.clone(),
    }
}

fn wire(ui: &Rc<Ui>, app: &App) {
    let actions = app.global::<SheetActions>();

    // These take no arguments: every value is an `in-out` property the starter
    // reads back, so a retry uses what is on screen now rather than replaying
    // the arguments of the click that failed.
    actions.on_generate({
        let ui = ui.clone();
        move || start_generate(&ui)
    });

    actions.on_validate_existing({
        let ui = ui.clone();
        move || start_validate(&ui)
    });

    actions.on_init_root({
        let ui = ui.clone();
        move || start_init_root(&ui)
    });

    actions.on_retry_job({
        let ui = ui.clone();
        move || match ui.job.get() {
            Some(JobKind::Validate) => start_validate(&ui),
            Some(JobKind::InitRoot) => start_init_root(&ui),
            Some(JobKind::Generate) | None => start_generate(&ui),
        }
    });

    actions.on_refresh({
        let ui = ui.clone();
        move || spawn_refresh(&ui, "새로 고침")
    });

    actions.on_create_server({
        let ui = ui.clone();
        move |id, mc_text, loader, loader_version| {
            let Some(app) = ui.app.upgrade() else { return };
            let form = app.global::<NewServerData>();
            form.set_id_error(SharedString::new());
            form.set_mc_error(SharedString::new());
            form.set_loader_version_error(SharedString::new());
            form.set_error_text(SharedString::new());

            let id = id.trim().to_owned();
            if id.is_empty() {
                return form.set_id_error("서버 ID가 비어 있습니다 — 예: Test".into());
            }
            let Ok(mc) = mc_text.trim().parse::<MinecraftVersion>() else {
                return form.set_mc_error(
                    "Minecraft 버전을 읽을 수 없습니다 — 1.20.1 처럼 입력하세요".into(),
                );
            };
            let version = loader_version.trim().to_owned();
            if loader != Loader::None && version.is_empty() {
                return form.set_loader_version_error(
                    "로더 버전이 비어 있습니다 — 최신/권장 버전 찾기를 누르거나 직접 입력하세요"
                        .into(),
                );
            }

            let current = ui.settings.borrow().clone();
            let cfg = match current.to_config() {
                Ok(cfg) => cfg,
                Err(error) => return form.set_id_error(error.to_string().into()),
            };
            let opts = CreateServerOptions {
                version: None,
                forge_version: (loader == Loader::Forge).then(|| version.clone()),
                neoforge_version: (loader == Loader::Neoforge).then(|| version.clone()),
                fabric_version: (loader == Loader::Fabric).then_some(version),
            };
            let name = app.global::<SheetData>().get_distro_name().to_string();

            spawn_job(
                &ui,
                "새 서버 만들기",
                Screen::NewServer,
                move || {
                    let root = Root::new(cfg.root, cfg.base_url);
                    let created = server::create_server(&root, &id, &mc, &opts)?;
                    Ok((created.dir.dir_name, snapshot(&current, &name)))
                },
                |ui, outcome| {
                    let Some(app) = ui.app.upgrade() else { return };
                    match outcome {
                        Ok((dir_name, snap)) => {
                            ui.apply_snapshot(snap);
                            let form = app.global::<NewServerData>();
                            form.set_id(SharedString::new());
                            form.set_loader_version(SharedString::new());
                            app.global::<AppData>().set_dialog(DialogKind::None);
                            log::info!("서버 {dir_name} 을(를) 만들었습니다.");
                            ui.toast(Tone::Success, &format!("서버를 만들었습니다: {dir_name}"));
                        }
                        Err(error) => {
                            let message = error.to_string();
                            log::error!("{message}");
                            // Only what the form itself can fix goes under a
                            // field; a disk or network failure is not the id's
                            // fault and must not be filed under it.
                            let form = app.global::<NewServerData>();
                            if message.contains("NeoForge") || message.contains("Minecraft") {
                                form.set_mc_error(message.into());
                            } else if message.contains("서버 폴더") || message.contains("server id")
                            {
                                form.set_id_error(message.into());
                            } else {
                                form.set_error_text(message.into());
                            }
                        }
                    }
                },
            );
        }
    });

    actions.on_lookup_loader_version({
        let ui = ui.clone();
        move |loader, promotion| {
            let Some(app) = ui.app.upgrade() else { return };
            let form = app.global::<NewServerData>();
            form.set_loader_version_error(SharedString::new());
            form.set_mc_error(SharedString::new());

            let promotion = match promotion.parse::<Promotion>() {
                Ok(promotion) => promotion,
                Err(error) => return log::error!("{error}"),
            };
            let mc = form
                .get_mc_version()
                .trim()
                .parse::<MinecraftVersion>()
                .ok();
            if loader != Loader::Fabric && mc.is_none() {
                return form.set_mc_error(
                    "Minecraft 버전을 먼저 입력하세요 — 1.20.1 처럼 입력하세요".into(),
                );
            }

            let started = spawn_job(
                &ui,
                "로더 버전 찾기",
                Screen::Lookup,
                move || {
                    let mc =
                        || mc.ok_or_else(|| Error::Invalid("Minecraft 버전이 없습니다".into()));
                    match loader {
                        Loader::Forge => remote::get_promoted_forge_version(&mc()?, promotion),
                        Loader::Neoforge => {
                            remote::get_promoted_neoforge_version(&mc()?, promotion)
                        }
                        Loader::Fabric => remote::get_promoted_fabric_version(promotion),
                        Loader::None => Err(Error::Invalid("로더를 먼저 고르세요".into())),
                    }
                },
                |ui, outcome| {
                    let Some(app) = ui.app.upgrade() else { return };
                    let form = app.global::<NewServerData>();
                    match outcome {
                        Ok(version) => form.set_loader_version(version.as_str().into()),
                        Err(error) => form.set_loader_version_error(
                            format!("버전을 가져오지 못했습니다: {error}").into(),
                        ),
                    }
                },
            );
            // The event loop is single threaded, so the job's result cannot have
            // been applied before this callback returns; `set_busy` has already
            // raised the flag, and this covers the job that never started.
            form.set_lookup_busy(started);
        }
    });

    actions.on_import_modpack({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<ImportData>();
            data.set_id_error(SharedString::new());
            data.set_error_text(SharedString::new());
            // The line belongs to the import that produced it; leaving it up
            // while the next one runs reads as a result that arrived early.
            data.set_result_line(SharedString::new());

            let id = data.get_server_id().trim().to_owned();
            if id.is_empty() {
                return data.set_id_error("서버 ID가 비어 있습니다 — 예: RL".into());
            }
            // The row index is authoritative: two packs can share a file name in
            // different `modpacks` subdirectories.
            let selected = usize::try_from(data.get_selected_zip()).ok();
            let Some(path) = selected.and_then(|i| ui.import_paths.borrow().get(i).cloned()) else {
                return data.set_id_error("가져올 모드팩을 목록에서 고르세요".into());
            };

            let current = ui.settings.borrow().clone();
            let cfg = match current.to_config() {
                Ok(cfg) => cfg,
                Err(error) => return data.set_id_error(error.to_string().into()),
            };
            ui.manual.set_vec(Vec::new());
            let name = app.global::<SheetData>().get_distro_name().to_string();

            spawn_job(
                &ui,
                "모드팩 가져오기",
                Screen::Import,
                move || {
                    let root = Root::new(cfg.root, cfg.base_url);
                    let report =
                        import::import_modpack(&root, &id, &path, &import::default_download)?;
                    let manual = report
                        .manual
                        .iter()
                        .map(|item| ManualDownloadData {
                            name: item.name.as_str().into(),
                            file: item.file_name.as_str().into(),
                            url: item.url.as_str().into(),
                        })
                        .collect::<Vec<_>>();
                    Ok((
                        report.created.dir.dir_name,
                        manual,
                        snapshot(&current, &name),
                    ))
                },
                |ui, outcome| {
                    let Some(app) = ui.app.upgrade() else { return };
                    match outcome {
                        Ok((dir_name, manual, snap)) => {
                            ui.apply_snapshot(snap);
                            ui.manual.set_vec(manual);
                            let data = app.global::<ImportData>();
                            data.set_id_error(SharedString::new());
                            data.set_result_line(format!("서버 {dir_name}").into());
                            log::info!("모드팩을 가져왔습니다: {dir_name}");
                            ui.toast(Tone::Success, "모드팩을 가져왔습니다");
                        }
                        Err(error) => {
                            let message = error.to_string();
                            log::error!("{message}");
                            // A broken archive or a refused download is not the
                            // server id's fault.
                            app.global::<ImportData>().set_error_text(message.into());
                            ui.toast(Tone::Danger, "가져오기 실패");
                        }
                    }
                },
            );
        }
    });

    actions.on_save_settings({
        let ui = ui.clone();
        move || {
            if let Some(app) = ui.app.upgrade() {
                let stored = ui.settings.borrow().clone();
                commit_settings(&ui, &app, form_from_ui(&app, &stored));
            }
        }
    });

    actions.on_settings_edited({
        let ui = ui.clone();
        move || ui.settings_check.arm()
    });

    actions.on_appearance_changed({
        let ui = ui.clone();
        // Appearance is the one setting that does not wait for a valid root and
        // base URL: it changes what the user is looking at, so it applies and
        // persists on its own.
        move |theme, reduced_motion| {
            let Some(app) = ui.app.upgrade() else { return };
            let palette = app.global::<Theme>();
            palette.set_preference(theme);
            palette.set_reduced_motion(reduced_motion);

            let mut next = ui.settings.borrow().clone();
            next.theme = theme_name(theme).to_owned();
            next.reduced_motion = reduced_motion;
            if let Err(error) = next.save() {
                log::error!("설정을 저장하지 못했습니다: {error}");
            }
            ui.settings.replace(next);
        }
    });

    actions.on_create_root({
        let ui = ui.clone();
        move |path| {
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<SettingsData>();
            let path = path.trim().to_owned();
            if path.is_empty() {
                return data.set_root_error("루트 폴더 경로를 먼저 입력하세요".into());
            }
            if let Err(error) = std::fs::create_dir_all(&path) {
                return data.set_root_error(format!("폴더를 만들지 못했습니다: {error}").into());
            }
            log::info!("루트 폴더를 만들었습니다: {path}");
            let stored = ui.settings.borrow().clone();
            commit_settings(&ui, &app, form_from_ui(&app, &stored));
        }
    });

    actions.on_publish({
        let ui = ui.clone();
        move |target, only_changed| {
            if target == PublishTarget::None {
                return;
            }
            let Some(app) = ui.app.upgrade() else { return };
            ui.publish_result(String::new(), String::new(), false);

            let current = ui.settings.borrow().clone();
            let cfg = match current.to_config() {
                Ok(cfg) => cfg,
                Err(error) => {
                    return ui.publish_result(format!("업로드 실패: {error}"), String::new(), true);
                }
            };
            let name = app
                .global::<SheetData>()
                .get_distro_name()
                .trim()
                .to_owned();

            spawn_job(
                &ui,
                "배포본 업로드",
                Screen::Publish,
                move || {
                    let root = Root::new(&cfg.root, cfg.base_url.clone());
                    let path = root.path.join(format!("{name}.json"));
                    let plan = publish::plan_from_distribution(&root, &path)?;
                    let mut sink: Box<dyn Target + Send> = match target {
                        PublishTarget::Webdav => Box::new(current.webdav_target(&cfg)?),
                        PublishTarget::Gdrive => {
                            Box::new(gdrive::GoogleDriveTarget::new(current.gdrive_config()?)?)
                        }
                        PublishTarget::None => {
                            return Err(Error::Invalid("배포 대상을 고르세요".into()));
                        }
                    };
                    let opts = PublishOptions {
                        only_changed,
                        // Drive hands back its own URLs, so the uploaded copy
                        // must carry them.
                        rewrite_urls: target == PublishTarget::Gdrive,
                    };
                    publish::publish(&root, &plan, sink.as_mut(), &opts)
                },
                move |ui, outcome| match outcome {
                    Ok(report) => {
                        ui.publish_result(
                            format!(
                                "{}로 {}개 업로드 · {}개 변경 없음",
                                target_label(target),
                                report.uploaded,
                                report.skipped
                            ),
                            report.distribution_url,
                            false,
                        );
                        ui.toast(Tone::Success, "업로드를 마쳤습니다");
                    }
                    Err(error) => {
                        let hint = publish_hint(&error);
                        log::error!("{hint}");
                        ui.publish_result(format!("업로드 실패: {hint}"), String::new(), true);
                        ui.toast(Tone::Danger, "업로드 실패");
                    }
                },
            );
        }
    });

    // The one job that does not take the global lock: it waits on a browser,
    // not on this machine, and five minutes of a consent screen is five minutes
    // in which a build, an import and a save all still make sense. Its own flag
    // is what refuses a second connect.
    actions.on_connect_gdrive({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            let data = app.global::<SettingsData>();
            if ui.gdrive_busy.get() {
                return;
            }
            data.set_gdrive_status(SharedString::new());
            data.set_gdrive_status_error(false);

            let cfg = match ui.settings.borrow().gdrive_config() {
                Ok(cfg) => cfg,
                Err(error) => {
                    log::error!("{error}");
                    data.set_gdrive_status(error.to_string().into());
                    return data.set_gdrive_status_error(true);
                }
            };
            log::info!(
                "브라우저에서 승인을 마칠 때까지 기다립니다. 승인을 마치지 않으면 5분 뒤 자동으로 취소됩니다."
            );
            ui.set_gdrive_busy(true);
            spawn_quiet(
                move || gdrive::authorize(&cfg, &gdrive::default_open_browser),
                |ui, outcome| {
                    ui.set_gdrive_busy(false);
                    let Some(app) = ui.app.upgrade() else { return };
                    let data = app.global::<SettingsData>();
                    match &outcome {
                        Ok(()) => {
                            log::info!("Google 계정을 연결했습니다.");
                            data.set_gdrive_status("연결됨 (토큰 저장됨)".into());
                            data.set_gdrive_status_error(false);
                        }
                        Err(error) => {
                            log::error!("Google 계정 연결 실패: {error}");
                            data.set_gdrive_status(format!("연결 실패: {error}").into());
                            data.set_gdrive_status_error(true);
                        }
                    }
                    // A failed retry must not clear a token that still works.
                    data.set_gdrive_connected(
                        outcome.is_ok() || settings::gdrive_token_path().is_file(),
                    );
                },
            );
        }
    });

    actions.on_open_folder(|path| open_folder(Path::new(path.as_str())));

    actions.on_clear_log({
        let ui = ui.clone();
        move || ui.logs.set_vec(Vec::new())
    });

    // `AppData.go` only fires this when the page actually changed, so opening a
    // page twice does not re-read the disk twice. Both list pages refresh:
    // the servers page was the one showing a stale list after a CLI run.
    actions.on_navigate({
        let ui = ui.clone();
        move |page| {
            if page == AppPage::Import || page == AppPage::Servers || page == AppPage::Mods {
                spawn_refresh(&ui, "목록 새로 고침");
            }
            if page == AppPage::Mods {
                // The pill strip comes back with the refresh above; the
                // installed list only reloads when its subject changed.
                mods::navigated(&ui);
            }
        }
    });

    actions.on_dismiss_toast({
        let ui = ui.clone();
        move |id| ui.dismiss_toast(id)
    });

    actions.on_open_dialog({
        let ui = ui.clone();
        // A dialog that opens over a running job offers actions the job lock
        // has already refused.
        move |kind| {
            if !ui.busy.get()
                && let Some(app) = ui.app.upgrade()
            {
                app.global::<AppData>().set_dialog(kind);
            }
        }
    });

    actions.on_close_dialog({
        let ui = ui.clone();
        move || {
            let Some(app) = ui.app.upgrade() else { return };
            app.global::<AppData>().set_dialog(DialogKind::None);
            // Errors belong to the attempt that raised them, not to the next
            // time the form is opened.
            let form = app.global::<NewServerData>();
            form.set_id_error(SharedString::new());
            form.set_mc_error(SharedString::new());
            form.set_loader_version_error(SharedString::new());
            form.set_error_text(SharedString::new());
        }
    });

    mods::wire(ui, app);
}

/// Validates the form, stores it, and re-reads the root under one busy window.
fn commit_settings(ui: &Rc<Ui>, app: &App, mut next: GuiSettings) {
    // Enter always follows typing, so a re-validation is nearly always pending
    // when a save runs. It has a laxer view of an empty field than this does
    // and would wipe the required-field error 300 ms after it appeared, which
    // reads as a save that silently did nothing. An explicit save wins.
    ui.settings_check.cancel();
    // A stamp names a hash of a file in the old root; pointing at a new one
    // makes it a claim about a file nobody validated.
    if next.root != ui.settings.borrow().root {
        ui.clear_result();
    }
    let data = app.global::<SettingsData>();
    data.set_root_error(SharedString::new());
    data.set_base_url_error(SharedString::new());
    data.set_java_error(SharedString::new());
    data.set_helios_error(SharedString::new());
    data.set_error_text(SharedString::new());
    data.set_root_missing(false);

    if next.root.is_empty() {
        data.set_root_error(
            "루트 폴더 경로가 비어 있습니다 — 배포본을 만들 폴더를 입력하세요".into(),
        );
        return refuse_save(ui, &data, "root");
    }
    if !Path::new(&next.root).is_dir() {
        data.set_root_missing(true);
        data.set_root_error(ROOT_MISSING.into());
        return refuse_save(ui, &data, "root");
    }
    if next.base_url.is_empty() {
        data.set_base_url_error(
            "기본 URL이 비어 있습니다 — https://cdn.example.com/ 처럼 입력하세요".into(),
        );
        return refuse_save(ui, &data, "base-url");
    }
    match Config::normalize_base_url(&next.base_url) {
        Ok(url) => {
            next.base_url = url.to_string();
            data.set_base_url(next.base_url.as_str().into());
        }
        Err(error) => {
            data.set_base_url_error(error.to_string().into());
            return refuse_save(ui, &data, "base-url");
        }
    }
    // Optional paths do not block the save; the readiness rows report them too.
    if !next.java_executable.is_empty() && !settings::is_file(&next.java_executable) {
        data.set_java_error(FILE_MISSING.into());
    }
    if !next.helios_data_folder.is_empty() && !settings::is_dir(&next.helios_data_folder) {
        data.set_helios_error(DIR_MISSING.into());
    }

    let theme = app.global::<Theme>();
    theme.set_preference(theme_preference(&next.theme));
    theme.set_reduced_motion(next.reduced_motion);

    let name = app.global::<SheetData>().get_distro_name().to_string();
    let stored = next.clone();
    spawn_job(
        ui,
        "설정 저장",
        Screen::Settings,
        move || {
            next.save()?;
            Ok(snapshot(&next, &name))
        },
        move |ui, outcome| match outcome {
            Ok(snap) => {
                // Only now: until the file is written, what the app believes
                // its settings are must still be what is on disk. A save that
                // fails would otherwise leave the process running on values no
                // restart would bring back.
                ui.settings.replace(stored);
                // Before the snapshot: a CurseForge key that just arrived is
                // what decides whether the 모드 page's provider segment works.
                mods::settings_saved(ui);
                ui.apply_snapshot(snap);
                if let Some(app) = ui.app.upgrade() {
                    app.global::<SettingsData>().set_dirty(false);
                }
                log::info!(
                    "설정을 저장했습니다: {}",
                    settings::settings_path().display()
                );
                ui.toast(Tone::Success, "설정을 저장했습니다");
            }
            Err(error) => {
                log::error!("{error}");
                if let Some(app) = ui.app.upgrade() {
                    // Not under 루트 폴더: the disk refusing the settings file is
                    // not the root path's fault, and filing it there sends the
                    // user to fix a field that is already correct.
                    app.global::<SettingsData>()
                        .set_error_text(error.to_string().into());
                }
                ui.toast(Tone::Danger, "설정 저장 실패");
            }
        },
    );
}

/// A save the form refused. The inline error is already up; this is what says
/// the button was heard, and puts the caret where the problem is.
fn refuse_save(ui: &Rc<Ui>, data: &SettingsData<'_>, field: &str) {
    data.set_focus_field(field.into());
    data.set_focus_tick(data.get_focus_tick() + 1);
    ui.toast(Tone::Danger, "저장하지 못했습니다: 입력값을 확인하세요");
}

/// Opens `path` in the platform file manager. Never a shell string.
fn open_folder(path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    // The modpacks folder is offered before anything has been put in it. This
    // is also why a URL cannot come through here: it would make a directory
    // named after the address.
    let _ = std::fs::create_dir_all(path);
    if let Err(error) = hand_to_shell(path.as_os_str()) {
        log::error!("폴더를 열지 못했습니다 ({}): {error}", path.display());
    }
}

/// Opens a URL in the default browser (§8.3's 프로젝트 페이지 열기). Same three
/// programs as [`open_folder`], and deliberately no `create_dir_all`.
fn open_external(url: &str) {
    if url.trim().is_empty() {
        return;
    }
    if let Err(error) = hand_to_shell(std::ffi::OsStr::new(url)) {
        log::error!("주소를 열지 못했습니다 ({url}): {error}");
    }
}

/// The one spawn both openers share. `explorer.exe` reports exit code 1 on
/// success, so only the spawn is checked.
fn hand_to_shell(target: &std::ffi::OsStr) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program).arg(target).spawn().map(|_| ())
}

#[cfg(test)]
pub(crate) struct TempDir(PathBuf);

#[cfg(test)]
impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "nebula-gui-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temp directory");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::validate::Finding;

    fn report(artifacts: usize, sha256: usize, findings: Vec<Finding>) -> Report {
        Report {
            findings,
            artifacts,
            sha256,
        }
    }

    fn finding(severity: Severity, location: &str) -> Finding {
        Finding {
            severity,
            location: location.into(),
            message: "메시지".into(),
        }
    }

    #[test]
    fn size_labels_drop_a_bare_decimal() {
        assert_eq!(size_label(0), "0 B");
        assert_eq!(size_label(1023), "1023 B");
        assert_eq!(size_label(1024), "1 KB");
        assert_eq!(size_label(38 * 1024 * 1024), "38 MB");
        assert_eq!(size_label(412 * 1024 * 1024), "412 MB");
        assert_eq!(size_label(12_897_485), "12.3 MB");
        assert_eq!(size_label(1_288_490_189), "1.2 GB");
    }

    #[test]
    fn the_summary_line_reports_what_the_report_holds() {
        let report = report(
            484,
            484,
            vec![
                finding(Severity::Warning, "servers[0]"),
                finding(Severity::Warning, "servers[1]"),
            ],
        );
        assert_eq!(
            summary_line(&report, true),
            "파일 484 · SHA-256 484 · 오류 0 · 경고 2"
        );
    }

    #[test]
    fn a_readable_file_the_launcher_refuses_says_so_in_the_summary() {
        // Nothing in the displayed report is an error, yet the strict pass
        // failed: the summary is the last line read before 업로드.
        let clean = report(484, 480, vec![finding(Severity::Warning, "servers[0]")]);
        assert!(summary_line(&clean, false).ends_with(" · 런처 배포 불가"));
        assert!(!summary_line(&clean, true).contains("런처 배포 불가"));

        // A report that already shows errors does not need the suffix; the
        // 오류 count says it.
        let broken = report(2, 0, vec![finding(Severity::Error, "servers[0]")]);
        assert!(!summary_line(&broken, false).contains("런처 배포 불가"));
    }

    #[test]
    fn findings_render_korean_severity_words() {
        let rows = finding_rows(
            &report(
                1,
                1,
                vec![
                    finding(Severity::Error, "servers[0].modules[12].artifact.MD5"),
                    finding(Severity::Warning, "servers[1]"),
                ],
            ),
            &[],
        );
        assert_eq!(rows[0].severity, "오류");
        assert!(rows[0].is_error);
        assert_eq!(rows[1].severity, "경고");
        assert!(!rows[1].is_error);
        // With no ids to name, the raw location is still better than nothing.
        assert_eq!(rows[0].location, "servers[0].modules[12].artifact.MD5");
    }

    #[test]
    fn findings_name_the_server_instead_of_its_index() {
        let ids = ["Test-1.12.2".to_owned(), "Sky-1.20.1".to_owned()];
        assert_eq!(
            finding_location("servers[0].modules[7].artifact.url", &ids),
            "Test-1.12.2 · modules[7].artifact.url"
        );
        assert_eq!(finding_location("servers[1]", &ids), "Sky-1.20.1");
        // An error about the array itself names no server.
        assert_eq!(finding_location("servers", &ids), "servers");
        assert_eq!(finding_location("version", &ids), "version");
        // An index past the end, or one that is not a number, is left alone
        // rather than dropped.
        assert_eq!(finding_location("servers[9].id", &ids), "servers[9].id");
        assert_eq!(finding_location("servers[x].id", &ids), "servers[x].id");
    }

    #[test]
    fn only_a_cache_failure_offers_the_cache_fix() {
        assert!(cache_hint("설치기가 ... 'Forge 설치 캐시 무효화'를 켜고").contains("캐시 무효화"));
        assert!(
            cache_hint("The cached Forge installation has no version manifest").contains("캐시")
        );
        assert_eq!(cache_hint("디스크가 가득 찼습니다"), "");
    }

    #[test]
    fn the_verdict_takes_eight_hex_characters() {
        assert_eq!(stamp_hash("a91f3c02deadbeef"), "a91f3c02");
        assert_eq!(stamp_hash("a91f"), "a91f");
    }

    #[test]
    fn the_verdict_tone_separates_unpublishable_from_broken() {
        let clean = report(484, 484, Vec::new());
        let warned = report(484, 484, vec![finding(Severity::Warning, "servers[0]")]);
        let broken = report(2, 0, vec![finding(Severity::Error, "servers[0]")]);

        assert_eq!(verdict_tone(&clean, true), Tone::Success);
        // Nothing in the displayed report is an error, but the launcher's own
        // pass failed: that is a warning about publishing, not a bad file.
        assert_eq!(verdict_tone(&warned, false), Tone::Warning);
        assert_eq!(verdict_tone(&warned, true), Tone::Success);
        // A report with errors is danger whatever the strict pass says.
        assert_eq!(verdict_tone(&broken, false), Tone::Danger);
        assert_eq!(verdict_tone(&broken, true), Tone::Danger);

        assert_eq!(verdict_title(Tone::Success), "런처 검증 통과");
        assert_eq!(verdict_title(Tone::Warning), "경고와 함께 통과");
        assert_eq!(verdict_title(Tone::Danger), "검증 실패");
    }

    #[test]
    fn the_verdict_detail_names_the_file_its_hash_and_its_size() {
        assert_eq!(
            verdict_detail(
                Path::new("D:/root/distribution.json"),
                "a91f3c02deadbeef",
                484
            ),
            "distribution.json · SHA-256 a91f3c02 · 파일 484"
        );
    }

    #[test]
    fn initials_take_two_latin_letters_or_one_hangul() {
        assert_eq!(initials("Test-1.12.2"), "TE");
        assert_eq!(initials("sky"), "SK");
        assert_eq!(initials("x"), "X");
        // One CJK glyph already fills the 36 px square.
        assert_eq!(initials("서버-1.20.1"), "서");
        // A digit is as good a letter as any when that is all there is.
        assert_eq!(initials("1.21"), "12");
        assert_eq!(initials("---"), "");
    }

    #[test]
    fn a_servers_hue_is_stable_and_in_range() {
        for id in ["Test-1.12.2", "Sky-1.20.1", "", "서버"] {
            let hue = hue_of(id);
            assert!((0..360).contains(&hue), "{id} gave {hue}");
            assert_eq!(hue, hue_of(id), "the same id must keep its colour");
        }
        assert_ne!(hue_of("Test-1.12.2"), hue_of("Sky-1.20.1"));
    }

    #[test]
    fn the_forge_warning_follows_the_core_threshold() {
        assert!(forge_mod_lists_warning(Loader::Forge, "1.20.3"));
        assert!(forge_mod_lists_warning(Loader::Forge, " 1.21 "));
        assert!(!forge_mod_lists_warning(Loader::Forge, "1.20.1"));
        assert!(!forge_mod_lists_warning(Loader::Forge, ""));
        assert!(!forge_mod_lists_warning(Loader::Neoforge, "1.21.1"));
    }

    #[test]
    fn a_cancelled_debounce_never_comes_due() {
        let debounce = Debounce::default();
        assert!(!debounce.due(Duration::ZERO));

        debounce.arm();
        debounce.cancel();
        assert!(
            !debounce.due(Duration::ZERO),
            "a cancelled delay must not fire"
        );

        // And it still fires once, not repeatedly, when it is left armed.
        debounce.arm();
        assert!(debounce.due(Duration::ZERO));
        assert!(!debounce.due(Duration::ZERO));
    }

    #[test]
    fn theme_names_round_trip() {
        for name in ["system", "light", "dark"] {
            assert_eq!(theme_name(theme_preference(name)), name);
        }
        assert_eq!(theme_name(theme_preference("nonsense")), "system");
    }

    #[test]
    fn publish_errors_carry_a_fix() {
        let hint = publish_hint(&Error::Http {
            url: "https://dav.example.invalid/".into(),
            message: "401 Unauthorized".into(),
        });
        assert!(hint.contains("401"), "{hint}");
        assert!(hint.contains("사용자와 비밀번호"), "{hint}");

        let plain = publish_hint(&Error::Invalid("무엇인가 잘못되었습니다".into()));
        assert_eq!(plain, "무엇인가 잘못되었습니다");
    }

    #[test]
    fn the_log_band_keeps_the_newest_rows_only() {
        let model: Rc<VecModel<LogRowData>> = Rc::new(VecModel::default());
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..(logsink::MAX_ROWS + 25) {
            tx.send(logsink::LogLine {
                time: "+00:00".into(),
                target: "test".into(),
                message: i.to_string(),
                level: "info".into(),
            })
            .unwrap();
        }
        drop(tx);

        // One tick is enough: a drain takes the whole channel, however far
        // ahead of the band a burst has got, and trims once at the end.
        drain_logs(&model, &rx);

        assert_eq!(model.row_count(), logsink::MAX_ROWS);
        assert_eq!(model.row_data(0).unwrap().message, "25");
        assert_eq!(
            model.row_data(logsink::MAX_ROWS - 1).unwrap().message,
            (logsink::MAX_ROWS + 24).to_string()
        );
    }

    #[test]
    fn the_busy_pill_names_the_job_and_counts() {
        // DESIGN-v2 §5: `배포본 생성 중 · 1:23`.
        assert_eq!(
            busy_pill("배포본 생성", Duration::from_secs(83)),
            "배포본 생성 중 · 1:23"
        );
        assert_eq!(
            busy_pill("설정 저장", Duration::ZERO),
            "설정 저장 중 · 0:00"
        );
    }

    #[test]
    fn a_failed_server_scan_reports_a_cause_instead_of_an_empty_list() {
        let dir = TempDir::new("scan-error");
        let root = dir.path();
        let base = Config::normalize_base_url("https://cdn.example.invalid/").unwrap();
        distribution::init_root(&Root::new(root, base.clone())).unwrap();
        server::create_server(
            &Root::new(root, base.clone()),
            "Test",
            &"1.12.2".parse().unwrap(),
            &CreateServerOptions::default(),
        )
        .unwrap();

        let current = GuiSettings {
            root: root.display().to_string(),
            base_url: "https://cdn.example.invalid/".into(),
            ..GuiSettings::default()
        };
        let ok = snapshot(&current, "distribution");
        assert!(ok.servers_error.is_none());
        assert_eq!(ok.servers.len(), 1);
        assert_eq!(ok.readiness[0].state, "ok");

        // One unreadable `servermeta.json` used to empty the whole list.
        let meta = root
            .join("servers")
            .join("Test-1.12.2")
            .join("servermeta.json");
        std::fs::write(&meta, "{ not json").unwrap();

        let broken = snapshot(&current, "distribution");
        let error = broken.servers_error.expect("a cause, not an empty list");
        assert!(error.contains("servermeta.json"), "{error}");
        assert!(error.contains("새로 고침"), "{error}");
        assert!(broken.servers.is_empty(), "the caller keeps its own rows");
        // …and 루트 stops claiming it was read.
        assert_eq!(broken.readiness[0].label, "루트");
        assert_eq!(broken.readiness[0].state, "error");
    }

    #[test]
    fn an_empty_snapshot_reports_everything_as_missing() {
        let snap = snapshot(&GuiSettings::default(), "distribution");
        assert!(!snap.root_ok);
        assert!(!snap.base_ok);
        assert!(!snap.dist_exists);
        assert!(!snap.install_local_enabled);
        assert!(snap.servers.is_empty());
        assert!(snap.imports.is_empty());
        assert_eq!(snap.readiness.len(), 4);
        assert!(snap.readiness.iter().all(|row| row.state == "missing"));
        // An empty value column beside a 미설정 chip reads as a rendering bug.
        assert!(snap.readiness.iter().all(|row| row.value == "미설정"));
        assert_eq!(snap.readiness[0].label, "루트");
        assert_eq!(snap.readiness[3].label, "Launcher 폴더");
    }

    #[test]
    fn a_real_root_populates_every_model() {
        let dir = TempDir::new("snapshot");
        let root = dir.path();
        let base = Config::normalize_base_url("https://cdn.example.invalid/").unwrap();

        distribution::init_root(&Root::new(root, base.clone())).unwrap();
        server::create_server(
            &Root::new(root, base),
            "Test",
            &"1.12.2".parse().unwrap(),
            &CreateServerOptions {
                forge_version: Some("14.23.5.2860".into()),
                ..CreateServerOptions::default()
            },
        )
        .unwrap();

        let modpacks = root.join("modpacks").join("curseforge");
        std::fs::create_dir_all(&modpacks).unwrap();
        // Not a zip at all: the row must say so instead of guessing.
        std::fs::write(modpacks.join("Broken-Pack.zip"), vec![0u8; 12_897_485]).unwrap();
        std::fs::write(root.join("distribution.json"), "{}").unwrap();

        let current = GuiSettings {
            root: root.display().to_string(),
            base_url: "https://cdn.example.invalid/dist".into(),
            ..GuiSettings::default()
        };
        let snap = snapshot(&current, "distribution");

        assert!(snap.root_ok && snap.base_ok && snap.root_initialised && snap.dist_exists);
        assert_eq!(snap.readiness[1].value, "https://cdn.example.invalid/dist/");
        assert_eq!(snap.readiness[1].state, "ok");

        assert_eq!(snap.servers.len(), 1);
        assert_eq!(snap.servers[0].id, "Test-1.12.2");
        assert_eq!(snap.servers[0].loader, "Forge 14.23.5.2860");
        assert_eq!(snap.servers[0].counts, "모드 0 · 파일 0 · 라이브러리 0");
        assert!(snap.servers[0].path.ends_with("Test-1.12.2"));
        assert_eq!(snap.servers[0].initials, "TE");
        assert!((0..360).contains(&snap.servers[0].hue));
        // No icon.png in the folder, so the row falls back to its initials.
        assert_eq!(snap.server_icons, vec![None]);

        // …and finds one when the server has put it there.
        let icon = root.join("servers").join("Test-1.12.2").join("icon.png");
        std::fs::write(&icon, b"not really a png, but it is a file").unwrap();
        let snap = snapshot(&current, "distribution");
        assert_eq!(snap.server_icons, vec![Some(icon)]);

        assert_eq!(snap.imports.len(), 1);
        assert_eq!(snap.imports[0].file_name, "Broken-Pack.zip");
        assert_eq!(snap.imports[0].format, "알 수 없음");
        assert!(!snap.imports[0].selectable);
        assert_eq!(snap.imports[0].size_label, "12.3 MB");
        assert_eq!(snap.import_paths.len(), 1);
        assert!(snap.import_paths[0].ends_with("Broken-Pack.zip"));
    }

    #[test]
    fn a_root_without_distrometa_is_reported_as_uninitialised() {
        let dir = TempDir::new("uninit");
        let current = GuiSettings {
            root: dir.path().display().to_string(),
            base_url: "https://cdn.example.invalid/".into(),
            ..GuiSettings::default()
        };
        let snap = snapshot(&current, "distribution");
        assert!(snap.root_ok);
        assert!(!snap.root_initialised);
        assert!(!snap.dist_exists);
    }
}
