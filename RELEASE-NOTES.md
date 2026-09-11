# Nebula 배포본 릴리스 노트

## 1.0.0 — Windows x86_64 포터블

### 이 배포본에 들어 있는 것

- `nebula.exe` — 콘솔 CLI. `nebula.exe gui`로 GUI도 띄웁니다.
- `nebula-gui.exe` — 콘솔 창 없이 GUI만 띄우는 실행 파일. 더블클릭용입니다.
- `README.md`, `SYNOLOGY_DEPLOY_GUIDE.md` — 사용법과 Synology 배포 안내.
- `LICENSE`, `THIRD-PARTY-NOTICES.md`, `licenses/` — MIT 라이선스와 제3자 구성 요소 고지
  (Slint, Pretendard, Lucide 등)와 글꼴·아이콘 라이선스 전문.
- `docs/launcher/` — Hambugui 런처의 NeoForge 파서 패치와 적용 안내.
- `SHA256SUMS.txt` — 이 폴더 안 모든 파일의 SHA-256.

설치 과정은 없습니다. 압축을 아무 폴더에나 풀면 그대로 실행됩니다.

### 요구 사항

- **Windows 10 이상, 64비트(x64).**
- **Java는 Forge/NeoForge 설치기를 돌릴 때만** 필요합니다(NeoForge는 Java 21 권장).
  Fabric 서버나 검증·배포만 한다면 Java 없이도 됩니다.
- 별도 런타임 설치는 필요 없습니다. 두 실행 파일은 모두 정적으로 링크되어 있습니다.

### 첫 실행 — SmartScreen 안내

두 실행 파일에는 코드 서명이 없습니다. 처음 실행하면 Windows SmartScreen이
"Windows의 PC 보호"라는 파란 창을 띄울 수 있습니다.
**추가 정보 → 실행**을 누르면 실행됩니다. 파일이 맞는지 확인하려면 먼저
`SHA256SUMS.txt`의 해시와 릴리스 페이지에 적힌 해시를 비교하세요.

```powershell
Get-FileHash .\nebula-gui.exe -Algorithm SHA256
```

### 실행 방법

- GUI: `nebula-gui.exe` 더블클릭 (또는 콘솔을 함께 보려면 `nebula.exe gui`)
- CLI: PowerShell에서 `.\nebula.exe --help`

### 설정이 저장되는 곳

- 기본값: `%APPDATA%\nebula\` — `settings.json`, Google Drive 토큰(`gdrive-token.json`).
- `NEBULA_CONFIG_DIR` 환경 변수를 지정하면 그 폴더를 그대로 씁니다(USB 포터블 사용에 유용).
- `nebula-gui.exe`는 콘솔이 없으므로 로그를 그 폴더의 `nebula-gui.log`에 씁니다.
  이 파일은 실행할 때마다 새로 쓰입니다. 문제를 신고할 때 이 파일을 첨부하세요.
