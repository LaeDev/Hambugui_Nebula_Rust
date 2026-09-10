# Hambugui 런처 NeoForge 파서 패치

Nebula가 만드는 `distribution.json`에 NeoForge 모듈이 들어 있으면 Hambugui 런처의 `main`은 그 파일을
**거부**합니다. `hambugui-distribution`의 `parse_module_type`은 모르는 `type` 값을 만나면
`DISTRIBUTION_CONTRACT_INVALID`로 실패하도록 되어 있고, 여기에는 Nebula가 새로 정의한 두 타입
`NeoForge`(로더 루트)와 `NeoForgeMod`(인스턴스 모드)가 포함되기 때문입니다.

이 디렉터리는 그 파서를 고치는 패치를 **두 가지 베이스**로 제공합니다. 이 저장소는
`H:\HambuguiLauncherRust`를 **읽기 전용**으로만 다루므로, 적용은 런처 관리자가 직접 수행합니다.

| 파일 | 베이스 | 적용 후 |
| --- | --- | --- |
| `hambugui-neoforge.patch` | 브랜치 `wip/distribution-neoforge-module-type` (`419a3af`) | 아래와 동일한 트리 |
| `hambugui-neoforge-from-main.patch` | `main` (현재 작업 트리) | 위와 동일한 트리 |

두 패치는 **같은 최종 트리**로 수렴합니다(패치 적용 결과를 `diff -r`로 대조해 동일함을 확인).
관리자는 자기 브랜치에 맞는 쪽 하나만 적용하면 됩니다.

## 먼저: `419a3af`는 컴파일되지 않습니다

WIP 브랜치를 읽기 전용으로 내보내서(`git archive 419a3af`) 그대로 테스트한 결과는 **실패**입니다.

```
error[E0425]: cannot find value `SHA` in this scope
   --> crates\hambugui-distribution\tests\nebula_contract.rs:229:185
error: could not compile `hambugui-distribution` (test "nebula_contract")
```

`SHA` 상수는 `tests/module_matrix.rs`에만 있고 `tests/nebula_contract.rs`에는 없습니다. WIP가 추가한
NeoForge 테스트가 `format!(... "{SHA}" ...)`를 쓰는 바람에 테스트 타깃이 빌드되지 않습니다.
`hambugui-neoforge.patch`는 이 줄을 같은 파일의 다른 테스트들과 동일한 스타일(해시 리터럴을 그대로 쓴
단일 raw 문자열)로 바꿔 **컴파일 오류를 고칩니다**. 즉 `419a3af`에는 이 패치가 필수입니다.

## 패치가 바꾸는 것

| 파일 | 변경 | main | 419a3af |
| --- | --- | --- | --- |
| `crates/hambugui-distribution/src/schema.rs` | `ModuleType`에 `NeoForge`, `NeoForgeMod` 추가, `parse_module_type` 분기 추가 | 필요 | 이미 있음 |
| `crates/hambugui-distribution/src/normalize.rs` | `type_base`: `NeoForge` → `libraries`, `NeoForgeMod` → `mods` | 필요 | 이미 있음 |
| `crates/hambugui-distribution/src/lib.rs` | `resolve_artifact_path`: `NeoForge` → 공용 루트, `NeoForgeMod` → 인스턴스 루트 | 필요 | 이미 있음 |
| `crates/hambugui-distribution/tests/module_matrix.rs` | 두 타입의 행 추가 | 필요 | 이미 있음 |
| `crates/hambugui-distribution/tests/module_matrix.rs` | 모든 행에 대해 `resolve_artifact_path` 목적지 루트를 검증하는 단언 추가 | 필요 | 필요 |
| `crates/hambugui-distribution/tests/nebula_contract.rs` | NeoForge Maven 좌표 → `libraries/...` 경로 파생 테스트 | 필요 | 컴파일 오류 수정 |
| `docs/distribution.md` | 지원 모듈 타입 목록과 "Path roots" 표에 두 타입 반영 | 필요 | 필요 |

`default_extension`은 이미 `_ => Some("jar")`가 기본값이라 두 타입 모두 `jar`로 처리됩니다.
따라서 이 함수는 **수정하지 않았습니다**.

`module_matrix.rs`를 함께 고치는 것은 ADR 0001의 요구사항입니다("Any new module type must update
`crates/hambugui-distribution/tests/module_matrix.rs` before being accepted"). 목적지 루트 단언은
`419a3af`에도 없던 것으로, `NeoForgeMod` → 인스턴스 루트 매핑을 검증하는 **유일한** 테스트입니다
(`419a3af`가 추가한 `nebula_contract.rs` 테스트는 `NeoForge` → 공용 루트만 덮습니다). 기존 타입 11개에
대해서도 그대로 통과합니다.

## 적용 방법

런처 저장소 루트(`H:\HambuguiLauncherRust`)에서 실행합니다. `<nebula>`는 이 저장소 경로입니다.

```powershell
cd H:\HambuguiLauncherRust

# 브랜치가 wip/distribution-neoforge-module-type (419a3af)일 때
git apply <nebula>\docs\launcher\hambugui-neoforge.patch

# 브랜치가 main일 때
git apply <nebula>\docs\launcher\hambugui-neoforge-from-main.patch

cargo test -p hambugui-distribution
cargo clippy -p hambugui-distribution --all-targets -- -D warnings
```

`patch` 계열 도구를 쓴다면 동일하게 `-p1`입니다.

```bash
patch -p1 --dry-run < <nebula>/docs/launcher/hambugui-neoforge.patch
```

### 검증 결과

| 항목 | 결과 |
| --- | --- |
| `main` 사본, 패치 전 | `cargo test -p hambugui-distribution` 11 pass (1 unit + 1 module_matrix + 9 nebula_contract) |
| `419a3af` 내보내기, 패치 전 | **컴파일 실패** (E0425 `SHA`) |
| `main` + `hambugui-neoforge-from-main.patch` | 11 pass, `cargo fmt --check` / `clippy -D warnings` 통과 |
| `419a3af` + `hambugui-neoforge.patch` | 11 pass |
| 두 패치 적용 결과 트리 | `diff -r` 동일 |
| `git apply --check` / `patch -p1 --dry-run` | 각자의 pristine 베이스에서 모두 통과 |

`module_matrix`와 `nebula_contract`는 테스트 함수 개수가 그대로라 pass 수는 11로 같습니다. 커버하는
모듈 타입 케이스는 11개 → 13개(+`NeoForge`, +`NeoForgeMod`)로 늘고, 모든 케이스에 목적지 루트 단언이
붙습니다. 단언이 무의미하지 않다는 것은 `resolve_artifact_path`에서 `NeoForgeMod`를 일부러 공용 루트로
바꿔 테스트가 실패하는 것(`common-root\instance\NeoForgeMod.jar` != `instance-root\...`)으로 확인했습니다.

## 적용 없이 검증하기

`scripts\verify-launcher-contract.ps1`은 런처 저장소를 건드리지 않고 같은 결과를 재현합니다.
임시 디렉터리에 `hambugui-distribution` / `hambugui-domain`을 복사하고, 그 사본에 패치를 적용한 뒤,
`scripts\launcher-contract-harness`를 빌드해서 지정한 파일을 파싱합니다.

```powershell
scripts\verify-launcher-contract.ps1 -Distribution fixtures\launcher\nebula-hardening-generated.json -Runtime
scripts\verify-launcher-contract.ps1 -Distribution fixtures\launcher\nebula-valid.json -Runtime
```

`-Patch`의 기본값은 `hambugui-neoforge-from-main.patch`입니다(런처 작업 트리가 `main`이라는 가정).
WIP 브랜치를 체크아웃한 상태라면 `-Patch docs\launcher\hambugui-neoforge.patch`를 넘기세요.
`-Runtime`은 `parse_runtime_distribution`까지 호출합니다(대상 기본값 `windows-x86_64`,
`-Target macos-aarch64`로 변경 가능).

두 번째 예시는 **실패가 정상**입니다. `nebula-valid.json`의 `com.example:common-lib:1.0.0` 라이브러리가
MD5만 가지고 있어서 런타임 계약이 `DISTRIBUTION_RUNTIME_UNTRUSTED`로 거부하며, 스크립트도 종료 코드 1을
반환합니다.

## `hambugui-launch`: 이미 NeoForge를 실행할 수 있습니다

이 패치는 **파서만** 고칩니다. 실행 커맨드라인은 `hambugui-launch`가 만드는데, 확인 결과 별도 패치가
필요하지 않습니다.

- `hambugui-launch`는 `hambugui-distribution`에 **의존하지 않습니다**(의존성은 `hambugui-domain`,
  `hambugui-java`, `serde`, `serde_json`, `url`). 저장소 전체에서 `ModuleType`을 참조하는 파일은
  `crates/hambugui-distribution/` 안의 5개 파일뿐이므로, `ModuleType`으로 분기하는 코드는 파서 밖에
  존재하지 않습니다. 즉 새 모듈 타입이 실행 경로를 깨뜨리지 않습니다.
- 대신 `manifest.rs`의 `resolve_version_profile(vanilla, loader, platform, common_root)`이 **버전
  매니페스트 JSON 자체**를 읽어 메인 클래스·클래스패스·JVM/게임 인자를 만듭니다. `main`에 이미
  `fixtures/launch/neoforge-21.1.250.json`과 `crates/hambugui-launch/tests/neoforge_profile.rs`가
  있고, 그 테스트는 `cpw.mods.bootstraplauncher.BootstrapLauncher` 메인 클래스, `maven.neoforged.net`
  라이브러리, `--fml.neoForgeVersion 21.1.250` 게임 인자를 확인합니다.

따라서 Nebula 쪽 계약은 하나입니다: **`VersionManifest` 모듈이 NeoForge 인스톨러가 만든 version json을
가공 없이 그대로 실어야 합니다**(현재 설계 그대로). `command.rs`의 `ModernModList::Forge` /
`ModernModList::Fabric`은 구식 모드 목록 파일 경로용이라 NeoForge에는 쓰이지 않습니다.
