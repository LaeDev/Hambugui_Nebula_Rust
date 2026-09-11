# Nebula (Rust 포트)

Helios Launcher와 Hambugui 런처가 그대로 읽을 수 있는 `distribution.json`을 생성하는
CLI/GUI 도구입니다. [dscalzi/Nebula](https://github.com/dscalzi/Nebula)의 Node/TypeScript
구현을 Rust로 새로 작성한 포트이며, 명령 구조와 파일 구조 명세를 그대로 유지합니다.
원본과 다르게 동작하는 부분은 [10. 원본과의 의도된 차이](#10-원본과의-의도된-차이)에 모았습니다.

원저작자는 Daniel Scalzi(`dscalzi/Nebula`)이며, 이 저장소는 MIT 라이선스 하에 배포됩니다.
자세한 내용은 [12. 라이선스](#12-라이선스)를 참고하세요.

## 2. 요구 사항

- **Rust 1.97** (이 저장소의 `rust-toolchain.toml`이 `1.97.0`을 고정합니다) 또는
  릴리스로 배포되는 `nebula` 바이너리를 직접 받아 쓰는 경우 Rust 툴체인이 필요 없습니다.
- **Java 8 이상.** Forge 설치기, `Claritas.jar`(모드 메타데이터 분석), `PackXZExtract.jar`
  (FG2 `.pack.xz` 라이브러리 압축 해제)를 실행하는 데 필요합니다.
  NeoForge를 빌드하는 경우 설치기와 FancyModLoader 동작을 위해 **Java 21을 권장**합니다.
- Windows / macOS / Linux를 지원합니다. (`nebula-core`는 `x86_64-pc-windows-msvc`,
  `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`를 CI 대상으로 삼습니다.)

## 3. 빠른 시작

저장소 루트에 `.env` 파일을 만듭니다.

```properties
ROOT=D:\TestRoot
BASE_URL=https://cdn.example.com/
JAVA_EXECUTABLE=C:\Program Files\Eclipse Adoptium\jdk-21.0.4.7-hotspot\bin\java.exe
HELIOS_DATA_FOLDER=C:\Users\user\AppData\Roaming\Helios Launcher
```

`ROOT`와 `BASE_URL`은 필수입니다. `BASE_URL`이 `http://`/`https://` 스킴 없이 시작하고
호스트가 `localhost`가 아니면 오류이며, `localhost`로 시작하는 경우에만 `http://`를
자동으로 붙입니다. `JAVA_EXECUTABLE`은 Forge/NeoForge/Claritas/PackXZ를 실행할 때만
필요합니다. `HELIOS_DATA_FOLDER`는 `generate distro --install-local`에서만 필요합니다.

```bash
cargo build --release                            # 결과: target/release/nebula(.exe)
nebula init root                                  # schemas/, meta/, servers/, modpacks/ 생성
nebula g server MyServer 1.20.1 --forge recommended   # 로더는 forge/neoforge/fabric 중 하나
# servers/MyServer-1.20.1/{files,libraries,forgemods/{required,optionalon,optionaloff}}/에 파일 배치
nebula g distro                                   # distribution.json 생성
nebula validate --strict                          # 런처 계약(Hambugui/Helios) 검증
nebula gui                                         # GUI 실행 (인자 없이 nebula만 실행해도 동일)
```

## 4. GUI

`nebula gui`(또는 인자 없이 `nebula`)는 3:4 세로 비율 창(기본 720×960, 최소 640×800,
크기 조절 가능)을 띄웁니다. 좌측 72px 내비게이션 레일에 여섯 페이지(빌드/서버/모드/
가져오기/배포/설정) 아이콘이 있고, 상단 바에는 현재 페이지 제목, 작업 중일 때 나타나는
상태 알약(경과 시간 표시), 라이트/다크 테마 토글 버튼이 있습니다.

1. **빌드**(기본 페이지) — 루트/기본 URL/Java/Launcher 폴더 준비 상태를 확인하고, 배포본
   파일 이름과 옵션(Launcher 폴더에 설치 / Forge 캐시 무효화 / 설치 출력 삭제)을 정해
   "배포본 생성" 또는 "기존 파일 검증"을 실행한 뒤 결과와 로그를 봅니다.
2. **서버** — `list_servers`가 읽은 서버 목록(로더, 모드/파일/라이브러리 개수)을 보고,
   "모드팩 가져오기"로 이동하거나 "새 서버" 대화상자로 서버를 만듭니다. 각 행의 셰브런을
   누르면 메뉴(모드 관리 / 폴더 열기 / 복제 / 삭제)가 열리며, 삭제는 서버 ID를 정확히
   입력해야 실행됩니다.
3. **모드** — 상단의 서버 선택 알약으로 관리할 서버를 고르고, 설치됨 / 찾기 화면을
   전환합니다. 설치됨은 관리 중인 모드와 손으로 넣은 모드를 함께 보여주며 업데이트 확인 /
   모두 업데이트를 제공하고, 자세히 버튼으로 모드 상세 대화상자(네임스페이스 이동, 다른
   버전, 삭제)를 엽니다. 찾기는 Modrinth/CurseForge에서 검색해 설치 확인 대화상자(해결된
   버전, 네임스페이스, 필수·선택 의존성)를 거쳐 설치합니다. **Modrinth는 API 키 없이
   검색·설치할 수 있습니다.** CurseForge 검색·설치는 사용자 자신의 API 키가 필요합니다 —
   설정 → CurseForge에서 입력하거나 환경 변수 `NEBULA_CURSEFORGE_API_KEY`를 설정하세요
   (둘 다 있으면 환경 변수가 우선합니다).
4. **가져오기** — `modpacks/` 폴더의 CurseForge/Modrinth/Prism 팩 중 하나를 골라 서버
   ID를 지정하고 가져옵니다. 자동으로 받을 수 없는 파일은 수동 다운로드 목록으로
   나타납니다.
5. **배포** — WebDAV/Google Drive 업로드 대상을 고르고, 변경된 파일만 올릴지 정한 뒤
   업로드를 실행합니다.
6. **설정** — 루트/기본 URL/Java/Launcher 폴더, WebDAV·Google Drive 자격 증명, CurseForge
   API 키, 테마와 모션 줄이기를 편집하고 저장합니다.

빌드 페이지에서 생성/검증이 끝나면 결과 카드에 **검증 배너**가 뜹니다: `런처 검증 통과`
(성공)/`경고와 함께 통과`(strict 검증만 실패)/`검증 실패`(오류 포함) 세 등급이며 파일명과
SHA-256 앞자리를 함께 보여줍니다. strict 검증을 통과하지 못한 배포본은 배포 페이지의
업로드 버튼이 비활성화되어 게이트됩니다. 작업이 끝나면 화면 하단 중앙에 토스트(성공/경고/
실패, 최대 3개 표시, 4초 후 자동 소멸)가 뜨고, "새 서버" 같은 화면은 모달 대화상자로
열립니다(작업 중에는 열리지 않습니다).

키보드: `Ctrl`/`Cmd`+`1`~`6`로 페이지 전환, `Esc`로 대화상자 닫기(작업 중이 아닐 때),
`Enter`로 대화상자 제출 및 설정 저장.

**설정 저장 위치**: 설정은 JSON으로 Windows `%APPDATA%\nebula\settings.json`, macOS
`~/Library/Application Support/nebula/settings.json`, Linux
`$XDG_CONFIG_HOME/nebula/settings.json`(없으면 `~/.config/nebula/settings.json`)에
저장됩니다. 환경 변수 `NEBULA_CONFIG_DIR`을 설정하면 위 규칙 대신 그 경로를 그대로
설정 디렉터리로 씁니다(끝에 `nebula`가 붙지 않습니다). Google Drive 리프레시 토큰도 같은
디렉터리의 `gdrive-token.json`에 저장됩니다. 설정 파일이 없으면 `.env`/환경 변수로
시드됩니다. 설정 페이지의 테마(시스템/라이트/다크)와 "모션 줄이기" 토글은 변경 즉시
적용되고 저장됩니다(라이트가 기본값).

GUI는 Pretendard 글꼴과 Lucide 아이콘을 바이너리에 번들로 포함합니다. 라이선스는
`crates/nebula/assets/fonts/LICENSE-Pretendard.txt`와
`crates/nebula/assets/icons/LICENSE-Lucide.txt`에 있습니다.

스크린샷: [`docs/plans/gui-v2/build-success.png`](docs/plans/gui-v2/build-success.png),
[`docs/plans/gui-v2/dark-build-success.png`](docs/plans/gui-v2/dark-build-success.png),
[`docs/plans/gui-v2/servers.png`](docs/plans/gui-v2/servers.png),
[`docs/plans/gui-v2/new-server-dialog.png`](docs/plans/gui-v2/new-server-dialog.png),
[`docs/plans/gui-v2/settings.png`](docs/plans/gui-v2/settings.png). 전체 상태 목록과
다시 만드는 방법은 [`docs/plans/gui-v2/README.md`](docs/plans/gui-v2/README.md)에,
디자인 배경과 토큰·레이아웃 세부 사항은
[`docs/plans/DESIGN-v2.md`](docs/plans/DESIGN-v2.md)에 있습니다.

## 5. 명령 레퍼런스

전역 옵션(모든 명령에 적용): `-v, --verbose`(디버그 로그), `-q, --quiet`(경고/오류만).
인자 없이 `nebula`만 실행하면 `nebula gui`와 동일합니다.

### init (별칭 `i`)

| 명령 | 설명 |
|---|---|
| `nebula init root` (`nebula i root`) | 빈 표준 파일 구조(schemas, meta/distrometa.json, servers/, modpacks/)를 생성합니다. |

### generate (별칭 `g`)

| 명령 | 인자/옵션 | 설명 |
|---|---|---|
| `generate server <ID> <VERSION>` | `--forge <v>` / `--fabric <v>` / `--neoforge <v>` (셋 중 하나만, `v`는 `latest`/`recommended` 가능) | 새 서버 설정을 만듭니다. |
| `generate server-import <ID> <FILE>` | `FILE`은 `<ROOT>/modpacks` 아래의 이름 또는 절대 경로 | CurseForge/Modrinth/Prism 모드팩 형식을 자동 감지해 서버를 만듭니다. |
| `generate server-curseforge <ID> <ZIP_NAME>` | | CurseForge 형식만 허용하는 `server-import`의 별칭입니다. |
| `generate distro [NAME]` | `--install-local`(별칭 `--installLocal`), `--discard-output`(별칭 `--discardOutput`), `--invalidate-cache`(별칭 `--invalidateCache`), `--helios-untracked` | 루트 파일 구조로부터 배포본 인덱스를 생성합니다. `NAME` 기본값은 `distribution`. |
| `generate schemas` | | `meta/distrometa.json`, `servermeta.json`용 JSON 스키마를 다시 생성합니다. |

camelCase 별칭(`--installLocal`, `--discardOutput`, `--invalidateCache`)은 kebab-case
플래그와 동일하게 동작합니다(업스트림 npm 스크립트 시절의 표기를 그대로 지원).

### 버전 조회

| 명령 | 설명 |
|---|---|
| `latest-forge <VERSION>` | 지정한 마인크래프트 버전의 최신 Forge 버전. |
| `recommended-forge <VERSION>` | 권장 Forge 버전(없으면 최신 버전으로 대체). |
| `latest-fabric` | 최신 Fabric 로더 버전. |
| `recommended-fabric` | 권장(stable) Fabric 로더 버전. |
| `latest-neoforge <VERSION>` | 지정한 마인크래프트 버전의 최신 NeoForge 버전. |
| `recommended-neoforge <VERSION>` | 권장(non-beta) NeoForge 버전(없으면 최신 버전으로 대체하며 경고). |

### validate / publish / gdrive-auth / gui

| 명령 | 인자/옵션 | 설명 |
|---|---|---|
| `validate [NAME]` | `--strict`, `--file <path>` | `<ROOT>/<NAME>.json`(또는 `--file`로 지정한 파일)을 런처 계약에 따라 검증합니다. `--strict`는 `http://` URL과 SHA-256 누락을 오류로 취급합니다. |
| `publish [NAME]` | `--target <webdav|gdrive>` (필수), `--all` | 생성된 배포본이 참조하는 파일만 골라 업로드합니다. `--all`은 `publish-state.json`의 변경 추적을 무시합니다. |
| `gdrive-auth` | | Google 계정에 대해 OAuth 인증을 수행하고 리프레시 토큰을 저장합니다. |
| `gui` | | GUI를 실행합니다(인자 없이 `nebula`를 실행한 것과 같습니다). |

### mods

`nebula mods <SERVER> <하위 명령>` — `<SERVER>`는 `<ROOT>/servers` 아래의 서버 디렉터리
이름입니다.

| 하위 명령 | 인자/옵션 | 설명 |
|---|---|---|
| `search <QUERY>` | `--provider modrinth\|curseforge`(기본 `modrinth`), `--sort relevance\|downloads\|newest`(기본 `relevance`), `--limit <N>`(기본 `20`), `--offset <N>`(기본 `0`) | 서버의 마인크래프트 버전·로더에 맞는 모드를 검색합니다. |
| `install <PROJECT>` | `--version <ID>`(생략 시 서버에 맞는 최신 버전을 고름), `--namespace required\|optionalon\|optionaloff`(기본 `required`), `--no-deps`(필수 의존성 없이 단독 설치 — 게임이 시작되지 않을 수 있음), `--provider modrinth\|curseforge`(기본 `modrinth`) | `PROJECT`(프로젝트 id 또는 slug)와 필수 의존성을 설치합니다. |
| `list` | | 관리 중인 모드와 손으로 넣은 모드를 모두 나열합니다. |
| `update` | `--apply` | `--apply` 없이 실행하면 업데이트 가능한 모드 목록만 보여주고 아무것도 설치하지 않습니다. `--apply`를 주면 실제로 설치합니다. |
| `remove <FILE>` | | 모드 파일과 색인 항목을 삭제합니다. |
| `move <FILE> <NAMESPACE>` | `NAMESPACE`는 `required`\|`optionalon`\|`optionaloff` | 모드를 다른 네임스페이스로 옮겨 켜거나 끕니다. |

### server

| 명령 | 인자/옵션 | 설명 |
|---|---|---|
| `server delete <DIR>` | `--yes`(필수) | 서버 디렉터리를 통째로 삭제합니다. `--yes` 없이 실행하면 삭제될 절대 경로만 출력하고 오류로 끝납니다(아무것도 지우지 않습니다). |
| `server duplicate <DIR> <NEW_ID>` | | `DIR`을 `NEW_ID`(마인크래프트 버전 접미사는 유지)로 복제합니다. `modindex.json`도 그대로 복사됩니다. |

## 6. 파일 구조 명세

Nebula는 파일 구조 자체로 정보를 보존하는 방식을 씁니다. 이 구조로부터 Helios/Hambugui용
`distribution.json` 전체를 생성합니다. 구조는 디렉터리 패턴으로 캡슐화되어 있으며, 아래
명령으로 자동 생성하거나 손으로 만들 수 있습니다.

배포본 객체는 루트 디렉터리 하나로 표현되고, 그 아래 모든 서버가 각자의 폴더에
캡슐화됩니다. 서버 폴더 이름은 id와 마인크래프트 버전을 모두 포함합니다(`<id>-<mc>`).

```
TestRoot/                          배포본을 감싸는 루트 디렉터리
└── servers/                       모든 서버 파일이 저장되는 디렉터리
    └── TestServer-1.12.2/         id=TestServer, 버전=1.12.2인 서버
        ├── servermeta.json
        ├── TestServer-1.12.2.png  서버 아이콘 (선택)
        ├── files/                 File 타입 모듈 전체
        ├── libraries/             Library 타입 모듈 전체
        ├── forgemods/             ForgeMod 타입 모듈 전체 (Forge 서버)
        ├── fabricmods/            FabricMod 타입 모듈 전체 (Fabric 서버)
        └── neoforgemods/          NeoForgeMod 타입 모듈 전체 (NeoForge 서버, 업스트림에는 없는 로더)
```

`forgemods`/`fabricmods`/`neoforgemods`는 **토글 가능한 모듈** 디렉터리이며, 각각
`required`(필수), `optionalon`(선택·기본 켬), `optionaloff`(선택·기본 꺼짐) 세
하위 디렉터리로 파일을 분류해야 합니다.

**서버 아이콘**: (1, 권장) 서버 루트 디렉터리에 `jpg`/`png` 파일을 둡니다 — 이름은
무관하고, 여러 개면 디렉터리 정렬 순서상 마지막 파일이 선택됩니다. (2) `servermeta.json`의
`meta.icon`에 전체 URL을 직접 적습니다(직접 호스팅 권장). `servermeta.json`의 값이 비어
있지 않고 유효한 URL이면 항상 그 값이 우선하며, 비어 있거나 무효하면 1번으로 대체됩니다.

### 모드 카탈로그 파일

`nebula mods`와 GUI 모드 페이지가 관리를 위해 서버 디렉터리 안에 두는 파일입니다. 셋 다
**`distribution.json` 생성에는 전혀 영향을 주지 않습니다** — 어떤 스캐너도 읽지 않습니다.

| 경로 | 내용 |
|---|---|
| `<server>/modindex.json` | 서버별 설치 색인(제공자, 버전, 해시, 의존성 등). 지워도 안전합니다 — 지우면 해당 모드들이 목록에서 `직접 추가`로만 보입니다. |
| `<server>/.nebula-tmp/` | 다운로드 스테이징 디렉터리. |
| `<server>/.nebula-lock` | 모드 설치/업데이트/삭제/이동, 서버 삭제/복제 중에 생성되는 프로세스 간 배타적 잠금 파일. |
| `<root>/cache/icons/` | 모드 아이콘 캐시. |

### 추가 메타데이터

파일 구조만으로는 표현할 수 없는 값을 위해 파일 두 개를 씁니다. 필요하면 기본값이
생성되며, 둘 다 JSON 스키마가 함께 생성되어 편집을 돕습니다.

`${ROOT}/meta/distrometa.json` — 배포본 메타데이터:

```json
{
  "$schema": "file:///${ROOT}/schemas/DistroMetaSchema.schema.json",
  "meta": {
    "rss": "<LINK TO RSS FEED>",
    "discord": {
      "clientId": "<FILL IN OR REMOVE DISCORD OBJECT>",
      "smallImageText": "<FILL IN OR REMOVE DISCORD OBJECT>",
      "smallImageKey": "<FILL IN OR REMOVE DISCORD OBJECT>"
    }
  }
}
```

`servers/${YOUR_SERVER}/servermeta.json` — 서버 메타데이터. NeoForge 서버는
`forge`/`fabric` 대신 `neoforge: { "version": "…" }`를 씁니다(세 로더는 서로 배타적):

```json
{
  "$schema": "file:///${ROOT}/schemas/ServerMetaSchema.schema.json",
  "meta": {
    "version": "1.0.0",
    "name": "Test (Minecraft 1.12.2)",
    "description": "Test Running Minecraft 1.12.2 (Forge v14.23.5.2854)",
    "icon": "How to set the server icon: https://github.com/dscalzi/Nebula#setting-the-server-icon",
    "address": "localhost:25565",
    "mainServer": false,
    "autoconnect": true
  },
  "forge": { "version": "14.23.5.2854" },
  "untrackedFiles": []
}
```

**Untracked files** (선택): glob 패턴(minimatch 방식, `*`는 `/`를 건너지 않음)에 매칭되는
파일을 표시합니다. `appliesTo`에는 구조 디렉터리 이름(`files`, `forgemods`, `fabricmods`,
`neoforgemods`, `libraries` 등)이 올 수 있습니다.

```json
{
  "untrackedFiles": [
    { "appliesTo": ["files"], "patterns": ["config/*.cfg", "config/**/*.yml"] }
  ]
}
```

업스트림은 매칭된 파일에 다이제스트를 전혀 붙이지 않지만, Nebula Rust 포트는 기본적으로
**항상** MD5/SHA-256을 계산하고 대신 모듈에 `"untracked": true`를 붙입니다. 자세한 내용은
[8. Hambugui 호환성](#8-hambugui-호환성)을 참고하세요.

### JSON 스키마에 대한 참고

`$schema` 속성은 선택 사항이며, JSON 스키마 파일을 가리키는 URL입니다. VS Code 같은
에디터가 이를 이용해 검증·자동완성·속성 설명을 제공합니다. Nebula는 스키마를
`${ROOT}/schemas`에 저장하며, `init root`로 처음 생성되고 `generate schemas`로
갱신됩니다. 자세한 내용은 [JSON Schema 웹사이트](https://json-schema.org/)를 참고하세요.

## 7. 모드팩 가져오기

`generate server-import <id> <file>`은 `<ROOT>/modpacks/` 아래의 파일(또는 절대 경로)을
자동 감지해 CurseForge zip, Modrinth `.mrpack`, Prism/MultiMC 인스턴스 zip 중 하나로
가져옵니다. `generate server-curseforge`는 CurseForge 형식만 허용하는 별칭입니다.

이 형식 인식과 4절의 모드 관리 동작은 [Prism Launcher](https://github.com/PrismLauncher/PrismLauncher)
(GPL-3.0)를 **동작 참고용으로만** 사용합니다 — 엔드포인트·매개변수·엣지 케이스를 참고했을
뿐 코드는 전혀 복사하지 않았습니다.

| 형식 | 감지 기준(zip 내부 항목) | 로더 정보 출처 |
|---|---|---|
| CurseForge 팩 (`.zip`) | `manifest.json` | `minecraft.modLoaders[].id`: `forge-<v>`, `neoforge-<v>`, `fabric-<v>`. `quilt-<v>`나 1.20.1용 legacy NeoForge id(`neoforge-1.20.1-<v>`)는 지원하지 않아 오류를 냅니다. |
| Modrinth 팩 (`.mrpack`) | `modrinth.index.json` | `dependencies`: `minecraft`, `forge`, `neoforge`, `fabric-loader`. `quilt-loader`는 지원하지 않습니다. |
| Prism/MultiMC 인스턴스 (`.zip`) | `mmc-pack.json` | `components[].uid`: `net.minecraft`, `net.minecraftforge`, `net.neoforged`, `net.fabricmc.fabric-loader`. `org.quiltmc.quilt-loader`는 지원하지 않습니다. |

라우팅 규칙(required/optional/unsupported):

| 형식 | 필수 취급 | 선택(기본 켬) | 제외 |
|---|---|---|---|
| CurseForge | `required: true`인 항목 | `required: false`인 항목 | 없음(다운로드 실패 파일은 수동 목록) |
| Modrinth | `env.client`가 `required`이거나 누락 | `env.client == "optional"` | `env.client == "unsupported"`는 건너뜁니다 |
| Prism | `mods/*.jar` | `mods/*.jar.disabled` (`.disabled` 접미사를 떼고 `optionaloff/`로) | `logs/`, `crash-reports/`, `saves/`, `screenshots/`, `.fabric/`, `.mixin.out/`, `usercache.json`, `usernamecache.json`, `*.log`는 가져오지 않습니다 |

가져온 모든 파일은 서버 디렉터리에 배치됩니다(모드는 로더별 `required`/`optionalon`
디렉터리, 그 외 파일은 `files/` 아래 원래 상대 경로 그대로). Modrinth 팩은
`overrides/` 다음 `client-overrides/`를 순서대로 적용합니다(나중 것이 우선, `server-
overrides/`는 무시). 입력 파일은 `<ROOT>/modpacks/`(하위 폴더 포함) 또는 절대 경로로
지정합니다. 자동으로 받을 수 없는 파일(CurseForge의 `downloadUrl == null` 등)은 수동
다운로드 목록으로 출력되며, GUI 가져오기 화면도 같은 목록을 경고 상자에 표시합니다.

### CurseForge 이용 약관 안내

Nebula는 CurseForge 검색을 위한 자체 API 키를 배포판에 포함하지 않습니다 — 사용자가
[console.curseforge.com](https://console.curseforge.com)에서 직접 발급받은 키를 넣어야
CurseForge 검색·설치가 열립니다(4절). CurseForge API가 돌려준 데이터(제목, 아이콘 등)는
디스크에 캐시하지 않습니다 — `modindex.json`에는 사용자가 실제로 설치한 파일의 id와
해시만 남습니다. 원저작자가 제3자 배포를 허용하지 않은 CurseForge 파일은 설치 목록에
자동으로 받을 수 없다는 표시와 함께 수동 다운로드 링크·대상 폴더가 나타나며, 사용자가
직접 내려받아 그 폴더에 넣어야 합니다.

## 8. Hambugui 호환성

Hambugui 런처가 배포본을 그대로 받아들이려면 다음을 지켜야 합니다.

* **모든 아티팩트에 MD5와 SHA-256을 함께 씁니다.** Hambugui 런타임은 다이제스트가
  전혀 없는 아티팩트를 거부합니다(레거시 모드가 없습니다). Helios는 MD5만 보고,
  Hambugui는 SHA-256을 요구합니다.
* **런타임에는 HTTPS가 필수입니다.** Nebula는 `http://` URL을 경고로만 표시하지만
  (`BASE_URL=http://localhost:8080/` 같은 로컬 개발은 계속 동작해야 하기 때문입니다),
  `nebula validate --strict`에서는 이를 오류로 취급합니다.
* **경로는 깨끗한 상대 경로여야 합니다.** 선행 `/`, `..`, 드라이브 문자, UNC 경로,
  역슬래시는 모두 금지되며 `\` → `/`로 정규화됩니다.
* 배포 전에는 항상 `nebula validate --strict`로 검증하세요.
* **untracked 파일 결정**: [6절](#6-파일-구조-명세)에서 설명한 대로 항상 다이제스트를
  계산하고 `"untracked": true`를 붙이는 것이 기본 동작입니다. `generate distro
  --helios-untracked`로 업스트림의 다이제스트 없는 Helios 전용 동작으로 되돌릴 수
  있으며, 이 경우 `validate`는 다이제스트가 없는 모든 아티팩트를 오류로 보고합니다.
* **NeoForge는 런처 패치가 필요합니다.** Hambugui의 `hambugui-distribution`은
  `NeoForge`/`NeoForgeMod` 모듈 타입을 아직 모르므로 그대로는 배포본을 거부합니다.
  패치와 적용 방법은 [`docs/launcher/README.md`](docs/launcher/README.md)를 참고하세요.
  `validate`는 NeoForge 모듈이 있는 배포본마다 경고를 하나 추가합니다.
* **FG2(마인크래프트 1.7–1.12, Forge ≤ 14.23.5.2847)는 `.pack.xz` 대신 일반 jar를
  배포합니다.** Helios는 `.pack.xz`를 풀어서 jar의 MD5를 검사하고, Hambugui는
  내려받은 바이트 자체의 SHA-256과 길이를 검사하므로 압축 파일을 그대로 배포하면
  둘 중 하나를 만족시킬 수 없습니다. Nebula는 `.pack.xz`를 내려받아
  PackXZExtract로 풀고, 풀린 jar를 `repo/lib`에 저장한 뒤 `@jar` id와 jar 자체의
  크기/MD5/SHA-256으로 모듈을 냅니다. `validate`는 `@jar.pack.xz` id가 남아 있으면
  "Hambugui 런타임이 pack.xz 아티팩트를 검증할 수 없다"는 오류를 냅니다.

## 9. 배포 (publish)

`nebula publish --target <webdav|gdrive>`는 생성된 `distribution.json`이 실제로
참조하는 파일만 골라 업로드합니다. 캐시, 설치 프로그램 출력, 스키마는 올라가지
않습니다. 변경 추적은 `<ROOT>/publish-state.json`에 `대상 이름 → { 상대 경로 → SHA-256 }`
형태로 남으며, 기본적으로 이전 업로드와 SHA-256이 같은 파일은 건너뜁니다. `--all`을
쓰면 이 추적을 무시하고 전부 다시 올립니다.

### WebDAV (헤놀로지/Synology)

정적 파일 트리를 그대로 서빙하는 NAS에 적합하며, 업로드 URL과 공개 URL이 같은 트리를
가리키므로 **URL을 다시 쓰지 않습니다.** Synology/헤놀로지 DSM 설정 전체 절차는
[`SYNOLOGY_DEPLOY_GUIDE.md`](SYNOLOGY_DEPLOY_GUIDE.md)에 있습니다.

| 환경 변수 | 설명 |
|---|---|
| `NEBULA_WEBDAV_URL` | 업로드용 WebDAV 기본 URL (예: `https://nas.example.com:5006/nebula-dist/`). |
| `NEBULA_WEBDAV_USER` | WebDAV 계정. |
| `NEBULA_WEBDAV_PASSWORD` | WebDAV 비밀번호. |
| `NEBULA_WEBDAV_PUBLIC_URL` | 다운로드용 공개 URL. 비우면 `BASE_URL`을 그대로 씁니다. |

### Google Drive

드라이브는 CDN이 아니므로 **대안이지 권장 경로가 아닙니다.** 가능하면 WebDAV를
쓰세요. 사용하려면 먼저 데스크톱 OAuth 클라이언트를 만들어야 합니다.

1. [Google Cloud Console](https://console.cloud.google.com/)에서 프로젝트를 만들거나 선택합니다.
2. **OAuth 동의 화면**을 설정합니다. 사용자 유형은 **외부**를 선택하고, **테스트 사용자**에
   자기 Google 계정을 추가합니다.
3. **사용자 인증 정보** 메뉴로 이동합니다.
4. **사용자 인증 정보 만들기 → OAuth 클라이언트 ID**를 선택합니다.
5. 애플리케이션 유형으로 **데스크톱 앱**을 선택하고 이름을 정한 뒤 만듭니다.
6. 발급된 **클라이언트 ID**와 **클라이언트 시크릿**을 아래 환경 변수에 넣습니다.

| 환경 변수 | 설명 |
|---|---|
| `NEBULA_GDRIVE_CLIENT_ID` | 위에서 발급한 OAuth 클라이언트 ID. |
| `NEBULA_GDRIVE_CLIENT_SECRET` | 클라이언트 시크릿. |
| `NEBULA_GDRIVE_FOLDER_ID` | 업로드 대상 폴더 ID. 드라이브에서 폴더를 연 뒤, 주소창 URL의 마지막 세그먼트(`https://drive.google.com/drive/folders/<여기>`)입니다. |

그 다음 `nebula gdrive-auth`를 실행하면 브라우저가 열리고, 동의 후 리프레시 토큰이
설정 디렉터리의 `gdrive-token.json`에 저장됩니다(자세한 위치는
[4. GUI](#4-gui)의 설정 저장 위치 표 참고). 이후 `nebula publish --target gdrive`는
저장된 토큰으로 자동 인증합니다.

**주의할 점**: 드라이브는 CDN이 아닙니다. 100MB가 넘는 파일은 바이러스 검사 확인
페이지를 거치므로 Nebula는 `drive.usercontent.google.com/download?id=<id>&export=download&confirm=t`
형태의 직접 다운로드 URL을 씁니다. 인기 있는 파일은 파일별 다운로드 할당량 때문에
403이 날 수 있습니다. 공개 URL은 파일 ID를 기반으로 하므로 같은 파일을 갱신
업로드해도(파일 ID가 유지되는 한) URL은 그대로 유지됩니다. 업로드 후에는 URL을 다시
쓴 `<name>.gdrive.json` 사본이 함께 생성·업로드되며, 이 파일의 공개 URL이 런처에
배포할 배포본 URL입니다.

## 10. 원본과의 의도된 차이

- **`BASE_URL`은 항상 슬래시로 끝나도록 강제합니다.** 업스트림은 끝에 슬래시가 없으면
  마지막 경로 세그먼트를 조용히 잘라먹었습니다(`https://host/dist` → `dist`가 사라짐).
- **모드 정렬 방식(collation)이 다릅니다.** 업스트림은 `String.prototype.localeCompare`
  (ICU 정렬)를 쓰지만, 이 포트는 소문자로 변환 후 바이트 순서로 정렬합니다. 대소문자나
  선행 문장부호만 다른 이름에서 순서 차이가 생길 수 있습니다.
- **디렉터리 스캔을 정렬해서 수행합니다.** 업스트림은 정렬하지 않은 `readdir` 결과를
  그대로 썼습니다. 정렬은 결정론성을 높이기 위한 개선입니다.
- **`test` 디버그 명령이 없습니다.**
- **Forge 설치기는 기본적으로 헤드리스로 실행됩니다.** `--installClient=<dir>`을(반드시
  `=`로 붙여서) 우선 시도하고, 매니페스트가 생성되지 않으면 표준입력이 터미널이거나
  `NEBULA_GUI=1`일 때만 설치기 GUI로 폴백합니다. `NEBULA_FORGE_GUI_FALLBACK=0`으로 이
  폴백을 완전히 끌 수 있습니다(CI에서 창이 뜨는 것을 막기 위함).
- **log4j 보안 경고 후 15초 대기가 없습니다.** 업스트림은 취약한 log4j 버전을 쓰는
  Forge를 감지하면 15초간 대기했지만, 이 포트는 경고만 로그로 남기고 계속 진행합니다.
- **FG2(1.7–1.12)는 `.pack.xz` 대신 일반 jar를 배포합니다.** [8절](#8-hambugui-호환성) 참고.
- **서버 id는 단일 경로 세그먼트여야 합니다.** `/`, `\`, `:`, NUL, `..` 세그먼트를 포함할
  수 없습니다 — 서버 디렉터리 이름과 launcher 계약 양쪽에서 안전한 경로를 보장합니다.

## 11. 문제 해결

**Forge 설치기 창이 열렸습니다.** `generate server`가 헤드리스 설치에 실패해 GUI로
폴백한 경우입니다. 로그에 출력된 디렉터리를 그대로 설치 위치로 지정하세요. 이 폴백을
원하지 않으면 `NEBULA_FORGE_GUI_FALLBACK=0`을 설정하세요(헤드리스 설치가 실패하면
바로 오류로 끝납니다).

**"JAVA_EXECUTABLE is not set" 오류.** `.env`에 `JAVA_EXECUTABLE`을 설정하지 않은 채
Forge/NeoForge를 빌드하거나 모드를 스캔했을 때 나옵니다. Java 8 이상(NeoForge는 21
권장) 실행 파일의 전체 경로를 넣으세요.

**`validate` 오류 읽는 법.** 출력은 한 줄에 하나씩 `심각도 위치 메시지` 형태입니다
(예: `error servers[0].modules[3].artifact.path 잘못된 상대 경로`). `위치`는
`servers[i].modules[j]…` 형태의 JSON 경로입니다. `--strict` 없이는 경고였던 항목이
`--strict`에서는 오류가 될 수 있습니다(HTTP URL, SHA-256 누락 등). 마지막 줄의
`N artifacts, M with SHA-256, E errors, W warnings` 요약으로 상태를 확인하세요.

**다른 Nebula 작업이 이 서버를 사용 중입니다.** 다른 프로세스(또는 다른 작업)가 같은
서버의 `.nebula-lock`을 쥐고 있습니다. 대개는 그 작업이 끝날 때까지 기다리면 됩니다. 그
프로세스가 이미 죽었는데 잠금이 30분 넘게 남아 있다면 `.nebula-lock` 파일을 지우고 다시
시도하세요.

**요청이 너무 많습니다.** Modrinth/CurseForge의 속도 제한에 걸렸다는 뜻입니다. 잠시 뒤
다시 시도하세요(설치 작업 중에는 한 번 자동으로 재시도합니다).

**설치된 모드 목록의 `배포 불가` 표시.** 네임스페이스 디렉터리(`required`/`optionalon`/
`optionaloff`) 안에 `.jar`로 끝나지 않거나 열리지 않는(손상된) 파일이 있으면 배포본 생성
전체가 실패합니다. 그 파일을 지우거나 올바른 jar로 바꾸세요.

**네트워크가 필요한 테스트 실행.** 기본 `cargo test --workspace`는 오프라인 전용입니다.
Forge/Fabric/NeoForge 다운로드가 필요한 테스트는 `NEBULA_NET_TESTS=1`일 때만 켜집니다.

```bash
NEBULA_NET_TESTS=1 JAVA_EXECUTABLE="C:\...\java.exe" cargo test --workspace
```

## 12. 라이선스

이 저장소는 [MIT 라이선스](LICENSE)로 배포됩니다. 원저작자는 Daniel Scalzi(2019–2020,
`dscalzi/Nebula`)이며, Rust 포트 저작권은 LaeDev(2026)에 있습니다.

GUI 바이너리는 [Slint](https://slint.dev/)를 정적으로 링크합니다. Slint는
[Royalty-free License 2.0](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md)
조건으로 사용할 수 있습니다.

`libraries/java/` 아래 번들 jar(`Claritas.jar`, `PackXZExtract.jar`)는 각 원 프로젝트의
라이선스를 따릅니다. Nebula는 이 jar들을 바이너리에 내장(`include_bytes!`)해 실행 시
추출할 뿐이며, 라이선스를 재부여하지 않습니다.
