# GUI crate review fixes — decisions for Task 6e

Source: quality review of `crates/nebula` (25 findings) plus the GUI interaction QA report (`docs/verification/gui-qa/2026-09-10-gui-qa.md`, appended when available). All findings are accepted unless listed under "skip".

## Fix list (numbers follow the review)

1. `commit_settings`: replace `ui.settings` only in the success branch; pass a clone to the worker.
2. Delete `cli::config_dir`; re-export and use `gui::settings::config_dir` (honours `NEBULA_CONFIG_DIR`) from the CLI.
3. Add `SettingsData.error-text` rendered as a danger `Callout`; save failures go there plus a danger toast `설정 저장 실패`.
4. Clear `ImportData.result-line` at the start of an import.
5. `SegmentedControl`: remove `root.selected = index`, make `selected` an `in` property; callers already update the model in `changed`.
6. Preserve the selected import path across refreshes (re-find by `PathBuf`, else -1).
7. Startup: show the window with an empty snapshot and run the first scan through `spawn_refresh` on the first tick.
8. Add `Screen::Publish` + `PublishData.busy`; publish button `loading: PublishData.busy`.
9. Refresh on entering the 서버 page as well as 가져오기 (only when the page actually changes — see 16).
10. Google 계정 연결: run outside the global busy lock (own `gdrive-busy` flag already exists); wording `승인을 마치지 않으면 5분 뒤 자동으로 취소됩니다`.
11. `Verdict.name` keyed by the field value; single key derivation.
12. Dialog: `forward-focus` to the first field on open; disable the page tree while a dialog is open.
13. Lift `expanded` (findings 더 보기) into `SheetData`; `clear_result()` resets it.
14. `logsink::install(level)` takes the CLI level; `nebula gui -v` works.
15. `drain_logs` drains the whole channel per tick, then trims once; adjust the unit test.
16. `go(page)` no-ops when the page is unchanged.
17. Move `refresh_strict` to a quiet worker thread (not `spawn_job`); post the result with `invoke_from_event_loop`; keep `validate_settings_form` but avoid repeated filesystem probes (probe once per debounce, on the worker).
18. Remove the 13 dead callback parameters (`generate`, `validate-existing`, `save-settings`, `import-modpack`) and the `selected-name` property; update README §2.
19. Delete dead properties/functions: `SheetData.summary-error`, `TextField.focus-input`, `Button.exclusive/selected/danger` (keep `danger` only if a caller appears — none today, so remove), `Card.gap/pad` overrides, `Type.body-leading/regular`, `KeyValue.mono`, `ListRow.subtitle-mono`, `ProgressBar.value` (keep the determinate branch only if used — not used, remove), `IconButton.tint`; update state JSONs and README.
20. Deduplicate strings: helper text constants; the strict-blocked reason comes only from `PublishData.blocked-reason`.
21. `AppData.go(page)` public function; all five call sites use it.
22. Unify `연결되지 않음`.
23. Toast `서버를 만들었습니다: {dir_name}`.
24. Add `Theme.sp-half: 6px`; derive the 21 px offset or align the lookup buttons in the field row; sweep off-grid literals.
25. Log rows `accessible-role: list-item` with a label; loading `ProgressBar` inside `Button` gets `a11y-label: root.text`; `NavItem` FocusScope `enabled` + `accessible-enabled`.

## Skip

- None from the review. QA-report items are appended below by the coordinator.

## Verification

`slint-viewer --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, regenerate every `docs/plans/gui-v2/*.png`, 8 s live launch. Update `docs/plans/gui-v2/README.md` for every contract change.

## Appended: GUI interaction QA defects (docs/verification/gui-qa/2026-09-10-gui-qa.md)

- D1 (high): a failed server scan must not present an empty list as "no servers". `server_rows` returns `Result`; on error keep the previous rows, show a danger `Callout` on the 서버 page (`서버 목록을 읽지 못했습니다: <cause>` + fix hint naming the offending `servermeta.json`) and a toast; the readiness row for 루트 stays `확인됨` only when the scan succeeded. `read_server_meta` strips a UTF-8 BOM before parsing.
- D2 (medium, ceiling): Slint's ScrollView exposes only visible children to UIA and Tab does not scroll offscreen controls into view. Document in DESIGN-v2 §6 as an accepted ceiling; mitigate by giving the log view and long lists a focusable header so keyboard users can scroll with the mouse wheel alternative (`ScrollView` handles keys when focused): add a `FocusScope` to the content `ScrollView` that maps PageUp/PageDown/Home/End to `viewport-y`.
- D3 (medium): every zip open error carries the file path (`scan/fabricmod.rs`, `scan/forgemod.rs`, `import.rs`): `"{path}: {error}"`.
- D4 (medium): Korean message for the missing URL scheme in `config.rs` (`기본 URL에 프로토콜이 없습니다. https:// 로 시작하는 주소를 입력하세요`), keep the English in the log if desired.
- D5 (low): refused save shows a toast `저장하지 못했습니다: 입력값을 확인하세요` and focuses the first invalid field.
- D6 (low): `can-publish` also requires the chosen target to be configured (WebDAV URL/user set; Drive client id + folder id set); the blocked reason names the missing setting.
- D7 (low): unselectable import rows set `accessible-enabled: false`.
- D8 (low, ceiling): Slint 1.17 does not map `accessible-description` to UIA HelpText; keep it, and append the error text to the field's `accessible-label` when an error is present so it is announced.
- D9 (low): give every interactive control an `accessible-id` (kebab-case, e.g. `nav-build`, `build-generate`, `settings-save`) so automation has stable AutomationIds; icon-only `Image`s inside labelled controls get `accessible-role: none`.
- D10 (low): busy pill text `배포본 생성 중 · 1:23` (job label + `중`, mono `m:ss`), per DESIGN-v2 §5.
- D11 (low): element order = visual order; move `설정 열기` (card header action) so that Tab reaches it first on the 빌드 page.
