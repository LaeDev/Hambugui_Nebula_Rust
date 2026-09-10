# Synology / 헤놀로지에 배포본 올리기

Nebula가 만든 `<ROOT>` 디렉터리를 시놀로지 NAS에서 HTTPS로 서빙해, Hambugui·Helios
런처가 그대로 내려받게 만드는 방법입니다. 런처는 `distribution.json`에 적힌 URL을
그대로 GET 하므로, **NAS가 `<ROOT>`를 그 URL 구조 그대로 보여주기만 하면** 끝입니다.

## 두 가지 방법

**A. 마운트 방식 (업로드 없음, 권장)**
NAS의 공유 폴더를 PC에 드라이브 문자로 마운트하고 `ROOT`를 그 안으로 지정합니다.
`nebula generate distro`가 파일을 쓰는 순간 Web Station이 바로 서빙하므로 업로드
단계 자체가 사라집니다. 같은 랜에서 작업하고 배포본이 크면 이쪽이 훨씬 빠릅니다.

```properties
ROOT=Z:\nebula-dist
BASE_URL=https://mc.example.com/
```

**B. WebDAV 업로드 방식**
작업 PC가 NAS와 떨어져 있거나 마운트를 상시 유지할 수 없을 때 씁니다.
`nebula publish --target webdav`가 `distribution.json`이 참조하는 파일만 골라
올립니다. 캐시·설치 프로그램 출력·스키마는 올라가지 않습니다.
업로드 URL과 공개 URL이 같은 트리를 가리키므로 **URL 재작성이 없습니다.**

## DSM 설정

1. **공유 폴더** — `제어판 > 공유 폴더 > 생성`으로 `nebula-dist`를 만듭니다.
   휴지통은 꺼 두는 편이 좋습니다(배포본은 다시 만들면 그만입니다).
2. **전용 계정** — `제어판 > 사용자 및 그룹`에서 배포 전용 계정(`nebula`)을 만들고
   `nebula-dist`에만 읽기/쓰기를 줍니다. 관리자 계정은 쓰지 마세요.
3. **Web Station** — 패키지 센터에서 설치한 뒤 `웹 서비스 > 생성`으로
   문서 루트를 `nebula-dist`로 지정하고, `웹 포털 > 생성`에서 가상 호스트
   (`mc.example.com`)를 이 웹 서비스에 연결합니다. HTTP 백엔드는 Nginx로 둡니다.
4. **인증서** — `제어판 > 보안 > 인증서 > 추가`에서 Let's Encrypt 인증서를
   `mc.example.com`으로 발급하고, `설정`에서 해당 가상 호스트에 배정합니다.
   발급에는 80/443 포트가 외부에서 열려 있어야 합니다.
5. **역방향 프록시 (선택)** — Web Station 포털을 443이 아닌 포트에 두었다면
   `제어판 > 로그인 포털 > 고급 > 역방향 프록시`로 `https://mc.example.com:443`을
   내부 포트에 연결합니다. HSTS를 켜도 런처 동작에는 영향이 없습니다.
6. **WebDAV Server** — 방법 B에서만 필요합니다. 패키지 센터에서 설치하고
   `HTTPS 활성화` 체크, 포트는 기본값 **5006**을 씁니다. HTTP(5005)는 끄세요.
   WebDAV의 경로 루트는 공유 폴더 목록이라 URL은 `https://호스트:5006/nebula-dist/`
   형태가 됩니다.

## `.env` 값

```properties
ROOT=D:\nebula-dist
BASE_URL=https://mc.example.com/

# 방법 B에서만
NEBULA_WEBDAV_URL=https://mc.example.com:5006/nebula-dist/
NEBULA_WEBDAV_USER=nebula
NEBULA_WEBDAV_PASSWORD=<비밀번호>
# 서빙 주소가 BASE_URL과 다를 때만 지정합니다. 비우면 BASE_URL을 씁니다.
NEBULA_WEBDAV_PUBLIC_URL=https://mc.example.com/
```

`BASE_URL`은 항상 슬래시로 끝나야 합니다. `https://mc.example.com/dist`처럼 쓰면
Nebula가 `/dist/`로 고쳐 주지만, 명시하는 편이 헷갈리지 않습니다.

`NEBULA_WEBDAV_URL`은 **업로드용**, `BASE_URL`/`NEBULA_WEBDAV_PUBLIC_URL`은
**다운로드용** 주소입니다. 둘이 같은 파일 트리를 가리키지 않으면 런처가 404를
받습니다. 위 예시에서 5006 포트는 `nebula-dist` 공유 폴더를,
443 포트는 그 폴더를 문서 루트로 삼은 가상 호스트를 가리키므로 서로 맞습니다.

## 배포 실행

```bash
nebula generate distro          # <ROOT>/distribution.json 생성
nebula validate --strict        # 런처 계약 검증 (HTTP URL을 오류로 취급)
nebula publish --target webdav  # 방법 B: 바뀐 파일만 업로드
nebula publish --target webdav --all   # 변경 추적 무시하고 전부 업로드
```

변경 추적은 `<ROOT>/publish-state.json`에 `대상 이름 → { 상대 경로 → SHA-256 }`으로
남습니다. 지우면 다음 배포 때 전부 다시 올라갑니다. 배포본을 통째로 옮겼거나
NAS 쪽 파일을 손으로 건드렸다면 지우고 다시 올리세요.

## 확인

```bash
# 아티팩트 하나가 실제로 200으로 응답하는지 (URL은 distribution.json에서 복사)
curl -I "https://mc.example.com/servers/Hambugui-1.20.1/files/config/client.cfg"
# 배포본 자체
curl -I "https://mc.example.com/distribution.json"
```

`200 OK`와 함께 `content-length`가 실제 파일 크기와 같아야 합니다. `401`이면 Web
Station 쪽에 인증이 걸린 것이고, `404`면 문서 루트와 `BASE_URL`이 어긋난 것입니다.
`nebula validate --strict`가 통과하고 위 두 요청이 200이면 런처에 URL을 넘겨도
됩니다.

## 주의할 점

- **헤놀로지(XPEnology)** 는 비공식 DSM입니다. Web Station·WebDAV Server·인증서
  발급은 정품 DSM과 동일하게 동작하지만, 패키지 센터에서 일부 항목이 보이지 않거나
  DSM 업데이트로 부팅이 막힐 수 있습니다. 배포본은 NAS 밖에도 사본을 두세요.
- **QuickConnect 주소는 쓰지 마세요.** 중계 서버를 거치기 때문에 느리고, 대용량
  파일에서 끊기며, 주소가 바뀌면 배포본의 모든 URL이 죽습니다. 도메인 + 포트포워딩
  (80/443, 방법 B는 5006 추가)으로 직접 연결하세요.
- **HTTPS는 선택이 아닙니다.** Hambugui 런처 런타임은 `http://` 아티팩트 URL을
  거부합니다. Nebula는 경고만 내지만 `validate --strict`에서는 오류입니다.
  (`BASE_URL=http://localhost:8080/` 같은 로컬 테스트는 계속 됩니다.)
- 공유기 방화벽과 DSM 방화벽(`제어판 > 보안 > 방화벽`) 양쪽을 열어야 합니다.
  WebDAV 5006 포트는 업로드하는 PC의 IP로만 제한하는 편이 안전합니다.
- 파일 이름의 공백·`#`·한글은 그대로 써도 됩니다. Nebula가 URL을 퍼센트 인코딩하고
  WebDAV 업로드 경로에도 같은 인코딩을 씁니다.

## Google Drive는?

`nebula publish --target gdrive`도 있지만 **대안이지 권장 경로가 아닙니다.**
드라이브는 CDN이 아니라서 100MB가 넘는 파일은 바이러스 검사 페이지를 거치고,
인기 있는 팩은 파일별 다운로드 할당량 때문에 403이 날 수 있습니다. 또 공개 URL이
파일 ID라서 `distribution.<대상>.json`으로 URL을 다시 쓴 사본을 만들어 올립니다.
OAuth 설정 절차와 제약은 README의 배포(publish) 절을 보세요. 가능하면 이 문서의
방법 A나 B를 쓰세요.
