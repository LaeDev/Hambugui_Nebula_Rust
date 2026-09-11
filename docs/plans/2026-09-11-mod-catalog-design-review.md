# 모드 page addendum — merged design review and decision list

Date: 2026-09-11. For: design lead. Status: not committed; no code or spec edited.

Inputs merged (three independent reviews of `DESIGN-v2.md` §8 and the GUI section of `2026-09-11-mod-catalog.md`):

| Tag | Lens | Scale used |
|---|---|---|
| **N** | Nielsen's 10 heuristics + WCAG 2.2 AA; persona = Korean server admin, Windows, mouse / keyboard-only / UIA screen reader | Nielsen 0–4 |
| **A** | antislop-ui checklist against §8 read with §1–§3 as the governing system | high / medium / low |
| **S** | Slint 1.17.1 feasibility, verified from crate sources, target fingerprints and three headless software-renderer probes | blocker / pitfall / must-add / minor |

Merged scale: **P0** = blocks implementation start; **P1** = must-fix before implementation (spec gap that would be filled silently by whoever implements); **P2** = must be worded, may be settled during implementation; **P3** = nice-to-have. Every item carries `Gate: must-fix` or `Gate: nice-to-have`.

Where two reviews disagree the item says so and recommends one option. Proposed wording is written to drop into DESIGN-v2 §8 (or the plan) as-is; strings in backticks are final UI copy.

---

## 0. Decisions required from the design lead

| # | Decision | Recommendation | Items |
|---|---|---|---|
| D1 | Where the target server (id · loader · MC version) is displayed and named in toasts/dialogs; selection default and persistence | Context caption under the strip; server id in every job toast and dialog title; in-session persistence, default first server | P0-1 |
| D2 | WebP icon decoding path | Add direct `image` dep with `webp`; worker transcodes to 72 px PNG (plan `icons.rs` already says this — the dep line is what is missing) | P0-2, P1-13 |
| D3 | Server selector: wrapping pill row vs horizontal `Flickable` strip | Wrapping row (removes overflow, scroll-into-view and wheel rules; servers are few) | P1-1 |
| D4 | Persistent install-progress carrier | One sticky toast updated in place with `accessible-live-region: polite`, plus the §5 top-bar pill + bar | P1-2 |
| D5 | Busy scope for catalog jobs | Per-page `ModsData.busy`, server-scoped; top-bar pill/bar shown; `DialogKind` enumerated | P1-3 |
| D6 | Whether an install always confirms through a dialog | Yes: one `install` dialog showing resolved version, release type, size, namespace, dependencies, total — replaces "optional `함께 설치할 항목`" | P1-5, P1-6, P1-7, P1-8 |
| D7 | Default namespace and where it is chosen | `required/`; `SegmentedControl` in the install dialog; dependencies follow the parent | P1-8 |
| D8 | Inline `정말 삭제` kept vs replaced | Kept (one `Timer`, no Rust), with a double-click guard, Esc interception and stated focus target | P1-9 |
| D9 | Accent budget | Namespace chips all neutral; `업데이트 가능` is the only row-level accent; drop `neutral outline` | P1-11 |
| D10 | `버전 선택` link on 찾기 rows | Drop it; 찾기 rows get the same trailing chevron → mod dialog, whose version rows install | P1-6 |
| D11 | Single source for GUI spec | DESIGN-v2 §8; plan GUI section reduced to a pointer | P2-6 |

---

## 1. P0 — blocks implementation start

### P0-1 · Installs target an invisible server
- **Sources**: N-1 (4).
- **Claim**: Nothing on the 모드 page says which server, loader and MC version the 설치됨 list and 찾기 results are scoped to, so an admin with `Test-1.12.2` and `Neo-1.21.1` installs into the wrong pack with no confirmation.
- **Evidence**: §8.1 pills carry only the id; §8.3 header `설치된 모드 N`, §8.5 toast `모드 3개를 설치했습니다` — neither names the server; plan §Scope filters by the server's MC version and loader but the filter is never displayed; TopBar title stays the page name (§3).
- **Proposed wording → §8.1** (append):
  > Under the strip, a 13 px text-2 caption for the selected server: `Minecraft 1.21.1 · NeoForge 21.1.72` (loader + version from the server's `sheet.json`; `로더 없음` if none). Every job toast and dialog title carries the server id: `Neo-1.21.1에 모드 3개를 설치했습니다`, `설치 실패 · Neo-1.21.1`, install dialog title `Neo-1.21.1에 설치`. Selection: default = first server in the 서버 page order; `모드 관리` from 서버 selects that server; the selection survives page switches within a session and is not persisted across restarts; when the selected server is deleted the first remaining server is selected (EmptyState if none); `복제` keeps the source selected.
- **Gate**: must-fix.

### P0-2 · The running binary cannot decode WebP, and format is picked by file extension
- **Sources**: S-1 (blocker).
- **Claim**: Modrinth serves icons as `.webp`; the target build of `image` has only `png` + `jpeg`; Slint's `Image::load_from_path` resolves the decoder from the extension and never sniffs bytes; decode errors are swallowed by `.ok()` into the initials fallback — silent failure for every Modrinth icon.
- **Evidence**: `target/debug/.fingerprint/image-*/lib-image.json` features `["jpeg","png"]`; `i-slint-core-1.17.1/Cargo.toml:191-196`; `graphics/image/cache.rs:102` `image::open(path)`; `image-0.25.10/src/io/image_reader_type.rs:325-361`. `crates/nebula/Cargo.toml` has no direct `image` dependency (confirmed); `image-webp 0.2.4` is already in `Cargo.lock`.
- **Proposed wording → plan `icons.rs` row** (replace):
  > `icons.rs`: fetch `icon_url` on the worker → `image::load_from_memory` (crate `image = { version = "0.25", default-features = false, features = ["png","jpeg","webp"] }` added to `crates/nebula/Cargo.toml`; cargo unifies it into i-slint-core's target build, no new lockfile packages) → `imageops::resize` to 72×72 → `save` PNG at `<root>/cache/icons/<provider>-<project_id>-<sha1 of source bytes>.png`. The GUI only ever loads `.png`. Do **not** enable slint's `image-default-formats` (drags avif/exr/tiff and a target-side rav1e build).
- **Gate**: must-fix.

---

## 2. P1 — must-fix before implementation

### P1-1 · Server selector is not a real control
- **Sources**: N-2 (3), A-6 (low), S-5 (must-add) — merged; S and A disagree on layout.
- **Claim**: `Chip` is a non-interactive `Rectangle` (no click, focus, role); 24 px against the ≥ 44 px hit rule; the strip reuses status-chip vocabulary with no group label; a horizontal scroller has no stated overflow affordance and nothing scrolls the selected pill into view.
- **Evidence**: `chip.slint` `export component Chip inherits Rectangle` with `text`/`tone`/`icon` only; §3 hit rule; §8.1 "Chip-sized"; `segmented.slint` is `tab-list`, capped at 2–4 options. S probe: horizontal `Flickable` does work nested in the page `ScrollView` (vertical wheel forwards out, Shift+wheel scrolls it), but clips its viewport and would need an imperative `viewport-x` bring-into-view.
- **Decision (D3)**: A recommends a wrapping row; S built the Flickable. Recommend **wrapping row** — one layout property, no overflow/scroll rules to specify, and `Test-1.12.2`-length ids fit ~6 per 720 px line.
- **Proposed wording → §8.1** (replace sentence 1):
  > Server selector: new `ui/lib/pill-strip.slint` (`PillStrip`, cloned from `SegmentedControl`): a wrapping row (`HorizontalLayout` inside a wrapping flow, 8 px gaps) of 32 px `r-pill` pills, each in a 44 px hit box; unselected = surface-2 fill + border, text-2; selected = accent-soft fill + accent border, accent text; group label `서버` 13/500 text-2 to the left. Keyboard: one tab stop for the group, Left/Right/Home/End move selection, focus ring per §2. Accessibility: root `accessible-role: radio-group`, `accessible-label: 서버`; each pill `radio-button` + `accessible-checkable/checked`, `accessible-label` = server id, `accessible-id: mods-server-<id>`.
- **Gate**: must-fix.

### P1-2 · Per-file progress toasts contradict the Toast contract
- **Sources**: N-3 (3), S-7 (minor), A-11 (low).
- **Claim**: Toasts auto-dismiss after 4 s and stack max 3, so a 5-file install loses its progress toast or evicts others; toasts are `accessible-role: text` with no live region; the two docs disagree on copy (`… · fabric-api-0.92.jar` vs `… · 12.3 MB`).
- **Evidence**: `gui/mod.rs:44-45` `TOAST_LIFETIME = 4 s`, `MAX_TOASTS = 3`, `:590-596` trims from the front and schedules `single_shot`; `toast.slint` role text; `accessible-live-region` (off/polite/…) exists in 1.17.1 (`typeregister.rs:348-351`, `accesskit.rs:691`). §5 already has a persistent carrier (top-bar pill + indeterminate bar).
- **Decision (D4)**: Recommend a sticky toast updated in place (keeps §8's promise that the file name reaches the reader) **and** the §5 top-bar pill.
- **Proposed wording → §8.5** (replace "per-file progress toasts (…)"):
  > Progress: one sticky `Toast neutral` (no auto-dismiss, exempt from the max-3 trim, updated in place; `accessible-live-region: polite`) reading `다운로드 중 2/5 · fabric-api-0.92.jar`; the top bar shows the §5 pill `모드 설치 중 · 0:12` + indeterminate bar. The clicked `설치` button is `loading`; all other `설치` / `업데이트` / `삭제` buttons for that server are disabled (helper on the card header: `작업이 끝나면 다시 시도할 수 있습니다`). When the job ends the sticky toast is replaced by the success or failure toast.
  > Rust: `progress_toast(id, message)` pushes without the `single_shot`, updates via `VecModel::set_row_data`, is skipped by the trim, and is dismissed on completion. Plan line 33 copy is superseded by this section.
- **Gate**: must-fix.

### P1-3 · Busy semantics and dialog inventory for catalog jobs are undefined
- **Sources**: N-4 (3), S-8 (minor: `DialogKind` variants).
- **Claim**: §5 says dialogs cannot open while busy and `dialog.slint` refuses Esc while a job runs, yet the flow is resolve (network) → dialog → download. `DialogKind` has `none, new-server`; §8 and the plan add at least six dialogs without naming them or their transitions.
- **Evidence**: `data.slint:10` `enum DialogKind { none, new-server }`; ux-fixes item 6 already made busy per-screen; plan Reliability rule 2 "one catalog job at a time per server".
- **Proposed wording → §8, new sub-section "Busy and dialogs"**:
  > Catalog jobs set `ModsData.busy` (per page, server-scoped: `ModsData.busy-server` = id). While set: top-bar pill + bar per §5; on the 모드 page the row menus, mod dialog and install buttons for that server are disabled; other servers, navigation and 찾기 typing stay enabled. `resolve` runs busy; when it completes, busy clears **before** the host opens the install dialog, so the user is never in a dialog while busy and Esc (cancel of the whole install) works per `dialog.slint`. Confirming the dialog sets busy again for `apply`.
  > `DialogKind { none, new-server, mod, install, server-menu, duplicate-server, delete-server }`. Transitions: 찾기/설치됨 row chevron → `mod`; `설치` (row or version row) → resolve → `install`; `install` confirm → closes, apply runs, no dialog; `install` cancel/Esc → closes, nothing installed, focus returns to the invoking button; 서버 row menu button → `server-menu`; `복제` → `duplicate-server`; `삭제` → `delete-server`; closing any of the last two returns to the page, not to `server-menu`.
- **Gate**: must-fix.

### P1-4 · Generic install/update/search failure UI is missing
- **Sources**: N-5 (3), A-3 (medium: error must name what failed).
- **Claim**: Only the CurseForge manual-download case is specified. Rollback, 429, offline and search-provider failures have no copy or location; the 찾기 error callout has action but no cause.
- **Evidence**: §8.5 warning callout only; §5 failure pattern (toast + `Callout danger` with cause + fix + `다시 시도`); plan `install.rs` "rollback on any failure"; facts A.1 429 with `X-Ratelimit-Reset` seconds.
- **Proposed wording → §8.5** (append "Failures"):
  > Failures follow §5. Install/update: `Toast danger` `설치 실패 · {server}` + `Callout danger` at the top of the 설치됨 card, title = cause (`Modrinth에 연결하지 못했습니다` / `요청이 너무 많습니다 — {N}초 후 다시 시도하세요` / `네트워크에 연결되어 있지 않습니다` / `해시가 일치하지 않습니다 · {file}`), body always ends with `설치된 파일 없음 — 변경 사항을 되돌렸습니다`, action `다시 시도` re-runs **the same resolved plan** (a fresh resolve only if the plan is older than 10 minutes). Search: `Callout danger` in the results area, title `{provider}에 연결하지 못했습니다`, body = error text, `다시 시도` re-runs the same query/provider/sort/offset 0. The callout clears on the next successful job of the same kind.
- **Gate**: must-fix.

### P1-5 · The installed version is decided silently after the click
- **Sources**: N-6 (3).
- **Claim**: Newest-beta-newer-than-newest-release is the common case on both providers; the beta fallback "warning" has no location or blocking rule; "no compatible version" is unspecified; `버전 선택` exists in the plan but not in §8.
- **Evidence**: plan "latest release for mc+loader; else latest beta with a warning"; facts A.4 (Modrinth element #2 is a newer beta), B.4 (`releaseType: 2` on newest 1.20.1 Forge JEI).
- **Decision (D6)**: Make the install dialog unconditional (see P1-7 for its contents).
- **Proposed wording → §8.5** (replace "resolve → optional `함께 설치할 항목` dialog"):
  > `설치` → resolve → **install dialog always opens** (`DialogKind.install`) showing the resolved version before any download. Beta/alpha fallback: `Callout warning` inside the dialog, `정식 버전이 없어 {베타|알파} {version}을 설치합니다` — informational, not blocking. No version matches mc+loader: the 찾기 row's `설치` is disabled with subtitle suffix `· {mc} {loader}용 버전 없음`; search facets pre-filter by mc+loader where the provider supports it (Modrinth facets, CurseForge `gameVersion`+`modLoaderType`), so this state is rare.
- **Gate**: must-fix.

### P1-6 · `설치` on a version row of an installed mod is ambiguous
- **Sources**: N-7 (3), plan-only `버전 선택` link (N-17).
- **Claim**: Replace vs add-second-jar is unstated; a second jar with the same mod id crashes the game; a downgrade silently replaces a newer file.
- **Evidence**: §8.3 version list `설치`/`설치됨`; plan `apply_update` = install new + remove old atomically.
- **Decision (D10)**: Drop `버전 선택`; version picking = mod dialog for both installed and not-yet-installed projects.
- **Proposed wording → §8.3 mod dialog**:
  > Version list rows: current version `설치됨` chip (no button); newer versions `업데이트`; older versions `되돌리기`; for an uninstalled project every row shows `설치`. Any version-row action opens the install dialog with the first row `{title} {old} → {new}` (`다운그레이드` warning chip on `되돌리기`); confirming replaces the installed file atomically in its current namespace (plan `apply_update`) and the index records the new version id, file hash and `installed_at`. 찾기 rows gain the same trailing `IconButton chevron-right` (`accessible-label: {title} 자세히`) opening this dialog.
- **Gate**: must-fix.

### P1-7 · The dependency dialog is under-specified on the cases that break packs
- **Sources**: N-8 (3).
- **Claim**: Unchecking a required dependency, optional/incompatible/embedded dependencies, dependencies with no compatible version (Prism drops silently), dependencies already present as `직접 추가` or other-provider jars, and total size are all unstated.
- **Evidence**: facts C.5: only `REQUIRED` handled, others skipped without warning, unresolvable dropped silently, provider-scoped satisfaction; plan `plan_install` `optional_deps` listed not installed, "already-installed skipped".
- **Proposed wording → §8.5 "Install dialog"** (replaces `함께 설치할 항목`):
  > Title `{server}에 설치`. Body: (1) the mod row — title, `{version}` + release-type chip (`정식`/`베타`/`알파`), size; (2) `SegmentedControl` 필수 / 선택 켜짐 / 선택 꺼짐 (default 필수, see P1-8); (3) `함께 설치할 항목` — required dependencies as checkable `ListRow`s, pre-checked, with title/version/size; unchecking shows an inline `Callout warning` `{dep}이 없으면 {mod}이 로드되지 않습니다`; a dependency already present (matched by provider project id, **or** by mod id read from jar metadata for `직접 추가` jars) is listed unchecked-disabled with a `설치됨` chip, never hidden; (4) `선택 항목` — optional dependencies listed unchecked, collapsed by default (`선택 항목 N개` disclosure); (5) incompatible dependencies present in the pack → `Callout danger` `{dep}과 함께 사용할 수 없습니다` and the primary is disabled; (6) a required dependency with no version for mc+loader → `Callout danger` `{dep}의 {mc} {loader}용 버전이 없습니다` and the primary is disabled (never a silent drop); embedded dependencies are not shown. Footer: caption `{N}개 · {size}` left, `취소` / primary `{N}개 설치` right. Esc = 취소.
- **Gate**: must-fix.

### P1-8 · Namespace at install time is never shown
- **Sources**: N-9 (3).
- **Claim**: Namespaces decide whether players receive the mod; the CLI has `--namespace`, the GUI flow says nothing.
- **Evidence**: plan CLI `--namespace required|optionalon|optionaloff`; `apply(plan, target namespace)`; §8.3 only has post-install move actions.
- **Decision (D7)**: dependencies follow the parent's namespace (a required dep of an optional mod should not be forced on every player).
- **Proposed wording → §8.5 install dialog** (item 2 above, extend):
  > Default namespace `필수` (`required/`); changeable in the dialog; helper under the control `의존 항목도 같은 위치에 설치됩니다`. Dependencies already installed elsewhere are left where they are.
- **Gate**: must-fix.

### P1-9 · Inline `정말 삭제` is a double-click hazard with no announcement, permanence, dependents or focus rule
- **Sources**: N-10 (3), S-3 (pitfall).
- **Claim**: The morphing button can be hit by the second click of a double-click; the accessible name changes silently; Esc bubbles to app.slint and closes the whole dialog; no statement of permanence (unlink vs recycle bin), dependents, or focus after the row disappears. S confirmed `Timer` does the 5 s with zero Rust but cannot be a direct child of `if`/`for`.
- **Evidence**: `builtins.slint:2695-2745` Timer; `slint-viewer --check` "Timer cannot be directly repeated or conditional"; app.slint root FocusScope closes any dialog on Esc; plan `index.rs` has no reverse-dependency data.
- **Decision (D8)**: keep the inline pattern.
- **Proposed wording → §8.3 mod dialog `삭제`**:
  > `Button danger` `삭제` (`accessible-label: {title} 삭제`). Click → armed: label `정말 삭제`, `accessible-label: {title} 영구 삭제 — 파일을 지웁니다`, disabled for 400 ms (double-click guard), disarms after 5 s of no interaction, on Esc, or on focus leaving the button. Esc while armed only disarms (a `FocusScope` around the dialog body accepts `Key.Escape` only when armed). Deletion unlinks the file permanently (no recycle bin) and removes the index entry; helper under the button while armed: `{file}을 영구 삭제합니다`. If the index lists installed mods that depend on this one (see plan change below) the helper reads `{a}, {b}이 이 모드를 필요로 합니다`. After deletion the dialog closes, `Toast neutral` `{title}을 삭제했습니다`, focus moves to the chevron of the row that took its place (or the card's `업데이트 확인` when the list is empty).
  > Slint: `ModData.delete-armed` global bool (so `--load-data` renders the state), `Timer { interval: 5s; running: ModData.delete-armed; triggered => { ModData.delete-armed = false } }` as a direct child of the dialog component, not under `if`.
- **Proposed plan change → `index.rs`**: each entry records `dependencies: [provider project id]` from the install plan, so reverse lookup is a scan of the index.
- **Gate**: must-fix.

### P1-10 · Server-delete dialog rules
- **Sources**: N-11 (3).
- **Claim**: Match rule, primary enable logic, Enter behaviour (§6 makes Enter submit), absolute path, distribution consequence, blocking while jobs run, and focus return are unstated.
- **Evidence**: §8 서버 page; §6 "Enter submits dialogs"; plan `delete_server` refuses outside `servers/`; ux-fixes item 12 house style for path copy.
- **Proposed wording → §8 서버 page `삭제`**:
  > `DialogKind.delete-server`, title `{id} 삭제`. Body: `Callout danger` title `폴더를 영구 삭제합니다`, body `{absolute path}` (read-only `TextField`, `select-all`, helper `드래그해서 선택한 뒤 Ctrl+C로 복사`), second line `다음 배포본부터 플레이어에게 이 서버가 표시되지 않습니다`. `TextField` label `확인하려면 서버 ID를 입력하세요`; match = trimmed input equals the id exactly (case-sensitive; paste allowed). Primary `영구 삭제` (`Button danger`) is disabled until the match holds; Enter submits only when it holds. Disabled with helper `이 서버에서 작업이 진행 중입니다` while a build or catalog job targets the server. On success: dialog closes, `Toast neutral` `{id}를 삭제했습니다`, focus to the next row's menu button (or `새 서버` when none remain); the 모드 page reselects per P0-1.
- **Gate**: must-fix.

### P1-11 · Accent budget and the invented `neutral outline` tone
- **Sources**: A-1 (high), A-10 (low: chip inflation).
- **Claim**: §8.3 puts accent on `선택 켜짐`, `업데이트 가능`, `모두 업데이트`, the selected pill and the nav item — a 40-mod list shows ~25 accent chips; `neutral outline` does not exist in `theme.slint` `Tone { neutral, accent, success, warning, danger }` and contradicts "no new tokens". Version as a chip is data, not status.
- **Evidence**: DESIGN-v2 §8.3; `theme.slint:10`; `chip.slint`.
- **Proposed wording → §8.3 rows** (replace the chip clause):
  > subtitle `0.92.0 · fabric-api-0.92.jar · Modrinth` (version moves into the subtitle) or `0.92.0 · fabric-api-0.92.jar · 직접 추가`; at most two chips: namespace (`필수` / `선택 켜짐` / `선택 꺼짐`, all neutral — the text differs) and `업데이트 가능` (accent, the only row-level accent because it is the same signal as the `모두 업데이트` primary). Accent on the page: nav item, selected pill, one primary button, `업데이트 가능` chips — nothing else.
- **Gate**: must-fix (spec references a token that does not exist).

### P1-12 · 설치됨 has no empty or loading state
- **Sources**: A-2 (high).
- **Claim**: N = 0 renders a header over a blank card; nothing is shown while the jar list/index is read.
- **Evidence**: §8.3 lists header, buttons, rows only; `servers.slint:98` already has `if SheetData.loading : EmptyState`.
- **Proposed wording → §8.3** (append):
  > States: loading — `EmptyState` `모드 목록을 읽는 중` (same pattern as servers.slint); empty — `EmptyState` title `설치된 모드가 없습니다`, body `찾기에서 검색해 설치하세요`, button `찾기` switches the view.
- **Gate**: must-fix.

### P1-13 · Runtime icons: UI-thread decode, path-keyed 5 MB LRU, empty-image struct field
- **Sources**: S-2 (pitfall).
- **Claim**: `slint::Image` is `!Send`; the image cache is keyed by path with a 5 MB weight so a re-downloaded file at the same path is served stale and one 1024² RGBA icon nearly fills it; `--load-data` cannot write an empty image into a struct.
- **Evidence**: `gui/mod.rs:445-455` (paths cross threads, decode on the event loop); `i-slint-core-1.17.1/graphics/image/cache.rs:47-51`; `data.slint:97-101` and `SheetData.server-icons: [image]`.
- **Proposed wording → §7 Implementation notes** (append):
  > Mod and result icons follow the server-icon pattern: parallel `[image]` models (`ModsData.mod-icons`, `ModsData.result-icons`) next to the row structs, decoded on the UI thread from worker-produced PNG paths; cache files are content-hash named (P0-2) so a changed icon is a new path; the worker downsizes to 72 px so decode never hitches the event loop.
- **Gate**: must-fix.

### P1-14 · Mod dialog layout traps, namespace verb buttons, and the missing `Button danger`
- **Sources**: A-4 (medium), S-4 (must-add).
- **Claim**: Three verb buttons (필수로 / 선택(켜짐)으로 / 선택(꺼짐)으로) express one parameter and one is always dead; five actions do not fit the 520 px dialog body (first collapsed to `…` in the probe); `ListView` inside `Dialog` renders at its `min-height` and needs an explicit height; the fluent scrollbar overlays the trailing button; delegates need explicit `height`. §3 promises a `danger` Button variant that `button.slint` does not have.
- **Evidence**: probe2.png; `button.slint:5` `enum ButtonVariant { primary, secondary, ghost }` (confirmed); `widgets/common/listview.slint`.
- **Proposed wording → §8.3 mod dialog** (replace the actions clause):
  > Actions row: `SegmentedControl` 필수 / 선택 켜짐 / 선택 꺼짐 bound to the namespace (shows and changes it), `Button ghost` `프로젝트 페이지 열기` (hidden when the row has no provider project id), `Button danger` `삭제`. Version list: `ListView { height: 360px }` with `ListRow { height: 56px }` delegates and `padding-right: Theme.sp-3` so the trailing button clears the scrollbar; rows scrolled out are absent from the accessibility tree (§6 ceiling, accepted).
- **Proposed lib change → §3 Button**: add `ButtonVariant.danger` (danger-soft fill, danger text, danger-soft border) — §3 already specifies it; the lib is behind the spec.
- **Gate**: must-fix.

---

## 3. P2 — must be worded; may be settled during implementation

### P2-1 · `모두 업데이트` / `업데이트 확인` feedback and freshness
- **Sources**: N-12 (2), A-5 (medium, gating), A-8 (low, freshness).
- **Claim**: No preview, no zero-updates state, no partial-failure report, no in-progress location; `업데이트 확인` has no destination when nothing is provider-backed; `업데이트 가능` on page load from a stale check is an unsourced claim.
- **Proposed wording → §8.3 header**:
  > Header `설치된 모드 N` (N counts every jar incl. `직접 추가`) + caption `업데이트 M개 · 마지막 확인 {N}분 전` once a check has run this session; `업데이트 가능` chips appear only after this session's check. `업데이트 확인` uses the P1-2 carrier (`업데이트 확인 중 3/12 · sodium.jar`); result `Toast neutral` `업데이트가 없습니다` or the header caption updates; disabled with helper `확인할 수 있는 모드가 없습니다` when no row is provider-backed. `모두 업데이트` opens the install dialog in list form (`{title} {old} → {new}` per row, all checked, total size) — same component as P1-7; partial failure reports `모드 {k}/{n}개를 업데이트했습니다` + `Callout danger` naming the failed rows. Update candidates use the same release-type preference as install.
- **Gate**: must-fix for copy/states; the preview list is the recommended shape but a written "no preview" decision is acceptable.

### P2-2 · View switcher semantics, state retention, post-install routing
- **Sources**: N-13 (2).
- **Proposed wording → §8.2**:
  > 설치됨 / 찾기 `SegmentedControl` is a `tab-list` (same role as elsewhere; here it switches views, accepted). Query text, provider and sort persist for the session across view switches and server changes; results and `설치됨` markers are recomputed when the server changes (results re-fetched only on the next search). Install success stays on 찾기: the row's `설치` becomes the `설치됨` chip and the toast fires; no auto-switch.
- **Gate**: must-fix.

### P2-3 · 설치됨 list container, unmanaged rows, filter
- **Sources**: N-14 (2), A-5 (medium).
- **Proposed wording → §8.3**:
  > Rows live in a `ListView` (max height per ux-fixes item 14; PageUp/PageDown/Ctrl+Home/End per §6). Sort: `업데이트 가능` first, then title. `직접 추가` rows: chevron opens the mod dialog with only the namespace control and `삭제`; `프로젝트 페이지 열기`, version list and update are hidden with body text `직접 추가한 파일입니다 — 출처를 알 수 없어 업데이트를 확인할 수 없습니다`.
- **Gate**: must-fix (container + unmanaged actions); a text filter is nice-to-have (P3-2).

### P2-4 · Unique accessible names for repeated controls
- **Sources**: N-15 (2).
- **Proposed wording → §8 Accessibility**:
  > `Button 설치` `accessible-label: {title} 설치`, `accessible-id: mods-install-{provider}-{project id}`; row chevron `{title} 자세히`, `mods-detail-{…}`; result rows are not `selectable` (the buttons are the tab stops); once a row shows `설치됨` the chevron remains its tab stop. Server pills `mods-server-{id}`.
- **Gate**: must-fix.

### P2-5 · Dialog initial focus, trap and return
- **Sources**: N-16 (2).
- **Proposed wording → §3 Dialog** (append, applies to all `DialogKind`s):
  > On open, focus goes to the first text field if any (`duplicate-server`, `delete-server`), else to the first action (`server-menu`) or the close button (`mod`, `install`); Tab cycles inside the card; on close focus returns to the invoking control (row chevron, server menu button, `설치` button). `install` Esc = cancel of the whole install.
- **Gate**: must-fix (five new dialogs land at once).

### P2-6 · DESIGN §8 vs plan GUI section diverge
- **Sources**: N-17 (2), A-11 (low).
- **Divergences**: header (`설치된 모드 N` vs `N개 · 업데이트 M개`) → P2-1 wording; trailing icon (`chevron-right` vs "(more)") → `chevron-right`; row-menu `업데이트` (plan only) → dropped, updates live in the version list; progress copy → P1-2; `버전 선택` link → dropped (P1-6); chip label (`업데이트 가능` vs `업데이트`) → `업데이트 가능`.
- **Proposed change → plan §GUI**: replace the section body with `See DESIGN-v2.md §8 (single source). Nav: 모드 between 서버 and 가져오기, Ctrl+1..6.` and keep only backend-relevant bullets.
- **Gate**: must-fix (cheap; without it implementation picks silently).

### P2-7 · Manual-download callout: folder, continuation, reconciliation, dismissal
- **Sources**: N-18 (2).
- **Proposed wording → §8.5**:
  > `Callout warning` in the 설치됨 card, title `CurseForge에서 배포를 허용하지 않는 파일입니다`, body `{file} — 아래 링크에서 직접 내려받아 {absolute path of <mods>/<namespace>/}에 넣으세요`, URL in a read-only `TextField` (`select-all`), actions `링크 열기` and `폴더 열기`. The rest of the plan is installed (the disallowed file is excluded from the count: `모드 2개를 설치했습니다 · 1개는 직접 내려받아야 합니다`). On the next index reconcile a hand-dropped jar whose hash matches a plan file gains that provenance (`versions_by_hashes`/fingerprints) and stops being `직접 추가`. The callout clears when the file appears or on its close button.
- **Gate**: must-fix.

### P2-8 · Provider/sort mapping and trigger rules
- **Sources**: N-19 (2).
- **Proposed wording → §8.4**:
  > Sort mapping (verify enum values against facts §B): 관련도 → Modrinth `relevance` / CurseForge `Popularity`; 다운로드 → `downloads` / `TotalDownloads`; 최신 → `newest` / `ReleasedDate`. Changing provider or sort re-runs the current query immediately (offset reset) when the field is non-empty; typing waits for Enter. CurseForge segment disabled with helper `CurseForge API 키가 없습니다 (NEBULA_CURSEFORGE_API_KEY)` when no key resolves.
- **Gate**: must-fix.

### P2-9 · 서버 row menu: trigger, `폴더 열기` stop, `복제` rules
- **Sources**: N-20 (2).
- **Proposed wording → §8 서버 page**:
  > Trailing controls per row: `IconButton folder-open` (`{id} 폴더 열기`, stays the direct action per ux-fixes item 15) then `IconButton ellipsis-vertical` (`{id} 메뉴`, `accessible-id: servers-menu-{id}`; asset `ellipsis-vertical.svg` to be added — not shipped today) opening `server-menu` with `모드 관리`, `복제`, `삭제`. `복제`: `DialogKind.duplicate-server`, `TextField` 새 서버 ID with the same validation as 새 서버's 서버 ID, primary `복제`; busy uses the §5 pill `서버 복제 중 · 0:04`; on success focus to the new row's menu button and `Toast success` `{new id}를 만들었습니다`.
- **Gate**: must-fix.

### P2-10 · Copy and format rules
- **Sources**: N-21 (2), A-7 (low).
- **Proposed wording → §8 (new "Formats")**:
  > Release types `정식` / `베타` / `알파`. Counts in Korean units: `다운로드 120만`, `3.2억`, below 1만 plain digits. Sizes `12.3 MB` (unit symbols are an accepted exception to §5). Dates `2026-09-11`; relative `수정 3일 전` when sort is 최신. Game-version lists truncate to three + `외 N개`. Shortcut renumbering: 설정 moves Ctrl+5 → Ctrl+6 (accepted; update §3 NavRail and §6).
- **Gate**: must-fix (English `1.2M` breaks §5); downloads as caption instead of chip is P3-1.

### P2-11 · Loading, skeleton and pagination accessibility
- **Sources**: N-22 (2), A-9 (low), S-9 (minor).
- **Proposed wording → §8.4**:
  > Search field `accessible-label: 모드 검색`, `accessible-id: mods-search`; use the existing trailing `action-icon: search.svg` + `accepted` (no leading-icon slot in `TextField`; if a leading icon is insisted on, add `in property <image> leading-icon`). Loading = `ProgressBar { a11y-label: "{provider}에서 검색 중" }` under the field; skeleton = three static `Rectangle { height: 56px; background: surface-2 }` (`accessible-role: none`, never animated — the software renderer draws only the first frame). `더 보기` is `Button.loading` while fetching, keeps existing rows, is hidden unless the last response was a full page, and keeps focus after append. Pre-search: `EmptyState` `검색어를 입력하세요` body `{server} · {mc} · {loader}에 맞는 모드만 표시됩니다`.
- **Gate**: must-fix.

### P2-12 · Nav rail sixth item
- **Sources**: S-8 (minor).
- **Proposed wording → §3/§6/§7**: `AppPage { build, servers, mods, import, publish, settings }`; NavRail "6 items"; `package.svg` (shipped); keys 3→mods, 4→import, 5→publish, 6→settings; §6 "Ctrl/Cmd+1..6"; rail 400 px < 800 px floor. Update `gui-v2` state JSON that documents shortcuts.
- **Gate**: must-fix (spec text still says 1..5 in §3/§6).

### P2-13 · Result-row description line
- **Sources**: S-6 (minor).
- **Proposed wording → §3 ListRow**: `in property <string> description;` third line 13 px text-2 `overflow: elide` (the property is `overflow`, not `text-overflow`); row grows past 56 px.
- **Gate**: must-fix (§8.4 needs it; one property).

---

## 4. P3 — nice-to-have

### P3-1 · Downloads as caption, not chip
- **Sources**: A-7. `작성자 · 다운로드 120만` on the author line; a count is data, not status. Recommend adopting together with P2-10.

### P3-2 · Text filter on 설치됨
- **Sources**: N-14. `TextField` 필터 above the list when N > 20. Add when a real pack exceeds one screen and the sort in P2-3 is not enough.

### P3-3 · Sticky toast live region property
- **Sources**: S-7. `in property <bool> live` on `Toast` → `accessible-live-region: polite`. Already folded into P1-2; listed here because it can ship one iteration later without changing copy.

### P3-4 · `TextField.leading-icon`
- **Sources**: S-6. Zero-change alternative (trailing `action-icon`) is in P2-11; only add if the lead wants the leading glyph.

### P3-5 · Elapsed time in the progress toast
- **Sources**: N-3. The top-bar pill carries `0:12`; duplicating it in the toast is optional.

---

## 5. Slint 1.17.1 capabilities — confirmed / missing

Verified by the S review (crate sources, `target/debug/.fingerprint`, headless software-renderer probes) and re-checked against the worktree where noted.

| Capability | Status | Note |
|---|---|---|
| **WebP decoding at runtime** | **Missing** | Target `image` built with `png`+`jpeg` only; `load_from_path` picks decoder by extension; errors swallowed to fallback. Fix: direct `image` dep with `webp` (lockfile already has `image-webp 0.2.4`) + worker transcode to PNG. Not `image-default-formats`. |
| PNG / JPEG decoding at runtime | Confirmed | Enabled in the target build. |
| Runtime images from paths (`Image::load_from_path` on UI thread, paths cross threads) | Confirmed | Existing pattern `gui/mod.rs:445-455`; cache is path-keyed 5 MB LRU → content-hash file names. |
| Empty `image` in a struct via `--load-data` | Missing | Use parallel `[image]` model (`SheetData.server-icons` precedent). |
| `Text { overflow: elide }` | Confirmed | Renders ellipsis under the software renderer; `text-overflow` does not exist. |
| `Timer` element (`interval`, `running`, `triggered`, start/stop/restart) | Confirmed | Cannot be a direct child of `if`/`for`; runs by default (`running: true`). |
| Horizontal `Flickable` nested in the page `ScrollView` | Confirmed | Vertical wheel forwards out; Shift+wheel scrolls it; clips viewport; no automatic bring-into-view (binding `viewport-x` kills drag). Not needed if D3 = wrapping row. |
| `ListView` inside `Dialog` | Confirmed | Needs explicit `height`; delegates need explicit `height`; scrollbar overlays right edge; virtualised; off-screen rows absent from a11y tree. |
| `AccessibleRole.radio-button` / `radio-group` | Confirmed | Map to AccessKit RadioButton/RadioGroup. |
| `accessible-live-region` (`polite`) | Confirmed | Present and mapped in the winit backend; not yet used anywhere in the app. |
| Determinate `ProgressBar` | Missing by design | `progress.slint` is indeterminate-only and says why; the `2/5` counter is text. |
| `Button danger` | **Missing** | §3 specifies it; `button.slint:5` has `primary, secondary, ghost`. Add. |
| `Chip neutral outline` | Missing | `Tone` has no outline; drop the variant (P1-11). |
| Selectable pill / radio strip | Missing | `Chip` is a `Rectangle`; new `pill-strip.slint` cloned from `SegmentedControl`. |
| `TextField` leading icon | Missing | Only trailing `action-icon`; use it (P2-11) or add a property. |
| `ListRow` third text line | Missing | One `description` property (P2-13). |
| Sticky / in-place-updated toast | Missing (Rust) | `TOAST_LIFETIME`/`MAX_TOASTS` plumbing in `gui/mod.rs:582-596` needs a `progress_toast` path. |
| Rounded 36 px avatars from runtime images | Confirmed | `clip: true` + `border-radius` + `image-fit: cover` as in `list-row.slint`. |
| Icons: `package`, `search`, `chevron-right`, `folder-open`, `trash-2` | Confirmed shipped | `crates/nebula/assets/icons/`. |
| Icon: `ellipsis-vertical` (row menu trigger) | Missing | Add one Lucide SVG (P2-9). |
| Sixth nav item / Ctrl+6 | Confirmed feasible | Pure `.slint` edits; no exhaustive Rust match on `AppPage`. |
| Animated skeletons | Avoid | Software renderer draws first frame only; static blocks or bind to `Theme.progress-phase`. |

---

## 6. Document edits implied (checklist)

- [ ] DESIGN-v2 §8: apply P0-1, P1-1…P1-12, P1-14, P2-1…P2-11 wording; add sub-sections "Busy and dialogs", "Failures", "Install dialog", "Formats".
- [ ] DESIGN-v2 §3: NavRail 6 items / Ctrl+1..6; Dialog focus rules (P2-5); ListRow `description`; confirm `Button danger` is to be built.
- [ ] DESIGN-v2 §6: "Ctrl/Cmd+1..6".
- [ ] DESIGN-v2 §7: `AppPage`, `DialogKind` enumeration, `ModsData` globals, icon model pattern (P1-13), `pill-strip.slint` in the file list, `mods.slint` screen, render states to add (`mods-installed`, `mods-installed-empty`, `mods-search`, `mods-search-loading`, `mods-install-dialog`, `mods-delete-armed`, `server-delete-dialog`).
- [ ] Plan `2026-09-11-mod-catalog.md`: `icons.rs` row (P0-2); `index.rs` records `dependencies` (P1-9); GUI section → pointer (P2-6); CLI unchanged.
- [ ] `crates/nebula/Cargo.toml`: `image` dep line (P0-2) — implementation, not spec.

## 7. Passes (no action)

Chips always carry text (colour never sole carrier); server deletion friction proportional; required deps pre-checked, optional not auto-installed; newest-release-with-fallback deliberately diverges from Prism's newest-file-wins; manual-download copy follows cause-then-fix and the ux-fixes 12 URL style; search empty/error states honest with retry; one selection vocabulary (accent-soft); no new tokens besides the `neutral outline` slip; skeletons reduced-motion safe; `accessible-id` contract carries over; reliability rules (atomic index, rollback, one job per server) leave no half-state to explain; no invented numbers, filler data, emoji, arrows, bento, glass, or decorative motion; Lucide is a declared decision; existing `Dialog`, `SegmentedControl`, `Chip`, `Callout`, `EmptyState`, `IconButton`, `Button ghost`, `ProgressBar` reuse needs no lib change.
