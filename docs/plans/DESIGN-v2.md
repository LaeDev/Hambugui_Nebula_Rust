# Nebula GUI — Design System v2 ("Quiet Studio")

Supersedes `DESIGN.md` (the build-sheet direction). The user asked for a cleaner, modern interface that does not follow the launcher's blocky design rules. This record is the spec for Task 6d: a new component library and screens built from scratch in Slint 1.17.1, reusing the Rust wiring contract (globals/callbacks) where it fits.

## 1. Direction

**Quiet Studio.** A calm, modern desktop tool in the register of Modrinth App / Linear / Raycast: soft layered surfaces, generous whitespace, 1 px hairline borders instead of shadows, one accent used for the primary action and the active navigation item, semantic colours for status only, Pretendard for every string, Lucide icons at 20 px. Nothing skeuomorphic (the paper stamp is gone), nothing blocky, no gradients, no glass, no glow, no decorative motion.

Research inputs: Modrinth App design tokens (three surface levels, radius 8–20 px, 4/8/12/16/24 gaps), 2026 desktop trends (soft UI, progressive disclosure, minimal chrome), antislop-ui (no blue-purple gradients, no bento filler, no fake stats, accent at the key moment), ui-ux-pro-max (light-first with a full dark theme, 44 px targets, 150–250 ms motion, visible focus, semantic tokens). Rejected from the research: dark-by-default, glassmorphism, ambient blobs, Noto Sans KR (Pretendard is the modern Korean/Latin pairing), bento grids.

**Signature element:** the **verdict banner** — after a build, a full-width rounded banner (success green / warning amber / danger red) with an icon, the verdict (`런처 검증 통과`), the file name and the SHA-256 prefix. It is the one place the page "speaks"; everything else stays quiet.

Portrait 3:4 stays: default 720×960, minimum 640×800, resizable.

## 2. Tokens (`ui/lib/theme.slint`, global `Theme`)

| Token | Light | Dark | Use |
|---|---|---|---|
| bg | `#F4F5F7` | `#131518` | window ground |
| surface | `#FFFFFF` | `#1C1F24` | cards, dialogs, inputs |
| surface-2 | `#F0F1F3` | `#24282E` | hover fills, secondary buttons, chips, nav rail |
| surface-3 | `#E6E8EB` | `#2D3239` | pressed fills, progress track |
| border | `#E3E5E8` | `#2E333B` | hairlines (1 px) |
| border-strong | `#C9CDD3` | `#3C424B` | input borders on hover |
| text | `#161B22` | `#EDEFF2` | primary text |
| text-2 | `#5C6470` | `#A2A9B4` | secondary text, icons |
| text-3 | `#7C8390` | `#6F7782` | placeholders, captions |
| accent | `#0B7578` | `#3ACBB9` | primary button, active nav, links, focus ring |
| accent-hover | `#095F62` | `#5AD7C8` | primary hover |
| on-accent | `#FFFFFF` | `#0B1F1C` | primary label |
| accent-soft | `#E3F4F4` | `#153634` | active nav pill, selected rows |
| success / success-soft | `#15753F` / `#E4F5EA` | `#4ADE80` / `#12301F` | verdict banner, chips |
| warning / warning-soft | `#A34A07` / `#FCEFD9` | `#F5B453` / `#3A2A12` | warnings |
| danger / danger-soft | `#C42B2B` / `#FCE7E7` | `#F87171` / `#3B1717` | errors, destructive |
| scrim | `#161B22` @ 40% | `#000000` @ 55% | dialog backdrop |

Contrast (AA, measured 2026-09-10 with `scratchpad/contrast_v2.py`): light text/bg 15.86, text-2/surface 5.98, text-2/surface-2 5.29, text-3/surface 3.81 (captions only, never the sole carrier), on-accent/accent 5.48, accent/surface 5.48, accent/accent-soft 4.83, success/success-soft 5.08, warning/warning-soft 5.22, danger/danger-soft 4.75; dark text/bg 15.88, text-2/surface 6.98, on-accent/accent 8.49, accent/surface 8.20, accent/accent-soft 6.48, success/success-soft 8.21, warning/warning-soft 7.59, danger/danger-soft 5.75. Borders are 1.26–1.30 by design (hairlines are decorative; component boundaries that must be perceivable — inputs — use `border-strong` on hover and the accent ring on focus).

- **Radius:** `r-sm 6` (chips, small inputs), `r-md 10` (buttons, inputs), `r-lg 14` (cards, callouts), `r-xl 20` (dialogs), `r-pill 999` (nav pill, status pills).
- **Spacing:** 4-base: 4 / 8 / 12 / 16 / 20 / 24 / 32. Page inset 24. Card padding 20. Card gap 16. Row height 44 minimum.
- **Elevation:** cards = surface + 1 px border, no shadow. Dialogs/toasts = shadow `drop-shadow-blur: 24px; drop-shadow-offset-y: 8px; drop-shadow-color: #000 @ 12% light / 40% dark`. Only these two float.
- **Type (Pretendard, bundled `assets/fonts/Pretendard-{Regular,Medium,SemiBold}.ttf`, `default-font-family: "Pretendard"`):** page title 22/600, section title 17/600, row title 15/500, body 14/400 (line-height 1.5), secondary 13/400 text-2, caption 12/500 text-3, button 14/500, mono (hashes, paths, log) `"Consolas"` 12.5 with `"Cascadia Mono"` unavailable → Consolas is fine on Windows; macOS falls back to Menlo automatically.
- **Icons:** Lucide SVGs in `assets/icons/`, 20 px in rows/buttons, 16 px in chips, 24 px in empty states; `colorize` with `text-2` (or `on-accent` inside primary buttons, semantic colour in callouts).
- **Motion:** `fast 120ms` (hover/press fills), `base 200ms` (dialog scrim/scale, toast slide), `slow 300ms` (page fade). Reduced motion (settings switch) → all 0 ms. Indeterminate progress bar = a 30 %-wide accent segment sliding across the track (`animate x` loop via a timer-driven property from Rust, or Slint `animate` with `iteration-count: -1`).
- **Focus:** 2 px `accent` ring with 2 px offset (`border-width: 2px; border-color: Theme.accent` on an outer wrapper 4 px larger), visible in both themes; never removed.

## 3. Component library (`ui/lib/`)

Every interactive component: `accessible-role`, Korean `accessible-label`, `accessible-enabled`, hover/pressed/focused/disabled/loading states, `mouse-cursor: pointer` when enabled, Enter/Space activation, ≥ 44 px hit area (visual height may be 36–40 with hit padding).

| Component | Spec |
|---|---|
| `Button` | variants `primary` (accent fill, on-accent text), `secondary` (surface-2 fill, border, text), `danger` (danger-soft fill, danger text; §8), `ghost` (transparent, text-2 → text on hover), `danger` (danger-soft fill, danger text); sizes `md` 36 px / `lg` 44 px; optional leading icon 18 px; `loading` shows the indeterminate dots `···` replaced by a 16 px `ProgressRing`? — no ring (rotation unsupported): loading = label unchanged + a 2 px indeterminate bar along the button's bottom edge + disabled input. Radius r-md. |
| `IconButton` | 36×36, ghost by default, tooltip-free, label via `accessible-label`; hover surface-2. |
| `TextField` | label 13/500 text-2 above, optional 18 px leading icon (mirrors the suffix slot), input 40 px surface + border (border-strong on hover, accent 2 px on focus), placeholder text-3, helper/error 12 px below (error in danger with a 16 px `circle-alert` icon), optional password mode with eye toggle, optional trailing `IconButton`, `accessible-description` = error or helper. |
| `Switch` | 40×22 pill track (surface-3 → accent), 18 px knob, label on the left, 44 px row; `accessible-role: checkbox`. |
| `SegmentedControl` | 2–4 options in a surface-2 pill/rounded track, selected option = surface card with border; used for loader, publish target, theme. |
| `Card` | surface, border, r-lg, padding 20, optional title row (section title + trailing actions). |
| `ListRow` | 56 px (72 px with the optional `description` third line, `overflow: elide`): optional leading avatar/icon 36 px, title 15/500, subtitle 13 text-2, trailing chips/actions; hover surface-2; selectable variant with accent-soft fill + accent border. |
| `Chip` | 24 px, r-sm, 12/500, tones neutral/accent/success/warning/danger (soft fill + tone text), optional 16 px icon (a 14 px glyph off the 4 px grid was rounding the Lucide stroke). |
| `Callout` | r-lg, soft tone fill, tone icon 20 px, title 14/500, body 13, optional action button; tones info(accent-soft)/success/warning/danger. |
| `VerdictBanner` | Callout variant, full width, 16 padding, icon 24: title `런처 검증 통과` / `경고와 함께 통과` / `검증 실패`, second line `distribution.json · SHA-256 a91f3c02 · 파일 484`. |
| `ProgressBar` | 4 px track surface-3, accent fill; determinate (0–1) and indeterminate. |
| `NavRail` | 72 px wide left rail: 6 items (icon 22 + label 11/500; 빌드 서버 모드 가져오기 배포 + bottom item 설정), active = accent-soft pill 56×56 with accent icon/label, hover surface-2; keyboard Ctrl+1..6 handled in app.slint. |
| `PillStrip` | wrapping row of 36 px `r-pill` selectable pills under a caption; radio-group semantics, one tab stop, roving focus, Enter/Space selects (§8.2). |
| `ProgressRow` | `ProgressBar` + mono text line; the row is the single announced progress indicator (`accessible-live-region: polite`), the inner bar is `accessible-role: none` (§8.7). |
| `TopBar` | 56 px: page title 22/600 left, right cluster (status pill while busy, theme `IconButton` sun/moon, help none). Below it a 2 px `ProgressBar` (indeterminate) while busy. |
| `Dialog` | scrim + centered card 560 px (max 90 %), r-xl, shadow, title 17/600, close `IconButton` top-right, body, footer buttons right-aligned (primary right). Esc closes when not busy; a dialog open when a job starts stays open with disabled controls until the job ends. No scroller of its own — any list inside owns its scrolling. Opens with 200 ms fade + scale 0.98→1. |
| `Toast` | bottom-centre, 360–520 px, surface + border + shadow, icon by tone, message 14, auto-dismiss 4 s (Rust timer), stacked max 3. |
| `EmptyState` | centered icon 24 (text-3), title 15/500, body 13 text-2, up to two buttons. |
| `KeyValue` | 2-column row for readiness/settings summaries: key 13 text-2, value 14 mono/regular, status chip right. |
| `LogView` | monospace list, 12.5 px, alternating none, rows `+MM:SS  target  message`, level colour on the message (danger/warning/text), auto-scroll to end, max 400 rows, header with `지우기` `IconButton` (trash-2). |

## 4. Screens (`ui/screens/`)

Shell: `NavRail` left, `TopBar` + content right; content scrolls (`ScrollView`), page inset 24, cards stacked with 16 gap.

**빌드 (home, icon `layers`)** — the hero page.
1. Readiness card: four `KeyValue` rows (루트 / 기본 URL / Java / Launcher 폴더) with `Chip` `확인됨`(success)/`미설정`(warning); a `설정 열기` ghost button top-right.
2. Build card: `TextField` 파일 이름 (suffix `.json`), three `Switch` rows (Launcher 폴더에 설치 / Forge 캐시 무효화 / 설치 출력 삭제), footer: `Button primary lg` 배포본 생성 + `Button secondary` 기존 파일 검증. Helper under the primary when disabled.
3. Result card (visible after a run): `VerdictBanner` + summary chips + findings list (each: tone icon 16, `Test-1.12.2 · modules[7].artifact.url` mono 12 text-2, message 13; capped list with `더 보기`) + when strict fails, a `Callout warning` "런처 배포 불가: …".
4. Log card: `LogView`, 180 px tall.

**서버 (icon `server`)** — header actions `모드팩 가져오기`(secondary, icon `download`) and `새 서버`(primary, icon `plus`), list of `ListRow`s (avatar = server `icon.png` when present else initials on accent-soft; title id; subtitle `모드 42 · 파일 118 · 라이브러리 3`; chips loader `Forge 14.23.5.2860`; trailing `IconButton chevron-right` (`{id} 메뉴`; opens the row menu dialog with 모드 관리 / 폴더 열기 / 복제 / 삭제, §8.5)). Empty state with the two actions. `새 서버` opens a `Dialog`: 서버 ID, Minecraft 버전 (side by side), `SegmentedControl` 없음/Forge/NeoForge/Fabric, 로더 버전 with `최신` / `권장` ghost buttons inline, notices as `Callout` (Forge ≥ 1.20.3 warning; NeoForge patch note "Hambugui 런처에 NeoForge 패치가 필요합니다 — README 참고"), footer 취소 / 서버 만들기.

**가져오기 (icon `download`)** — folder `KeyValue` with `폴더 열기`, list of packs as selectable `ListRow`s (title file name, subtitle size, chip format `CurseForge`/`Modrinth`/`Prism 인스턴스`/`알 수 없음`(neutral, row disabled with helper)), `TextField` 서버 ID, primary 모드팩 가져오기; result `Callout success` with counts; manual downloads `Callout warning` listing name · file · URL (read-only mono field per item with `copy`-icon `IconButton` that selects all text).

**배포 (icon `cloud-upload`)** — `SegmentedControl` WebDAV / Google Drive (none = nothing selected → helper `설정에서 대상을 구성하세요`), a `KeyValue` summary of the target (URL / 폴더 ID, 연결 상태 chip), `Switch` 변경된 파일만 업로드, primary 업로드 (helper when disabled: 대상 없음 / 배포본 없음 / 런처 검증 실패), result `Callout success` `WebDAV로 312개 업로드 · 172개 변경 없음` + 배포 URL field with select action; Drive `Callout info` about quota/100 MB.

**설정 (icon `settings`)** — cards: 기본 (루트 with `폴더 만들기` when missing, 기본 URL, Java, Launcher 폴더), CurseForge (`CurseForge API 키` password-mode `TextField`, helper `console.curseforge.com에서 발급받은 본인 키를 입력하세요. 환경 변수 NEBULA_CURSEFORGE_API_KEY가 있으면 그 값이 우선합니다.`, `accessible-id: settings-curseforge-key`), WebDAV (URL, 사용자, 비밀번호, 공개 URL), Google Drive (클라이언트 ID, 시크릿, 폴더 ID, `Google 계정 연결` button + status chip 연결됨/연결되지 않음), 모양 (`SegmentedControl` 시스템/라이트/다크, `Switch` 모션 줄이기 — applied immediately). Sticky footer bar inside the page: `저장` primary (Enter in any field also saves) + `변경 사항 없음/저장되지 않은 변경` caption.

## 5. States and copy

- Busy: top-bar status pill `배포본 생성 중 · 1:23` (mono time) + indeterminate bar; primary buttons in `loading`; navigation stays enabled; dialogs cannot open.
- Success: `Toast success` `배포본을 생성했습니다` + verdict banner; the page scrolls the result card into view.
- Failure: `Toast danger` `생성 실패` + `Callout danger` in the relevant card with cause + fix and `다시 시도` (same job kind, current values) + cache hint when relevant.
- Validation: strict gates the verdict banner tone and 업로드 (as decided in `2026-09-10-ux-fixes.md` item 1).
- Copy: sentence case, verbs on buttons (`생성`, `가져오기`, `업로드`, `저장`, `연결`), errors state cause then fix, no emoji, no arrows, no English in user-facing text except proper nouns and file names.

## 6. Accessibility

Focus order = element order; Ctrl/Cmd+1..6 switch pages; Esc closes dialogs; Enter submits dialogs and settings; every icon-only control has a Korean `accessible-label`; chips never carry meaning by colour alone (they always have text); contrast per §2; reduced-motion switch zeroes every duration.

Every interactive control also carries a kebab-case `accessible-id` (→ UIA AutomationId): `nav-build`, `build-generate`, `settings-save`. The Korean label is a caption and may be reworded; the id is the stable handle automation matches on. Decorative glyphs inside a control that already names itself are `accessible-role: none`, so a screen reader never reads an unnamed graphic — and the nav item's own `Text` is silenced for the same reason, or it reads `빌드 빌드`.

### Accepted ceilings (Slint 1.17.1, measured 2026-09-10)

- **Content scrolled outside a `ScrollView` is absent from the accessibility tree**, not merely `IsOffscreen`, and Tab never scrolls a control into view. A keyboard-only reader would have no way to reach the log or the verdict banner. Mitigated, not fixed: the root `FocusScope` maps **PageUp/PageDown** and **Ctrl+Home/Ctrl+End** to the page's `viewport-y`, from wherever the focus is.
- **A roving focus ring inside a single-tab-stop group is invisible to UIA** (measured 2026-09-11 on `PillStrip`): Slint 1.17.1 exposes no focused-child property, so Left/Right move only the painted ring while `FocusedElement` stays on the group; selection (`accessible-checked`) is still announced on Enter/Space. Recorded here rather than implied to work.
- **`accessible-description` does not reach a UIA client** — `HelpText` and `ItemStatus` both come back empty. It is still set, for a Slint that carries it later; meanwhile a field with an error folds that error into its `accessible-label`, which does arrive. The visible error `Text` is a second, independent carrier.

## 7. Implementation notes

- Files: `ui/lib/{theme,typography,button,icon-button,text-field,switch,segmented,card,list-row,chip,callout,progress,progress-row,pill-strip,nav-rail,top-bar,dialog,toast,empty-state,key-value,log-view}.slint`, `ui/screens/{build,servers,mods,import,publish,settings}.slint`, `ui/app.slint`. Delete the old `ui/components/` and `ui/screens/{sheet,new-server}.slint`.
- Globals: keep `Theme`, `SheetData` → rename to `AppData`? Keep the existing global names and property names where the wiring already exists (`SheetData`, `PublishData`, `NewServerData`, `ImportData`, `SettingsData`, `SheetActions`) to limit Rust churn; add `AppData.page: AppPage { build, servers, mods, import, publish, settings }`, `AppData.toasts: [ToastData]`, `AppData.dialog: DialogKind { none, new-server, mod-detail, install-confirm, version-picker, update-preview, mod-delete, server-menu, server-delete, server-duplicate }`, `SheetActions.dismiss-toast(int)`, and the `ModsData` global (§8; row structs `InstalledModRowData`, `ModHitRowData`, `VersionRowData`, `DepRowData`, `UpdateRowData`, `ManualDownloadData`; flags `search-busy`, `resolving`, `list-busy`, `checking`, `has-cf-key`).
- Icons in lists follow the server-icon pattern: parallel `[image]` models (`ModsData.installed-icons`, `ModsData.hit-icons`) beside the row structs, because an `image` field inside a struct is unrenderable by `slint-viewer --load-data`. Modrinth and jar icons are decoded on the UI thread from worker-produced PNG paths; CurseForge icons never touch disk and arrive as `slint::Image::from_rgba8` built on the UI thread from the session memory cache. Both land in the same model, so rows do not distinguish them.
- Fonts: `import "../assets/fonts/Pretendard-Regular.ttf";` etc. at the top of `app.slint`; `default-font-family: "Pretendard"` on the Window.
- Verify every file with `slint-viewer --check`, render every state (light/dark × build-empty, build-busy, build-success, build-failed, servers, servers-empty, new-server-dialog, server-menu-dialog, server-delete-dialog, mods-installed, mods-installed-empty, mods-search, mods-search-loading, mods-install-confirm, mods-version-picker, mods-detail, import, publish, settings, toast) to `docs/plans/gui-v2/*.png`, and review each render against §2–§4 before declaring done.

## 8. 모드 page and server management (final v1.1, 2026-09-11; supersedes the earlier addendum)

This section is the single source for the mod catalog UI; the plan's GUI section points here and the integration map yields to it where they disagree. Same tokens and components; additions are listed in §8.7. Design-panel findings (UX, antislop, Slint feasibility) and the critic panel's GUI findings are resolved below; deliberate deviations from the panel are marked *(deviation)*.

### 8.1 Shell changes
- Nav rail: **모드** (`package`) between 서버 and 가져오기; `AppPage` gains `mods`. Shortcuts renumber to Ctrl/Cmd+1..6 (빌드 서버 모드 가져오기 배포 설정) — §3, §6 and §7 are updated accordingly; README documents the change.
- `DialogKind` gains: `mod-detail`, `install-confirm`, `version-picker`, `update-preview`, `mod-delete`, `server-menu`, `server-delete`, `server-duplicate`. One dialog at a time; opening replaces.
- 설정 gains a **CurseForge** card (§4). `ModsData.has-cf-key` is true when the settings field or `NEBULA_CURSEFORGE_API_KEY` resolves; it enables the CurseForge provider segment in §8.4.

### 8.2 Server selector, view switcher and scope
- **`PillStrip`**: a wrapping row (no horizontal scroll) of 36 px `r-pill` pills under a `서버` caption; root `accessible-role: radio-group`, each pill `radio-button` with `accessible-checked` following the *selection*; selected pill = accent-soft fill + accent border; hit area ≥ 44 px (36 + 4 px padding). Keyboard: ONE tab stop for the group; Left/Right/Home/End move a roving focus ring **without selecting**; Enter or Space selects the focused pill; mouse click selects directly. This deliberately differs from `SegmentedControl` (which commits on arrow) because a selection change wipes the query and re-runs the installed-list job.
- **View switcher**: a `SegmentedControl` 설치됨 / 찾기 directly under the strip (`accessible-role: tab-list`, `accessible-id: mods-view`), default 설치됨. Exactly one card is on screen at a time. Query text, provider and sort persist for the session across view switches and server changes; results are re-fetched only on the next search; `설치됨` markers on cached results are recomputed when the server changes. A successful install stays on 찾기 (no auto-switch).
- **Scope line** in the header of the visible card: `Neo-1.21.1 · Minecraft 1.21.1 · NeoForge 21.1.72` (loader + version from the server's meta; `로더 없음` if none; KeyValue style, mono for the id). Every dialog title on this page carries the server (`Neo-1.21.1에 설치`, `{제목} — Neo-1.21.1`); progress text and result toasts name the server (`Neo-1.21.1: 모드 3개를 설치했습니다`).
- Selection persists across pages **and restarts** via `GuiSettings.last-mods-server` (`#[serde(default)]`, kebab-case, written on change, not a settings-form field) *(deviation from the panel's in-session rule: an operator works one modpack for days)*. If the stored id no longer names a server, the first server is selected and a toast says so. No servers → `EmptyState` with `새 서버` / `모드팩 가져오기`.
- Changing the server clears search results (the query text stays), reloads the 설치됨 list (quiet job) and drops any in-flight quiet result for the previous server.

### 8.3 설치됨 (installed)
- Card header: title `설치된 모드 N`, subtitle `관리 12 · 직접 추가 3 · 마지막 업데이트 확인 5분 전` (session-relative; `확인 전` before the first check), actions: secondary `업데이트 확인`, primary `모두 업데이트 (N)` only when N > 0.
- List: `ListView { height: 8 * 72px }` with fixed 72 px delegates; order 업데이트 가능 first, then name. **Accepted ceiling (§6)**: rows scrolled out of the ListView leave the accessibility tree and Tab will not scroll them in. Mitigation: the header count is never hidden, and when N > 8 the header gains a ghost `모두 보기` that removes the cap so the page ScrollView (PageUp/PageDown/Ctrl+Home/End) owns the scrolling.
- Row: `ListRow` with avatar 36 (icon — cached PNG for Modrinth/jar icons, memory-only for CurseForge — else initials), title 15/500, subtitle `0.92.0 · fabric-api-0.92.jar · Modrinth` or `0.92.0 · fabric-api-0.92.jar · 직접 추가`, **no description line** (the third line is a 찾기-only affordance). Chips: namespace **neutral** (`필수` / `선택 켜짐` / `선택 꺼짐`); `업데이트 가능` is the ONLY accent chip, shown only after a check in this session; `파일 없음` (warning) for an index entry whose file is absent; `배포 불가` (danger) for a file the distribution scanners will reject (not `.jar`, or not a zip) with the helper `배포본을 만들 수 없는 파일입니다 — 이 파일을 지우거나 올바른 jar로 바꾸세요`. Trailing `IconButton chevron-right` labelled `{제목} 자세히`, `accessible-id: mods-row-<file>`.
- Empty: `EmptyState` `설치된 모드가 없습니다` / `찾기에서 검색해 설치하세요` + button `찾기` (switches the view). Loading: indeterminate bar under the header labelled `모드 목록을 읽는 중`.
- `mod-detail` dialog (title `{제목} — Neo-1.21.1`): description (2 lines; jar description for 직접 추가), KeyValue rows (버전, 파일, 제공자, 해시 SHA-1 첫 12자), namespace `SegmentedControl` 필수 / 선택 켜짐 / 선택 꺼짐 that **stages** the choice; the dialog's primary `이동` applies it (a job) and toasts `선택 켜짐으로 이동했습니다` — the control never fires a job on change, so a user can never be locked in the dialog by a control they tabbed through. `다른 버전…` (opens `version-picker`; hidden for 직접 추가), `프로젝트 페이지 열기` (hidden without a provider id), `삭제` (danger). Initial focus: the segmented control; Esc closes when not busy; on close focus returns to the page's key scope (`keys.focus()`, §8.7) — the row's chevron is inside a virtualised `ListView` and cannot be re-focused reliably (measured 2026-09-11).
- `mod-delete` dialog: `모드 삭제 — fabric-api-0.92.jar`, body `이 파일이 영구 삭제됩니다: <절대 경로>`; when other managed mods list it as a required dependency (index `dependencies`), a warning Callout names them (`fabric-api를 필요로 하는 모드: Sodium, Lithium`); buttons 취소 / 삭제 (danger; initial focus on 취소). No timed morphing controls anywhere.
- `version-picker` dialog: `ListView { height: 360px }` of versions (same §6 ceiling note; fixed 64 px delegates): version name, chips 정식/베타/알파, game versions truncated to 3 + `외 N개`, date `2025-03-03`, size `2.1 MB`, trailing button `설치` / `교체` / `이전 버전으로 교체` / disabled `설치됨`. `교체` runs the three-step replace of plan `update.rs` (old file moved out, new file in, index rewritten) keeping the namespace; `이전 버전으로 교체` asks a one-line confirmation inside the dialog (`현재 0.92.0 → 0.90.1로 내립니다`).
- `업데이트 확인` results: toast `업데이트 N개 있음` or `모두 최신입니다`; header caption updates. `모두 업데이트` opens `update-preview`: list `mod: 0.91.0 → 0.92.0` (type chip), buttons 취소 / `N개 업데이트`; updates apply one mod at a time; per-mod failures are listed in the card's danger Callout afterwards; successes toast once.

### 8.4 찾기 (search)
- Search row: `TextField` with leading `search` icon, placeholder `모드 이름 또는 키워드`, Enter searches, `accessible-label` `모드 검색`. Below: `SegmentedControl` provider (Modrinth / CurseForge — CurseForge disabled with helper `설정에서 CurseForge API 키를 입력하세요` when `has-cf-key` is false), `SegmentedControl` sort with per-provider options: Modrinth 관련도 / 다운로드 / 최신 (`updated`); CurseForge 인기 / 다운로드 / 최신. Changing provider or sort re-searches immediately when a query exists; offset resets. Facets pre-filter by mc + loader (Modrinth facets; CurseForge `gameVersion` + `modLoaderType`).
- Pre-search: `EmptyState` `검색어를 입력하세요` / `Neo-1.21.1 · Minecraft 1.21.1 · NeoForge에 맞는 모드만 표시됩니다`. Loading: indeterminate bar under the search row labelled `검색 중`, helper `Modrinth에서 검색 중`; no skeleton blocks. Empty result: `검색 결과가 없습니다 — 다른 검색어나 제공자를 시도하세요`. Error: `Callout danger` with a title by `CatalogError` variant — `Modrinth에 연결하지 못했습니다` / `요청이 너무 많습니다 — N초 후 다시 시도하세요` / `네트워크에 연결되어 있지 않습니다` — body = cause + fix, `다시 시도`. The GUI matches on the error variant, never on message text.
- Result row: avatar 36, title, caption `작성자 · 다운로드 120만 · 수정 3일 전` (when sorted by 최신 the date comes first), description one line (`overflow: elide`), trailing `Button secondary md` `설치` labelled `{제목} 설치` (`accessible-id: mods-install-{provider}-{project id}`; slugs are provider-scoped and re-assignable). Once installed it becomes a disabled `설치됨` (the tab stop stays). Rows are not selectable. `더 보기` at the bottom only when the last page was full; it keeps existing rows, shows `Button.loading`, focus stays on it.
- Install flow: click 설치 → quiet resolve (`ModsData.resolving`, the button shows loading). If no version exists for the server's mc + loader, no dialog opens: the row's `설치` becomes disabled with the caption suffix `· {mc} {loader}용 버전 없음` and a neutral toast `{제목}: {mc} {loader}용 버전이 없습니다`. Otherwise **`install-confirm` always opens** (title `Neo-1.21.1에 설치`): resolved version line (`fabric-api 0.92.0 · 정식 · 2.1 MB · 2025-03-03`) with `다른 버전…`; a warning Callout when the version is 베타/알파 (`정식 버전이 없어 베타를 선택했습니다`); namespace `SegmentedControl` (default 필수; dependencies inherit it); dependency list as `ListView { height: min(4 * 56px, N * 56px) }` with 56 px delegates (the `Dialog` card has no scroller of its own): required deps as `Switch` rows pre-checked (unchecking shows `필수 의존성을 빼면 게임이 시작되지 않을 수 있습니다` under the row); a required dependency already present — matched by provider project id, or by jar mod id for `직접 추가` files (best-effort, stated in the row helper) — is a disabled row with a `설치됨` chip, never omitted; optional deps collapsed behind `선택 의존성 N개` and expanding inside the same list (unchecked); incompatible with an installed mod → danger Callout naming both, primary disabled; unresolvable required dependency → danger Callout naming it, primary disabled. Footer caption `{N}개 · {total size}` counts only files that will be downloaded; primary `N개 설치` (initial focus). Esc cancels. The plan carries the server, the index sha1 and a resolved-at time; apply refuses with `서버가 바뀌었습니다 — 다시 검색하세요` if they no longer match, and re-resolves silently when older than 10 minutes.
- Apply: a global job (busy). Progress carrier is **one `ProgressRow` in the header area of the card that owns the job** — 찾기 for install, 설치됨 for update-apply / remove / move — reading `다운로드 중 2/5 · fabric-api-0.92.jar · 0:12` (elapsed computed by the host), mirrored by the top-bar pill (`모드 설치 중` / `모드 업데이트 중` / `모드 삭제 중` / `모드 이동 중`; server jobs `서버 복제 중` / `서버 삭제 중` use the pill alone). Toasts are never used for progress. Result: success toast `Neo-1.21.1: 모드 3개를 설치했습니다`; the row shows `설치됨`; failure → danger Callout `설치 실패` with cause + fix and `다시 시도` (re-uses the plan while valid, otherwise re-resolves and re-opens `install-confirm`); the body adds `설치된 파일 없음 — 변경 사항이 되돌려졌습니다` only when the report's `left_behind` is empty, else `일부 파일을 되돌리지 못했습니다` followed by the paths and `폴더 열기`; 429 → `요청이 너무 많습니다 — N초 후 자동으로 다시 시도합니다` (one automatic retry, wait capped at 30 s).
- CurseForge blocked file: not a failure — it is removed from the plan before any download and listed in the report. Callout warning `직접 내려받아야 하는 파일` listing name · file · selectable URL, the exact target folder (`<server>/neoforgemods/required`) with `폴더 열기`, and the rule `파일 이름은 .jar로 끝나야 하며, 완료되지 않은 다운로드(.crdownload 등)는 배포본 생성을 막습니다`; the other files still install; after the user drops the file, the next `업데이트 확인` attaches provenance: unmanaged jars are looked up once by sha1 (Modrinth) and, when a key is present, by fingerprint (CurseForge), and matches become managed rows (toast `직접 추가 N개를 관리 목록에 연결했습니다`). The list refresh itself stays offline.

### 8.5 서버 page changes
- Row trailing control: the `IconButton folder-open` of §4 is **replaced** by `IconButton chevron-right` (`accessible-label: {id} 메뉴`, `accessible-id: servers-menu-{id}`), keeping one tab stop per row (ux-fixes item 15); 폴더 열기 moves into the `server-menu` dialog as its second item. Menu: 모드 관리 (goes to 모드 with this server selected), 폴더 열기, 복제, 삭제 (danger). `chevron-right.svg` already ships.
- `server-duplicate` dialog: field `새 서버 ID` with the 새 서버 rules (single segment, not existing), primary `복제`, progress in the top-bar pill (global job), success toast + list refresh; the source server stays selected on the 모드 page.
- `server-delete` dialog: body `이 폴더가 영구 삭제됩니다: <절대 경로>` + `배포본을 다시 생성하면 이 서버가 사라집니다.`; field `삭제하려면 서버 ID를 입력하세요`; primary `삭제` (danger) enabled only when the trimmed input equals the directory name exactly (case-sensitive); Enter submits only when enabled; blocked while busy.

### 8.6 Busy model for catalog jobs
- Mutating jobs (install apply, update apply, remove, move, delete/duplicate server) use the global job lock: `SheetData.busy` true, top-bar pill + indeterminate bar, every page's job-starting primary disabled, no new dialog opens, **and a dialog that is already open stays open with its controls disabled and Esc refused until the job ends** (a job started from a dialog is already writing to disk). This is why dialog controls stage changes and a primary applies them (§8.3).
- Quiet jobs (search, resolve, installed-list reload, update check) use `ModsData.search-busy` / `resolving` / `list-busy` / `checking`, never take the global lock, and are single-flight per kind: starting one supersedes the in-flight one of the same kind; the applier drops a result whose captured sequence number (`Ui.search_seq` / `list_seq`, incremented at spawn) no longer matches.

### 8.7 New/changed components
- `PillStrip` (§8.2). `ProgressRow`: a `ProgressBar` plus a mono text line; the **row** owns the announcement — `accessible-role: progress-indicator`, `accessible-label` = the text line, `accessible-live-region: polite` (present in 1.17.1; this supersedes the ux-fixes ceiling "no live-region API"); the inner bar is `accessible-role: none` with no label so the tree carries one indicator. The top-bar pill never scrolls and repeats the same text, so it stays the reader's guaranteed carrier.
- `ListRow.description` (third line, `overflow: elide`; used by 찾기 rows only); `TextField.leading-icon` (mirrors the existing suffix slot); `Button` regains the `danger` variant (two brushes on the existing ternaries).
- `Dialog` keeps its shape — Slint has no element-valued property, so initial focus is not a Dialog property. Each dialog body is a component in `ui/screens/mods.slint` declaring its own `forward-focus` (mod-detail → namespace control, install-confirm → primary, mod-delete → 취소, server-delete/duplicate → text field, version-picker → list); `app.slint` focuses it in the Dialog's `init` and returns focus with `keys.focus()` in `closed`, as `new-server` does today.
- No skeletons, no timed controls, no new tokens.

### 8.8 Copy formats
Type chips 정식/베타/알파; counts `120만`/`3.2억`; dates `YYYY-MM-DD`; sizes `12.3 MB` (unit symbols are an accepted exception to §5's no-English rule, like `SHA-256`); relative time `N분 전`/`N일 전`; game versions `1.21.1, 1.21, 1.20.6 외 4개`.
