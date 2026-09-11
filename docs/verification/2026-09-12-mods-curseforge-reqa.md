# Nebula Rust 포트 — 모드 카탈로그 **CurseForge 결함 수정 재검증(re-QA)** 보고서

- **일자**: 2026-09-12
- **대상 커밋 (HEAD)**: `31067f6` — *Treat one mod as one mod across providers, and speak CurseForge's
  failures in Korean* (워크트리 `rust-port`, `2d9e500` 다음의 CurseForge 수정 패스)
- **범위**: `docs/verification/2026-09-11-mods-curseforge-qa.md` §결함의 **CF-01 ~ CF-08 여덟 건만**
  재검증. 함께 검증한 문서 근거는 `docs/plans/2026-09-11-mod-catalog.md`의
  **"Fix notes (CurseForge QA)"** 절(무엇을 어떻게 고쳤다고 주장하는지)입니다.
  새 기능 검증이나 전체 회귀는 범위 밖입니다.
- **검증 방식**: 블랙박스(실서버 live 호출) + 주장 대조가 필요한 곳에서만 소스 확인.
  작업 트리 `crates/`는 읽기 전용으로만 접근했고, 쓴 파일은 이 보고서와
  `docs/verification/gui-qa/cf-reqa-*` 뿐입니다. 커밋하지 않았습니다.
- **스냅샷/스크래치**: `<scratchpad>/cf-reqa` — `src/`(= `git archive HEAD`), `target/`(릴리스 빌드),
  `root/`(테스트 ROOT), `config/`·`config-gui/`(설정), `logs/`(증거 로그), `gui/`(UIA 시나리오).
- **키 취급**: CurseForge 키는 자식 프로세스의 환경 변수로만 전달했습니다. 명령줄·로그·스크린샷·
  이 보고서 어디에도 값이 남지 않으며, 종료 전 `docs/verification/**` 와 `<scratchpad>/cf-reqa/**`
  전체를 키 앞 8자로 검색해 **적중 0건**, 로그 내 `x-api-key` 문자열 **0건**을 확인했습니다.

## 실행 환경

| 항목 | 값 |
| --- | --- |
| OS | Windows 10 Pro 19045 |
| Java | Eclipse Temurin `jdk-21.0.10.7-hotspot` (`JAVA_EXECUTABLE`) |
| `ROOT` | `<scratchpad>/cf-reqa/root` (사용자 GUI pid 19856의 `H:\…\rust-port` 루트와 완전 분리) |
| `BASE_URL` | `https://cdn.example.invalid/` |
| `NEBULA_CONFIG_DIR` | `<scratchpad>/cf-reqa/config` (CLI) / `config-gui` (GUI) |
| `CARGO_TARGET_DIR` | `<scratchpad>/cf-reqa/target` |
| 서버 | `Neo-1.21.1` (NeoForge 21.1.250) — 여덟 결함 전부 이 한 서버에서 재현 가능했습니다 |
| 빌드 | `cargo build --release -p nebula` → **성공** (9m 12s) |
| 테스트 | `cargo test --workspace` → **348 passed / 0 failed / 1 ignored** (12개 스위트 합계) |

> 실행 도중 C: 드라이브가 가득 차 첫 `cargo test`가 `os error 112`로 죽었습니다. 같은 스크래치패드에
> 남아 있던 이전 QA 패스의 **빌드 캐시만**(`qa/target`, `cli-qa/target`, 본인 `cf-reqa/target/debug`)
> 지워 11.5 GB를 확보한 뒤 재실행해 위 결과를 얻었습니다. 증거 로그·루트·스크린샷은 건드리지 않았습니다.

## 종합 결과

| 결함 | 판정 |
| --- | --- |
| CF-01 (high) | **PASS** (양쪽 절반 모두) |
| CF-02 (medium) | **PASS** |
| CF-03 (medium) | **PASS** |
| CF-04 (medium) | **PASS** |
| CF-05 (medium) | **PASS** |
| CF-06 (low) | **PASS** |
| CF-07 (low) | **PASS** |
| CF-08 (low) | **PASS (설계상 수용 + 계측 수단 추가)** — 단, plan R7 문언은 아직 "프로세스 내" 한정이 없음 |

**여덟 건 모두 스펙대로 동작합니다. 새로 발견된 결함은 없습니다.**
남은 항목은 코드가 아니라 문서 한 줄로, 아래 "잔여 지적"에 적었습니다.

수정 노트가 **QA 제안과 다르게 간 세 곳**(CF-02 `displayName` 무파싱, CF-03 `text_provider` 대신
`PlannedFile::source()`, CF-04 한국어 문구)은 모두 노트에 근거가 적혀 있고 관측 결과가 노트와
일치하므로 **문서화된 의도적 편차**로 처리했고 결함으로 세지 않았습니다.

## 항목별 결과

| 항목 | 명령 / 조작 | 기대 | 관측 | 판정 | 증거 |
| --- | --- | --- | --- | --- | --- |
| CF-01a 준비 | `mods Neo-1.21.1 install jei --provider modrinth` | Modrinth JEI 1개 설치 | `jei-1.21.1-neoforge-19.51.0.418.jar` 1개, 인덱스 `provider: modrinth / u6dRKJwZ` | — | `cf01d-install-jei-modrinth.log` |
| **CF-01b 교체(결정적)** | `mods Neo-1.21.1 install 238222 --version 8857687 --provider curseforge` | 제공자가 달라도 **같은 모드 = 교체**, jar 1개 | jar가 `jei-…-19.54.0.429.jar` **하나만** 남고 Modrinth jar는 사라짐. 인덱스 항목이 `provider=curseforge project=238222 version=8857687`로 **치환**, `title`/`project_slug` **없음(D2)** | **PASS** | `cf01e-install-jei-cf-othername.log` |
| **CF-01c §8.4 입양 경로 전체** | 손으로 넣은 CF jar → `mods update`(입양) → `mods install 238222 --provider curseforge` | 입양 후 CF 설치가 **중복이 아니라 교체** | 입양: `Adopted … as modrinth u6dRKJwZ` → CF 설치 후 jar 1개(`19.51.0.418`), 인덱스 `provider=curseforge 238222`. `mods list` 2행(JEI·Sodium), `generate distro`에 `generated.neoforgemod:jei:…@jar` **1개**, `validate --strict` `0 errors` | **PASS** | `cf01k-update-adopt.log`, `cf01l-install-cf-after-adoption.log`, `cf01m-list-final.log`, `cf01n-distro-final.log`, `cf01o-validate-final.log` |
| **CF-01d 손배치 거절** | 인덱스 없는 `jei-handplaced.jar` 배치 후 `mods install 238222 --provider curseforge` | `NameTaken` — **파일 이름을 말하는** 거절, 다운로드 0 | `error: jei-handplaced.jar은(는) 이미 있는 파일 이름입니다 — 먼저 그 파일을 지우세요` (종료 1). 디렉터리 불변, `.nebula-tmp` **비어 있음** = 스테이징 전에 거절 | **PASS** | `cf01g-install-cf-over-handplaced.log` |
| **CF-02 설치/검색 표기** | `mods … install … --provider curseforge` | 파일 id가 아닌 사람이 읽는 버전 | `Just Enough Items (JEI) 19.51.0.418 for NeoForge 1.21.1 · 정식 · … · 2026-09-02`, `Sodium Sodium 0.8.13 for NeoForge 1.21.1 · 정식 · …` | **PASS** | `cf01l-…`, `cf05-cf08-sodium-trace.log` |
| **CF-02 업데이트 from/to** | `mods Neo-1.21.1 update` (19.50.0.414 설치 상태) | 양쪽 모두 사람이 읽는 버전 | `jei-…-19.50.0.414.jar : 19.50.0.414 → 19.51.0.418 for NeoForge 1.21.1 (정식)` — 예전 결함의 `→ 8792638` 사라짐 | **PASS** | `cf02e-update-dryrun.log` |
| CF-02 D2 무누출 | `modindex.json` | `displayName`이 인덱스에 남지 않을 것 | 모든 CF 항목에 `title`/`project_slug` 필드 자체가 없음; `version_id`/`file_id`만 파일 id | **PASS** | `cf01l-install-cf-after-adoption.log` |
| CF-02 검색 | `mods Neo-1.21.1 search structory --provider curseforge --limit 3` | 파일 id 노출 없음 | 프로젝트 행만 출력(`636540  Structory (structory) · 다운로드 88098183 · botanydev`) — 버전 자리 자체가 없음 | **PASS** | `cf02f-search-cf.log` |
| **CF-03 (GUI, UIA)** | 찾기 → 제공자 `CurseForge` → `jei` 검색 → 설치 확인 → `다른 버전…` | 버전 목록이 **CurseForge의 것** | 아래 "CF-03 관측 전문" 참조 — 행 id 5개 전부 숫자 CurseForge 파일 id, 행 제목도 CF 표기. 예전 결함의 Modrinth id(`version-row-idDIE0HV` 꼴) 사라짐 | **PASS** | `cf-reqa-03-versions.png`, `cf-reqa-03-confirm.png`, `cf-reqa-03-search.png`, `cf-reqa-03.ps1` (+ `cf-reqa-03.log`, 아래 주석) |
| CF-03 (GUI, CF-02 겸) | 같은 설치 확인 대화상자 | §8.4 줄 형식 | `Just Enough Items (JEI) 19.51.0.418 for NeoForge 1.21.1 · 정식 · 1.7 MB · 2026-09-02` | **PASS** | `cf-reqa-03-confirm.png` |
| **CF-04 키 거부(검색)** | 엉터리 키로 `mods … search jei --provider curseforge` | `CurseForge API 키가 거부되었습니다 — 설정의 키를 확인하세요`, 3.5 s 재시도 없음 | 문구 일치, 종료 1, **1657 ms**(프로세스 기동 + TLS 콜드 포함) | **PASS** | `cf04-badkey-search.log` |
| **CF-04 키 거부(설치=`send` 경로)** | 엉터리 키로 `mods … install 238222 --provider curseforge` | 같은 문구, 숫자 403 노출 없음, 재시도 없음 | 같은 문구, 종료 1, **279 ms** — 백오프(0.5+1+2 s)가 전혀 돌지 않음 | **PASS** | `cf04-badkey-install.log` |
| CF-04 GUI 분기 | 소스 대조 | 콜아웃이 변형으로 분기 | `gui/mods.rs:2988` `CatalogError::Unauthorized => ("CurseForge가 API 키를 거부했습니다", "설정의 CurseForge API 키를 확인하세요 …")` | **PASS** | `crates/nebula/src/gui/mods.rs:2988` |
| **CF-05 로더 불일치** | `mods Neo-1.21.1 install 306612 --provider curseforge` (Fabric API) | `1.21.1 NeoForge용 버전이 없습니다` | 문구 그대로, 종료 1 | **PASS** | `cf05-fabric-only.log` |
| **CF-05 없는 프로젝트** | `mods Neo-1.21.1 install 999999999 --provider curseforge` | `프로젝트 또는 파일을 찾을 수 없습니다` | 문구 그대로, 종료 1 | **PASS** | `cf05-unknown-id.log` |
| CF-05 명시 버전 미스(분리 확인) | `mods … install 238222 --version 1 --provider curseforge` | `NotFound` 유지 (사다리 미스만 `NoVersion`) | `프로젝트 또는 파일을 찾을 수 없습니다` | **PASS** | `cf05-bogus-version.log` |
| CF-05 참고 | `mods Neo-1.21.1 install 394468 --provider curseforge` (sodium) | — | sodium(394468)은 이제 **NeoForge 1.21.1 빌드가 실재**해 정상 설치됨. 과제가 지정한 "Fabric 전용" 예시가 상류에서 바뀐 것이므로, 진짜 Fabric 전용인 **Fabric API(306612)** 로 대체해 검증했습니다 | — | `cf05-cf08-sodium-trace.log` |
| **CF-06 키 없음** | 키 미설정 상태 `search` / `install` | 설정 화면 **과** 환경 변수를 모두 안내 | `CurseForge API 키가 없습니다 — 설정 → CurseForge API 키에 값을 입력하거나 환경 변수 NEBULA_CURSEFORGE_API_KEY를 설정하세요 (console.curseforge.com)` (두 명령 동일) | **PASS** | `cf06-search-nokey.log`, `cf06-install-nokey.log` |
| **CF-07 slug 거절** | `RUST_LOG=trace mods Neo-1.21.1 install jei --provider curseforge` | 한국어 조기 거절 + **요청 0건** | `error: CurseForge는 숫자 프로젝트 id만 받습니다 — jei은(는) 숫자가 아닙니다. slug는 Modrinth에서만 쓸 수 있습니다` (종료 1). trace 로그에 `curseforge GET/POST`·`api.curseforge.com`·`api.modrinth.com` **0줄** | **PASS** | `cf07-slug-trace.log` |
| **CF-07 `--help`** | `mods Neo-1.21.1 install --help` | 제공자별로 정정된 문구 | `<PROJECT>  Project id, or a Modrinth slug — CurseForge accepts numeric project ids only` | **PASS** | `cf07-install-help.log` |
| **CF-08 trace 한 줄/요청** | `RUST_LOG=trace mods … search jei --provider curseforge` | `curseforge GET <url>`, 헤더 없음 | `[TRACE] [nebula_core::catalog::http] curseforge GET https://api.curseforge.com/v1/mods/search?gameId=432&classId=6&searchFilter=jei&gameVersion=1.21.1&sortField=2&sortOrder=desc&pageSize=3&index=0&modLoaderType=6` — URL만, 헤더 0 | **PASS** | `cf08-search-trace.log` |
| **CF-08 프로세스 내 간격** | `RUST_LOG=trace mods Neo-1.21.1 update` (CF 요청 6건 연속) | 연속 요청 ≥ 250 ms | 간격 **241 / 251 / 250 / 250 / 249 ms** (5구간 합 1241 ms, 평균 248.2 ms). 250 ms 예약 간격과 일치 — 아래 계측 주석 참조 | **PASS** | `cf08-update-trace.log` |
| CF-08 헤더 미기록 | trace 로그 전수 | `x-api-key` 미기록 | `<scratchpad>/cf-reqa/logs/*.log` 에 `x-api-key` **0건**, 키 앞 8자 **0건** | **PASS** | 아래 "비밀 취급" |
| **R5 배포본 불변** | `generate distro` × 3 (있음 / 있음(대조) / 없음) | 카탈로그 파일 유무와 무관하게 **바이트 동일** | 세 번 모두 SHA-256 `9C7A079C1EC07AC15DB627A7A6A4347F54A7C3D0EB5A80AE735CAAACE9947328`, `54 artifacts, 54 with SHA-256, 0 errors` | **PASS** | `r5a-distro-with-catalog.log`, `r5b-distro-with-catalog-again.log`, `r5c-distro-without-catalog.log` |
| **워크스페이스 테스트** | `cargo test --workspace` (스냅샷) | 수정 노트의 348 | **348 passed, 0 failed, 1 ignored** (66+146+32+48+2+2+8+18+4+15+7+0) | **PASS** | `logs/cargo-test.log` |

### CF-03 관측 전문 (UIA)

버전 선택기의 AutomationId 다섯 개와 화면에 실제로 그려진 제목/캡션입니다. **전부 CurseForge**의
파일 id와 `displayName`이며, Modrinth 버전 id는 한 건도 없습니다.

```
  button name='다른 버전 고르기'
  version rows: version-row-8857687 ; version-row-8850347 ; version-row-8829744 ;
                version-row-8815666 ; version-row-8811159
  --- version picker texts ---
    Just Enough Items (JEI) 버전 — Neo-1.21.1
    19.54.0.429 for NeoForge 1.21.1 / 1.21, 1.21.1 · 2026-09-11 · 1.8 MB / 베타
    19.54.0.427 for NeoForge 1.21.1 / 1.21, 1.21.1 · 2026-09-10 · 1.8 MB / 베타
    19.53.0.426 for NeoForge 1.21.1 / 1.21, 1.21.1 · 2026-09-07 · 1.8 MB / 베타
    19.53.0.425 for NeoForge 1.21.1 / 1.21, 1.21.1 · 2026-09-05 · 1.8 MB / 베타
    19.52.0.424 for NeoForge 1.21.1 / 1.21, 1.21.1 · 2026-09-05 · 1.8 MB / 베타
[PASS] every version row id is a CurseForge file id (numeric) -- numeric=5 of 5
RESULT: all checks passed
```

설치 확인 대화상자(같은 실행, CF-02의 GUI 쪽 근거):

```
    Neo-1.21.1에 설치
    Just Enough Items (JEI) 19.51.0.418 for NeoForge 1.21.1 · 정식 · 1.7 MB · 2026-09-02
    다른 버전…
```

> **주의**: 원본 로그 `docs/verification/gui-qa/cf-reqa-03.log` 는 저장소의 `.gitignore:3` (`*.log`)에
> 걸립니다. 이 절에 관측 전문을 그대로 옮겨 두었으므로 보고서만으로 검증이 가능하지만, 이 파일들을
> 커밋할 때는 이전 QA의 `cf-s*.log` 처럼 `git add -f` 가 필요합니다.

### CF-08 간격 계측에 대한 주석

`init_logger`(`crates/nebula/src/cli.rs:333`)의 커스텀 포맷은 타임스탬프를 찍지 않으므로, 간격은
**stderr 줄이 읽히는 시각**을 PowerShell 파이프에서 찍어 계산했습니다(`<scratchpad>/cf-reqa/runts.ps1`).
따라서 ±10 ms 수준의 리더 지터가 섞여 있고, 첫 구간 `241 ms`는 그 지터입니다
(5구간 총합 1241 ms ≒ 5 × 250 ms). `CURSEFORGE_NEXT`가 "마지막 요청 시각"이 아니라 **예약**이라
누적 드리프트가 생기지 않는다는 점도 이 수치와 일치합니다. 프로세스 내 250 ms 간격은
**실측으로 확인**되었다고 판단합니다.

## 수정 노트의 주장 대조

| 수정 노트의 주장 | 대조 결과 |
| --- | --- |
| **CF-01 후반부는 근거를 들어 기각** — `adopt_unmanaged`는 관리 중인 항목을 덮어쓸 수 없다 (`update.rs:150-154`, `251-253`) | **기각이 타당합니다.** 락 이전 필터 `!index.mods.contains_key(&candidate.file_name)`(`crates/nebula-core/src/catalog/update.rs:152`)와 락 이후 `*index = ModIndex::load(server)?;`(`update.rs:248`) 재적재 뒤의 재확인 `if index.mods.contains_key(file_name) { continue; }`(`update.rs:251-253`)가 **둘 다** 존재합니다. 실측에서도 손배치 jar만 입양되고(`cf01k-update-adopt.log`) 관리 중이던 `sodium` 항목은 그대로였습니다. QA가 말한 "조용히 `provider: modrinth`로 **재기록**"은 정확하지 않으며, 입양은 **첫 항목을 만드는** 동작입니다. 그로 인해 생기던 중복은 CF-01 전반부였고 지금 고쳐져 있습니다(CF-01c 행) |
| CF-02: `displayName`을 **파싱하지 않고 그대로** 쓴다 (QA 제안과 다름) | 관측 일치 — `19.51.0.418 for NeoForge 1.21.1`, `Sodium 0.8.13 for NeoForge 1.21.1` 처럼 원문 그대로. 파싱 실패 시 파일 id로 되돌아가는 위험이 없어집니다. **의도적 편차, 결함 아님** |
| CF-03: `text_provider`가 아니라 `PlannedFile::source()` | 소스 확인 `gui/mods.rs:1925` `let (provider, project_id) = plan.root.source();`, 그리고 `gui/mods.rs:1388`에 "여기서는 일부러 `source()`를 쓰지 않는다"는 주석(다운로드 삼중항 유지). GUI 실측 PASS. **의도적 편차, 결함 아님** |
| CF-04: 한국어 문구가 QA 제안(`…거부했습니다 — console.curseforge.com에서…`)과 다름 | 실제 구현은 CLI `CurseForge API 키가 거부되었습니다 — 설정의 키를 확인하세요`(`catalog/mod.rs:585`), GUI 콜아웃 `CurseForge가 API 키를 거부했습니다` + `설정의 CurseForge API 키를 확인하세요 (console.curseforge.com).`(`gui/mods.rs:2988-2990`). 처방은 동일. **의도적 편차, 결함 아님** |
| CF-07: 가드를 CLI가 아니라 `CurseForge::project` 한 곳에 둔다 | `curseforge.rs:226` `check_numeric_id(id)?;` 가 HTTP 호출 앞줄. 모든 호출자가 이 한 경로를 지나므로 CLI·GUI·의존성 해석 전부 커버됩니다. 실측 요청 0건 |
| CF-08: 교차 프로세스 스로틀은 **추가하지 않음**, 대신 `ponytail:` 주석 + trace 계측 | `http.rs:36-43`에 한계(119 ms 실측)와 승격 경로(`.nebula-lock` 옆 파일 기반 예약)가 적힌 주석 존재, `trace_request`(`http.rs:201-204`)가 `throttle` 뒤에서 한 줄 기록. **코드 쪽 주장은 전부 사실** |

## 잔여 지적 (결함 1건, 문서)

**RQ-01 (low, 문서) — plan의 R7 문언에 아직 "프로세스 내" 한정이 없습니다.**

- 위치: `docs/plans/2026-09-11-mod-catalog.md:108`
  ```
  | R7 | CurseForge client-side throttle: ≥ 250 ms between requests, search results reuse
        `latestFilesIndexes` instead of per-mod `/files` calls; Modrinth honours `X-Ratelimit-*` … |
  ```
- CF-08의 제안 수정은 **두 가지**였습니다 — (a) `http.rs:36` 근처에 한계를 명시하는 주석, (b) **plan의
  R7 항목에 "프로세스 내" 한정 추가**. (a)는 되어 있으나 (b)는 반영되지 않았고, 수정 노트도 (a)만
  언급합니다. 지금 문언만 읽으면 보증 범위가 프로세스 경계를 넘는 것처럼 읽힙니다.
- 제안 수정(한 줄): `docs/plans/2026-09-11-mod-catalog.md:108`의 `≥ 250 ms between requests` 를
  `≥ 250 ms between requests **within one process** (a `static` reservation; two back-to-back CLI runs
  can be ~119 ms apart — see the `ponytail:` note at `catalog/http.rs:36`)` 로 바꾸십시오.
- 코드 동작에는 영향이 없으므로 릴리스 차단 사유는 아닙니다.

## 결함이 아닌 관측

- **sodium(CF `394468`)은 더 이상 Fabric 전용이 아닙니다.** 실측에서 `sodium-neoforge-0.8.13+mc1.21.1.jar`
  가 NeoForge 1.21.1용으로 정상 설치되었습니다. CF-05의 "로더 불일치" 경로는 진짜 Fabric 전용인
  **Fabric API(`306612`)** 로 검증했습니다. 상류 변화이지 Nebula 결함이 아닙니다.
- **CF-01 기본 재현(Modrinth → CurseForge 동일 버전)은 판별력이 없습니다.** 두 제공자가 같은 파일 이름을
  주면 옛 `check_name_free`만으로도 걸리기 때문입니다. 그래서 파일 이름이 **다른** CF 파일
  (`--version 8857687`)로 다시 실행해 `Installed::find`의 jar `mod_id` 폴백이 실제로 동작함을 확인했습니다
  (CF-01b 행). 같은 이유로 §8.4 입양 경로 전체도 별도로 재현했습니다(CF-01c 행).
- **`mods update` 는 `/v1/fingerprints` 없이** 관리 중인 CF 모드마다 `/v1/mods/{id}/files` 를 페이지 단위로
  돌았습니다(JEI 5페이지 + Sodium 1페이지). D7이 규정한 비용 구조와 일치하며, 그 덕분에 CF-08의
  250 ms 간격을 6건 연속으로 깨끗하게 잴 수 있었습니다.
- **CurseForge `/v1/mods/search` 전역 403 구간은 이번 세션에서 재현되지 않았습니다.** 이전 보고서가
  상류 상태로 분류한 것과 일치합니다.

## 미수행 항목과 사유

| 항목 | 사유 |
| --- | --- |
| Fabric 1.20.1 서버 | 여덟 결함 전부 `Neo-1.21.1` 한 서버에서 재현·검증 가능했습니다. 서버를 늘리면 실서버 호출만 늘고 판별력은 늘지 않습니다 |
| CF-03의 **R3 미러 경로**(차단 파일이 Modrinth로 구제되는 경우) GUI 재현 | 과제가 지정한 조작(“CurseForge에서 jei 검색 → 설치 확인 → 다른 버전…”)을 그대로 수행해 PASS를 받았습니다. 미러 경로는 차단된 특정 CF 파일이 필요하고, 수정 노트가 그 형태를 `PlannedFile::source()` 단위 테스트로 잠가 두었으며(`cargo test --workspace` 348 통과), 소스에서 `gui/mods.rs:1925`가 `source()`를 쓰는 것을 확인했습니다. 남은 위험은 낮다고 판단합니다 |
| CF-04 GUI 콜아웃 화면 캡처 | 과제 범위 밖(CLI 문구 + 지연 시간이 요구사항). 분기 존재는 `gui/mods.rs:2988` 소스로 확인했습니다 |
| `cargo clippy` / `cargo deny` | 이번 재검증의 요구사항이 아니며, 수정 노트가 이미 청결을 주장합니다. 재검증 대상인 여덟 결함의 판정에 영향을 주지 않습니다 |

## 비밀 취급

- 키는 `run.ps1` / `runts.ps1` / `gui/cf-env.ps1` 이 **파일에서 읽어 환경 변수에만** 넣었고, 어떤
  명령줄에도 값이 등장하지 않습니다. 엉터리 키 단계(CF-04)는 실제 키가 아닌 고정 더미 문자열입니다.
- 종료 전 `<scratchpad>/cf-reqa/**` 와 `H:\…\rust-port\docs\verification\**` 전체(png/jar/exe 등 바이너리
  제외, 20 MB 미만 파일)를 키 **앞 8자**로 검색 → **적중 0건**.
- `<scratchpad>/cf-reqa/logs/*.log` 를 `x-api-key` 로 검색 → **0건**. `RUST_LOG=trace` 는 URL만 남기고
  헤더를 남기지 않는다는 CF-08의 주장과 일치합니다.
- 이 보고서와 `docs/verification/gui-qa/cf-reqa-*` 에도 키가 없습니다.
