# Nebula GUI — Design Record ("Build Sheet")

Stack: Slint 1.17.1, software renderer, same toolchain as Hambugui Launcher.

## 1. Subject, audience, job

- **Subject:** a manifest builder. The product's real output is a ledger of files: name, size, MD5/SHA-256, URL. Everything on screen serves producing and proving that ledger.
- **Audience:** one Korean-speaking server admin maintaining Hambugui/Helios modpacks. Expert user, runs this a few times a week, mostly in the same root.
- **Single job of the window:** *get from "files on disk" to "a distribution.json the launcher accepts" without leaving the window.* The one decision the user makes here is "is this build ready to publish?" The validation verdict is therefore the hero, not a stat row.
- **Aspect:** 3:4 portrait (default 720×960 logical px, minimum 600×800, resizable). Portrait is the shape of a sheet of paper, which is the metaphor: **the window is the build sheet**.

## 2. Direction: "Build Sheet"

A printed packing slip / build ledger: ruled bands, tabular figures, a verification stamp. Not a dashboard, not a launcher shell. Quiet paper surfaces, one ink colour for text, one accent used exactly once (the stamp / primary action).

**Signature element:** the **Stamp**. When validation passes, a rotated (−6°) double-bordered rectangle is stamped over the result band with three lines: `검증 통과`, the first 8 hex chars of the SHA-256 of the written `distribution.json`, and the timestamp. It is the only element that animates (200 ms scale 1.12→1.0, 0 ms under reduced motion) and the only place the accent colour appears besides the primary button. The stamp is real evidence (hash + time), never decoration.

**Rejected defaults (with reasons):**
- ui-ux-pro-max recommendation "Modern Dark, glassmorphism, indigo, glow, Inter" — rejected: dark-for-no-reason, glass and glow are antislop-ui tells; the product is a document tool, light is the honest default and dark is supported as a token swap.
- Left nav rail (launcher pattern) — rejected: four functions do not justify a rail in a 720 px wide window; one scrolling sheet with bands is faster.
- Stat cards / KPI row — rejected: the only numbers that matter are per-server counts and the validation summary; both live where the decision is made.
- Emoji, arrows on buttons, uppercase wide-tracked labels, capsule badges, purple/blue gradients — none used.
- Warm cream + terracotta + serif (the current AI "paper" default) — avoided: paper here is cool ledger stock with a verdigris stamp, system sans, no serif.

## 3. Tokens (`ui/tokens.slint`, global `Theme`)

| Token | Light | Dark | Use |
|---|---|---|---|
| bg | `#E9E4D8` | `#15181D` | window ground (ledger stock / ink) |
| surface | `#F7F4EC` | `#1E2229` | bands, fields |
| raised | `#DDD6C4` | `#2A2F38` | hover, disabled fills, log band |
| text | `#1B1F24` | `#ECE7DA` | primary text |
| muted | `#5A6170` | `#A8A296` | secondary text, eyebrows |
| line | `#857D6B` | `#737B88` | rules, borders (3.7:1 on surface, WCAG 1.4.11) |
| primary | `#14675C` | `#3FB79F` | primary button, stamp, success |
| on-primary | `#F7F4EC` | `#10201C` | primary button label |
| focus | `#B4530D` | `#F2A65A` | 2 px focus ring (never used for anything else) |
| danger | `#B3261E` | `#FF6B5E` | errors, destructive |
| warning | `#8A5A00` | `#FFC857` | warnings |

Contrast (WCAG AA, measured with the script in §9): light text/bg 13.05, muted/surface 5.66, on-primary/primary 6.12, danger/surface 5.95, warning/surface 5.39, focus/surface 4.57, text/raised 11.43; dark text/bg 14.41, muted/surface 6.29, on-primary/primary 6.81, danger/surface 5.71, warning/surface 10.37, focus/surface 7.89, text/raised 10.89. Rule: `muted` is never placed on `raised` in light mode (4.29, below AA); log timestamps on the raised log band use `text`.

- Theme: `Theme.preference` ∈ system/light/dark; `dark = preference == dark || (system && Palette.color-scheme == dark)`.
- Motion: `micro = reduced ? 0ms : 150ms`, `stamp = reduced ? 0ms : 200ms`. `Theme.reduced-motion` is an `in` property set from settings (Slint has no OS reduced-motion query in 1.17).
- Radius: `0px` bands and rules, `2px` fields and buttons, `4px` server rows. No larger radius anywhere.
- Spacing scale: 4/8/12/16/24/32. Page inset 24 px. Band gap 16 px. Row padding 12 px.
- Type: system default sans (`default-font-family` unset so Korean renders with Malgun Gothic / Apple SD Gothic Neo). Sizes: eyebrow 12 (muted, weight 600), body 14, row title 15 (600), band title 16 (700), wordmark 22 (700), stamp 18 (700). Digits, hashes, log lines: `font-family: "Consolas"` (falls back automatically), size 12–13.
- Minimum interactive size 44×44 px. Shadows: none. Glass/blur: none. Gradients: none.

## 4. Layout (`ui/app.slint`), 720×960

```
┌────────────────────────────────────────────────────────────┐ 24px inset
│ Nebula                             [설정]                  │ header: wordmark 22px + one ghost button
│ 배포본 빌드 시트                                             │ eyebrow (document type, muted)
│ ───────────────────────────────────────────────────────────│ rule
│ 루트          D:\TestRoot                                   │ readiness table: 4 rows label(muted)/value(mono)
│ 기본 URL      https://cdn.example.com/                       │ value column tabular; missing → "미설정" in warning
│ Java          C:\...\jdk-21\bin\java.exe   확인됨            │
│ Helios 폴더   미설정                                          │
│ ═══════════════════════════════════════════════════════════│ band rule (2px)
│ 서버  3                    [모드팩 가져오기] [새 서버]         │ band title + count (mono) + 2 secondary buttons
│ ┌─────────────────────────────────────────────────────────┐│
│ │ Test-1.12.2                      Forge 14.23.5.2860     ││ row: id (15/600) + loader label (12 mono, bordered)
│ │ 모드 42 · 파일 118 · 라이브러리 3            [폴더 열기]   ││ counts mono 13, one ghost action
│ ├─────────────────────────────────────────────────────────┤│
│ │ Sky-1.20.1                        Fabric 0.15.11        ││
│ │ …                                                       ││ list scrolls inside the band (max 3.5 rows visible)
│ └─────────────────────────────────────────────────────────┘│
│ ═══════════════════════════════════════════════════════════│
│ 배포본                                                      │ band title
│ 파일 이름  [distribution        ].json                       │ Field: label left, LineEdit, suffix
│ [x] 생성 후 Helios 데이터 폴더에 설치  (disabled if unset)     │ Toggle rows (checkbox style, 44px)
│ [ ] Forge 설치 캐시 무효화    [ ] 설치 출력 삭제               │
│ ┌──────────────────────┐  [기존 파일 검증]                    │ primary 240×48 + one secondary
│ │      배포본 생성      │                                    │
│ └──────────────────────┘                                    │
│ 아티팩트 163 · SHA-256 163 · 오류 0 · 경고 2                  │ result summary (mono), empty until a run
│  경고  servers[1].modules[7].artifact.url  http:// URL…      │ findings list (severity as text + colour)
│  경고  …                                                    │
│                       ╔══════════════════╗                  │ Stamp (rotated −6°) only on success
│                       ║   검증 통과       ║                  │
│                       ║   a91f3c02 · 14:02║                  │
│                       ╚══════════════════╝                  │
│ ═══════════════════════════════════════════════════════════│
│ 로그                                              [지우기]   │ log band (raised bg), mono 12, 400 rows max
│ 14:02:11 forge   Using cached results at D:\TestRoot\cache… │
│ 14:02:13 files   118 modules hashed                          │
└────────────────────────────────────────────────────────────┘
```

Vertical budget at 960: header 96, readiness 4×28=112, servers band 40 + rows (3×64) = 232, build band 40+40+2×44+64+ result ≈ 300, log band ≥ 140 (flex). The whole sheet is one `VerticalLayout` inside a `ScrollView` (Slint `Flickable`) so smaller heights scroll instead of clipping; the primary button stays inside the flow (no sticky footer).

### Screens (page switch inside the same window, `AppPage` enum: sheet, new-server, import, settings)

Each secondary screen replaces the sheet below the header and starts with a ghost button `← 시트로 돌아가기` (44 px). Navigation is disabled while `busy`.

**new-server**: fields `서버 ID` (LineEdit, required, helper "폴더 이름은 <ID>-<버전>이 됩니다"), `Minecraft 버전` (LineEdit, required, e.g. 1.20.1), `로더` (three exclusive toggle buttons: 없음 / Forge / Fabric), `로더 버전` (LineEdit + two secondary buttons `최신 버전 찾기` / `권장 버전 찾기` which fill the field from the network; hidden when 로더=없음). Forge with MC ≥ 1.20.3 shows the inline warning text from upstream (Forge 1.20.3+ removed `--fml.modLists`; Helios cannot load mods). Primary `서버 만들기`. Errors appear under the offending field in danger colour with the cause and the fix.

**import**: lists `*.zip` in `<root>/modpacks/curseforge/` (empty state: "modpacks/curseforge 폴더에 CurseForge 모드팩 zip을 넣으면 여기에 표시됩니다." + `폴더 열기`), field `서버 ID`, primary `모드팩 가져오기`. After import, manual-download list (name, file, URL as selectable text) is shown in a warning box; nothing is hidden in the log only.

**settings**: fields 루트, 기본 URL, Java 실행 파일, Helios 데이터 폴더 (LineEdits; no native file dialog dependency), 테마 (시스템/라이트/다크), 애니메이션 줄이기 (toggle). Primary `저장`. Validation on blur: base URL must parse and gets a trailing `/`; root must exist or offers `폴더 만들기`.

## 5. Components (`ui/components/`)

- `SheetButton` (kind: primary | secondary | ghost; `text`, `a11y-label`, `enabled`, `loading`): Rectangle, radius 2, border 1 line (2 focus), primary fill = `Theme.primary`; hover = raised; pressed = line; loading shows `text` unchanged with a 3-dot prefix "… " and `accessible-enabled: false`. FocusScope handles Enter/Space. `min-height: 44px`.
- `Field` (label, value <=> LineEdit, helper, error, suffix): label 12 muted above, LineEdit 44 px, helper/error 12 below; error text starts with "오류: ".
- `ToggleRow` (label, checked <=> , enabled): 44 px row, 18 px square box with 2 px border, filled with `primary` and a check drawn with two `Path`/`Rectangle` strokes when checked; role checkbox, label is the accessible label.
- `ServerRow`: as drawn; `폴더 열기` ghost button; row is focusable but not a button (no action besides opening the folder).
- `FindingRow`: severity word (`오류`/`경고`, 12/600, coloured) + location (mono 12 muted) + message (13, wraps).
- `Stamp`: Slint 1.17.1 has **no rotation on `Rectangle`** (verified: `Unknown property rotation-angle in Rectangle`). Build it as: an `Image { source: @image-url("../assets/stamp-frame.svg"); colorize: Theme.primary; transform-rotation: -6deg; }` whose SVG is a 240×96 double border (3 px outer, 1 px inner 4 px inset, transparent centre), plus three `Text`s each with `transform-rotation: -6deg` and `transform-origin` at the centre, laid over the image. Verify with a software-renderer screenshot; if rotated text renders incorrectly, ship the upright variant (same frame, no rotation) — the evidence (hash + time) matters more than the tilt. Appears via `if stamped : Stamp {}` with `animate opacity { duration: Theme.stamp; }` (opacity rather than size, so layout never shifts).
- `LogRow`: time (mono muted) + target (mono, 6 chars padded) + message (mono, wraps).

All custom interactive components: `accessible-role`, `accessible-label`, `accessible-enabled`, visible 2 px focus ring in `Theme.focus`, `mouse-cursor: pointer` when enabled.

## 6. States and copy (Korean, sentence case, active voice)

| State | What shows |
|---|---|
| First run (no settings) | Readiness rows show `미설정`; servers band empty state: "루트 폴더를 먼저 설정하세요." + `설정 열기`. Primary disabled with helper "루트와 기본 URL을 설정하면 생성할 수 있습니다." |
| Root set, no servers | "아직 서버가 없습니다. 새 서버를 만들거나 CurseForge 모드팩을 가져오세요." + the two band buttons. |
| Root set, root not initialised | Banner in build band: "이 루트에는 meta/distrometa.json이 없습니다." + `루트 초기화` secondary button (runs init root). |
| Busy | Primary label → `생성 중…` (loading), navigation and other actions disabled, band eyebrow "작업 중: 배포본 생성" ; log streams. |
| Success | Summary line + findings + Stamp; log line "distribution.json 저장: <path>". If installed locally: second line "Helios 데이터 폴더에 설치: <path>". |
| Validation errors | Summary with 오류 N in danger; findings list; no stamp; primary re-enabled. |
| Job failure | Danger-bordered box: "생성 실패: <error message>" + `다시 시도` secondary + hint when the message mentions Forge installer directory. |
| Loader version lookup | Buttons show loading; on failure the field error says "버전을 가져오지 못했습니다: <reason>". |

Never show fabricated numbers: counts come from a real scan (`list_servers`), the summary from `validate::Report`, the stamp hash from the written file.

## 7. Keyboard and accessibility

- Tab order = visual order; Enter/Space activate; Esc on secondary screens returns to the sheet (when not busy).
- Every button/toggle/field has an accessible label in Korean; log and findings are readable text (selectable `TextInput { read-only: true }` for the manual-download URLs).
- Focus ring 2 px `Theme.focus`, contrast ≥ 3:1 against both surface and bg in both themes (`#B4530D` on `#F7F4EC` 4.9:1; `#F2A65A` on `#1E2229` 7.7:1).
- Colour is never the only carrier: severity words, `확인됨`/`미설정` text, stamp text.

## 8. Implementation notes for Task 6

- Slint file layout: `app.slint` (imports tokens + screens, `export component App inherits Window`), `tokens.slint` (Theme, enums `AppPage`, `ButtonKind`, structs `ServerRowData { id, loader, counts, path }`, `FindingRowData { severity, location, message, is-error }`, `LogRowData { time, target, message }`, `ReadinessRowData { label, value, state }`), `components/*.slint`, `screens/{sheet,new-server,import,settings}.slint`.
- Globals: `SheetData` (in properties: readiness rows, servers model, summary text, findings model, stamped, stamp-hash, stamp-time, busy, busy-label, error-text, root-initialised, can-generate, install-local-enabled), `SheetActions` (callbacks: generate(name, install-local, invalidate-cache, discard-output), validate-existing(name), open-folder(path), refresh, init-root, navigate(AppPage), create-server(id, mc, loader, loader-version), lookup-loader-version(loader, promotion), import-modpack(id, zip), save-settings(...), clear-log).
- Rust wires globals via `app.global::<SheetActions>().on_generate(...)`; models are `Rc<VecModel<_>>`.
- Verify with `slint-viewer --check` and screenshots (`--screenshot`, `--load-data` JSON states for dark/busy/error/stamped) before declaring done; put the PNGs under `docs/plans/gui/`.

## 9. Additions (2026-09-10, later in the day): NeoForge, import formats, publishing

These extend §4–§6 without changing the direction, tokens, or rhythm.

- **Loader selector (new-server screen):** four exclusive toggle buttons 없음 / Forge / NeoForge / Fabric (`Loader` enum). NeoForge shows helper text "Minecraft 1.20.2 이상" and the same 최신/권장 lookup buttons. Forge on MC ≥ 1.20.3 keeps the upstream warning; NeoForge shows a one-line note "Hambugui 런처에 NeoForge 패치가 필요합니다 (docs/launcher)".
- **Import screen:** lists every `modpacks/**/*.zip` and `*.mrpack`; each row shows the file name, the detected format as a small bordered mono label (`CurseForge` / `Modrinth` / `Prism 인스턴스` / `알 수 없음`), and the size. Rows with `알 수 없음` are not selectable and explain why in the helper text ("manifest.json, modrinth.index.json, mmc-pack.json 중 하나가 없습니다"). Primary stays `모드팩 가져오기`.
- **배포 band (sheet, between the build band and the log band):** title `배포`; target selector as three exclusive toggle buttons 없음 / WebDAV / Google Drive; a toggle row `변경된 파일만 업로드` (checked by default); one secondary button `업로드` (disabled when target is 없음, when no distribution.json exists, or when busy); result line in mono: `N개 업로드 · M개 변경 없음 · 배포 URL: …` with the URL as selectable read-only text. Google Drive additionally shows an eyebrow-sized note: "Drive는 CDN이 아닙니다. 100 MB 초과 파일과 다운로드 쿼터에 주의하세요 (README 참고)". The band never contains the primary button; `배포본 생성` remains the only primary on the sheet.
- **Settings screen additions:** two groups under the existing fields. `WebDAV`: 업로드 URL, 사용자, 비밀번호 (`LineEdit { input-type: password; }`), 공개 URL (helper: "비우면 기본 URL을 사용합니다"). `Google Drive`: 클라이언트 ID, 클라이언트 시크릿 (password input), 폴더 ID, and a secondary button `Google 계정 연결` whose result line reads `연결됨 (토큰 저장됨)` or `연결되지 않음`. Group titles are 16/700 like band titles; groups are separated by the 2 px band rule.
- **Vertical budget:** the 배포 band adds ~150 px; the whole sheet already scrolls, and at 960 px the log band keeps a minimum of 140 px.
- **Copy rules unchanged:** sentence case, no emoji, no arrows, active verbs (`업로드`, `연결`), errors state cause + fix ("업로드 실패: 401 인증 거부 — 설정에서 WebDAV 사용자와 비밀번호를 확인하세요").
- **Globals to add:** `PublishData { target: PublishTarget (none|webdav|gdrive), only-changed: bool, can-publish: bool, result-line: string, distribution-url: string }`, `SheetActions.publish(PublishTarget, bool)`, `SettingsData` fields for the eight new values plus `gdrive-connected: bool`, `SheetActions.connect-gdrive()`, `ImportRowData { file-name, format, size-label, selectable }`.

## 10. Contrast check script

```python
def lum(h):
    r,g,b=[int(h[i:i+2],16)/255 for i in (1,3,5)]
    f=lambda c: c/12.92 if c<=0.03928 else ((c+0.055)/1.055)**2.4
    return 0.2126*f(r)+0.7152*f(g)+0.0722*f(b)
def cr(a,b):
    la,lb=sorted([lum(a),lum(b)],reverse=True); return (la+0.05)/(lb+0.05)
```
