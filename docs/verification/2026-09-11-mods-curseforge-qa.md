# Nebula Rust 포트 — 모드 카탈로그 **CurseForge** QA 검증 보고서

- **일자**: 2026-09-11 ~ 2026-09-12
- **대상 커밋 (HEAD)**: `e14e42f` (워크트리 `rust-port`, master 병합분과 동일)
- **범위**: 모드 카탈로그의 **CurseForge 경로 전체** — CLI(`nebula mods … search/install/update/list`)와
  GUI(설정 CurseForge 카드, 찾기 제공자 세그먼트, 설치, 차단 파일 콜아웃, 업데이트 확인).
  근거 문서는 plan v1.1 **D2 / D3 / D6 / D7 / R3 / R4 / R5 / R6 / R7**, DESIGN-v2 **§8.3 / §8.4**,
  facts §B·§E.2 입니다.
- **검증 방식**: 블랙박스 + 필요한 곳에서만 소스 대조. 사용자의 실제 CurseForge API 키로
  **실서버(live)** 호출을 수행했습니다. 작업 트리는 읽기 전용으로만 접근했고, 쓴 파일은
  이 보고서와 `docs/verification/gui-qa/cf-*` 뿐입니다.
- **스냅샷/스크래치 경로**: `<scratchpad>/cf-qa` (`src/` = `git archive HEAD`, `target/` 릴리스 빌드,
  `root/` 테스트 ROOT, `config/` GUI 설정, `logs/` 증거 로그)
- **증거 로그**: `<scratchpad>/cf-qa/logs/<step>.log` (stdout+stderr, 종료 코드, 소요 시간).
  GUI 로그·스크린샷은 `docs/verification/gui-qa/cf-*.log` / `cf-*.png`.
- **키 취급**: 키는 프로세스 환경 변수(또는 GUI 설정 필드)로만 전달했고 로그·스크린샷·이 보고서
  어디에도 남기지 않았습니다. 종료 전 `docs/verification/**` 와 `<scratchpad>/cf-qa/**` 전체를
  키 앞 8자로 검색해 **적중 0건**을 확인했습니다(아래 "비밀 취급" 절).

## 실행 환경

| 항목 | 값 |
| --- | --- |
| OS | Windows 10 Pro 19045 |
| Java | Eclipse Temurin 21.0.10+7-LTS |
| `ROOT` | `<scratchpad>/cf-qa/root` |
| `BASE_URL` | `https://cdn.example.invalid/` |
| `NEBULA_CONFIG_DIR` | `<scratchpad>/cf-qa/config` (사용자가 쓰고 있는 GUI(pid 19856)와 완전 분리) |
| `NEBULA_CURSEFORGE_API_KEY` | 단계별로 설정/해제 (값은 기록하지 않음) |
| `CARGO_TARGET_DIR` | `<scratchpad>/cf-qa/target` |
| 서버 | `Fg-1.20.1` (Forge 47.4.23), `Neo-1.21.1` (NeoForge 21.1.250) |

빌드: `cargo build --release -p nebula` → **성공**. 산출물 `cf-qa/target/release/nebula.exe`.

## 종합 결과

**CLI 34단계 + GUI 6시나리오 실행 완료.** 스펙 대비 **결함 8건** — **high 1, medium 4, low 3**.

가장 무거운 것은 **CF-01**: 같은 모드의 인덱스 항목을 다른 제공자가 갖고 있으면
CurseForge 설치가 교체 대신 **두 번째 jar를 추가로 설치**하고, 그 상태가 경고 없이
`distribution.json`까지 그대로 실려 나갑니다. 이 상태는 §8.4가 정상 흐름으로 설명하는
"차단 파일 → 수동 배치 → 업데이트 확인(입양)" 경로만 밟아도 도달합니다.

또한 검증 중 **CurseForge `/v1/mods/search` 가 약 20분간 전역적으로 403을 반환하는 구간**을
만났습니다. 이는 Nebula의 결함이 아니라 상류(upstream) 상태이지만, 그 덕분에
**CF-04**(키 거부를 "네트워크 오류"로 표시)를 실제로 관측할 수 있었습니다. 해당 구간이 끝난 뒤
검색 관련 단계는 전부 다시 수행했습니다.

## CLI 단계별 결과

| # | 명령 | 기대 (근거) | 관찰 | 종료 | 시간 | 증거 |
| --- | --- | --- | --- | --- | --- | --- |
| 0a | `init root` | 표준 레이아웃 | `meta/ modpacks/ schemas/ servers/` | 0 | 54ms | `s00-init.log` |
| 0b | `generate server Neo 1.21.1 --neoforge latest` | NeoForge 서버 | `neoforgemods/{required,optionalon,optionaloff}` | 0 | 976ms | `s00-gen-neoforge.log` |
| 0c | `generate server Fg 1.20.1 --forge latest` | Forge 서버 | `forgemods/…` | 0 | 568ms | `s00-gen-forge.log` |
| 0d | `generate distro` (설치 전 기준선) | R5 기준선 | sha256 `2CE5FB24…` | 0 | 59.2s | `s00-distro-baseline.log` |
| 1a | `mods Neo search jei --provider curseforge --limit 5` (403 구간) | D2 검색 | `error: 네트워크 오류: https://api.curseforge.com/v1/mods/search?…: 403 Forbidden: API Key missing or invalid` — **키 거부가 "네트워크 오류"로 표시됨 (CF-04)**. URL 질의는 `gameId=432&classId=6&gameVersion=1.21.1&…&modLoaderType=6`으로 D2/facts §B.2와 일치 | **1** | 314ms | `s01a-search-cf-jei.log` |
| 1b | `… --sort newest --limit 3 --offset 3` (403 구간) | 정렬·페이징 파라미터 | URL에 `sortField=3`·`pageSize=3`·`index=3` 확인 (응답은 403) | **1** | 303ms | `s01b-search-cf-sort-offset.log` |
| 1c | 불가능 질의 (403 구간) | — | 같은 403 | **1** | 270ms | `s01c-search-cf-impossible.log` |
| 1d | `mods Neo search jei --provider curseforge --limit 5` (403 해제 후) | 결과·캡션·로더 필터 | `curseforge · 검색 결과 149개 중 5개 (Minecraft 1.21.1 · NeoForge)` + 5행(프로젝트 id·제목·slug·다운로드·작성자·요약 1줄) | 0 | 1209ms | `s01a2-search-cf-jei-retry.log` |
| 1e | `… --sort downloads --limit 3` | 다운로드 내림차순 | 621,604,849 → 240,592,119 → 108,451,616 | 0 | 329ms | `s01d-search-downloads.log` |
| 1f | `… --sort newest --limit 3` | 최신순 | `jei-unofficial`(881) → `cobbledex-rei-emi-jei` → `jei-fork` — 다운로드순과 완전히 다른 집합 | 0 | 813ms | `s01e-search-newest.log` |
| 1g | `… --sort downloads --limit 3 --offset 3` | 4~6위가 이어짐 | 57,195,749 → 54,610,327 → 52,497,198 (108M 다음, 내림차순 연속, 중복 없음) | 0 | 1124ms | `s01f-search-offset.log` |
| 1h | `… --limit 20` | `pageSize` 존중 | 20행 | 0 | 156ms | `s01h-search-limit20.log` |
| 1i | `search zzqqxxnonexistentmodzz --provider curseforge` | 빈 결과 경로 | `검색 결과 0개 중 0개`, 패닉 없음 | 0 | 628ms | `s01g-search-impossible.log` |
| 2a | 키 없이 `search … --provider curseforge` | D2 거절 | `error: CurseForge API 키가 없습니다 — 설정 화면의 CurseForge API 키에 값을 입력하세요 (console.curseforge.com)` — 환경 변수명을 말하지 않음 **(CF-06)** | **1** | 32ms | `s02-search-cf-nokey.log` |
| 2b | 키 없이 `install 238222 --provider curseforge` | 같은 거절 | 동일 문장 | **1** | 18ms | `s02b-install-cf-nokey.log` |
| 3a | `mods Neo install 238222 --provider curseforge` | D3 정식 우선, R1 해시 검증, D2 무-자유텍스트 | 계획 `Just Enough Items (JEI) 8792638 · 정식 · jei-1.21.1-neoforge-19.51.0.418.jar · 2026-09-02`. **최신 파일은 베타(8857687)인데 정식(8792638)을 골랐다 — D3 준수.** `neoforgemods/required`에 1개. 인덱스: `provider=curseforge, project_id=238222, version_id/file_id=8792638, release=release, hashes{sha1,md5}, namespace=required` — `title`/`project_slug`/`icon_url` **없음**. 다운로드가 성공했으므로 CDN 리다이렉트를 넘어 `x-api-key`가 살아남음(R2) | 0 | 3785ms | `s03-install-jei-neo.log` |
| 3b | `mods Neo install 1405107 --provider curseforge` (AxoChaos) | D6 의존성 | 계획에 `필수 의존성 Curios API` / `선택 의존성 Better Combat`. `설치 2 · 건너뜀 0`; `curios-neoforge-9.5.1+1.21.1.jar` 동반 설치, 선택 의존성은 미설치. 두 항목 모두 CF 자유텍스트 없음 | 0 | 5378ms | `s03b-install-cf-with-deps.log`, `s03b-index.log` |
| 4a | `mods Fg install 876772 --provider curseforge` (Immersive Ores, 전 파일 `downloadUrl:null`) `RUST_LOG=debug` | R3 미러 | `[INFO] Immersive Ores is blocked on CurseForge; using Modrinth's copy` → `설치 1 · 직접 다운로드 0`. 인덱스는 `provider=modrinth` + sha1/sha512/md5, **title/slug 없음**(F2의 `text_provider` 규칙 준수) | 0 | 5334ms | `s04a-install-blocked-mirror.log` |
| 4b | `mods Fg install 1604184 --version 8402559 --provider curseforge` (차단 + Modrinth 미러 없음) | R3/R4 수동 다운로드 | `설치 0 · 건너뜀 0 · 직접 다운로드 1`, stderr에 `직접 내려받아야 하는 파일 1개 / Flashier Flashlights (Flashier Flashlights 1.0.jar) - https://www.curseforge.com/minecraft/mc-mods/flashier-flashlights/files/8402559 / 받은 파일을 여기에 넣으세요: …\forgemods\required / 파일 이름은 .jar로 …`. **`edge.forgecdn.net` 재구성 없음(R4)**, Modrinth sha1 조회를 먼저 시도한 흔적(`api.modrinth.com` 연결) 확인. 종료 0 = 실패가 아님 | 0 | 2254ms | `s04b-install-blocked-manual.log` |
| 4c | 틀린 키로 `mods Neo search jei --provider curseforge` | 키 거부를 키 문제로 설명해야 함 | `error: 네트워크 오류: …/v1/mods/search?…: 403 Forbidden: API Key missing or invalid` **(CF-04)** | **1** | 1235ms | `s04c-bad-key-search.log` |
| 4d | 틀린 키로 `mods Neo install 238222 --provider curseforge` | 같음 | `error: 네트워크 오류: https://api.curseforge.com/v1/mods/238222: 403` — 숫자만 남아 더 불친절 **(CF-04)** | **1** | 278ms | `s04d-bad-key-install.log` |
| 5a | `mods Fg install 238222 --provider curseforge` | 로더별 파일 선택 | `jei-1.20.1-forge-15.56.0.205.jar` (Forge 파일, 정식). 상위 베타들을 건너뜀 | 0 | 3203ms | `s05a-install-jei-forge.log` |
| 5b | `mods Fg install 306612 --provider curseforge` (Fabric API) | "이 mc/로더용 버전 없음" 한국어 거절 (§8.4) | `error: 프로젝트 또는 파일을 찾을 수 없습니다` — 프로젝트는 존재하고 Forge 빌드만 없는데 "프로젝트 없음"과 구분 불가 **(CF-05)** | **1** | 479ms | `s05b-install-fabric-only-on-forge.log` |
| 6a | `mods Neo install 238222 --version 8759217 --provider curseforge` | 3단계 교체 | `19.50.0.414`로 교체, 디렉터리에 jei jar **1개** | 0 | 3118ms | `s06a-install-older-jei.log` |
| 6b | `mods Neo update` | D3: 베타보다 정식 | `업데이트 1개 / jei-…-19.50.0.414.jar : 19.50.0.414 → 8792638 (정식)` — **더 최신 베타(8857687)를 제안하지 않음**. 다만 오른쪽이 파일 id **(CF-02)** | 0 | 1068ms | `s06b-update-dryrun.log` |
| 6c | `mods Neo update --apply` (+ 5ms 디렉터리 폴링) | 어느 순간에도 jei jar 2개 금지 | `업데이트 1 · 실패 0`. **196회 폴링, 관측된 jei jar 최대 개수 = 1** | 0 | 3343ms | `s06c-update-apply.log` |
| 7a | `mods Neo install 1668140 --provider curseforge` (CurseForge 전용 모드) | R6 파일명 검증 | `[NeoForge - 1.21.1] Block Jackpot - Casino Machines - 0.4.0.jar` — 대괄호·공백이 있는 이름을 **그대로** 기록·저장(R6 통과) | 0 | 2061ms | `s07a-install-cfonly.log` |
| 7b | `modindex.json` 삭제 후 `mods Neo update` | 출처 입양 (F7) | `직접 추가 2개를 관리 목록에 연결했습니다: [NeoForge …] Block Jackpot …, jei-…-19.51.0.418.jar` + `모두 최신입니다`. 로그: `Adopted [NeoForge …] … as curseforge 1668140`(지문 경로), `Adopted jei-… as modrinth u6dRKJwZ`(sha1 우선 경로). CF 항목에 자유텍스트 없음 | 0 | 1271ms | `s07b-update-adoption.log` |
| 8a | 검색 2회 연속 실행(별도 프로세스) | R7 ≥250ms | 두 프로세스의 CurseForge 요청 간격 **119ms** — 스로틀은 프로세스 전역이라 CLI 반복 호출에는 적용되지 않음 **(CF-08)** | 0 | 676/103ms | `s08a-…`, `s08b-…` |
| 8b | `mods Fg update` (한 프로세스에서 CF 4요청) | R7 in-process | 3097ms (하한 3×250=750ms 충족). 다만 CurseForge 자체 왕복이 150~550ms라 스로틀을 외부에서 분리 측정할 수 없음 — 아래 "미수행" 참조 | 0 | 3097ms | `s08c-update-inprocess-multicall.log` |
| 9a | `mods Neo install 999999999 --provider curseforge` | 완결된 한국어 문장, `HTTP :` 금지 | `error: 프로젝트 또는 파일을 찾을 수 없습니다` — 완결 문장, 빈 HTTP 조각 없음 | **1** | 344ms | `s09a-install-unknown-id.log` |
| 9b | `mods Neo install jei --provider curseforge` (slug) | `--help`는 "Project id or slug" | 같은 not-found. CurseForge `project()`는 숫자 id만 받으므로 slug는 원천적으로 동작 불가 **(CF-07)** | **1** | 269ms | `s09b-install-nonnumeric-slug.log` |
| 10a | `generate distro` (카탈로그 파일 있음) | R5 | sha256 `EF8D4FA1…` | 0 | 3397ms | `s10a-distro-with-catalog-files.log` |
| 10b | `modindex.json`·`.nebula-tmp` 이동 후 `generate distro` | R5 바이트 동일 | sha256 `EF8D4FA1…`, 줄 diff 0 → **바이트 동일** | 0 | 5715ms | `s10b-…`, `s10-r5-summary.log` |
| 10c | `validate distribution --strict` | 0 errors | `100 artifacts, 100 with SHA-256, 0 errors, 1 warnings` (경고는 기존 NeoForge 런처 패치 안내) | 0 | 29~75ms | `s10c-validate-strict.log`, `s11f-validate-with-duplicate.log` |
| 11 | (CF-01 재현) `mods Neo install 238222 --version 8759217 --provider curseforge` — 단 인덱스의 jei 항목이 입양으로 `provider=modrinth`가 된 뒤 | 같은 모드 1개 유지 | **jei jar 2개 공존**: `jei-…-19.50.0.414.jar`(curseforge/238222) + `jei-…-19.51.0.418.jar`(modrinth/u6dRKJwZ). `mods list`에 `Just Enough Items` 2행, 경고 없음 | 0 | 3593/100ms | `s15-prep-downgrade-jei.log`, `s11d-duplicate-jei-list.log` |
| 11b | 그 상태로 `generate distro` + `validate --strict` | 검증이 잡아내야 함 | `generated.neoforgemod:jei:19.50.0.414@jar` 와 `…:19.51.0.418@jar` **둘 다 배포본에 포함**, `0 errors` | 0 | 6383/75ms | `s11e-…`, `s11f-…` |

## GUI 단계별 결과 (UIA)

드라이버는 기존 `docs/verification/gui-qa/drive.ps1` + `mods-drive-mods.ps1` 를 그대로 재사용했고,
실행 계약(`cf-gui-env.ps1`)만 별도로 두어 사용자의 실행 중 GUI(pid 19856)와 ROOT·설정·프로세스를
완전히 분리했습니다. 창 연결은 항상 `Connect-App -ProcessId`로 했습니다.

| # | 시나리오 | 기대 (근거) | 관찰 | 결과 | 증거 |
| --- | --- | --- | --- | --- | --- |
| 11a | 키가 아무 데도 없을 때 찾기 | §8.4: CurseForge 세그먼트 비활성 + 도움말 | `제공자 CurseForge` `IsEnabled=False`, `설정에서 CurseForge API 키를 입력하세요` 표시. 정렬 라벨은 Modrinth용 `관련도/다운로드/최신` | PASS | `cf-s11-settings-key.log`, `cf-11-segment-disabled.png` |
| 11b | 설정 → CurseForge 카드 | §4: password 필드 + 헬퍼 | `settings-curseforge-key` = `ControlType.Edit`, **`IsPassword=True`** (마스킹 확인은 스크린샷이 아니라 UIA 속성으로 수행). 헬퍼 문구 `console.curseforge.com에서 발급받은 본인 키를 입력하세요. 환경 변수 NEBULA_CURSEFORGE_API_KEY가 있으면 그 값이 우선합니다.` | PASS | `cf-11-settings-card-masked.png` |
| 11c | 키 입력 후 `설정 저장` | 저장되고 세그먼트 활성 | `settings.json`에 60자 저장(값은 확인만, 기록 안 함). 찾기의 CurseForge 세그먼트 `IsEnabled=True`, 도움말 문구 사라짐, 정렬 라벨이 **`인기/다운로드/최신`으로 전환** | PASS | `cf-11-segment-enabled-by-field.png` |
| 11d | 필드를 비우고 저장 | 다시 비활성 | `stored key: empty`, 세그먼트 `IsEnabled=False` | PASS | `cf-11-segment-disabled-after-clear.png` |
| 11e | 설정 필드는 빈 채로, 환경 변수만 주고 실행 | D2: 환경 변수가 우선 | 세그먼트 `IsEnabled=True` | PASS | `cf-s12-search-updates.log`, `cf-12-segment-enabled-by-env.png` |
| 12a | 찾기 · CurseForge 검색 `jei` (Neo-1.21.1) | §8.4 캡션·id·아이콘 | 20행(가시 6행은 가상화 때문), id는 전부 `mods-install-curseforge-<project id>`. 캡션 `mezz · 다운로드 6.2억 · 수정 1시간 전` 형식(사람이 읽는 단위). **최신 정렬에서는 날짜가 앞으로** (`수정 7시간 전 · StardustMINUS · 다운로드 881`) | PASS | `cf-s14-search-install.log`, `cf-12-search-curseforge.png`, `cf-12-sort-newest.png` |
| 12b | CurseForge 아이콘의 디스크 캐시 | D2/D5: CF 아이콘은 메모리 전용 | CurseForge 검색 전후 `<root>/cache/icons` 파일 수 **23 → 23 (증가 0)** 인데 **같은 화면의 6개 행 모두 썸네일이 렌더링**됨 — `cf-12-search-curseforge.png`에서 JEI/JER/JEP/JEBr/EMI/More Overlays 아바타 확인(즉 "아이콘 가져오기가 조용히 실패한 것"이 아님). 대조군으로 같은 질의를 Modrinth로 돌리면 **4 → 23 (19개 생성)** | PASS | `cf-s13-search-error.log`, `cf-s14-search-install.log`, `cf-12-search-curseforge.png` |
| 12c | `더 보기` | 첫 페이지가 가득 찰 때만 노출, 기존 행 유지 | 목록을 스크롤해 `검색 결과 더 보기` 확인, 누르면 새 행(`mods-install-curseforge-1495382`)이 **덧붙음** | PASS | `cf-s16-more-and-blocked.log`, `cf-12-search-more.png` |
| 12d | 정렬 전환 | 제공자 변경 시 라벨 교체, offset 초기화 후 재검색 | `관련도↔인기` 교체 확인, 정렬 변경 시 행 수가 한 페이지로 되돌아감 | PASS | `cf-s14-search-install.log` |
| 13 | GUI에서 CurseForge 모드 설치 (`Better Loot Zibura`) | §8.4 install-confirm | `Neo-1.21.1에 설치` / `Better Loot Zibura 8858195 · 정식 · 646.3 KB · 2026-09-11` / `다른 버전…` / 네임스페이스 세그먼트 / 필수 의존성 `Polymorph 8849478` 스위치 + `필수 의존성을 빼면 게임이 시작되지 않을 수 있습니다` + `168.6 KB` / 푸터 `2개 · 815 KB` / 기본 `2개 설치`. 적용 후 행이 `설치됨`으로 바뀌고 설치됨 목록 부제가 `2.0.1 · betterloot-…jar · CurseForge`. 인덱스에 자유텍스트 없음. **버전 자리가 파일 id (CF-02)** | PASS (CF-02 제외) | `cf-13-install-confirm.png`, `cf-13-progress.png`, `cf-13-installed-row.png` |
| 14 | 차단 파일 (`Structory`, Fg-1.20.1, 최신 정식이 차단 + Modrinth 미러 없음) | §8.4 경고 콜아웃 | `Callout warning 직접 내려받아야 하는 파일 / CurseForge가 배포를 허용하지 않는 파일입니다. 아래 주소에서 직접 내려받아 폴더에 넣으세요.` + 이름 `Structory` + 파일 `Structory_1.20.x_v1.3.5.jar` + **선택 가능한 mono 주소 필드** `https://www.curseforge.com/minecraft/mc-mods/structory/files/535…` (복사 아이콘 버튼 포함, 스크린샷으로 확인 — UIA `Text` 노드가 아니라 `TextInput` 값이라 텍스트 덤프에는 잡히지 않음) + `폴더 C:\…\Fg-1.20.1\forgemods\required` + `폴더 열기` + `.crdownload` 규칙 문구. `폴더 열기`를 누르면 탐색기 창이 실제로 열림(0 → 1) | PASS | `cf-s18-blocked.log`, `cf-14-confirm-blocked.png`, `cf-14-manual-callout.png` |
| 15 | `업데이트 확인` (Neo-1.21.1) | 입양 + 업데이트 개수 + 미리보기 | 헤더가 `관리 4 · 직접 추가 1 · 확인 전` → `관리 5 · 직접 추가 0 · 마지막 업데이트 확인 방금 전`으로 바뀌고, `직접 추가` 행이던 Block Jackpot 부제가 `· CurseForge`가 됨(지문 입양). `모두 업데이트 (1)` 노출, 대상 행에 `업데이트 가능` 칩. 미리보기 줄: `19.50.0.414 → 8792638 · jei-…-19.50.0.414.jar` — **from 쪽은 사람이 읽는 버전, to 쪽은 파일 id (CF-02)** | PASS (CF-02 제외) | `cf-15-installed-before-check.png`, `cf-15-after-check.png`, `cf-15-update-preview.png` |
| 16 | 환경 변수 없이 + 설정 키 빈 값으로 재시작 | 세그먼트 비활성 + 헬퍼 | `IsEnabled=False`, `설정에서 CurseForge API 키를 입력하세요` | PASS | `cf-s19-restart-nokey.log`, `cf-16-restart-no-key.png` |

## 결함

### CF-01 (high) — 모드 동일성을 제공자별로만 판정해, 제공자를 바꿔 설치하면 같은 모드가 중복 설치된다

`apply`는 루트 모드가 "이미 설치되어 있는지"를 **`(provider, project_id)` 한 쌍으로만** 판정합니다.
의존성 판정에 쓰이는 `Installed::find`(`install.rs:432-443`)는 slug → jar `mod_id` 폴백을 갖고 있지만,
루트에는 그 폴백이 적용되지 않습니다.

```
crates/nebula-core/src/catalog/install.rs:504-508
    let held: BTreeMap<(ProviderId, String), String> = index.mods.iter()
        .map(|(file, entry)| ((entry.provider, entry.project_id.clone()), file.clone()))
```

**기본 재현 경로는 찾기 페이지의 제공자 세그먼트 그 자체입니다.** JEI를 Modrinth로 설치한 뒤
같은 JEI를 CurseForge(`238222`)로 설치하면 `(provider, project_id)`가 다르므로 `held`가 비고,
두 번째 jar가 그대로 추가됩니다. 파일 이름이 **완전히 같을 때만** `check_name_free`가
`CatalogError::NameTaken`으로 잡아내는데, 제공자가 다르면 대개 파일 이름도 다르므로
일반적인 경우에는 아무것도 막지 못합니다.

이 상태는 제공자를 일부러 바꾸지 않아도 도달합니다. `update::adopt_unmanaged`는 직접 추가된 jar를
**Modrinth sha1로 먼저** 조회하므로(`update.rs:173-193`), CurseForge에서 받아 손으로 넣은 jar가
Modrinth에도 올라와 있으면 그 항목이 조용히 `provider: modrinth`로 재기록됩니다
(§8.4의 "차단 파일 → 수동 배치 → 업데이트 확인" 흐름). 그 뒤 사용자가 같은 모드를
CurseForge에서 설치하면 자신은 제공자를 바꾼 적이 없는데도 중복이 발생합니다.
이번 재현이 정확히 그 형태였습니다(CF 파일 `19.50.0.414` vs 입양된 Modrinth 항목 `19.51.0.418`).

관측: `neoforgemods/required`에 `jei-1.21.1-neoforge-19.50.0.414.jar`(curseforge)와
`jei-1.21.1-neoforge-19.51.0.418.jar`(modrinth)가 공존. `mods list`는 `Just Enough Items` 2행을
경고 없이 출력하고, `generate distro` + `validate --strict`는 `generated.neoforgemod:jei:19.50.0.414@jar`
와 `…:19.51.0.418@jar`를 **둘 다 배포본에 실으면서 `0 errors`** 를 반환합니다.
게임은 같은 mod id가 두 번 로드되어 기동에 실패합니다.

- 증거: `s15-prep-downgrade-jei.log`, `s11d-duplicate-jei-list.log`, `s11e-distro-with-duplicate.log`,
  `s11f-validate-with-duplicate.log`, `docs/verification/gui-qa/cf-15-installed-before-check.png`
- 제안 수정: `crates/nebula-core/src/catalog/install.rs:505` — `held`를 `Installed`로 만들고
  루트에도 `Installed::find(provider, project_id, Some(slug))`를 태워, jar `mod_id`가 겹치면
  교체(또는 `CatalogError::NameTaken`에 준하는 명시적 거절)로 보내십시오. D6이 의존성에 대해
  이미 요구하는 규칙을 루트에 적용하는 것뿐입니다.

### CF-02 (medium) — CurseForge 버전 번호 자리에 파일 id가 표시된다

```
crates/nebula-core/src/catalog/curseforge.rs:606
    number: file_id.unwrap_or_default(),
```

`displayName`에는 사람이 읽는 버전이 들어 있는데(`19.51.0.418 for NeoForge 1.21.1`),
`number`는 파일 id가 됩니다. `cli.rs:987-994`의 `plan_line`은 주석에
"`version_number` is CurseForge's real file number"라고 적어 두었지만 사실과 다릅니다.

결과: `Structory 5351581 · 정식 · 1.2 MB · 2024-05-17`, `Better Loot Zibura 8858195 · 정식 · …`,
업데이트 미리보기 `19.50.0.414 → 8792638`. DESIGN-v2 §8.4가 규정한
`fabric-api 0.92.0 · 정식 · 2.1 MB · 2025-03-03` 형식과 어긋나고, from/to가 서로 다른 체계라
사용자가 무엇으로 바뀌는지 읽을 수 없습니다. (R3 미러 경로는 Modrinth의 `number`를 쓰므로
같은 대화상자에서 `Flashier Flashlights 1.2.0`처럼 정상 표기됩니다 — 같은 화면 안에서 두 표기가 섞입니다.)

- 증거: `s06b-update-dryrun.log`, `cf-13-install-confirm.png`, `cf-15-update-preview.png`,
  `cf-14-confirm-blocked.png`
- 제안 수정: `crates/nebula-core/src/catalog/curseforge.rs:606` — `number`를 `display_name`에서
  끌어내고(형태가 제각각이므로 — 이번 세션에서만 `19.51.0.418 for NeoForge 1.21.1`,
  `Structory_1.20.x_v1.3.5`, `Flashier Flashlights 1.2.0` 세 가지를 봤습니다 — 파싱에 실패하면
  파일 id로 폴백), 파일 id는 `version_id`/`file_id`에만 남기십시오.
  `cli.rs:982-986`의 주석도 함께 고쳐야 합니다.
  **주의**: `display_name`은 화면 표시 전용입니다. D2가 CurseForge 자유텍스트 캐싱을 금지하므로
  이 값을 `modindex.json`에 기록해서는 안 됩니다.

### CF-03 (medium) — 미러로 구제된 CurseForge 결과의 `다른 버전…`이 조용히 Modrinth 목록을 보여준다

R3 미러가 걸리면 `PlannedFile.provider`가 `Modrinth`로 바뀝니다(`install.rs:302-320`).
GUI 버전 선택기는 그 값을 그대로 씁니다.

```
crates/nebula/src/gui/mods.rs:1917-1922
    (plan.root.provider, plan.root.project_id.clone(), VersionTarget::Hit { provider: plan.root.provider, … })
```

관측: CurseForge를 고르고 `Flashier Flashlights`를 설치하려다 `다른 버전…`을 열면
행 id가 `version-row-idDIE0HV`, `version-row-w0FqVEfq` … 즉 **Modrinth 버전 id**입니다.
사용자에게는 아무 표시가 없고, 그 프로젝트의 CurseForge 파일은 GUI에서 도달할 수 없게 됩니다.

- 증거: `docs/verification/gui-qa/cf-s17-blocked.log`
- 제안 수정: `crates/nebula/src/gui/mods.rs:1917` — `plan.root.provider` 대신
  **`plan.root.text_provider`** (사용자가 고른 제공자)를 쓰십시오. 이 필드는 F2가
  "다운로드 제공자와 자유텍스트 제공자는 다르다"를 표현하려고 이미 추가해 둔 것입니다.

### CF-04 (medium) — API 키가 거부되면(HTTP 403) "네트워크 오류"로 보고된다

```
crates/nebula-core/src/catalog/http.rs:171-173
    if !(200..300).contains(&status) { return Err(network(url, format!("HTTP {status}"))); }
```

401/403에 전용 변형이 없어 `CatalogError::Network`가 되고, CLI는
`error: 네트워크 오류: https://api.curseforge.com/v1/mods/search?…: 403 Forbidden: API Key missing or invalid`
를 출력합니다. DESIGN-v2 §8.4는 GUI가 **오류 변형으로 분기**한다고 규정하므로, GUI에서는
`CurseForge에 연결하지 못했습니다`라는 잘못된 진단과 잘못된 처방이 나옵니다 —
실제 처방은 "키를 다시 발급/확인하라"입니다. 게다가 질의 문자열이 통째로 사용자에게 노출됩니다.

**틀린 키 하나면 언제든 재현됩니다.** 유효하지 않은 60자 값을 `NEBULA_CURSEFORGE_API_KEY`에
넣고 실행한 결과:

- `mods search … --provider curseforge` → `error: 네트워크 오류: …/v1/mods/search?…: 403 Forbidden: API Key missing or invalid` (종료 1)
- `mods install 238222 --provider curseforge` → `error: 네트워크 오류: https://api.curseforge.com/v1/mods/238222: **403**` —
  본문이 없는 경로에서는 **숫자 403만** 남아, 사용자는 무슨 일이 일어났는지 알 수 없습니다.
  (`classify`가 만드는 `HTTP {status}`가 아니라 `send`의 본문 포함 경로라 검색과 설치의 문구도 서로 다릅니다.)

- 증거: `s04c-bad-key-search.log`, `s04d-bad-key-install.log` (의도적으로 틀린 키 — 사용자의 실제 키 아님).
  같은 증상을 2026-09-11 CurseForge `/v1/mods/search` 전역 403 구간에서도 관측했습니다
  (`s01a-search-cf-jei.log`, `s01b-…`, `s01c-…`). 그 구간에는 정상 키로도 `/mods/search`만 403이고
  `/mods/{id}`, `/mods/{id}/files`, `/fingerprints`, `/games/432/versions`는 200이었으며,
  약 20분 뒤 복구되었습니다 — 상류 상태이지 Nebula 결함이 아닙니다.
- 제안 수정: `crates/nebula-core/src/catalog/http.rs:165` — 401/403에
  `CatalogError::Unauthorized { provider }`를 두고, 한국어 문장은
  `CurseForge가 API 키를 거부했습니다 — console.curseforge.com에서 키를 확인하세요`로,
  GUI 콜아웃 제목도 같은 변형으로 분기시키십시오(`gui/mods.rs:2901`의 `failure_copy`).

### CF-05 (medium) — "이 버전/로더용 빌드가 없음"이 "프로젝트 없음"과 같은 문장으로 나온다

```
crates/nebula-core/src/catalog/install.rs:106-112
    .ok_or(CatalogError::NotFound)?;
```

`plan_install`은 `source.project()`가 200으로 성공한 뒤 버전 목록이 비어도 `NotFound`를 냅니다.
CLI는 Fabric 전용 프로젝트를 Forge 서버에 설치하려 했을 때와 존재하지 않는 id를 넣었을 때
**완전히 같은** `프로젝트 또는 파일을 찾을 수 없습니다`를 출력합니다.
DESIGN-v2 §8.4는 전자에 대해 `{mc} {loader}용 버전이 없습니다`를 규정하고 있고,
GUI는 `resolve_version`의 `Ok(None)` 경로로 그 문장을 내지만 CLI에는 그 경로가 없습니다.

- 증거: `s05b-install-fabric-only-on-forge.log`(Fabric API on Forge), `s09a-install-unknown-id.log`(없는 id)
- 제안 수정: `crates/nebula-core/src/catalog/install.rs:110` — 프로젝트 조회는 성공했는데
  버전이 비었을 때 `CatalogError::NoVersion { mc, loader }`를 반환하고,
  `cli.rs`의 `mods_install`이 §8.4 문장을 쓰게 하십시오.

### CF-06 (low) — CLI의 키 없음 안내가 환경 변수를 말하지 않는다

```
crates/nebula-core/src/catalog/install.rs:71-77
    "CurseForge API 키가 없습니다 — 설정 화면의 CurseForge API 키에 값을 입력하세요 (console.curseforge.com)"
```

D2는 `NEBULA_CURSEFORGE_API_KEY` **또는** 설정 필드를 인정하는데, 문구는 GUI에만 있는
"설정 화면"만 가리킵니다. CLI만 쓰는 사용자에게는 실행 가능한 처방이 아닙니다.

- 증거: `s02-search-cf-nokey.log`, `s02b-install-cf-nokey.log`
- 제안 수정: `crates/nebula-core/src/catalog/install.rs:73` — 문장 끝에
  `또는 환경 변수 NEBULA_CURSEFORGE_API_KEY를 설정하세요`를 덧붙이십시오.

### CF-07 (low) — `--provider curseforge`에서 slug는 동작하지 않는데 `--help`는 된다고 말한다

`CurseForge::project`는 `/v1/mods/{id}`만 호출하므로 숫자 프로젝트 id만 받습니다
(`curseforge.rs:224-227`). 그런데 `mods install --help`는 `<PROJECT>  Project id or slug`라고
안내하고, slug를 넣으면 CF-05와 같은 일반 not-found가 납니다.

- 증거: `s09b-install-nonnumeric-slug.log`, `s09c-mods-help.log`
- 제안 수정: `crates/nebula/src/cli.rs:141` 인자 도움말을 제공자별로 정정하거나,
  `curseforge.rs:224`에서 비숫자 입력을 명시적으로 거절하는 한국어 문장을 내십시오.

### CF-08 (low) — R7의 250 ms 간격은 프로세스 전역일 뿐이라 CLI 반복 호출에는 적용되지 않는다

`CURSEFORGE_NEXT`는 `static Mutex`(`http.rs:36`)이므로 프로세스 경계를 넘지 않습니다.
연속 실행한 두 `nebula mods … search`가 CurseForge에 **119 ms 간격**으로 요청했습니다.
plan의 문언("process-global")과는 어긋나지 않지만, 스크립트 루프는 CurseForge가 기대하는
호출 간격을 쉽게 위반합니다. 서버 락(`.nebula-lock`)처럼 파일 기반 간격 예약을 두거나,
문서에 한계를 명시하는 편이 좋습니다.

- 증거: `s08a-search-back-to-back-1.log`, `s08b-search-back-to-back-2.log`
- 제안 수정: `crates/nebula-core/src/catalog/http.rs:36` 근처에 한계를 명시하는 주석 +
  plan의 R7 항목에 "프로세스 내" 한정을 추가.

## 결함이 아닌 관측

- **CurseForge `/v1/mods/search` 403 구간(상류)**: 2026-09-11 약 20분간 같은 키로
  `/mods/search`만 403(`Forbidden: API Key missing or invalid`)이고 `/mods/{id}`,
  `/mods/{id}/files`, `/fingerprints`, `/games/{id}/versions`는 정상이었습니다. 이후 복구.
  Nebula의 질의 문자열 자체는 facts §B.2와 일치했습니다(403 URL로 확인).
- **CLI 검색 캡션은 원시 다운로드 수**(`다운로드 621604849`)입니다. §8.4의
  `다운로드 120만` 요구는 GUI 전용이고 GUI는 `다운로드 6.2억`으로 올바르게 표시합니다.
- **입양은 Modrinth 항목의 제목도 버립니다**(`update.rs`가 두 제공자 모두 `IndexEntry::new` 사용).
  주석이 의도라고 밝히고 있고 설치됨 목록은 jar 메타데이터에서 제목을 얻으므로 화면상 문제는 없습니다.
- **CurseForge 검색의 페이지 경계**는 `pageSize`에 따라 달라집니다(상류 특성, facts §E.2.3의
  `totalCount` 주의와 같은 계열). 같은 `sortField` 안에서는 `index` 페이징이 연속적이고
  중복이 없음을 확인했습니다(단계 1e/1g).

## 미수행 항목과 사유

| 항목 | 사유 |
| --- | --- |
| R7 250 ms 간격의 **직접 측정** | 코드가 요청마다 타임스탬프 로그를 남기지 않고(`env_logger` 포맷에 시각 없음, `http.rs`는 재시도/429에만 debug 로그), CurseForge 자체 왕복이 150~550 ms로 250 ms보다 크거나 비슷해 외부 벽시계로 스로틀을 분리할 수 없습니다. 대신 (a) 한 프로세스에서 CF 4~5요청을 하는 `mods update`의 벽시계가 하한 `(N-1)×250 ms`를 항상 넘김, (b) `http.rs:322-330`의 예약식 구현 확인, (c) 프로세스 간에는 간격이 지켜지지 않음(CF-08)을 기록했습니다. 제안: `throttle()`에 `log::trace!("curseforge slot at {:?}", start)` 한 줄이면 이후 QA에서 관측 가능해집니다. |
| **의존성이 있는 차단 파일**에서 "나머지 파일은 그대로 설치" | live에서 `allowModDistribution=false` 이면서 필수 의존성을 가진 프로젝트를 찾지 못했습니다(검색 4페이지 × 2조합 스윕에서 차단 프로젝트 8건, 모두 의존성 0). 차단 파일이 계획에서 제거되고 나머지가 설치되는 코드 경로 자체는 `install.rs:535`에서 확인했습니다. |
| 429(요청 과다) 문구 | 정상적인 호출량(총 CurseForge 요청 약 120건, 250 ms 간격)으로는 한 번도 429를 받지 못했습니다. 인위적 유발은 사용자의 실제 키에 대한 위험이라 하지 않았습니다. |
| install-confirm의 **베타/알파 경고 Callout** | 이번에 고른 CurseForge 파일이 전부 D3의 `정식`으로 해결되어(JEI도 베타 대신 정식 선택) 경고 경로가 열리지 않았습니다. |
| 차단 파일을 **손으로 넣은 뒤 다음 업데이트 확인이 출처를 붙이는** GUI 왕복 | CLI에서 지문 입양(단계 7b)과 GUI에서 입양 토스트(단계 15)를 각각 확인했으므로 같은 코드 경로가 양쪽에서 검증되었습니다. 차단 파일 자체를 내려받아 넣는 행위는 수동 단계라 자동화하지 않았습니다. |
| `mods remove/move`의 CurseForge 특이사항 | 제공자 무관 경로이고 `2026-09-11-mods-cli-qa.md`에서 이미 검증되었습니다. |

## 비밀 취급

- 키는 `Set-CfKey` / `Start-App -WithKey`가 **실행 시점에 파일에서 읽어** 자식 프로세스 환경에만
  넣고, 부모 셸에서는 즉시 제거했습니다. 스크립트에 리터럴로 박은 곳은 없습니다.
- 증거 로그의 명령줄은 인자 목록만 기록하며, 키 상태는 `set (redacted)` / `unset`으로만 남습니다.
  키는 HTTP **헤더**로만 전송되므로 URL이 찍히는 오류 문장에도 포함되지 않습니다
  (CF-04의 403 문장에서 직접 확인).
- GUI 설정 필드는 `IsPassword=True`임을 UIA 속성으로 확인했습니다. 키가 들어 있는 상태로 찍힌
  유일한 스크린샷 `cf-11-settings-card-masked.png`는 **픽셀 단위로 육안 확인**했고, 60자가 전부
  점(●)으로만 렌더링되어 판독 가능한 문자가 없습니다.
- CF-04 재현에 쓴 키는 사용자의 키가 아니라 의도적으로 만든 무효 값입니다.
- 단계 11c 동안 `<scratchpad>/cf-qa/config/settings.json`에 키가 평문으로 저장됩니다(스크래치 한정).
  단계 11d에서 비웠고, 종료 시점 값은 길이 0입니다. 저장소에는 어떤 형태로도 쓰지 않았습니다.
- 종료 전 `docs/verification/**` 와 `<scratchpad>/cf-qa/**` 전체(스크립트·로그·스크린샷 포함)를
  키 앞 8자로 검색 → **적중 0건**.
