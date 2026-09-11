# Nebula Rust 포트 — 모드 카탈로그 CLI QA 검증 보고서

- **일자**: 2026-09-11
- **대상 커밋 (HEAD)**: `761c8dd4ff230b6a86bddbe7d4b13bf6ab9ee082`
  ("Wire the 모드 page to the catalog: quiet lookups, locked mutations, icons")
- **범위**: `nebula mods …` (search / install / list / update / remove / move), `nebula server delete|duplicate`,
  `.nebula-lock` 경합, `modindex.json` 정합성, 그리고 이 기능이 `distribution.json`을 바꾸지 않는다는 불변식(R5).
- **검증 방식**: 블랙박스. 작업 트리에 다른 에이전트의 GUI 작업이 진행 중이므로 `git archive HEAD`로
  스냅샷을 떠서 격리된 디렉터리에서 빌드·실행했습니다. 메인 작업 트리는 읽기 전용으로만 접근했고
  이 보고서 외에는 아무 파일도 쓰지 않았습니다.
- **스냅샷/스크래치 경로**: `<scratchpad>/cli-qa`
  (`src/` 스냅샷, `target/` 릴리스 빌드, `root/` 테스트 ROOT, `evidence/` 증거 로그)
- **증거 로그**: `<scratchpad>/cli-qa/evidence/<step>.log` — 모든 명령의 stdout+stderr, 종료 코드, 소요 시간 포함.
- **제공자**: Modrinth 실서버(live). 검색 8회 · 설치 10회 · 업데이트 확인 3회로 요청량을 제한했습니다.
  **CurseForge는 키 없음 경로만 검증**했습니다 (`NEBULA_CURSEFORGE_API_KEY` 미설정) — 아래 "미수행" 절 참조.

## 실행 환경

| 항목 | 값 |
| --- | --- |
| OS | Windows 10 Pro 19045 |
| Java | Eclipse Temurin 21.0.10+7-LTS (`C:/Program Files/Eclipse Adoptium/jdk-21.0.10.7-hotspot/bin/java.exe`) |
| `ROOT` | `<scratchpad>/cli-qa/root` |
| `BASE_URL` | `https://cdn.example.invalid/` |
| `NEBULA_CONFIG_DIR` | `<scratchpad>/cli-qa/cfg` |
| `NEBULA_CURSEFORGE_API_KEY` | **미설정** |
| `CARGO_TARGET_DIR` | `<scratchpad>/cli-qa/target` (GUI 레인과 경합하지 않도록 분리) |

빌드: `cargo build --release -p nebula` → **성공, 10분 00초 (601초)**, 증거 `00-build.log`.
산출물 `cli-qa/target/release/nebula.exe`로 이하 전 단계를 수행했습니다.

## 종합 결과

**63개 단계 실행 완료. 실행 차단(블로커)이나 데이터 손실 결함 0건.**
스펙(plan v1.1 D1–D11 / R1–R12, DESIGN-v2 §8) 대비 **결함 7건**을 확인했습니다 —
high 1, medium 2, low 2, nit 2. 가장 무거운 것은 **업데이트 확인이 제공자에 따라 다르게 동작해,
Modrinth 쪽에서는 이미 최신 정식인 모드를 베타로 교체 제안하고 `--apply`가 아무 관문 없이
그대로 설치하는 문제**입니다(결함 1). 아래 표의 "종료" 열에서 0이 아닌 값은
대부분 **의도된 실패 경로**(에러 처리 검증)입니다.

## 단계별 결과

| # | 명령 | 기대 (근거) | 관찰 | 종료 | 시간 | 증거 |
| --- | --- | --- | --- | --- | --- | --- |
| 1a | `nebula --version` | 버전 출력 | `nebula 1.0.0` | 0 | 875ms | `01a-version.log` |
| 1b | `nebula mods --help` | plan "CLI" 절의 6개 하위 명령 | `search install list update remove move` 전부 존재, 인자·플래그가 plan 문자열과 일치 | 0 | 57ms | `01b-mods-help.log` |
| 2 | `nebula init root` | 표준 레이아웃 | `schemas/` 2종 + `servers/` 생성 | 0 | 112ms | `02-init-root.log` |
| 3 | `generate server Fab 1.20.1 --fabric latest` | Fabric 서버, **Java 불필요** | `fabric.version 0.19.5`; `fabricmods/{required,optionalon,optionaloff}` + `files` + `libraries` 생성. Java 없이 성공 | 0 | 173ms | `03-gen-server-fabric.log` |
| 4a | `mods Fab-1.20.1 search sodium --limit 5` | mc+loader 패싯 필터, 관련도순 | `modrinth · 검색 결과 36개 중 5개 (Minecraft 1.20.1 · Fabric)`, 5행 | 0 | 389ms | `04a-search-default.log` |
| 4b | `… --sort downloads --limit 3` | 다운로드 내림차순 | 223M → 94.8M → 79.4M | 0 | 403ms | `04b-search-downloads.log` |
| 4c | `… --sort downloads --limit 3 --offset 3` | 4~6위가 이어짐 | 43.9M → 22.3M → 20.0M (중복 없음, 내림차순 연속) | 0 | 384ms | `04c-search-offset.log` |
| 4d | `search zzqqxxnonexistentmodzz` | 빈 결과 경로 | `검색 결과 0개 중 0개`, 패닉·에러 없음 | 0 | 371ms | `04d-search-empty.log` |
| 4e | `search … --provider curseforge` (키 없음) | D2: 키 없으면 명확한 안내 | `error: CurseForge API 키가 없습니다 — 설정 화면의 CurseForge API 키에 값을 입력하세요 (console.curseforge.com)` | **1** | 109ms | `04e-search-cf-nokey.log` |
| 5a | `mods install sodium` | R1: 검증 후 `fabricmods/required`에 배치, 인덱스 기록 | `sodium-fabric-0.5.13+mc1.20.1.jar` → `fabricmods/required/`; `modindex.json`에 sha1/sha512/md5 3종, `namespace: required` 기록. Sodium은 필수 의존성이 없어 계획에 의존성 행 없음 | 0 | 1072ms | `05a-install-sodium.log` |
| 5b | `mods install sodium-extra` | D6: 필수 의존성 재귀 해결, 이미 설치된 것은 `설치됨`으로 표기하되 숨기지 않음 | 계획에 `필수 의존성 Sodium (설치됨: sodium-fabric-…jar)` / `필수 의존성 Fabric API` / 선택 의존성 2건. 결과 `설치 2 · 건너뜀 1`; `fabric-api-0.92.12+1.20.1.jar` 동반 설치 | 0 | 4114ms | `05b-install-sodium-extra-deps.log` |
| 5c | `mods install sodium` (재설치) | 중복 금지, 같은 이름은 교체 | 디렉터리·인덱스 모두 sodium 1개, 3파일 유지 | 0 | 632ms | `05c-install-sodium-again.log` |
| 6 | `mods list` | §8.3 행 구성 | 3행, `파일명 / 제목 / 네임스페이스`, 마커 없음 | 0 | 78ms | `06-list-after-install.log` |
| 7a | `mods install sodium --version mhZtY2lR` | 다운그레이드도 3단계 교체 | `0.5.8`로 교체, 디렉터리에 sodium jar **1개** | 0 | 614ms | `07a-install-older-version.log` |
| 7b | `mods update` | 후보만 표시 | `업데이트 1개 / sodium-fabric-0.5.8…jar : mhZtY2lR → mc1.20.1-0.5.13-fabric (정식)` + `--apply를 붙이면 설치합니다` | 0 | 334ms | `07b-update-dryrun.log` |
| 7c | `mods update --apply` (+ 동시 디렉터리 폴링) | 어느 순간에도 sodium jar 2개가 보이면 안 됨 | `업데이트 1 · 실패 0`. 실행 중 **167회 폴링, 관측된 최대 sodium jar 수 = 1** | 0 | 729ms | `07c-update-apply.log` |
| 8a–c | `mods move … optionaloff` → `list` → `… required` | 네임스페이스 이동 = 활성/비활성 (`.disabled` 없음) | 파일이 `optionaloff/`로 이동, `list`의 네임스페이스 열이 따라 바뀜, 원복도 정상 | 0 | 44/68/36ms | `08a`–`08c-*.log` |
| 9a | `generate distro` (`modindex.json` + `.nebula-tmp` + `.nebula-lock` 존재) | R5 | `12 artifacts, 0 errors` | 0 | 1041ms | `09a-distro-with-catalog.log` |
| 9b | 세 파일을 서버 밖으로 옮기고 `generate distro` | 위와 **바이트 동일** | 아래 "R5 불변식" 절 — sha256 동일, `fc /b` "no differences" | 0 | 74ms | `09b-…`, `09c-distro-diff.log` |
| 9d/e | `validate` / `validate --strict` | 에러 0 | 둘 다 `12 artifacts, 12 with SHA-256, 0 errors, 0 warnings` | 0 | 35/18ms | `09d`, `09e-*.log` |
| 10a | jar 뒤에 64바이트를 덧붙이고 `list` | D1: 조정(reconcile)은 **읽기 전용** | sha1이 바뀌어도 행은 정상. 해시 재검증은 하지 않음 — 아래 "관찰" 절 | 0 | 81ms | `10a-list-hash-drift.log` |
| 10b | jar를 4096바이트로 절단하고 `list` | R9/§8.3: `배포 불가` + 수정 안내 | `fabric-api-… required 배포 불가` + `배포본을 만들 수 없는 파일입니다 (…: invalid Zip archive: Could not find EOCD) — 지우거나 올바른 jar로 바꾸세요`. 다른 2개 행은 정상(한 파일이 목록 전체를 깨뜨리지 않음) | 0 | 77ms | `10b-list-corrupt-jar.log` |
| 10c | 그 상태로 `generate distro` | R9의 전제(스캐너는 중단) | `error: … invalid Zip archive: Could not find EOCD`, 배포본 미생성 | **1** | 53ms | `10c-distro-aborts-on-corrupt.log` |
| 11a | `required/notes.txt`를 손으로 두고 `list` | §8.3: `배포 불가` + `직접 추가` | `notes.txt … 배포 불가 · 직접 추가` + `(the name does not end in .jar)` | 0 | 85ms | `11a-list-nonjar.log` |
| 11b | `mods remove notes.txt` | jar가 아니어도 삭제 가능(`locate`는 R6를 적용하지 않음) | `notes.txt을(를) 삭제했습니다`, 파일 제거됨 | 0 | 23ms | `11b-remove-nonjar.log` |
| 11c | 관리 jar를 손으로 지우고 `list` | §8.3 `파일 없음`, 항목은 남음 | `sodium-extra-… required 파일 없음`; 인덱스 항목 유지 | 0 | 75ms | `11c-list-missing-file.log` |
| 11d | `mods remove nosuchmod.jar` | 명확한 거부 | `error: nosuchmod.jar을(를) 찾을 수 없습니다` | **1** | 23ms | `11d-remove-unknown.log` |
| 12a | 신선한 `.nebula-lock`(pid 424242) 후 `mods move` | R6: 깨끗한 거부 | `error: 다른 Nebula 작업이 이 서버를 사용 중입니다 (pid 424242)` | **1** | 35ms | `12a-lock-fresh-move.log` |
| 12b | 같은 잠금에서 `mods install cloth-config` | 동일 | 계획은 출력되나 apply가 거부, 파일 미생성 | **1** | 782ms | `12b-lock-fresh-install.log` |
| 12c | 같은 잠금에서 `mods remove` | 동일 | 동일한 거부. **거부 후 잠금 파일은 그대로 남아 있음**(실수로 깨지지 않음) | **1** | 24ms | `12c-lock-fresh-remove.log` |
| 12d | 같은 잠금에서 `mods list` | 읽기 전용은 잠금과 무관 | 4행 정상 출력 | 0 | 84ms | `12d-lock-fresh-list-readonly.log` |
| 12e | `started_at`을 40분 전으로 바꾸고 `-v mods move` | R6: 30분 초과 잠금은 깨짐 + 로그 | `[WARN] Breaking a stale Nebula lock at … (pid Some(424242))` 후 이동 성공, 잠금 파일 소멸 | 0 | 42ms | `12e-lock-stale-broken.log` |
| 12f/z | `mods move … required` (상태 원복) | — | 12e/12a 시험 후 네임스페이스 원복 | 0 | 25/42ms | `12f-move-back.log`, `12z-move-back.log` |
| 13a | `server duplicate Fab-1.20.1 Fab2` | `.partial` 잔존 금지, `.nebula-tmp`/`.nebula-lock` 미복사 | `Fab2-1.20.1` 생성. `servers/`에 `.partial` 없음. 사본에 `.nebula-tmp`/`.nebula-lock` 없음, jar 4개 + `modindex.json` 복사됨 | 0 | 45ms | `13a-server-duplicate.log` |
| 13b | 서버 2개로 `generate distro` | id 충돌 없음 | `servers[0].id Fab-1.20.1`, `servers[1].id Fab2-1.20.1`, 26 artifacts, 0 errors. **단, description은 원본 id를 유지 — 결함 4** | 0 | 152ms | `13b-distro-two-servers.log` |
| 14a | `server delete Fab2-1.20.1` (`--yes` 없음) | 거부 + 비영 종료 | 삭제될 경로 2줄 출력 후 `error: … 정말 지우려면 --yes를 붙이세요`, 폴더 잔존 | **1** | 36ms | `14a-delete-no-yes.log` |
| 14b/c | `server delete .` / `..` `--yes` | 한 단계 폴더만 허용 | `error: 서버 폴더 .을(를) 쓸 수 없습니다 — 이름은 한 단계 폴더여야 합니다` (`..`도 동일) | **1** | 21ms | `14b`, `14c-*.log` |
| 14e | `server delete "Tmp-1.20.1 "` (`--yes` 없음, 직접 호출) | 후행 공백이 argv까지 살아 있어야 유효한 시험 | argv `<Tmp-1.20.1 >` 확인, 에러 문구도 `Tmp-1.20.1 을(를)`로 공백 포함 | **1** | 20ms | `14e-delete-trailing-space-no-yes.log` |
| 14f | `server delete "Tmp-1.20.1 " --yes` | **반드시 거부** (Windows canonicalize가 후행 공백을 지우므로 이름 재확인이 필요) | `error: unsafe path: …\Tmp-1.20.1 is not a server directory of …\servers` — 폴더 그대로 남음 | **1** | 20ms | `14f-delete-trailing-space-yes.log` |
| 14g | `server delete "..\..\evil" --yes` | 경로 탈출 거부 | `error: 서버 폴더 ..\..\evil을(를) 찾을 수 없습니다 — 있는 서버: …` | **1** | 22ms | `14g-delete-path-escape.log` |
| 14h | `.Ghost-1.20.1.partial`를 두고 `-v server duplicate` | 중단된 복제 청소 | `[WARN] Removing an interrupted duplicate: .Ghost-1.20.1.partial` 후 정상 복제 | 0 | 46ms | `14h-duplicate-sweeps-partial.log` |
| 14i | 기존 이름으로 `server duplicate` | 충돌 거부 | `error: 서버 폴더 Fab3-1.20.1이(가) 이미 있습니다 — …` | **1** | 34ms | `14i-duplicate-existing.log` |
| 14j–l | `server delete … --yes` ×3 | 삭제 성공 | Fab2/Fab3/Tmp 전부 삭제, `servers/`에 `Fab-1.20.1`만 잔존 | 0 | 24ms ×3 | `14j`–`14l-*.log` |
| 15a | `generate server Neo 1.21.1 --neoforge latest` | NeoForge 서버 (생성 시점엔 인스톨러 미실행) | `neoforge 21.1.250`, `neoforgemods/` 컨테이너 | 0 | 768ms | `15a-gen-server-neoforge.log` |
| 15b/c | `mods Neo-1.21.1 search jei` → `install jei` | 로더별 컨테이너 (`server::namespace_dir`) | `jei-1.21.1-neoforge-19.51.0.418.jar` → `neoforgemods/required/` | 0 | 320/858ms | `15b`, `15c-*.log` |
| 16a | `generate distro` (Java 미지정) | 명확한 안내 | `error: JAVA_EXECUTABLE이 설정되지 않았습니다 — 서버 Neo-1.21.1의 NeoForge를 처리하려면 …` | **1** | 82ms | `16a-distro-final.log` |
| 16b | `validate --strict` (16a 실패 직후) | 참고 | `26 artifacts … 0 errors` — 16a가 실패해 **디스크에 남아 있던 이전(서버 2개 Fabric) 파일**을 검증한 것입니다. 최종본 검증은 16e | 0 | 20ms | `16b-validate-final.log` |
| 16c | `JAVA_EXECUTABLE` 지정 후 `generate distro` | 인스톨러 headless 실행 | `66 artifacts, 66 with SHA-256, 0 errors, 1 warnings` (warning은 기존 NeoForge 런처 안내) | 0 | 30.0s | `16c-distro-neoforge-java.log` |
| 16d | 모든 단계 종료 후 `generate distro` 재실행 | NeoForge 캐시 재사용 | `67 artifacts, 67 with SHA-256, 0 errors, 1 warnings` (`Using cached results at …/cache/neoforge/21.1.250`) | 0 | — | `16d-distro-regen-final.log` |
| 16e | **최종 산출물에 `validate --strict`** | 에러 0 | `67 artifacts, 67 with SHA-256, 0 errors, 1 warnings` | 0 | 26ms | `16e-validate-final-strict.log` |
| 17 | `verify-launcher-contract.ps1` | 런처 본체 파서가 카탈로그 설치 모드를 포함한 산출물을 수용 | `servers: 2 / modules: 66 / FabricMod=4, NeoForgeMod=1 / digests: SHA256=66 / parse: OK` | 0 | 11.5s | `17-launcher-contract.log` |
| 18a | `mods install zzz-no-such-project-zzz` | 없는 프로젝트의 명확한 거부 | `error: HTTP : not found` — **결함 2** | **1** | 422ms | `18a-install-unknown-project.log` |
| 18b | `mods install cloth-config --namespace optionalon` | 지정한 네임스페이스로 | `fabricmods/optionalon/cloth-config-11.1.136-fabric.jar` | 0 | 1095ms | `18b-install-namespace-optionalon.log` |
| 18c | `mods Neo-1.21.1 update` | D3: 정식이 최신이면 "모두 최신" | `업데이트 1개 … → 19.54.0.427 (베타)` — **결함 1** | 0 | 314ms | `18c-update-uptodate.log` |
| 19a/b | 손으로 둔 jar를 `mods move` | 인덱스 없는 파일도 이동 가능 | 이동 성공, `list`에 `직접 추가` 마커 유지 | 0 | 37/135ms | `19a`, `19b-*.log` |
| 19c/d | `mods remove fabric-api-…jar` (sodium-extra가 의존) | §8.3/D11: 의존 모드 경고 | **경고도 확인도 없이 삭제, 종료 0** — 결함 3 | 0 | 23/106ms | `19c`, `19d-*.log` |
| 20a | `mods install sodium-extra --no-deps` | §8.4: 필수 의존성 제외 시 경고 문구 | 계획에는 `필수 의존성 Fabric API`가 보이지만 실행 시 경고 없음 — 결함 5 | 0 | 2517ms | `20a-install-no-deps.log` |
| 99 | `cargo test --workspace` | 전부 통과 | **314 passed, 0 failed, 1 ignored** (12개 타깃 합계 315건) | 0 | 183.5s | `99-cargo-test.log` |

## R5 불변식: `distribution.json`은 카탈로그 파일에 영향을 받지 않는다

"카탈로그 파일이 있기 전/후"를 문자 그대로 "모드 설치 전/후"로 읽으면 안 됩니다 — 모드를 설치하면
모듈이 정당하게 늘어나기 때문입니다. R5가 말하는 것은 **카탈로그 자체의 산출물이 배포본으로 새지
않는다**는 것이므로 다음과 같이 검증했습니다.

1. 모드 3개를 설치한 상태에서 `<server>/modindex.json`, `<server>/.nebula-tmp/`,
   `<server>/.nebula-lock`(합성)을 모두 둔 채 `generate distro` → **A**
2. 세 파일/디렉터리를 서버 폴더 밖으로 옮기고 `generate distro` → **B**
3. 비교 후 원복

```
A sha256=E301357FA5E95B79DFE57A1326C6D4D6CAB0940A3E322F33B2A074CBFEA99412 bytes=6835
B sha256=E301357FA5E95B79DFE57A1326C6D4D6CAB0940A3E322F33B2A074CBFEA99412 bytes=6835
IDENTICAL=True
FC: no differences encountered
```

증거 `09c-distro-diff.log`, 산출물 사본 `evidence/distribution-A.json` / `distribution-B.json`.
`validate`와 `validate --strict` 모두 `0 errors`(`09d`, `09e`).
런처 계약 하네스도 카탈로그 설치 모드 5개(FabricMod 4 + NeoForgeMod 1)를 포함한 최종 산출물을
`parse: OK`로 수용했습니다(`17-launcher-contract.log`).

## "sodium jar 두 개" 불변식을 어떻게 확인했는가

사후 디렉터리 목록만으로는 순간 불변식을 증명할 수 없으므로, `mods update --apply`를 자식
프로세스로 띄우고 부모에서 `fabricmods/required`를 **바쁜 루프로 폴링**했습니다.
729ms 동안 **167회 표본**, 관측된 `sodium-fabric-*.jar` 개수의 최댓값은 **1**이었습니다
(`07c-update-apply.log`의 `POLL:` 줄).

한계를 분명히 해 둡니다: 폴링은 표본이므로 "0개인 찰나"도 포착하지 못했습니다. 즉 이 증거는
**2개가 동시에 보인 적은 없다**를 표본 수준에서 뒷받침할 뿐, 순간 원자성을 형식적으로 증명하지는
않습니다. 순서 자체(`update.rs::replace_file_locked`의 검증 → 구파일 반출 → 신파일 반입 → 인덱스
갱신)는 코드에서 확인했습니다.

## 결함

### 결함 1 (high) — 업데이트 확인이 제공자에 따라 다르게 동작하고, Modrinth 쪽은 정식 설치본을 베타로 올린다

핵심은 **내부 비대칭**입니다. 같은 사용자 행동(`mods update`)의 두 제공자 분기가 다릅니다.

- CurseForge: `catalog/curseforge.rs:301` `latest_file` → `preferred_version(&versions)` 적용
- Modrinth: `catalog/modrinth.rs:240` `updates_by_hashes` → `/v2/version_files/update` 응답을
  매핑만 하고 `preferred_version`을 **부르지 않음**

`preferred_version`(`catalog/mod.rs:310`)은 "정식 → 베타 → 알파" 순서를 구현한 D3의 함수이고,
주석에도 `D3:`라고 적혀 있습니다. 설치 경로는 이 함수를 4곳에서 호출하는데
(`install.rs:64,110,357`, `curseforge.rs:301`) Modrinth 업데이트 경로만 빠져 있습니다.
D7이 업데이트 확인의 요청 방식만 규정하고 릴리스 등급을 다시 언급하지 않는 것은 사실이지만,
두 분기가 같은 규칙을 공유하도록 만들어졌다는 것은 CurseForge 쪽 코드 자체가 증거입니다.

실측(`18c-update-uptodate.log`):

- `Neo-1.21.1`에 설치된 JEI = `19.51.0.418`, Modrinth `version_type = release`.
- 1.21.1 / NeoForge에 해당하는 JEI 버전 207건 중 release는 2건이며 그중 최신이 바로 `19.51.0.418`.
  즉 **D3 기준으로는 이미 최신이므로 "모두 최신입니다"가 정답**입니다.
- 실제 출력: `업데이트 1개 / jei-…-19.51.0.418.jar : ufHUqt9b → 19.54.0.427 (베타)`.

`mods update --apply`는 후보 전체를 추가 확인 없이 설치하므로, 스크립트 프로비저닝에서
정식 릴리스가 조용히 베타로 교체됩니다. D3의 근거가 "프로비저닝 도구가 알파를 몰래 배포해서는
안 된다"인 만큼 방향이 어긋납니다.

제안: `check_updates`의 Modrinth 분기에서 후보의 `release`가 설치본의 릴리스 등급보다 낮으면
후보에서 제외하거나(또는 `preferred_version` 동등 규칙을 적용하고), 최소한 `--apply`에
`--allow-prerelease` 같은 옵트인을 두는 것.

### 결함 2 (medium) — `Error::Http`가 빈 url로 렌더링되어 `HTTP : not found`가 출력된다

`catalog/mod.rs:549`의 `From<CatalogError> for Error`는 `NotFound` / `RateLimited` /
`HashMismatch` / `Blocked`를 전부 `Error::Http { url: String::new(), message }`로 감쌉니다.
`Error::Http`의 Display는 `error.rs:15`의 `"HTTP {url}: {message}"`이므로, url이 비면
콜론이 허공에 뜬 문장이 나옵니다.

관찰(`18a-install-unknown-project.log`):

```
$ nebula mods Fab-1.20.1 install zzz-no-such-project-zzz
error: HTTP : not found
```

같은 경로로 `RateLimited`는 `error: HTTP : rate limited; retry in 60s`,
`HashMismatch`는 `error: HTTP : <file>: expected …, downloaded …`가 됩니다. CLI의 다른 모든
에러가 완성된 한국어 문장인 것과 대비되고, DESIGN-v2 §8.4가 변종별 문구를 요구하는 것과도 어긋납니다.

제안: 이 네 변종을 `Error::Http`로 감싸지 말고 각자의 한국어 문장으로 매핑할 것
(`NotFound` → `프로젝트를 찾을 수 없습니다`, `RateLimited` → `요청이 너무 많습니다 — N초 후
다시 시도하세요` 등). 최소 수정만 원한다면 `Error::Http`의 Display가 url이 빌 때 `"{message}"`만
쓰도록 고치는 것으로도 문장은 복구됩니다.

### 결함 3 (medium) — `mods remove`가 다른 모드의 필수 의존성을 지우면서 의존 모드를 알리지 않는다

`modindex.json`은 의존성을 정확히 기록합니다:

```
sodium-extra-0.5.9+mc1.20.1.jar: deps=[AANobbMI, P7dR8mSH]   ← Sodium, Fabric API
```

그럼에도 `catalog/manage.rs::remove_mod`는 이 `dependencies`를 전혀 보지 않습니다.
관찰(`19c-remove-dependency-no-warning.log`):

```
$ nebula mods Fab-1.20.1 remove fabric-api-0.92.12+1.20.1.jar
fabric-api-0.92.12+1.20.1.jar을(를) 삭제했습니다
--- exit=0
```

DESIGN-v2 §8.3의 `mod-delete` 다이얼로그는 이 경우 `fabric-api를 필요로 하는 모드: Sodium,
Lithium` 형태의 경고 Callout을 띄우도록 명시하고, 인덱스에는 그 정보가 실제로 들어 있습니다.
CLI에는 그 경고가 아예 없습니다. 배포본은 정상 생성되므로(런타임에만 깨짐) 실수가 배포 이후에야
드러납니다.

지적 범위를 분명히 해 둡니다: plan의 CLI 절은 `… remove <file>`만 규정하고 `--yes`는
`server delete`에만 붙이므로, "`remove`에 `--yes`가 없다"는 것 자체는 규정 위반이 아닙니다.
결함은 **의존 모드 경고의 부재**입니다.

제안: `remove_mod` 호출 전에 인덱스의 `dependencies`를 역참조해 의존 모드를 출력할 것.
(선택) 의존 모드가 있을 때만 `--yes`를 요구하는 게이트를 추가하는 것도 검토할 만합니다.

### 결함 4 (low) — 복제한 서버가 `meta.description`에 원본 서버 id를 남기고, 그대로 `distribution.json`에 나간다

`catalog/manage.rs::rename_copy`는 "새 서버와 같은 모양"을 만든다고 주석에 적어 두고
`meta.meta.name`만 고칩니다. 그러나 새 서버의 기본 description은 `meta.rs:119`의
`format!("{id} Running Minecraft {mc_version}")`이므로 id가 들어갑니다.

관찰(`13a`, `13b-distro-two-servers.log`):

```
servers[1].id          = Fab2-1.20.1
servers[1].name        = Fab2 (Minecraft 1.20.1)
servers[1].description = Fab Running Minecraft 1.20.1 (Fabric v0.19.5)   ← 원본 id
```

런처 서버 목록에 그대로 노출되는 사용자 가시 문자열입니다. id 충돌은 없으므로(서버 id는 폴더
이름에서 옴) 심각도는 낮습니다.

제안: `rename_copy`에서 description이 `"{원본 id} Running Minecraft"`로 시작하면 새 id로
같이 치환할 것.

### 결함 5 (low) — `--no-deps`가 실행 시점 경고를 내지 않는다

`mods install sodium-extra --no-deps`는 계획에 `필수 의존성 Fabric API`를 출력한 뒤 그대로
단독 설치하고 끝납니다(`20a-install-no-deps.log`, 종료 0, 경고 0줄).
DESIGN-v2 §8.4는 GUI에서 필수 의존성 체크를 해제하면 `필수 의존성을 빼면 게임이 시작되지 않을
수 있습니다`를 행 아래에 띄우도록 규정합니다. `--help` 문구에만 같은 취지가 있고 실행 출력에는
없습니다.

제안: `--no-deps`로 제외된 필수 의존성이 있을 때 같은 문장을 stderr에 한 줄 출력할 것.

### 결함 6 (nit) — 여러 줄짜리 제공자 설명이 검색 결과 들여쓰기를 깨뜨린다

`cli.rs:777`은 `println!("      {}", hit.description.trim())`으로 설명을 그대로 찍습니다.
Modrinth 설명에 개행이 있으면 둘째 줄부터 들여쓰기가 사라져 다음 결과 행과 섞입니다
(`15b-neo-search.log`의 `cobbledex-rei-emi-jei` 행). 제안: 첫 줄만 쓰거나
`description.lines().next()`로 자를 것(§8.4의 결과 행도 설명 1줄입니다).

### 결함 7 (nit) — 후행 공백 삭제 거부 메시지가 실제 원인을 가린다

`14f`의 거부 문구는 `… \Tmp-1.20.1 is not a server directory of …\servers`인데, 부모는
정확히 그 `servers`이고 실제 원인은 **이름 불일치**(후행 공백)입니다. 영어 문장이라는 점도
다른 거부 문구들과 다릅니다. 안전 동작 자체는 정확하므로 문구만의 문제입니다.
제안: 이름 재확인 실패는 별도 문장으로(`서버 폴더 이름이 정확히 일치하지 않습니다: "{입력}"`).

## 관찰 (결함 아님)

- **`list`는 설치된 jar의 해시를 재검증하지 않습니다.** 설치본에 64바이트를 덧붙여 sha1을
  바꿔도 행은 정상으로 표시됩니다(`10a-list-hash-drift.log`). 이는 D1이 "조정은 읽기 전용"으로
  명시적으로 설계한 결과이고, 스펙 어디에도 목록이 다이제스트를 검증한다는 요구가 없으므로
  결함으로 보고하지 않습니다. 다만 결과적으로 `mods update`도 디스크가 아니라 **인덱스의**
  해시를 기준으로 판단하므로(R12), 손으로 바꿔치기한 jar는 낡은 출처를 계속 달고 다닙니다.
- **`--version <id>`에 넣을 버전 id를 CLI만으로는 알아낼 수 없습니다.** GUI에는
  `version-picker`가 있지만 CLI에는 버전 나열 명령이 없어, 이 검증에서도 Modrinth API를 직접
  호출해 id를 얻어야 했습니다. D11의 패리티는 "변경(mutation)"에 대한 것이라 규정 위반은
  아니지만, 문서화된 플래그가 외부 도구 없이는 쓰이지 못합니다. `mods <server> versions <project>`
  한 줄이면 메워집니다.
- **진행 표시는 stderr, 결과는 stdout으로 갑니다.** 증거 로그에서 `다운로드 중 1/1 …`이
  `설치 1 · 건너뜀 0 …` 뒤에 찍히는 것은 로그 병합 순서 때문이며 제품 동작이 아닙니다
  (`cli.rs:963` `progress_line`이 `stderr`에 씁니다). 파이프라인 친화적인 올바른 분리입니다.
- **잠금 거부 후에도 잠금 파일은 보존됩니다**(`12c`). 남의 잠금을 지우지 않는다는 뜻이라 옳습니다.
- **`.nebula-tmp/`는 설치 후 빈 부모 디렉터리로 남습니다**(스테이징 하위 디렉터리는 정리됨).
  R5에 따라 어떤 스캐너도 읽지 않고, 실제로 배포본 바이트에 영향이 없음을 위에서 확인했습니다.
- **워크스페이스 테스트 315건 중 314 통과 / 1 무시 / 0 실패**. phase C 노트의 "297 → 314"와
  일치합니다(무시 1건은 `NEBULA_NET_TESTS` 게이트).

## 미수행 / 한계

- **CurseForge 검색·설치·업데이트: 미수행 — 키 없음.** 셸에 `NEBULA_CURSEFORGE_API_KEY`가
  설정돼 있지 않아, D2가 규정한 "키 없음" 경로만 검증했습니다(`04e`, 정확한 한국어 안내 + 종료 1).
  따라서 R2(리다이렉트 후에도 `x-api-key` 유지), R3/R4(차단된 파일의 수동 다운로드 경로),
  R7(250ms 스로틀), D7의 CurseForge 업데이트 분기는 이번 회차에서 **실측하지 못했습니다**.
- **잠금 경합은 합성 잠금 파일로만 재현**했습니다(실제 두 프로세스 동시 실행 아님).
  `create_new(true)` 기반이므로 관측된 거부/파기 동작은 동일한 코드 경로입니다.
- **12a/12b의 1차 시도는 폐기했습니다.** 첫 시도에서 PowerShell `Set-Content -Encoding` 오류로
  잠금 파일이 쓰이지 않아, 두 명령이 **잠금 없이 정상 실행**됐습니다(네임스페이스 이동 1회 +
  `lithium` 설치 1회). 해당 로그 2개를 지우고 잠금을 제대로 쓴 뒤 재실행한 것이 표의 12a/12b입니다.
  12d 이후 목록에 보이는 `lithium-fabric-mc1.20.1-0.11.4.jar`은 그 1차 시도의 산물이며,
  표에 대응하는 설치 단계가 없는 이유가 이것입니다(실제 Modrinth 설치 호출 총 10회).
- **후행 공백 시험에는 별도 러너를 썼습니다.** `Start-Process -ArgumentList`는 인자의 후행
  공백을 지워 버려 14e/14f가 무의미해집니다. 호출 연산자(`& $exe @args`)로 실행하는
  `run-direct.ps1`을 만들어 argv에 공백이 살아 있음을 먼저 확인한 뒤 시험했습니다.
  14f는 지워져도 무해한 사본(`Tmp-1.20.1`)을 대상으로 수행했습니다.
- **순간 원자성**은 위에 적은 대로 폴링 표본 수준의 증거입니다.
- **`generate server-import` / `publish` / `gdrive-auth`** 등 모드 카탈로그 밖 명령은 이번
  레인의 범위가 아니며 2026-09-10 보고서에서 다룹니다.
- 런처 계약 하네스는 `H:\HambuguiLauncherRust`를 **읽기 전용으로 복사**해 임시 워크스페이스에서
  빌드합니다(스크립트 설계). 해당 체크아웃은 수정하지 않았습니다.
