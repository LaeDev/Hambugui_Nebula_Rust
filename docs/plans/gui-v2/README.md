# GUI v2 — the Quiet Studio shell

The record for the component library in `crates/nebula/ui/lib/`, the screens in
`crates/nebula/ui/screens/`, and the contract between them and
`crates/nebula/src/gui/`. The design it implements is
[`../DESIGN-v2.md`](../DESIGN-v2.md); §-references below point there. This
supersedes the old `docs/plans/gui/`, which described the build-sheet UI that
this replaced.

`states/*.json` feed `slint-viewer --load-data`, which sets the exported globals
directly, so a state can be reviewed without a root, a network, or a build.
`--load-data` cannot move keyboard focus or run a callback: these are pictures
of static state. Interaction is checked by running the real binary
(`cargo run -p nebula -- gui`).

## 1. The component library

| File | Exports | Notes |
|---|---|---|
| `lib/theme.slint` | `Theme`, `ThemePreference`, `Tone` | Every token in §2. `Theme.dark` follows `preference`, falling back to `Palette.color-scheme`. `reduced-motion` zeroes `fast`/`base`/`slow`. `tone-fg`/`tone-soft`/`tone-icon` are the single source of what a tone looks like. |
| `lib/typography.slint` | `Type` | Sizes and weights only; Pretendard is selected by weight from the three bundled TTFs. |
| `lib/data.slint` | the globals and structs below | No component: `slint-viewer --check` on it reports "No component found", which is expected. |
| `lib/button.slint` | `Button`, `ButtonVariant`, `ButtonSize` | `primary`/`secondary`/`ghost`/`danger`, `md` 36 px / `lg` 44 px visual inside a 44/52 px hit box, optional 18 px leading icon, `loading` draws a 2 px indeterminate bar along the bottom edge and names the button. `danger` is a soft-red fill with a red label (not a solid fill), added for 삭제 actions (DESIGN-v2 §8.7). |
| `lib/icon-button.slint` | `IconButton` | 36×36 ghost square in a 44 px hit box. `a11y-label` is required — it has no visible text. |
| `lib/text-field.slint` | `TextField` | Label / 40 px input / helper or error. `password` adds the eye toggle, `read-only` renders mono and keeps the field selectable, `action-icon` puts an `IconButton` inside the input, `leading-icon` puts an 18 px decorative glyph before the text (mirrors the trailing slot; added for the 모드 검색 field, §8.7). `select-all()` and `focus-input()` are public; `forward-focus` reaches the input, so `forward-focus: field` works on a form. |
| `lib/switch.slint` | `Switch` | 44 px row, 40×22 track, 18 px knob, optional helper line. |
| `lib/segmented.slint` | `SegmentedControl` | `options: [string]`, `selected: int` (`-1` selects nothing), Left/Right arrows move the selection. `selected` is strictly `in`: the caller derives it from the value it owns and updates that value in `changed`, so the track can never show a selection the caller refused. `option-enabled: [bool]` disables single options — **empty (the default) enables all**, so every existing caller is unchanged; a disabled option dims to `Theme.text-3`, reports `accessible-enabled: false`, takes no hover, click or arrow, and is the reason the 제공자 control no longer refuses CurseForge silently. Arrow keys move one option at a time and refuse a disabled neighbour rather than hopping over it (a Slint function has no loop; both callers have two options). |
| `lib/pill-strip.slint` | `PillStrip` | The 모드 page's server selector (§8.2): a wrapping grid of equal-width pills under a caption, `accessible-role: radio-group`. Deliberately not a `SegmentedControl` — Left/Right/Home/End move a roving focus ring **without selecting**, Enter/Space or a click commits, because a selection change re-runs the installed-list job. |
| `lib/card.slint` | `Card`, `CardHeader` | Surface + one hairline + r-lg + 20 padding. `CardHeader` is a plain child, so a card's title row and its actions compose like anything else. |
| `lib/list-row.slint` | `ListRow` | 56 px, or 72 px when the optional third `description` line is set (§8.7; a 찾기 result's blurb — an installed mod has none). `selectable` is what makes the row a control; a server row is not one (its trailing chevron is the single tab stop). Avatar = `avatar` image, else `initials` on a `hue`-tinted fill, else `icon`. `accessible-enabled` follows `enabled`, so a dimmed row reads as disabled. |
| `lib/chip.slint` | `Chip` | 24 px, always carries its text, so tone is never the only carrier. |
| `lib/callout.slint` | `Callout`, `VerdictBanner` | `Callout` takes an optional trailing action as a child. `VerdictBanner` is the signature element (§1). Both join their `accessible-label` conditionally — title alone when the second string is empty, otherwise the two separated by one space — because the copy already ends its own sentences. |
| `lib/progress.slint` | `ProgressBar` | Indeterminate only, driven by `Theme.progress-phase`. Nothing this app does reports real progress, and a determinate bar over a Forge install would be a fake number. |
| `lib/progress-row.slint` | `ProgressRow` | The single progress carrier for a catalog job (§8.4, §8.7): a 4 px bar it draws itself (not a reused `ProgressBar`) plus one mono text line, with `accessible-role: progress-indicator` and `accessible-live-region: polite` on the row itself so a reader announces the text as it changes. `fraction` is 0–1 for a counted job, negative for indeterminate (slides on `Theme.progress-phase`). |
| `lib/nav-rail.slint` | `NavRail` | 72 px, five pages (빌드/서버/모드/가져오기/배포) then 설정 at the bottom, active = 56×56 accent-soft pill. Ctrl/Cmd+1..6 selects a page, handled in `app.slint` so the keys can be captured before a text field sees them. |
| `lib/top-bar.slint` | `TopBar` | 56 px title row + the busy status pill + the theme toggle, with the 2 px indeterminate bar on its bottom edge. |
| `lib/dialog.slint` | `Dialog` | Scrim + centred 560 px card. Body and footer are passed as children. **Has no scroller and must not get one** (§3, §8.4) — a body longer than the card is a body with a list in it, and the list owns its own scrolling (`install-confirm`'s dependency `ListView`, `version-picker`'s version list). While a job is running the host clears `closable` and every control inside the open dialog gates on `SheetData.busy`, so a dialog open when the job starts stays open with its controls disabled until the job ends (§8.6) — a job started from a dialog is already writing to disk. |
| `lib/toast.slint` | `Toast` | One notice; `app.slint` stacks them and the host owns the 4 s timer. |
| `lib/empty-state.slint` | `EmptyState` | Centred glyph, two lines, and up to two buttons as children. |
| `lib/key-value.slint` | `KeyValue` | Key / value / trailing status, used by the readiness and target summaries. |
| `lib/log-view.slint` | `LogView` | `Flickable` + `for`, pinned to the end. |

Screens: `screens/{build,servers,import,publish,settings,mods}.slint`.
`servers.slint` also exports `NewServerForm` (the dialog's body) and
`settings.slint` also exports `SettingsFooter` (the sticky save bar, which
`app.slint` keeps outside the scrolling area). `screens/mods.slint` exports
the 모드 page itself, `ModsScreen`, and eight dialog body components — one per
`DialogKind` the 모드 page and the server row menu open — each declaring its
own `forward-focus` (DESIGN-v2 §8.7):

| Component | `DialogKind` | Initial focus |
|---|---|---|
| `ModDetailDialogBody` | `mod-detail` | the namespace `SegmentedControl` |
| `InstallConfirmDialogBody` | `install-confirm` | the primary (설치) button |
| `VersionPickerDialogBody` | `version-picker` | 취소 |
| `UpdatePreviewDialogBody` | `update-preview` | the primary (업데이트) button |
| `ModDeleteDialogBody` | `mod-delete` | 취소 (never the danger button) |
| `ServerMenuDialogBody` | `server-menu` | 모드 관리 |
| `ServerDuplicateDialogBody` | `server-duplicate` | the new-id `TextField` |
| `ServerDeleteDialogBody` | `server-delete` | the confirmation `TextField` |

`ui/lib/data.slint` gains the `ModsData` global (server selector and scope,
찾기 controls and results, 설치됨 rows, the quiet-job flags, the one progress
carrier, and the staged fields for each of the eight dialogs above) and the
`ModsActions` global (the callbacks those controls and dialogs call out on) —
see `data.slint` itself for the full property/callback list rather than
duplicating it here.

Every interactive component sets `accessible-role`, a Korean
`accessible-label`, `accessible-enabled`, `accessible-id` (§9),
hover/pressed/focus/disabled states, `mouse-cursor: pointer`, Enter/Space
activation, and a ≥ 44 px hit target. Decorative glyphs inside a labelled
control set `accessible-role: none`.

## 2. The wiring contract

`crates/nebula/src/gui/mod.rs` drives the UI entirely through these globals.

### `Theme` (host writes)

| Property | Type | Meaning |
|---|---|---|
| `preference` | `ThemePreference` | `system` / `light` / `dark`. |
| `reduced-motion` | `bool` | Zeroes every duration. |
| `progress-phase` | `float` | 0–1, advanced by the 100 ms tick while a job runs; every indeterminate bar reads it. |
| `focus-demo` | `bool` | Screenshot aid: draws every focus ring at once. |
| `screenshot` | `bool` | Screenshot aid: raises the window minimum to the 720×960 design default. |

### `AppData` — the shell

| Property | Type | Meaning |
|---|---|---|
| `page` | `AppPage` (`build`/`servers`/`import`/`publish`/`settings`) | The visible page. Written **only** by `AppData.go`. |
| `dialog` | `DialogKind` (`none`/`new-server`) | Which modal is open. Only the host writes it, through `open-dialog`/`close-dialog`. |
| `toasts` | `[ToastData]` | At most three, newest last. |
| `page-live` | `bool` (out) | `!SheetData.busy && dialog == none`. Every page's controls gate on this; the nav rail deliberately does not. |
| `go(AppPage)` | function | The one way to change page: sets `page` and calls `navigate`, and **no-ops when the page is unchanged** so opening a page twice does not re-read the disk twice. All five call sites use it. |

### `SheetData` — the build page

`readiness: [ReadinessRowData]`, `servers: [ServerRowData]`,
`server-icons: [image]`, `findings: [FindingRowData]`, `log: [LogRowData]`,
`summary: string`, `artifacts/sha256-count/errors/warnings: int`,
`verdict-tone: Tone`, `verdict-title: string`, `verdict-detail: string`,
`busy: bool`, `busy-label: string`, `error-text/-kind/-hint: string`,
`has-root: bool`, `root-initialised: bool`, `can-generate: bool`,
`install-local-enabled: bool`, `loading: bool`, `servers-error: string`, and
the `in-out` values `distro-name`, `install-local`, `invalidate-cache`,
`discard-output`, `findings-expanded`.

| Property | Meaning |
|---|---|
| `verdict-tone` | `success` when the strict pass is clean, `warning` when only the strict pass failed, `danger` when the displayed report has errors. |
| `verdict-title` | `런처 검증 통과` / `경고와 함께 통과` / `검증 실패`. **Empty is what hides the banner.** |
| `verdict-detail` | `distribution.json · SHA-256 a91f3c02 · 파일 484`. |
| `summary` | The same verdict as one line; it is the `accessible-label` of the chip row that shows the four counts. |
| `error-kind` | `generate`, `validate` or `init-root`. The failure card turns it into `생성 실패` / `검증 실패` / `초기화 실패`, and `retry-job` re-runs it. |
| `error-hint` | One extra line under 다시 시도 when the cause has a known fix. |
| `server-icons` | Parallel to `servers`: the row's own `icon.png`/`icon.jpg`, or an empty image. |
| `loading` | True from launch until the first scan lands. Every readiness chip reads 불러오는 중 and both list pages show a 불러오는 중 empty state, because an empty list under it means "not read yet", never "there is nothing here". |
| `servers-error` | Why the last scan failed, naming the offending `servermeta.json` and what to do. The 서버 page renders it as a danger `Callout`, drops its `N개` count, and **keeps the rows the last successful scan produced** — the host does not replace them. The 루트 readiness row goes to `state: "error"` (`읽기 실패`), because 확인됨 claims the root was read. |
| `findings-expanded` | 더 보기 on the findings list. Here rather than in the screen so `clear_result()` resets it: six findings from another file are another list. |

`ReadinessRowData.state` is `ok`, `missing`, `error` or `loading`.

### `SheetActions`

`generate()`, `validate-existing()`, `open-folder(string)`, `refresh()`,
`init-root()`, `navigate(AppPage)`,
`create-server(string, string, Loader, string)`,
`lookup-loader-version(Loader, string)`, `import-modpack()`,
`save-settings()`, `create-root(string)`, `clear-log()`,
`publish(PublishTarget, bool)`, `connect-gdrive()`.

| Callback | Contract |
|---|---|
| `generate()`, `validate-existing()`, `import-modpack()`, `save-settings()` | **No arguments.** Every value is an `in-out` property the host reads back, so a retry runs with what is on screen *now* rather than replaying the arguments of the click that failed. Passing them was 13 parameters the host discarded. |
| `navigate(AppPage)` | Fired by `AppData.go` only when the page actually changed. The host re-reads the root on 가져오기 **and** 서버. |
| `retry-job()` | Re-runs the job named by `error-kind` with the field values that are on screen *now*. |
| `settings-edited()` | A settings field changed. The host debounces it (300 ms), re-validates, and recomputes `SettingsData.dirty`; do not call it per frame. |
| `appearance-changed(ThemePreference, bool)` | Theme and reduced motion apply and persist immediately, whether or not the root and base URL are valid. |
| `dismiss-toast(int)` | Takes the toast's **id**, never its index: the 4 s timer fires long after a row may have moved. |
| `open-dialog(DialogKind)` | Refused while `SheetData.busy`: a dialog over a running job offers actions the job lock has already declined. |
| `close-dialog()` | Closes it and clears the form's errors, which belonged to the attempt that raised them. |

Every settings field calls `save-settings(...)` on `accepted` (Enter) and
`settings-edited()` on `edited`.

### `PublishData`, `NewServerData`, `ImportData`, `SettingsData`

Unchanged from the v1 contract except:

| Property | Meaning |
|---|---|
| `ImportData.result-line` | `서버 RL-1.12.2` after a successful import; cleared when the next import starts. |
| `PublishData.busy` | True only while the upload runs, so 업로드 spins and a build elsewhere does not spin it. |
| `SettingsData.dirty` | The form differs from what is on disk. The sticky footer's caption reads `저장되지 않은 변경` / `변경 사항 없음`. |
| `SettingsData.error-text` | A save that failed for a reason no field owns — the disk, or the settings file. A danger `Callout` at the top of 기본, plus a `설정 저장 실패` toast. Never filed under 루트 폴더, which is not at fault. |
| `SettingsData.focus-field` / `focus-tick` | A refused save names the field to put the caret in (`root` / `base-url`) and bumps the counter. The screen watches the **counter**, so a second refusal on the same field still moves the focus. |

`PublishData.can-publish` is still gated on the **strict** validation of the
current file name, not on the report the findings list shows. The 배포 screen
adds one gate the host cannot compute — whether the chosen target is configured
— because the WebDAV and Drive values are the UI's own `in-out` form state and
the host is not told when they change. Its blocked line is layered: no target →
target not configured (naming the missing setting) → `blocked-reason`.

### Busy

`SheetData.busy` is the global job lock: it is what disables every control that
starts a job, and navigation is deliberately *not* one of those.
`NewServerData.busy`, `NewServerData.lookup-busy`, `ImportData.busy`,
`PublishData.busy` and `SettingsData.busy` are true only for their own job, and
only decide which primary shows its loading bar.

`SettingsData.gdrive-busy` is **not** one of them: Google 계정 연결 waits on a
browser rather than on this machine, so it runs outside the global lock. A
build, an import and a save all still work while a consent screen is open, and
`gdrive-busy` alone is what refuses a second connect.

`SheetData.busy-label` is the pill's whole text, formatted by the host:
`<job> 중 · m:ss` (DESIGN-v2 §5).

`LogRowData.level` is `error`, `warn` or anything else; the row colours the
message `danger` / `warning` / `text` from it.

## 3. What changed from the v1 contract

| v1 | v2 | Why |
|---|---|---|
| `SheetData.page` | `AppData.page` | Navigation belongs to the shell, not to the build page. |
| `AppPage { sheet, new-server, import, settings }` | `AppPage { build, servers, import, publish, settings }` | Five pages behind a nav rail; 새 서버 became a dialog and 배포 got a page of its own. |
| `SheetData.stamped: bool` | *(dropped)* | The paper stamp is gone. `verdict-title != ""` is what shows the banner now. |
| `SheetData.stamp-hash`, `stamp-file` | `verdict-detail` | One line: file, hash prefix and file count. |
| — | `verdict-tone`, `verdict-title` | The banner speaks in three registers, not two. |
| — | `artifacts`, `sha256-count`, `errors`, `warnings` | The summary is four chips; `summary` stays as their accessible label. |
| `ServerRowData.{initials,hue}` | added | The avatar fallback when a server has no `icon.png`. |
| — | `SheetData.server-icons: [image]` | A parallel model, not a field of `ServerRowData`: `--load-data` cannot write an empty image into a struct, which would make every state file listing a server unrenderable. |
| — | `AppData.toasts`, `AppData.dialog`, `dismiss-toast`, `open-dialog`, `close-dialog` | §7. |
| — | `ImportData.result-line`, `SettingsData.dirty` | §4 asks for an import result and a save-bar caption. |
| `ButtonKind` | `ButtonVariant` + `ButtonSize` | Four variants and two sizes. |

**2026-09-11 update — the table above is v1→v2 only, not current.** `AppPage`
has since gained `mods` (`build, servers, mods, import, publish, settings` —
six pages, not five) and `DialogKind` has grown eight 모드/서버 kinds beyond
`new-server` (`mod-detail`, `install-confirm`, `version-picker`,
`update-preview`, `mod-delete`, `server-menu`, `server-delete`,
`server-duplicate`, per §1 above). See `crates/nebula/ui/lib/data.slint` for
the authoritative enums rather than this historical table.

The shapes `2026-09-10-ux-fixes.md` decided all survive — one job at a time, a
strict gate the displayed report cannot override, an elapsed-time busy pill,
per-screen busy flags, debounced settings validation, `appearance-changed` and
`retry-job`. The 2026-09-10 review moved four of them without changing what
they promise:

| Was | Now | Why |
|---|---|---|
| The busy pill read `작업 중: 배포본 생성 · 0초` | `배포본 생성 중 · 1:23` | DESIGN-v2 §5 says so, and `m:ss` is one column rather than prose that changes width. |
| Google 계정 연결 took the global lock | its own `gdrive-busy` | It waits on a browser, not on this machine. Five minutes of a consent screen is five minutes in which everything else still makes sense. |
| The strict re-validation ran inline on the tick | a quiet worker thread | Reading and validating a large `distribution.json` is tens of milliseconds, on the event loop, every time a file name settles. |
| `validate_settings_form` probed the disk on the tick | the same worker | `is_dir` on a dead network path blocks for seconds. |

Neither quiet probe shows a spinner or takes the lock, and each drops its own
answer if the fields moved while it was out.

## 4. The states

| State | Shows |
|---|---|
| `build-empty` | First run: every readiness row 미설정, the primary disabled with its helper line, an empty log. |
| `build-ready` | Configured and idle. |
| `build-busy` | A running job: the top-bar status pill with elapsed time, both indeterminate bars mid-sweep, the primary loading. Also the only state with more log rows than the box holds, so it is what proves the log is pinned to its newest line. |
| `build-success` | A clean run: the success banner, the four summary chips, two warnings in the findings list. |
| `build-warning` | Readable but unpublishable: the strict pass failed, so the banner is amber and the `런처 배포 불가` callout appears. |
| `build-failed` | A job that never produced a file: the danger callout with its job prefix, 다시 시도, the cache hint, and the uninitialised-root callout. |
| `servers` | Three servers with initials avatars, loader chips and 폴더 열기. |
| `servers-empty` | The empty state; the header's two actions step aside so the pair is not on screen twice. |
| `servers-error` | A scan that failed: the danger callout naming the `servermeta.json` that broke it, the rows from the last successful scan still listed, the `N개` count withdrawn, and the toast. The state QA defect D1 was about. |
| `new-server-dialog` | The dialog over the servers page, NeoForge selected, with its patch warning. |
| `mods-installed` | 설치됨: a mix of 관리 and 직접 추가 rows, neutral namespace chips, and one 업데이트 가능 accent chip. |
| `mods-installed-empty` | 설치됨 with no mods: the `EmptyState` pointing at 찾기. |
| `mods-installed-many` | Over eight installed mods: the capped `ListView` and the `모두 보기` ghost button. |
| `mods-search` | 찾기 with search results, the provider and sort `SegmentedControl`s. |
| `mods-search-empty` | 찾기 with no results: `검색 결과가 없습니다 — 다른 검색어나 제공자를 시도하세요`. |
| `mods-search-loading` | 찾기 mid-search: the `ProgressRow` and the `Modrinth에서 검색 중` helper line. |
| `mods-search-error` | 찾기 with a danger `Callout` naming a `CatalogError` variant and `다시 시도`. |
| `mods-search-results-progress` | 찾기 results with an install job's `ProgressRow` running above them. |
| `mods-search-no-cf-key` | 찾기 without a CurseForge key: the provider control's second option is disabled (`option-enabled`), dimmed and not hoverable, with the 설정에서… helper under it. |
| `mods-search-rollback` | A rollback that could not remove everything: the failure `Callout` names the surviving path and gains `폴더 열기` beside 다시 시도 (`failure-folder` non-empty). `mods-search` is the clean-rollback twin, where the button is absent. |
| `mods-search-server-changed` | §8.4's `서버가 바뀌었습니다 — 다시 검색하세요` refusal: `failure-can-retry` false, so the `Callout` carries no button at all — the plan is dead and the copy already says what to do. |
| `mods-install-confirm` | The `install-confirm` dialog: resolved version line, namespace control, required/optional dependency rows. |
| `mods-detail` | The `mod-detail` dialog: description, `KeyValue` rows, namespace control, 다른 버전/프로젝트 페이지/삭제. |
| `mods-version-picker` | The `version-picker` dialog: version rows with 정식/베타/알파 chips and a downgrade confirmation line. |
| `mods-update-preview` | The `update-preview` dialog: one row per pending update and the `N개 업데이트` primary. |
| `mods-delete` | The `mod-delete` dialog, including the "이 모드를 필요로 하는 모드가 있습니다" dependents `Callout`. |
| `server-menu-dialog` | The 서버 row's menu dialog: 모드 관리 / 폴더 열기 / 복제 / 삭제. |
| `server-duplicate-dialog` | The `server-duplicate` dialog with its new-id field. |
| `server-delete-dialog` | The `server-delete` dialog: `삭제` disabled until the typed id matches the directory name exactly. |
| `import` | All three format labels, an unselectable `알 수 없음` row, and a manual-download box. |
| `publish` | The 배포 page after a WebDAV upload: the result callout and the selectable distribution URL. |
| `publish-blocked` | A target chosen but not configured: 업로드 disabled, the reason naming the missing setting, 설정 열기 beside the 미설정 chip. |
| `settings` | All four groups, Google Drive connected, and the sticky save bar reading 저장되지 않은 변경. |
| `toast` | Two stacked toasts over the build page. |
| `dark-build-success`, `dark-servers` | The same two pages in dark. |
| `dark-mods-installed`, `dark-mods-installed-empty`, `dark-mods-installed-many`, `dark-mods-search`, `dark-mods-search-empty`, `dark-mods-search-error`, `dark-mods-search-loading`, `dark-mods-search-results-progress`, `dark-mods-search-no-cf-key`, `dark-mods-search-rollback`, `dark-mods-search-server-changed`, `dark-mods-install-confirm`, `dark-mods-detail`, `dark-mods-version-picker`, `dark-mods-update-preview`, `dark-mods-delete`, `dark-server-menu-dialog`, `dark-server-duplicate-dialog`, `dark-server-delete-dialog` | The dark twins of the 모드-page and server-menu/duplicate/delete states above. |
| `focus-demo` | `Theme.focus-demo` renders every focus ring at once, because `--load-data` cannot move keyboard focus. |

## 5. Regenerating a screenshot

From the repository root, one command per state (the `.png` sits one directory
above the `.json` it came from):

```sh
SLINT_SCALE_FACTOR=1 slint-viewer --backend software \
  --load-data docs/plans/gui-v2/states/build-success.json \
  --screenshot docs/plans/gui-v2/build-success.png \
  crates/nebula/ui/app.slint
```

All of them:

```sh
for state in docs/plans/gui-v2/states/*.json; do
  SLINT_SCALE_FACTOR=1 slint-viewer --backend software --load-data "$state" \
    --screenshot "docs/plans/gui-v2/$(basename "$state" .json).png" \
    crates/nebula/ui/app.slint
done
```

`--backend software` matches the renderer the shipped binary uses, and
`SLINT_SCALE_FACTOR=1` keeps the output at 720×960 on a HiDPI machine. Check the
sources first — `slint-viewer --check crates/nebula/ui/app.slint` exits non-zero
on an error and prints warnings either way.

Adding a property to a global means adding it to the states that exercise it;
`--load-data` silently ignores names it does not recognise, so a typo shows up
as a state that did not change. The states are generated from one base in
`states/*.json`; keep them in sync by editing them together.

## 6. Deviations from DESIGN-v2, and why

- **§3 Dialog: no 200 ms fade/scale.** Slint 1.17's software renderer animates
  `opacity` from zero on the frame an element is created, so the first frame of
  a dialog that fades in is blank — which is exactly what `--screenshot`
  captures, and what a slow machine shows. The dialog is instant in both.
- **§2/§3 Indeterminate progress is host-driven.** `animate x` with
  `iteration-count: -1` renders only its first frame into a screenshot, so every
  busy state would be unreviewable and the bar unverifiable. `Theme.progress-phase`
  is advanced by the 100 ms tick that already runs for the busy pill; reduced
  motion parks it at 0.35, where the segment is still visible.
- **§4 Findings: `더 보기` reveals the rest in place** rather than the list
  scrolling on its own. A nested scroll area inside a scrolling page fights the
  wheel; `for index in shown-count` is one expression.
- **§5 The page does not scroll the result card into view after a build.**
  `ScrollView.viewport-y` is two-way bound to its scrollbar inside the widget,
  so binding it from outside makes the page unscrollable, and an imperative
  write needs the result card's `y` — which is inside an `if` and cannot be read
  from outside it. The success toast and the banner's own weight carry the
  signal instead. Accepted ceiling.
- **§5 copy: the build toast says `배포본을 검증했습니다`, not
  `배포본을 생성했습니다`.** One verdict path serves both 배포본 생성 and
  기존 파일 검증, and a toast that says "생성" after a validate would be a lie.
  The three toasts follow the tone: 검증했습니다 / 경고와 함께 통과했습니다 /
  검증에 실패했습니다.
- **§4 the import result callout carries the created server, not counts.**
  `import_modpack` reports the directory it made and its manual-download list;
  it does not return a file count, and inventing one would be a fake stat.
- **§3 TextField focus ring is a 2 px accent border on the input, not an offset
  ring.** The label and the helper line sit outside the input; a ring around all
  three would enclose text that is not the control.
- **`ui/lib/data.slint` is not in the §7 file list.** The globals and structs
  need a home that both the components and the screens can import without a
  cycle; keeping them in `theme.slint` would mix tokens with the host contract.
- **`Theme.screenshot`.** `slint-viewer --screenshot` sizes a headless window to
  the window's *minimum*: a `Window`'s own `preferred-width` never reaches the
  window constraints, and a literal `width` is refused alongside `min-width`
  because it would make the window fixed-size. The flag raises the minimum to
  720×960 for a render and changes nothing at runtime.

## 7. Three Slint traps this UI is built around

- **A layout overwrites a child's `width`/`height` when the child has children
  of its own**, using the children's preferred size instead — so `height: 52px`
  on a `Button` renders 26 px, and `min-height`/`max-height` do not save it. The
  size has to be stated on the innermost visual (`Button`'s `fill`,
  `IconButton`'s square), whose preferred size then propagates up.
- **A layout anchors a child to the *start* of the cross axis, and discards an
  explicit `x`/`y` on an axis it pads.** A `HorizontalLayout` puts a child
  shorter than the row at its top, not its middle — `alignment` only moves the
  main axis — and a `VerticalLayout` puts a narrower child at its left. That is
  what left every glyph in the app 5–17 px out of centre until 2026-09-11. An
  explicit `y` corrects it only while the layout has no `padding-top`/`-bottom`,
  which is why two such nudges here were dead code before they were removed. The
  fix that holds either way is a one-child layout to centre in:
  `VerticalLayout { alignment: center; … }` inside a row,
  `HorizontalLayout { alignment: center; … }` inside a column. Dropping the
  child's cross-axis size instead does centre it — `image-fit: contain` then
  fits the glyph — but it makes every ancestor stretchable, which grew the
  `servers-error` callout to the height of the page.
- **A layout's computed size is not readable as a property.** `body.min-height`
  reads the *unset* `min-height` property, which is `0`, so
  `min-height: body.min-height` silently removes the constraint the children
  would otherwise have given. Nothing here reads `x.min-height`/`x.preferred-height`
  off a layout.

## 8. Two things that did not change

**Log times are elapsed, not a clock.** The log shows `+MM:SS` since launch and
the verdict banner shows no time at all. Rust's standard library exposes no
local-time offset, the GUI adds no crate to get one, and a UTC clock shown to a
KST user is worse than no clock. The formatting is one function,
`crates/nebula/src/gui/logsink.rs::elapsed`.

**Where the GUI keeps its settings.** `<config dir>/nebula/settings.json`, where
the config dir is `%APPDATA%` on Windows, `$HOME/Library/Application Support` on
macOS, and `$XDG_CONFIG_HOME` (or `~/.config`) elsewhere — the same rule as
`cli::config_dir()`, which is also where the Google Drive refresh token lands.
`NEBULA_CONFIG_DIR` overrides that directory outright (used as given, without
the `nebula` suffix). Set it when testing so a run cannot touch real settings:

```sh
NEBULA_CONFIG_DIR=/tmp/nebula-test ROOT=/tmp/nebula-root \
  BASE_URL=https://cdn.example.invalid/ cargo run -p nebula -- gui
```

When the file is missing the settings are seeded from `Config::from_env()`, so a
user who already ran the CLI sees their root on the first launch. Once the file
exists it wins over the environment.


## 9. The automation contract (`accessible-id` → UIA AutomationId)

Slint maps `accessible-id` to accesskit's `author_id`, which the Windows
provider exposes as **AutomationId**. Before this, the app set none, and
`docs/verification/gui-qa/drive.ps1` had to find every control by its Korean
`Name` — a caption that may be reworded at any time. These are the stable
handles; treat them as the API a QA script is written against.

**Shell**

| Id | Control |
|---|---|
| `nav-build`, `nav-servers`, `nav-mods`, `nav-import`, `nav-publish`, `nav-settings` | The six nav-rail tabs. |
| `theme-toggle` | The top bar's light/dark button. |
| `log-clear` | 로그 지우기 in the log card. |
| `toast-<id>`, `toast-<id>-close` | A toast and its close button; `<id>` is `ToastData.id`. |

**빌드**

`build-open-settings`, `build-init-root`, `build-name`, `build-install-local`,
`build-invalidate-cache`, `build-discard-output`, `build-generate`,
`build-validate`, `build-retry`, `build-more-findings`.

**서버 and the 새 서버 dialog**

`servers-refresh`, `servers-import`, `servers-new`, `servers-empty-import`,
`servers-empty-new`, `servers-open-settings`; per row `server-row-<id>` and
**`servers-menu-<id>`** (the trailing chevron that opens the `server-menu`
dialog — this replaced the direct `server-open-<id>` 폴더 열기 button when the
row's control became a menu, §8.5), where `<id>` is the directory name. The
dialog is `new-server-dialog` / `new-server-dialog-close`, its fields
`new-server-id`, `new-server-mc`, `new-server-loader` (options
`new-server-loader-0..3`), `new-server-loader-version`, `new-server-latest`,
`new-server-recommended`, and its footer `new-server-cancel` /
`new-server-submit`.

**모드, and the server-menu/duplicate/delete dialogs (§8.1–§8.5)**

`mods-server` (the `PillStrip`, options `mods-server-0..`), `mods-view` (the
설치됨/찾기 `SegmentedControl`), `mods-show-all`, `mods-check-updates`,
`mods-update-all`, `mods-retry`, `mods-failure-folder` (the 폴더 열기 the
failure `Callout` grows when a rollback left files behind); per installed row `mods-row-<file>` (the
자세히 chevron, `<file>` is the mod's file name); 찾기: `mods-search`,
`mods-provider`, `mods-sort`, `mods-retry-search`, `mods-search-more`, and per
result row `mods-install-<provider>-<project id>` (provider-scoped, since
slugs/ids are re-assignable per provider) plus `mods-manual-url-<file name>`
and `mods-manual-folder` for the manual-download list. Dialog bodies:
`mod-detail-namespace`, `mod-detail-versions`, `mod-detail-project`,
`mod-detail-delete`, `mod-detail-cancel`, `mod-detail-apply`;
`install-confirm-versions`, `install-confirm-namespace`,
`install-confirm-optional`, `install-confirm-dep-<project id>`,
`install-confirm-cancel`, `install-confirm-submit`; `version-row-<version id>`,
`version-picker-cancel`; `update-preview-cancel`, `update-preview-submit`;
`mod-delete-cancel`, `mod-delete-submit`; `server-menu-mods`,
`server-menu-open-folder`, `server-menu-duplicate`, `server-menu-delete`,
`server-menu-cancel`; `server-duplicate-id`, `server-duplicate-cancel`,
`server-duplicate-submit`; `server-delete-confirm`, `server-delete-cancel`,
`server-delete-submit`. Each of the eight dialogs also carries its own wrapper
`Dialog` id (mirroring `new-server-dialog`, not derived from a body id):
`mod-detail-dialog`, `install-confirm-dialog`, `version-picker-dialog`,
`update-preview-dialog`, `mod-delete-dialog`, `server-menu-dialog`,
`server-duplicate-dialog`, `server-delete-dialog`.

**가져오기**

`import-open-folder`, `import-empty-open-folder`, `import-row-<file name>`,
`import-id`, `import-run`, `import-manual-url-<file name>`. The rows are keyed
by file name rather than by position, because a pack dropped into `modpacks`
re-sorts the list.

**배포**

`publish-target` (options `publish-target-0`/`-1`), `publish-open-settings`,
`publish-only-changed`, `publish-upload`, `publish-url`.

**설정**

`settings-root`, `settings-create-root`, `settings-base-url`, `settings-java`,
`settings-helios`, `settings-webdav-url`, `settings-webdav-user`,
`settings-webdav-password`, `settings-webdav-public-url`,
`settings-gdrive-client-id`, `settings-gdrive-client-secret`,
`settings-gdrive-folder-id`, `settings-connect-gdrive`, `settings-curseforge-key`
(the CurseForge API 키 field, §8.1), `settings-theme` (options
`settings-theme-0..2`), `settings-reduced-motion`, `settings-save`.

**Derived suffixes.** A `SegmentedControl` option is `<id>-<index>`. A
`TextField`'s password eye is `<id>-reveal` and its trailing action button
`<id>-action`. A `Dialog`'s close button is `<id>-close`.

Adding a control means adding its id here. `docs/verification/gui-qa/dump-tree.ps1`
prints the live tree and counts the ids, which is how the list is checked.
