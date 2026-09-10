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
| `Button` | variants `primary` (accent fill, on-accent text), `secondary` (surface-2 fill, border, text), `ghost` (transparent, text-2 → text on hover), `danger` (danger-soft fill, danger text); sizes `md` 36 px / `lg` 44 px; optional leading icon 18 px; `loading` shows the indeterminate dots `···` replaced by a 16 px `ProgressRing`? — no ring (rotation unsupported): loading = label unchanged + a 2 px indeterminate bar along the button's bottom edge + disabled input. Radius r-md. |
| `IconButton` | 36×36, ghost by default, tooltip-free, label via `accessible-label`; hover surface-2. |
| `TextField` | label 13/500 text-2 above, input 40 px surface + border (border-strong on hover, accent 2 px on focus), placeholder text-3, helper/error 12 px below (error in danger with a 16 px `circle-alert` icon), optional password mode with eye toggle, optional trailing `IconButton`, `accessible-description` = error or helper. |
| `Switch` | 40×22 pill track (surface-3 → accent), 18 px knob, label on the left, 44 px row; `accessible-role: checkbox`. |
| `SegmentedControl` | 2–4 options in a surface-2 pill/rounded track, selected option = surface card with border; used for loader, publish target, theme. |
| `Card` | surface, border, r-lg, padding 20, optional title row (section title + trailing actions). |
| `ListRow` | 56 px: optional leading avatar/icon 36 px, title 15/500, subtitle 13 text-2, trailing chips/actions; hover surface-2; selectable variant with accent-soft fill + accent border. |
| `Chip` | 24 px, r-sm, 12/500, tones neutral/accent/success/warning/danger (soft fill + tone text), optional 16 px icon (a 14 px glyph off the 4 px grid was rounding the Lucide stroke). |
| `Callout` | r-lg, soft tone fill, tone icon 20 px, title 14/500, body 13, optional action button; tones info(accent-soft)/success/warning/danger. |
| `VerdictBanner` | Callout variant, full width, 16 padding, icon 24: title `런처 검증 통과` / `경고와 함께 통과` / `검증 실패`, second line `distribution.json · SHA-256 a91f3c02 · 파일 484`. |
| `ProgressBar` | 4 px track surface-3, accent fill; determinate (0–1) and indeterminate. |
| `NavRail` | 72 px wide left rail: 5 items (icon 22 + label 11/500), active = accent-soft pill 56×56 with accent icon/label, hover surface-2; bottom item 설정; keyboard Ctrl+1..5 handled in app.slint. |
| `TopBar` | 56 px: page title 22/600 left, right cluster (status pill while busy, theme `IconButton` sun/moon, help none). Below it a 2 px `ProgressBar` (indeterminate) while busy. |
| `Dialog` | scrim + centered card 560 px (max 90 %), r-xl, shadow, title 17/600, close `IconButton` top-right, body, footer buttons right-aligned (primary right). Esc closes when not busy. Opens with 200 ms fade + scale 0.98→1. |
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

**서버 (icon `server`)** — header actions `모드팩 가져오기`(secondary, icon `download`) and `새 서버`(primary, icon `plus`), list of `ListRow`s (avatar = server `icon.png` when present else initials on accent-soft; title id; subtitle `모드 42 · 파일 118 · 라이브러리 3`; chips loader `Forge 14.23.5.2860`; trailing `IconButton folder-open`). Empty state with the two actions. `새 서버` opens a `Dialog`: 서버 ID, Minecraft 버전 (side by side), `SegmentedControl` 없음/Forge/NeoForge/Fabric, 로더 버전 with `최신` / `권장` ghost buttons inline, notices as `Callout` (Forge ≥ 1.20.3 warning; NeoForge patch note "Hambugui 런처에 NeoForge 패치가 필요합니다 — README 참고"), footer 취소 / 서버 만들기.

**가져오기 (icon `download`)** — folder `KeyValue` with `폴더 열기`, list of packs as selectable `ListRow`s (title file name, subtitle size, chip format `CurseForge`/`Modrinth`/`Prism 인스턴스`/`알 수 없음`(neutral, row disabled with helper)), `TextField` 서버 ID, primary 모드팩 가져오기; result `Callout success` with counts; manual downloads `Callout warning` listing name · file · URL (read-only mono field per item with `copy`-icon `IconButton` that selects all text).

**배포 (icon `cloud-upload`)** — `SegmentedControl` WebDAV / Google Drive (none = nothing selected → helper `설정에서 대상을 구성하세요`), a `KeyValue` summary of the target (URL / 폴더 ID, 연결 상태 chip), `Switch` 변경된 파일만 업로드, primary 업로드 (helper when disabled: 대상 없음 / 배포본 없음 / 런처 검증 실패), result `Callout success` `WebDAV로 312개 업로드 · 172개 변경 없음` + 배포 URL field with select action; Drive `Callout info` about quota/100 MB.

**설정 (icon `settings`)** — cards: 기본 (루트 with `폴더 만들기` when missing, 기본 URL, Java, Launcher 폴더), WebDAV (URL, 사용자, 비밀번호, 공개 URL), Google Drive (클라이언트 ID, 시크릿, 폴더 ID, `Google 계정 연결` button + status chip 연결됨/연결되지 않음), 모양 (`SegmentedControl` 시스템/라이트/다크, `Switch` 모션 줄이기 — applied immediately). Sticky footer bar inside the page: `저장` primary (Enter in any field also saves) + `변경 사항 없음/저장되지 않은 변경` caption.

## 5. States and copy

- Busy: top-bar status pill `배포본 생성 중 · 1:23` (mono time) + indeterminate bar; primary buttons in `loading`; navigation stays enabled; dialogs cannot open.
- Success: `Toast success` `배포본을 생성했습니다` + verdict banner; the page scrolls the result card into view.
- Failure: `Toast danger` `생성 실패` + `Callout danger` in the relevant card with cause + fix and `다시 시도` (same job kind, current values) + cache hint when relevant.
- Validation: strict gates the verdict banner tone and 업로드 (as decided in `2026-09-10-ux-fixes.md` item 1).
- Copy: sentence case, verbs on buttons (`생성`, `가져오기`, `업로드`, `저장`, `연결`), errors state cause then fix, no emoji, no arrows, no English in user-facing text except proper nouns and file names.

## 6. Accessibility

Focus order = element order; Ctrl/Cmd+1..5 switch pages; Esc closes dialogs; Enter submits dialogs and settings; every icon-only control has a Korean `accessible-label`; chips never carry meaning by colour alone (they always have text); contrast per §2; reduced-motion switch zeroes every duration.

Every interactive control also carries a kebab-case `accessible-id` (→ UIA AutomationId): `nav-build`, `build-generate`, `settings-save`. The Korean label is a caption and may be reworded; the id is the stable handle automation matches on. Decorative glyphs inside a control that already names itself are `accessible-role: none`, so a screen reader never reads an unnamed graphic — and the nav item's own `Text` is silenced for the same reason, or it reads `빌드 빌드`.

### Accepted ceilings (Slint 1.17.1, measured 2026-09-10)

- **Content scrolled outside a `ScrollView` is absent from the accessibility tree**, not merely `IsOffscreen`, and Tab never scrolls a control into view. A keyboard-only reader would have no way to reach the log or the verdict banner. Mitigated, not fixed: the root `FocusScope` maps **PageUp/PageDown** and **Ctrl+Home/Ctrl+End** to the page's `viewport-y`, from wherever the focus is.
- **`accessible-description` does not reach a UIA client** — `HelpText` and `ItemStatus` both come back empty. It is still set, for a Slint that carries it later; meanwhile a field with an error folds that error into its `accessible-label`, which does arrive. The visible error `Text` is a second, independent carrier.

## 7. Implementation notes

- Files: `ui/lib/{theme,typography,button,icon-button,text-field,switch,segmented,card,list-row,chip,callout,progress,nav-rail,top-bar,dialog,toast,empty-state,key-value,log-view}.slint`, `ui/screens/{build,servers,import,publish,settings}.slint`, `ui/app.slint`. Delete the old `ui/components/` and `ui/screens/{sheet,new-server}.slint`.
- Globals: keep `Theme`, `SheetData` → rename to `AppData`? Keep the existing global names and property names where the wiring already exists (`SheetData`, `PublishData`, `NewServerData`, `ImportData`, `SettingsData`, `SheetActions`) to limit Rust churn; add `AppData.page: AppPage { build, servers, import, publish, settings }`, `AppData.toasts: [ToastData]`, `AppData.dialog: DialogKind { none, new-server }`, `SheetActions.dismiss-toast(int)`.
- Fonts: `import "../assets/fonts/Pretendard-Regular.ttf";` etc. at the top of `app.slint`; `default-font-family: "Pretendard"` on the Window.
- Verify every file with `slint-viewer --check`, render every state (light/dark × build-empty, build-busy, build-success, build-failed, servers, servers-empty, new-server-dialog, import, publish, settings, toast) to `docs/plans/gui-v2/*.png`, and review each render against §2–§4 before declaring done.
