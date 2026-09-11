# Nebula Rust 포트 — 최종 검증 보고서 (2026-09-10)

브랜치 `worktree-rust-port` (HEAD `778277f`, upstream `dscalzi/Nebula@7ffc978` 기준 45개 커밋). master로 fast-forward 병합.

## 범위 요약

| 항목 | 결과 |
|---|---|
| Node/TypeScript 제거 | `src/`, `package*.json`, `tsconfig`, `eslint`, `.nvmrc`, `.vscode`, `FUNDING.yml` 삭제. Cargo 워크스페이스 2 크레이트(`nebula-core`, `nebula`). |
| 툴체인 | Rust 1.97.0 고정, Slint 1.17.1 고정 — Hambugui Launcher와 동일. |
| 기능 포팅 | init root, generate server/distro/schemas, Forge FG2·FG3(설치기 헤드리스 `--installClient=`), Fabric, NeoForge(신규), CurseForge/Modrinth/Prism 가져오기(신규 자동 감지), validate(신규 구현), publish WebDAV/Google Drive(신규), latest/recommended 조회. |
| 런처 호환 | 모든 아티팩트 MD5+SHA-256, 깨끗한 상대 경로, 단일 세그먼트 서버 id; `hambugui-distribution` 하네스로 parse+runtime OK (골든, 라이브러리 id, FG2 1.7.10, 5서버 통합 루트 152 모듈). NeoForge 타입은 런처 패치(`docs/launcher/`) 적용 후 수용. |
| GUI | 3:4 세로(720×960) "Quiet Studio" 디자인 시스템 v2: 내비 레일, 카드, 스위치, 세그먼트, 다이얼로그, 토스트, 검증 배너; Pretendard 번들, Lucide 아이콘; 라이트/다크; 키보드(Ctrl+1..5, Esc, Enter); AutomationId 계약. |
| 문서 | README(한국어), `SYNOLOGY_DEPLOY_GUIDE.md`, `docs/launcher/README.md`, `docs/plans/*`(계획·디자인·결정 기록), 검증 보고서 3종. |

## 품질 게이트 (HEAD 778277f)

| 게이트 | 결과 |
|---|---|
| `cargo fmt --all -- --check` | 통과 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |
| `cargo test --workspace` | 216 통과 / 0 실패 / 1 ignored(Java 필요) |
| `cargo deny check licenses bans sources advisories` | 모두 ok |
| 네트워크 테스트 `NEBULA_NET_TESTS=1` | Fabric 1.20.1, Forge 1.12.2(FG3 무설치기), Forge 1.7.10(FG2 plain jar), NeoForge 21.1.250 — 4/4 통과 |
| 릴리스 바이너리 | `target/release/nebula.exe` 34.1 MB (Pretendard 3종 6.2 MB 포함) |

## 리뷰·QA 이력

1. 계획 Critic 검토 → 사실 오류 12건 정정 (`docs/plans/2026-09-10-rust-port.md`).
2. 코어 코드 리뷰 17건 → 전부 수정 (`docs/plans/2026-09-10-review-fixes.md`): FG2 pack.xz → plain jar(릴리스 블로커), 서버 id 검증, 검증기 런처 패리티 5건, 설치기 폴백 플래그, 성능/단순화.
3. UX 휴리스틱 감사 41건 → 결정·수정 (`docs/plans/2026-09-10-ux-fixes.md`), 이후 전면 재설계(DESIGN-v2).
4. GUI 크레이트 품질 리뷰 25건 + GUI 실기 QA 결함 11건 → 전부 수정 (`docs/plans/2026-09-10-gui-review-fixes.md`).
5. CLI 실전 QA 17단계 (`docs/verification/2026-09-10-cli-qa.md`): 기능 결함 0. 
6. GUI 실기 QA 13 시나리오, UIA 구동 (`docs/verification/gui-qa/2026-09-10-gui-qa.md`): 빌드→검증→업로드 체인 통과.

## 의도된 차이(원본 대비)

BASE_URL 후행 슬래시 강제 · 모드 정렬 collation(소문자 바이트 순) · 정렬된 디렉터리 스캔 · `test` 명령 제거 · Forge 설치기 헤드리스 우선(GUI 폴백은 터미널/GUI에서만) · log4j 경고 15초 대기 제거 · FG2는 pack.xz 대신 jar 배포 · untracked 파일도 다이제스트 유지(`--helios-untracked`로 원본 동작) · 서버 id 단일 세그먼트.

## 수용된 한계 / 미검증

- Slint 1.17: ScrollView 밖 컨트롤은 UIA 트리·Tab 순환에서 제외(PageUp/PageDown 완화), `accessible-description` 미노출(오류를 라벨에 병합), 회전 애니메이션 미지원, 라이브 리전 없음.
- 작업 취소 없음(설치기 10분 타임아웃, Drive 로그인 5분 타임아웃).
- Google Drive 실제 업로드/OAuth: 계정 필요, 미검증(순수 로직만 단위 테스트).
- 스크린 리더 실사용, macOS/Linux 실기, CurseForge `files[]` 실다운로드, Claritas 성공 경로(실제 모드 jar): 미검증.
- Hambugui 런처의 NeoForge 실기 런칭: 런처 저장소 몫(패치 전달 완료).

## 실행 방법

```
cargo build --release -p nebula
target\release\nebula.exe            # GUI
target\release\nebula.exe --help     # CLI
NEBULA_NET_TESTS=1 JAVA_EXECUTABLE=... cargo test -p nebula-core --test resolver_net -- --test-threads=1
pwsh scripts\verify-launcher-contract.ps1 -Distribution <root>\distribution.json -Runtime
```

## 2026-09-12 — 모드 카탈로그

이 최종 보고서 작성(2026-09-10) 이후 추가된 기능. 별도 계획/QA 문서 체인으로
진행했으며, 이 섹션은 그 요약이다.

### 범위

Prism Launcher류 모드 브라우저를 GUI 모드 페이지와 CLI `mods`/`server` 명령으로
추가했다. 제공자는 Modrinth(키 불필요)와 CurseForge(사용자 자신의 API 키 필요).
설치/업데이트/삭제/네임스페이스 이동을 관리 색인(`modindex.json`)으로 추적하며,
`distribution.json` 생성에는 전혀 관여하지 않는다.

### 검증 체인

1. Critic 패널의 병합 설계 검토(`docs/plans/2026-09-11-mod-catalog-design-review.md`,
   P0 2건 + P1 14건) → 구현.
2. 코드 리뷰 12건(F1–F12) — 전부 수정(`docs/plans/2026-09-11-mod-catalog.md` "Fix notes").
3. CLI 실전 QA 63단계, 결함 7건 → GUI 실전 QA 14 시나리오, 결함 7건.
4. 두 QA 레인의 결함을 전부 수정 → 재-QA 18개 항목 전부 PASS(18/18), 회귀 없음
   (`docs/verification/2026-09-11-mods-reqa.md`).
5. CurseForge 전용 QA — CLI 34단계 + GUI 6 시나리오, 결함 8건(high 1·medium 4·low 3).
6. 여덟 결함을 전부 수정 → 재검증 8건 전부 PASS(8/8), 새로 발견된 결함 없음
   (`docs/verification/2026-09-12-mods-curseforge-reqa.md`).

### 최종 수치 (master `71cb52e`)

| 게이트 | 결과 |
|---|---|
| `cargo test --workspace` | 348 통과 / 0 실패 / 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |
| `cargo fmt --all -- --check` | 통과 |
| `cargo deny check licenses bans sources advisories` | 모두 ok |
| `distribution.json` 불변 | 카탈로그 파일(`modindex.json`/`.nebula-lock`/`.nebula-tmp`) 유무와 무관하게 바이트 동일(SHA-256 일치, `golden.rs` 회귀 테스트로 고정) |

### 보고서 링크

- CLI QA: [`docs/verification/2026-09-11-mods-cli-qa.md`](2026-09-11-mods-cli-qa.md)
- GUI QA: [`docs/verification/2026-09-11-mods-gui-qa.md`](2026-09-11-mods-gui-qa.md)
- 재-QA: [`docs/verification/2026-09-11-mods-reqa.md`](2026-09-11-mods-reqa.md)
- CurseForge QA: [`docs/verification/2026-09-11-mods-curseforge-qa.md`](2026-09-11-mods-curseforge-qa.md)
- CurseForge 재-QA: [`docs/verification/2026-09-12-mods-curseforge-reqa.md`](2026-09-12-mods-curseforge-reqa.md)

### 수용된 한계 / 미검증

- 429 응답 시 사용자에게 보이는 안내 문구 경로.
- 의존성이 걸린, 원저작자가 제3자 배포를 막은 CurseForge 파일(차단 파일)의 실다운로드
  경로.
- 베타/알파 경고 `Callout`.
- `PillStrip`의 로빙 포커스가 UIA에 노출되지 않는 것 — 접근성 상 받아들인 한계
  (§8.5의 `Group`+`SelectionPattern` 매핑은 정상이지만, 어느 pill에 링이 있는지
  알리는 UIA 신호는 없다).
