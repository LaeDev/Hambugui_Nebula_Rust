# 모드 카탈로그 GUI 실제 구동 QA — 2026-09-11

`nebula.exe gui`를 이 Windows 머신에서 실제로 띄우고 Windows UI Automation
(`System.Windows.Automation`)으로 조작한 결과다. 모든 판정은 실행 중인 창에서 읽은 UIA
값이거나 디스크에 남은 파일이며, 추정으로 채운 항목은 없다. 확인하지 못한 것은
§5 "검증하지 못한 것"에 그대로 적었다.

## 1. 대상과 환경

| 항목 | 값 |
|---|---|
| 커밋 | `761c8dd4ff230b6a86bddbe7d4b13bf6ab9ee082` (워크트리 `rust-port` HEAD) |
| 빌드 | `git archive HEAD` 스냅샷을 스크래치로 풀어 `cargo build --release -p nebula` (exit 0), `nebula.exe` 44,906,496 B |
| 툴체인 | Rust 1.97.0, Slint `=1.17.1` |
| OS | Windows 10 Pro 19045, DPI 96 (scale 1.0) |
| 창 | 736 × 999 (클라이언트 720 × 960) |
| 사양 문서 | `docs/plans/2026-09-11-mod-catalog.md` (계획 v1.1 + 구현 노트 A/B/C), `docs/plans/DESIGN-v2.md` §8 |

실행 환경변수 (매 시나리오 스크립트가 동일하게 설정):

```
ROOT=<scratch>\gui-qa\root
BASE_URL=https://cdn.example.invalid/
NEBULA_CONFIG_DIR=<scratch>\gui-qa\config
RUST_LOG=info
NEBULA_CURSEFORGE_API_KEY  (명시적으로 제거 — 아래 참조)
```

**CurseForge는 한 단계도 실행하지 않았다.** 이 셸에 `NEBULA_CURSEFORGE_API_KEY`가
설정되어 있지 않았고(값을 출력하지 않고 존재 여부만 확인), 스크립트가 앱을 띄우기 전
매번 이 변수를 제거한다(`mods-env.ps1`). 따라서 CurseForge 검색·설치·아이콘 경로는
모두 **미실행**이며, 시나리오 11은 "키 없음" 분기만 측정한다.

### 픽스처

`mods-setup-fixture.ps1`이 같은 빌드의 CLI로 만든다(`mods-setup.log`,
`mods-fixture-tree.txt`).

| 항목 | 값 |
|---|---|
| 서버 1 | `QAfab-1.20.1` — Fabric 0.19.5 (`generate server QAfab 1.20.1 --fabric latest`) |
| 서버 2 | `QAneo-1.21.1` — NeoForge 21.1.250 (`generate server QAneo 1.21.1 --neoforge latest`) |
| 직접 추가 jar | `cloth-config-15.0.140-neoforge.jar` — CLI로 `QAneo`에 설치한 뒤 그 jar를 `QAfab-1.20.1\fabricmods\optionaloff\`에 손으로 복사(인덱스 항목 없음). 복사 전 `ZipFile.OpenRead`로 `META-INF/neoforge.mods.toml`이 들어 있는 정상 zip임을 확인했다 |
| 배포 불가 파일 | `QAfab-1.20.1\fabricmods\required\notes.txt` (27 B) |

**직접 추가 jar를 fabric-api가 아니라 cloth-config로 둔 것은 의도적이다.** D6는 인덱스의
`(provider, project id)` 또는 `jarmeta`의 mod id로 "이미 설치됨"을 판정하므로, fabric-api를
손으로 놓으면 시나리오 5의 `install-confirm`에서 fabric-api가 **미리 체크된 스위치**가 아니라
**비활성 `설치됨` 행**으로 나타난다. 시나리오가 지정한 분기와 다른 분기를 재는 셈이라
Sodium이 의존하지 않는 모드를 골랐다.

### 드라이버

`docs/verification/gui-qa/drive.ps1`(2026-09-10 레인이 만든 것, 수정하지 않음)을
`mods-drive-mods.ps1`이 dot-source하고 다음을 더한다: `Find-ById` / `Find-ByIdPrefix` /
`Get-Ids`(AutomationId로 찾기 — 모드 화면은 전부 `accessible-id`를 달고 있어 한글 Name보다
안정적이다), `Get-Patterns`, `Get-Selected`(SelectionItemPattern.IsSelected),
`Get-LiveSetting`(UIA 30135), `Get-Texts` / `Has-Text` / `Find-TextLike`, `Focused-Desc`.

모든 스크립트는 UTF-8 **BOM 포함**이며 아래 형태로 실행했다(`-STA`가 없으면 `SendKeys`가
신뢰할 수 없다).

```
powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File mods-sNN.ps1
```

증거 파일은 모두 `docs/verification/gui-qa/` 아래에 있다. `mods-sNN-*.log`가 각 실행의
드라이버 로그, `mods-sNN-stdout.log.err`가 같은 실행의 앱 `RUST_LOG=info` 출력이다.

## 2. 시나리오별 결과

### 1. 실행 · Ctrl+3 · PillStrip

로그 `mods-s01-launch.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 앱이 뜨고 첫 화면은 빌드 | 창 736×999, 페이지 제목 `빌드`, `창을 띄웁니다: 0 ms` · `첫 스캔 반영: 185 ms` | 통과 | `mods-01-launch.png` |
| Ctrl+3이 모드 페이지에 닿는다 | 페이지 제목 `모드` | 통과 | `mods-01-ctrl3.png` |
| PillStrip이 radio-group으로 존재 | `mods-server` = UIA `ControlType.Group`, Name `서버`, SelectionPattern 보유. 자식 `mods-server-0` `QAfab-1.20.1` / `mods-server-1` `QAneo-1.21.1` = `ControlType.RadioButton` + SelectionItemPattern | 통과 (UIA에 `radio-group` 컨트롤 타입이 없어 `Group`+SelectionPattern이 accesskit의 정상 매핑) | `mods-tree-modspage.txt` |
| 탭 정지는 그룹 하나 | Tab 16회 순회: `nav-build … nav-settings`, `theme-toggle`, **`mods-server`(1회)**, `mods-view`, `mods-check-updates`, 행 셰브런 2개 → 다시 nav. 스트립 안쪽 pill은 정지가 아니다 | 통과 | `mods-s01-launch.log` |
| Right/Left가 선택을 바꾸지 않고 포커스 링만 옮긴다 | Right 전후 `IsSelected` = `mods-server-0=True, mods-server-1=False`로 동일. 화면에서는 pill 0 영역 diff 13.269, pill 1 영역 diff 13.269 → 링이 0에서 1로 이동. Left도 선택 불변 | 통과 | `mods-01-pill-ring-a.png`, `mods-01-pill-ring-b.png` |
| Enter가 선택한다 | Enter 후 `mods-server-1` 선택, 스코프 줄 `QAfab-1.20.1 · Minecraft 1.20.1 · Fabric 0.19.5` → `QAneo-1.21.1 · Minecraft 1.21.1 · NeoForge 21.1.250`, 설치됨 목록 재적재 | 통과 | `mods-01-pillstrip.png` |
| (관찰) 로빙 포커스가 UIA에 보이지 않는다 | Right 후 `AutomationElement.FocusedElement`는 여전히 `mods-server`(그룹). 어느 pill에 링이 있는지 알리는 UIA 신호가 없다 | **결함 D5** | 같은 로그 |

### 2. 설치됨 목록 — 직접 추가 / 배포 불가

로그 `mods-s02-installed.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 손으로 놓은 jar가 `직접 추가` | 행 `mods-row-cloth-config-15.0.140-neoforge.jar`, 제목 `Cloth Config v15 API`(jar 메타에서), 부제 `15.0.140 · cloth-config-15.0.140-neoforge.jar · 직접 추가`, 칩 `선택 꺼짐` | 통과 | `mods-02-installed.png` |
| `notes.txt`가 `배포 불가` + 도움말 | 행 `mods-row-notes.txt`, 부제 `버전 미상 · notes.txt · 직접 추가`, 칩 `필수` + `배포 불가`, 도움말 `배포본을 만들 수 없는 파일입니다 — 이 파일을 지우거나 올바른 jar로 바꾸세요` | 통과 | 같은 스크린샷 |
| 헤더 카운트 | `설치된 모드 2` / `관리 0 · 직접 추가 2 · 확인 전` | 통과 | 같은 스크린샷 |
| 도움말이 스크린 리더에 닿는가 | 행의 `HelpText` = 빈 문자열, FullDescription = 빈 문자열 | §6이 기록한 수용된 한계(`accessible-description` 미노출). 도움말은 별도 `Text` 노드로 트리에 존재하므로 정보 자체는 읽힌다 | 같은 로그 |

### 3. mod-detail — 네임스페이스 이동

로그 `mods-s03-mod-detail.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 행 셰브런이 mod-detail을 연다 | 셰브런 `Cloth Config v15 API 자세히`(Button) → 다이얼로그 `Cloth Config v15 API — QAfab-1.20.1` | 통과 | `mods-03-detail.png` |
| 초기 포커스는 네임스페이스 컨트롤 | `mod-detail-namespace` | 통과 | 같은 로그 |
| 직접 추가에는 `다른 버전…` / `프로젝트 페이지 열기`가 없다 | `mod-detail-versions`, `mod-detail-project` 모두 트리에 없음 | 통과 | 같은 로그 |
| 본문 | 설명 `An API for config screens.`, KeyValue `버전 15.0.140` / `파일 …jar` / `제공자 직접 추가` / `해시 c3e5733ba450`(12자) | 통과 | 같은 스크린샷 |
| 세그먼트는 staging, `이동`이 적용 | `선택 켜짐`(index 1) 선택 후에도 작업이 돌지 않음. 기본 버튼 `선택한 네임스페이스로 이동` 클릭 시 작업 시작 | 통과 | `mods-03-detail-staged.png` |
| 토스트 | `선택 켜짐으로 이동했습니다` | 통과 | `mods-03-moved.png` |
| 목록·디스크 반영 | 행 칩이 `선택 꺼짐` → `선택 켜짐`; 디스크 `optionaloff/` 비고 `optionalon/cloth-config-15.0.140-neoforge.jar` | 통과 | 같은 로그 |
| 닫힌 뒤 포커스가 행 셰브런으로 돌아간다(§8.3) | 포커스는 루트 `Window / Nebula`로 간다 | **결함 D4** | 같은 로그 |

### 4. 찾기 — 검색 · 캡션 · 아이콘 · 더 보기 · 정렬

로그 `mods-s04-search.log`, `mods-s04b-icons-more.log`, `mods-s04c-more.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 검색 전 상태 | EmptyState `검색어를 입력하세요` / `QAfab-1.20.1 · Minecraft 1.20.1 · Fabric 0.19.5에 맞는 모드만 표시됩니다.` | 통과 | `mods-04-presearch.png` |
| sodium + Enter가 결과를 낸다 | `mods-install-modrinth-AANobbMI` 등 Modrinth 프로젝트 id로 된 버튼. 같은 질의를 CLI로 돌리면 `검색 결과 36개 중 20개` — GUI도 20행을 만들고 그중 화면에 보이는 6행만 접근성 트리에 남는다(§6 수용된 한계) | 통과 | `mods-04-results-icons.png` |
| 캡션 `작성자 · 다운로드 N만 · 수정 …` | `jellysquid3 · 다운로드 2.2억 · 수정 8일 전`, `FlashyReese · 다운로드 9481만 · 수정 62일 전`, … (§8.8의 절사 표기) | 통과 | 같은 스크린샷 |
| 아이콘이 몇 초 안에 붙는다 | 아이콘 캐시를 비우고 Enter 직후 0.5 s 간격 8프레임 촬영: 프레임 0(아이콘 없음) → 프레임 1부터 diff 6.592로 고정. 즉 **0.5 s 안에 전부 도착**. `<root>/cache/icons`에 19개 PNG | 통과 | `mods-04-icons-before.png`, `mods-04-icons-after.png` |
| `더 보기`가 이어 붙인다 | 목록 끝까지 휠로 내리면 `mods-search-more`(`검색 결과 더 보기`) 등장. 클릭 후 보이던 9행이 그대로 남고 새 행이 추가됨(36개 중 2쪽째 16개). 2쪽이 꽉 차지 않아 버튼은 사라짐 | 통과 | `mods-04-more-before.png`, `mods-04-more-after.png` |
| 정렬이 재검색한다 | `mods-sort-0/1/2` = `관련도 / 다운로드 / 최신`. `최신`으로 바꾸면 결과 집합 자체가 바뀌고(`5vV6tCDu`가 1행) 캡션이 `수정 1일 전 · LoongLy · 다운로드 36만`처럼 **날짜부터** 나온다(§8.4) | 통과 | `mods-04-sort-downloads.png`, `mods-04-sort-newest.png` |

### 5. 설치 — install-confirm → 적용 → 목록

로그 `mods-s05-install.log`(Sodium), `mods-s05b-install-deps.log`(Sodium Extra).

Sodium 0.5.13(1.20.1)은 **필수 의존성을 선언하지 않는다**. 따라서 시나리오가 기대한
"fabric-api가 미리 체크된 3개 설치"는 이 버전으로는 재현되지 않고, 다이얼로그는
정확히 `1개 · 948.8 KB` / `1개 설치`를 보였다(사양대로 "내려받을 파일만 센다"). 의존성
분기를 재기 위해 fabric-api와 Sodium을 모두 요구하는 **Sodium Extra**로 한 번 더 돌렸다.

| 기대 | 관찰 (Sodium Extra) | 판정 | 증거 |
|---|---|---|---|
| 설치 클릭 → install-confirm이 항상 열린다 | 제목 `QAfab-1.20.1에 설치` | 통과 | `mods-05-install-confirm.png` |
| 해결된 버전 줄 | `Sodium Extra Sodium Extra 0.5.9 for Minecraft 1.20.1 · 정식 · 362 KB` — 제목이 두 번 나와 두 줄로 줄바꿈된다 | **결함 D1** | 같은 스크린샷 |
| 필수 의존성이 미리 체크 | `install-confirm-dep-P7dR8mSH` = `Fabric API [1.20.1] Fabric API 0.92.12+1.20.1 함께 설치`, CheckBox, Toggle=**On**, enabled=True, 해제 도움말 `필수 의존성을 빼면 게임이 시작되지 않을 수 있습니다` | 통과 | 같은 스크린샷 |
| 이미 설치된 의존성은 비활성 `설치됨` 행 | `install-confirm-dep-AANobbMI` = Sodium, Toggle=On, **enabled=False**, 칩 `설치됨`, 부제에 설치된 파일명 | 통과 | 같은 스크린샷 |
| 선택 의존성은 접힌다 | `install-confirm-optional` = `선택 의존성 보기`, 화면 문구 `선택 의존성 2개` | 통과 | 같은 스크린샷 |
| 푸터 개수/용량, 기본 버튼이 초점 | 푸터 `2개 · 2.4 MB`, 기본 `2개 설치`, 초기 포커스 `install-confirm-submit`. 다운로드되는 파일 수(fabric-api + sodium-extra)와 정확히 일치 | 통과 | 같은 스크린샷 |
| ProgressRow가 찾기 카드에 | `mods-search-progress`, Name = `다운로드 중 1/2 · fabric-api-0.92.12+1.20.1.jar · 0:00` → `다운로드 중 2/2 · sodium-extra-0.5.9+mc1.20.1.jar · 0:00` | 통과 | `mods-s05b-install-deps.log` |
| 상단 바 pill | `모드 설치 중 · 0:00` | 통과 | `mods-10-busy-dialog.png` |
| 베타/알파면 경고 Callout | Distant Horizons(1.21.1 NeoForge, 정식 없음) 설치 시 `Distant Horizons 3.2.0-b - 1.21.1 neo/fabric · 베타 · 28.8 MB` + 경고 `정식 버전이 없어 베타를 선택했습니다` | 통과 | `mods-10-busy-dialog.png` |
| 성공 토스트가 서버를 부른다 | `QAfab-1.20.1: 모드 2개를 설치했습니다` | 통과 | `mods-05-success-toast.png` |
| 행이 `설치됨`이 된다 | 화면 텍스트가 `설치` → `설치됨`으로 바뀌고 버튼 `IsEnabled=False`. 다만 UIA Name은 여전히 `Sodium 설치` | 부분 통과 — **결함 D2** | `mods-05-row-installed.png` |
| 찾기에 머문다(자동 전환 없음) | 검색 카드 유지 | 통과 | 같은 로그 |
| 설치됨 목록에 Modrinth 부제 | `Fabric API / 0.92.12+1.20.1 · fabric-api-0.92.12+1.20.1.jar · Modrinth`, `Sodium Extra / …· Modrinth`, `Sodium / …· Modrinth`; 헤더 `설치된 모드 5` `관리 3 · 직접 추가 2` | 통과 | `mods-05-installed-list.png` |
| 설치됨 목록의 아바타 | 스크린샷에서 Modrinth 관리 모드 3개는 제공자 아이콘, 직접 추가 jar는 **jar에서 뽑은 아이콘**(`<root>/cache/icons/local-<sha1>.png` 6개 생성 확인), `notes.txt`는 이니셜 `NO`로 폴백 | 통과 | 같은 스크린샷 |
| 디스크 | `fabricmods/required/`에 fabric-api·sodium-extra·sodium 3개 + notes.txt | 통과 | 같은 로그 |

### 6. 업데이트 확인 · 모두 업데이트

로그 `mods-s06-updates.log`. 시나리오 7의 다운그레이드를 먼저 돌려 실제 업데이트가
하나 생긴 상태에서 측정했다.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 업데이트 확인이 개수를 알린다 | 토스트 `업데이트 1개 있음` | 통과 | `mods-06-check.png` |
| 헤더 캡션이 갱신된다 | `관리 3 · 직접 추가 2 · 확인 전` → `관리 3 · 직접 추가 2 · 마지막 업데이트 확인 방금 전` | 통과 | 같은 스크린샷 |
| `업데이트 가능` 칩 | fabric-api 행에만, 그리고 그 행이 목록 맨 위로 정렬됨 | 통과 | 같은 스크린샷 |
| `모두 업데이트 (N)` | `mods-update-all`, 화면 문구 `모두 업데이트 (1)` | 통과 | 같은 스크린샷 |
| update-preview 목록 `mod: 0.91.0 → 0.92.0` | `Fabric API` / `xhLT3C5f → 0.92.12+1.20.1 · fabric-api-0.92.11+1.20.1.jar` — **왼쪽이 버전 번호가 아니라 Modrinth 버전 id** | **결함 D3** | `mods-06-update-preview.png` |
| 버튼 | 취소 `업데이트 취소` / 기본 `모드 1개 업데이트`, 초기 포커스는 기본 버튼 | 통과 | 같은 스크린샷 |
| 적용 중 동작 | 다이얼로그 유지 + pill `모드 업데이트 중` + `mods-installed-progress`(설치됨 카드) 관측, 2.3 s | 통과 | 로그 |
| 결과 | 토스트 `모드 1개를 업데이트했습니다`, 목록 `0.92.11+1.20.1` → `0.92.12+1.20.1`, 디스크에도 0.92.12만 존재 | 통과 | `mods-06-updated.png` |
| 두 번째 확인 | 토스트 `모두 최신입니다` | 통과 | `mods-06-uptodate.png` |

### 7. version-picker — 동작 라벨과 다운그레이드

로그 `mods-s07-version-picker.log`(다운그레이드), `mods-s06-updates.log` 앞부분(`교체` 라벨).

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 관리 모드에는 `다른 버전…`과 `프로젝트 페이지 열기`가 있다 | `mod-detail-versions` `다른 버전 고르기`, `mod-detail-project` `프로젝트 페이지 열기` | 통과 | `mods-07-detail-managed.png` |
| 설치된 버전 = 비활성 `설치됨` | `version-row-rvI2dfzR` `[1.20.1] Fabric API 0.92.12+1.20.1 설치됨`, `IsEnabled=False` | 통과 | `mods-07-version-picker.png` |
| 이전 버전 = `이전 버전으로 교체` | 0.92.11 / 0.92.9 / 0.92.8 / 0.92.7 네 행 모두 | 통과 | 같은 스크린샷 |
| 최신 버전 = `교체` | 0.92.11을 설치한 뒤 다시 열면 `[1.20.1] Fabric API 0.92.12+1.20.1 교체`, 0.92.11이 `설치됨` | 통과 | `mods-07-version-actions.png` |
| 다운그레이드는 다이얼로그 안 한 줄 확인 | 첫 클릭 → 경고 Callout `현재 0.92.12+1.20.1 → 0.92.11+1.20.1로 내립니다.`; 같은 행 두 번째 클릭이 적용 | 통과 | `mods-07-downgrade-confirm.png` |
| 3단계 교체가 한 버전만 남긴다 | 토스트 `모드를 교체했습니다`, `required/`에 fabric-api jar가 **정확히 1개** (0.92.11) | 통과 | `mods-07-downgraded.png` |
| 행 부제 | `0.92.11+1.20.1 · fabric-api-0.92.11+1.20.1.jar · Modrinth` (jar 자신의 버전) | 통과 | 같은 스크린샷 |

### 8. mod-delete — 의존 모드 경고

로그 `mods-s08-mod-delete.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 제목이 파일을 부른다 | `모드 삭제 — fabric-api-0.92.12+1.20.1.jar` | 통과 | `mods-08-delete-warning.png` |
| 본문이 절대 경로 | `이 파일이 영구 삭제됩니다: C:\…\root\servers\QAfab-1.20.1\fabricmods\required\fabric-api-0.92.12+1.20.1.jar` | 통과 | 같은 스크린샷 |
| 의존 모드 경고 | 경고 Callout `이 모드를 필요로 하는 모드가 있습니다` + `Fabric API를 필요로 하는 모드: Sodium Extra` | 통과 | 같은 스크린샷 |
| 초기 포커스는 취소 | `mod-delete-cancel` (`모드 삭제 취소`) | 통과 | 같은 로그 |
| 확인하면 지워진다 | 토스트 `모드를 삭제했습니다`, 디스크에서 사라지고 행도 사라짐, 헤더 `설치된 모드 4` `관리 2 · 직접 추가 2` | 통과 | `mods-08-deleted.png` |
| 취소는 아무것도 지우지 않는다 | 의존 모드가 없는 sodium-extra에서 경고 Callout 없음, 취소 후 파일 그대로 | 통과 | 같은 로그 |

### 9. 서버 메뉴 · 복제 · 삭제

로그 `mods-s09-server-menu.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 행 끝 컨트롤이 메뉴 셰브런 | `servers-menu-QAneo-1.21.1` = Button `QAneo-1.21.1 메뉴` (행당 탭 정지 1개) | 통과 | `mods-09-servers.png` |
| 메뉴 4항목 | `server-menu-mods` 모드 관리 / `server-menu-open-folder` 폴더 열기 / `server-menu-duplicate` 복제 / `server-menu-delete` 삭제 + 취소 | 통과 | `mods-09-server-menu.png` |
| 복제 다이얼로그 | `서버 복제 — QAneo-1.21.1`, 필드 `새 서버 ID`(초기 포커스), 도움말 `폴더 이름은 <ID>-<버전>이 됩니다` | 통과 | `mods-09-duplicate.png` |
| 새 서버가 생긴다 | `QAcopy` 입력 → 토스트 `서버를 복제했습니다: QAcopy-1.21.1`, 디스크 `servers\QAcopy-1.21.1`, 목록에 행 등장(`모드 1 · 파일 0 · 라이브러리 0`) | 통과 | `mods-09-duplicated.png` |
| 삭제 다이얼로그 | `서버 삭제 — QAcopy-1.21.1`, 본문 절대 경로 + `배포본을 다시 생성하면 이 서버가 사라집니다.`, 필드 `삭제하려면 서버 ID를 입력하세요` | 통과 | `mods-09-delete-empty.png` |
| 정확히 입력할 때까지 삭제 비활성 | 빈 값 → 비활성; `QAcopy` → 비활성; **`QAcopy-1.21.1␠`(뒤 공백) → 비활성**; `QAcopy-1.21.1` → 활성 | 통과 | `mods-09-delete-trailing-space.png`, `mods-09-delete-armed.png` |
| 비활성일 때 Enter는 아무 일도 없다 | 뒤 공백 상태에서 Enter → 다이얼로그 유지, 폴더 그대로 | 통과 | 같은 로그 |
| 확인하면 지워진다 | 토스트 `서버를 삭제했습니다: QAcopy-1.21.1`, 디스크와 목록에서 사라짐 | 통과 | `mods-09-deleted.png` |

### 10. busy 모델

**측정 방법**: §8.6의 busy 규칙은 전역(`SheetData.busy`)이므로 어떤 mutating job으로 재도
같다. 이 머신에서 모드 설치는 1–6 s, 서버 복제는 (600 MB·2 GB 단일 파일 기준) 1 s 안에
끝나 Esc·스크림·Ctrl+1을 차례로 밟는 동안 작업이 먼저 끝나 버렸다 — 첫 네 번의 시도
(`mods-s05-install.log`, `mods-s10-busy.log`, `mods-s10b-busy.log`, `mods-s10d-busy.log`,
`mods-s10c-busy-timeline.log`, `mods-s10e-busy-nav.log`)는 전부 그 타이밍 때문에 "실패"로
찍혔고, 실제 결함이 아니었다. 바이트가 아니라 **파일 개수**가 복제를 느리게 만든다는
것을 확인하고(`QAneo-1.21.1\files\pad\`에 1 KB 파일 15,000개), 작업이 10 s 동안 돌게 만든
뒤 150 ms마다 상단 바 pill·다이얼로그·다른 페이지 기본 버튼을 동시에 기록했다
(`mods-s10f-busy-nav.log`). 아래 판정은 **pill이 살아 있던 틱만** 센 것이다. 패드는 측정
후 삭제했다.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 상단 바 pill | `서버 복제 중 · 0:00` → `· 0:10`, 0.6 s부터 10.6 s까지 55틱 (모드 설치에서는 `모드 설치 중 · 0:00`) | 통과 | `mods-10-busy-dialog.png`, `mods-s10f-busy-nav.log` |
| 열린 다이얼로그가 닫히지 않는다 | 작업 내내 `server-duplicate-submit` 존재; 설치에서도 `install-confirm-submit` 유지 | 통과 | `mods-10-busy-dialog-duplicate.png`, `mods-10-busy-dialog.png` |
| 그 컨트롤들이 비활성 | `server-duplicate-submit.IsEnabled=False`, `server-duplicate-id.IsEnabled=False`; 설치에서는 `install-confirm-submit.IsEnabled=False` | 통과 | 같은 스크린샷 |
| Esc가 거부된다 | 0.6 s에 Esc → 이후 **busy였던 54틱 전부** 다이얼로그 유지 | 통과 | `mods-s10f-busy-nav.log` |
| 스크림 클릭도 닫지 않는다 | 카드 밖(창 기준 400,130) 물리 클릭 후에도 유지 | 통과 | `mods-s10d-busy.log` |
| 내비게이션은 살아 있다 | 1.3 s에 Ctrl+1 → 빌드 페이지가 렌더되고(`build-generate`가 트리에 등장) 남은 52틱 동안 유지 | 통과 | `mods-10-busy-nav.png` |
| 페이지를 옮겨도 다이얼로그가 남는다 | Ctrl+1 이후 busy였던 **52틱 전부** `server-duplicate-submit` 존재 | 통과 | 같은 로그 |
| 다른 페이지의 기본 버튼은 비활성 | 빌드의 `배포본 생성` `IsEnabled=False` — busy였던 52틱 중 활성인 틱 **0개**, 작업이 끝난 10.9 s 틱에서 곧바로 True | 통과 | `mods-10-busy-nav.png` |
| 새 다이얼로그가 열리지 않는다 | 미측정(§5) | — | — |

### 11. 설정 CurseForge 카드

로그 `mods-s11-settings-theme.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 설정에 CurseForge 카드 | 카드 제목 `CurseForge`, 필드 `settings-curseforge-key` `CurseForge API 키`, 도움말 `console.curseforge.com에서 발급받은 본인 키를 입력하세요. 환경 변수 NEBULA_CURSEFORGE_API_KEY가 있으면 그 값이 우선합니다.` | 통과 | `mods-11-settings-curseforge.png` |
| 빈 키를 저장해도 CurseForge는 거부된다 | 저장 후(`설정을 저장했습니다`) 찾기의 CurseForge 세그먼트를 클릭 → 선택이 바뀌지 않고, 이어서 `lithium`을 검색하면 결과가 전부 `mods-install-modrinth-*` | 통과 (기능) | `mods-11-provider-after-click.png` |
| 도움말이 뜬다 | `설정에서 CurseForge API 키를 입력하세요` | 통과 | `mods-11-provider-refused.png` |
| 세그먼트가 **비활성**이어야 한다(§8.4) | `mods-provider-1.IsEnabled=True`, 클릭해도 해당 영역 픽셀 diff **0** — 비활성 표시도, 눌린 피드백도 없다 | **결함 D6** | `mods-11-provider-a.png` / `-b.png` |

### 12. 다크 테마 토글 (모드 페이지)

로그 `mods-s11-settings-theme.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 모드 페이지에서 테마가 바뀐다 | `theme-toggle` 라벨 `라이트 테마로 전환` → 클릭 → `다크 테마로 전환`, 창 전체 평균 픽셀 차 201.253 | 통과 | `mods-12-theme-before.png`, `mods-12-theme-after.png` |

### 13. 재시작 후 서버 선택 유지

로그 `mods-s13-restart.log`.

| 기대 | 관찰 | 판정 | 증거 |
|---|---|---|---|
| 마지막 서버가 남는다 | 종료 전 `QAneo-1.21.1` 선택 → `settings.json`에 `"last-mods-server": "QAneo-1.21.1"` → 재시작 후 Ctrl+3에서 `mods-server-1.IsSelected=True`, 스코프 `QAneo-1.21.1 · Minecraft 1.21.1 · NeoForge 21.1.250` | 통과 | `mods-13-restart-persisted.png` |

### 14. 접근성 — AutomationId 계약

로그 `mods-s01-launch.log`, `mods-s13-restart.log`; 트리 `mods-tree-modspage.txt`, `mods-tree-final.txt`.

| 요구 id | 관찰 | 판정 |
|---|---|---|
| `mods-view` | `Tab` `모드 보기`, 자식 `mods-view-0` `모드 보기 설치됨` / `mods-view-1` `모드 보기 찾기`(TabItem + TogglePattern, On/Off로 선택 노출) | 존재 |
| `mods-row-<file>` | `mods-row-cloth-config-15.0.140-neoforge.jar`, `mods-row-notes.txt`, `mods-row-fabric-api-0.92.12+1.20.1.jar` 등 파일명 그대로 | 존재 |
| `mods-install-modrinth-<id>` | `mods-install-modrinth-AANobbMI`, `-PtjYWJkn`, `-Bh37bMuy` … | 존재 |
| `servers-menu-<id>` | `servers-menu-QAfab-1.20.1`, `servers-menu-QAneo-1.21.1`, 복제 후 `servers-menu-QAcopy-1.21.1` | 존재 |
| `settings-curseforge-key` | 설정 페이지에서 PageDown으로 도달 | 존재 |

그 밖에 이번 레인에서 실제로 조작한 id: `mods-server`(+`-0/-1`), `mods-search`,
`mods-provider`(+`-0/-1`), `mods-sort`(+`-0/-1/-2`), `mods-check-updates`,
`mods-update-all`, `mods-search-more`, `mods-search-progress`, `mods-installed-progress`,
`mod-detail-{dialog,dialog-close,namespace(+-0/-1/-2),versions,project,delete,cancel,apply}`,
`install-confirm-{dialog,dialog-close,versions,namespace(+-0/-1/-2),dep-<project id>,optional,cancel,submit}`,
`version-row-<version id>`, `version-picker-cancel`,
`update-preview-{cancel,submit}`, `mod-delete-{dialog,cancel,submit}`,
`server-menu-{mods,open-folder,duplicate,delete,cancel}`,
`server-duplicate-{id,cancel,submit}`, `server-delete-{confirm,cancel,submit}`,
`theme-toggle`, `nav-*`, `settings-save`. 전부 설계 문서의 kebab-case 규약과 일치한다.

**ProgressRow의 라이브 리전은 이 하네스로 확인할 수 없다.** `ProgressRow`는
`accessible-live-region: polite`를 선언하고 있고(`ui/lib/progress-row.slint:34`),
UIA 트리에서 `progress-indicator` 역할과 진행 문구를 그대로 담은 Name을 노출한다.
그러나 `LiveSetting`(속성 30135)은 UIA 3 속성이라 이 레인이 쓰는 .NET
`System.Windows.Automation`(UIA 2) 클라이언트가 `NotSupported`를 돌려준다. 앱이
값을 안 보내는 것인지 클라이언트가 못 읽는 것인지 구분할 수 없어 **미검증**으로 둔다.

## 3. 결함

| # | 심각도 | 내용 | 재현 |
|---|---|---|---|
| D1 | 중 | `install-confirm`의 해결 버전 줄과 의존성 행이 **프로젝트 제목을 두 번** 출력한다. `{title} {version_name}`(`gui/mods.rs:1409`, `:1464`) 형태인데 제공자의 version *name*이 대개 제목을 포함하기 때문이다: `Sodium Extra Sodium Extra 0.5.9 for Minecraft 1.20.1 · 정식 · 362 KB`, `Sodium Sodium 0.5.13 for Fabric`, `Fabric API [1.20.1] Fabric API 0.92.12+1.20.1`. 첫 줄이 두 줄로 줄바꿈되어 카드가 밀린다. §8.4의 형식은 `fabric-api 0.92.0 · 정식 · 2.1 MB`(제목 + 버전 **번호**)다. 버전 이름이 제목으로 시작하지 않으면(`Distant Horizons 3.2.0-b - 1.21.1 neo/fabric`) 멀쩡해 보이므로 Modrinth 관례에 따라 갈린다 | 시나리오 5 |
| D2 | 중 | 설치가 끝난 찾기 행의 버튼은 화면 글자가 `설치` → `설치됨`으로 바뀌지만 `a11y-label`은 `{제목} 설치`로 고정이다(`ui/screens/mods.slint:137`). 스크린 리더 사용자는 비활성 `Sodium 설치`만 듣고, 같은 방식으로 비활성이 되는 `no-version`(버전 없음) 상태와 구분할 수 없다 | 시나리오 5 |
| D3 | 중 | `update-preview` 행의 "이전 버전" 자리에 **제공자 버전 id**가 찍힌다: `xhLT3C5f → 0.92.12+1.20.1 · fabric-api-0.92.11+1.20.1.jar`. §8.3의 형식은 `mod: 0.91.0 → 0.92.0`이다. 원인은 `UpdateCandidate.from = entry.version_id`(`catalog/update.rs:100`)를 그대로 표시하는 것(`gui/mods.rs:2254`) | 시나리오 6 |
| D4 | 하 | mod-detail을 닫은 뒤 포커스가 §8.3이 말한 "행의 셰브런"이 아니라 루트 `Window / Nebula`로 간다. 키보드 사용자는 목록의 제자리를 잃고 Tab을 nav 레일부터 다시 돌려야 한다. 다만 **설계 문서 자체가 갈린다**: §8.7은 "`app.slint`가 `closed`에서 `keys.focus()`로 포커스를 돌려준다 — `new-server`가 지금 하는 그대로"라고 적고 있고 구현은 §8.7을 따른다. 사양 충돌로 기록하되, 사용자 비용은 실재한다 | 시나리오 3 |
| D5 | 하 | PillStrip의 로빙 포커스가 UIA에 전혀 보이지 않는다. Left/Right는 시각적 링만 옮기고(`focus-index`는 내부 속성), `FocusedElement`는 계속 그룹 `mods-server`다. 스크린 리더 사용자는 어느 서버에 링이 있는지 알 수 없어 Enter를 누르기 전까지 무엇을 고르는지 모른다 | 시나리오 1 |
| D6 | 하 | 키가 없을 때 CurseForge 제공자 세그먼트가 §8.4가 요구한 대로 **비활성이 아니다**. `IsEnabled=True`로 노출되고, 클릭해도 해당 영역 픽셀이 전혀 바뀌지 않는다(변경을 `changed(index)` 안에서 조용히 거부한다 — `ui/screens/mods.slint:401`). 기능상 Modrinth에 머무는 것은 맞지만, 눌러도 아무 일이 없는 활성 컨트롤이다 | 시나리오 11 |
| D7 | 하(니트) | `Callout`의 `accessible-label`이 `title + ". " + body`로 고정 조립되어, 제목이 이미 마침표로 끝나거나 본문이 비면 스크린 리더가 `…사라집니다.. `처럼 마침표 두 개와 꼬리 공백을 읽는다(`ui/lib/callout.slint:16`, `:77`). 관측 예: 서버 삭제 다이얼로그, 다운그레이드 확인 줄 | 시나리오 9, 7 |

### 결함이 아닌 것

- **Sodium에 fabric-api 의존성이 없다.** Sodium 0.5.13(1.20.1)은 필수 의존성을 선언하지
  않는다. `1개 · 948.8 KB` / `1개 설치`는 "내려받을 파일만 센다"는 §8.4 그대로다.
- **찾기 결과가 6행만 접근성 트리에 보인다.** 호스트는 20행을 만들고 ScrollView 밖으로
  밀린 행이 트리에서 빠지는 것으로, §6이 기록한 Slint 1.17.1의 수용된 한계다. 휠로 내리면
  나머지 행과 `더 보기`가 나타난다.
- **`배포 불가` 도움말이 `HelpText`로 오지 않는다.** §6의 `accessible-description` 미노출
  한계. 도움말은 독립 `Text` 노드로 트리에 있으므로 정보는 읽힌다.
- **PillStrip 루트가 UIA `Group`이다.** UIA에 radio-group 컨트롤 타입이 없다.
  SelectionPattern + 자식 RadioButton/SelectionItemPattern이 accesskit의 정상 매핑이며,
  선택 상태는 `IsSelected`로 정확히 노출된다.
- **`복제`·`모드 삭제` 작업 중 상단 pill을 못 봤다.** 두 작업이 0.8–0.9 s로 끝나 120 ms
  폴링으로도 놓쳤다. pill 자체는 설치·업데이트에서 관측했고, 라벨은
  `gui/mods.rs`의 `spawn_catalog_job` 호출부가 `서버 복제` / `모드 삭제`로 넘긴다.

## 4. CLI 대조

같은 빌드의 CLI로 GUI 관측을 한 번씩 교차 확인했다(`mods-setup.log`).

- `nebula mods QAfab-1.20.1 search sodium --limit 20` → `modrinth · 검색 결과 36개 중 20개
  (Minecraft 1.20.1 · Fabric)`. GUI가 보여 준 프로젝트 id·순서·다운로드 수와 일치.
- `nebula mods QAneo-1.21.1 install cloth-config --namespace required` → `설치 1 · 건너뜀 0
  · 직접 다운로드 0`, 진행 표시 `다운로드 중 1/1 · cloth-config-15.0.140-neoforge.jar`.

## 5. 검증하지 못한 것

- **CurseForge 전체**: 키가 없어 검색·설치·아이콘 메모리 캐시·차단 파일(R3/R4)·핑거프린트
  업데이트 경로 모두 미실행.
- **ProgressRow의 `LiveSetting`**: §2 §14 참조. UIA 2 클라이언트로는 읽을 수 없다.
- **실제 스크린 리더**(NVDA/내레이터) 사용감.
- **실패 경로**: 설치 실패 Callout, 롤백/`left_behind`, 429 자동 재시도, `서버가
  바뀌었습니다 — 다시 검색하세요`(stale plan). 네트워크를 실제로 끊거나 제공자를 흉내 내야
  해서 이번 레인에서는 시도하지 않았다.
- **busy 중 "새 다이얼로그가 열리지 않는다"**(§8.6): 다이얼로그를 하나 띄운 채 다른
  다이얼로그를 여는 경로를 따로 밟지 않았다.
- **`더 보기`의 "포커스가 버튼에 남는다"**: 이번 질의는 2쪽에서 끝나 버튼이 사라졌고,
  사라진 버튼에 포커스를 남길 수는 없다. 3쪽 이상인 질의로는 재지 않았다.
- **`모두 보기`(8행 초과 캡 해제)**, **update-preview의 개별 실패 목록**,
  **비호환/미해결 의존성으로 기본 버튼이 막히는 분기**: 이번 픽스처에서 해당 상태를 만들지
  못했다.

## 6. 기타 기록

- 이 레인의 로그·스크립트는 모두 `mods-` 접두사를 붙여 뒀다. 다만 처음 실행에서
  2026-09-10 레인의 `docs/verification/gui-qa/s01-launch.log`를 같은 이름으로 덮어썼다.
  이 디렉터리의 `*.log`는 gitignore 대상이라 추적되지 않으며(`git status --porcelain`에
  나타나지 않는다), 그 레인의 커밋된 증거(스크린샷·`tree*.txt`·`dump-tree.log`·보고서)는
  그대로다.
- `git status --porcelain`(워크트리 전체): 추적 파일 변경 **0건**, 추가된 것은 전부
  `docs/verification/` 아래 untracked 파일. `crates/`는 손대지 않았다.
- 픽스처 루트·설정 디렉터리·빌드 산출물은 전부 스크래치 디렉터리 안에 있다.
  시나리오 10이 만든 패드 파일(`QAneo-1.21.1/files/pad*`)과 복제본 `QAbusy-1.21.1`은
  측정 직후 지웠다.
