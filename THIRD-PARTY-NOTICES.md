# 제3자 구성 요소 고지 (Third-party notices)

Nebula 배포본(`nebula.exe`, `nebula-gui.exe`)에는 아래 구성 요소가 포함되어 있거나
정적으로 링크되어 있습니다. Nebula 자체는 [MIT 라이선스](LICENSE)로 배포됩니다.

---

## 1. Slint (UI 툴킷)

`nebula-gui.exe`와 `nebula.exe gui`의 사용자 인터페이스는 [Slint](https://slint.dev/) 1.17.1을
정적으로 링크합니다. Slint는 SixtyFPS GmbH의 다중 라이선스 제품이며, Nebula는
**Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**
(`LicenseRef-Slint-Royalty-free-2.0`) 조건으로 배포합니다.

### 저작자 표시 (§2 License Conditions — Attribution)

이 라이선스 §2는 (a) 애플리케이션의 About 화면에 `AboutSlint` 위젯을 표시하거나,
(b) 바이너리를 내려받을 수 있는 공개 웹페이지에 Slint 저작자 표시 배지를 눈에 잘 띄게
표시할 것을 요구합니다. Nebula는 **(b)** 를 택했습니다. 배포본을 내려받는 GitHub 저장소
첫 화면([README.md](README.md) 상단)과 각 GitHub Release 본문에 다음 배지를 싣습니다.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

### 제한 (§3 Limitations)

Nebula는 데스크톱 애플리케이션에 통합된 형태로만 Slint를 배포하며, Slint를 단독으로
배포하지 않고, 임베디드 시스템용이 아니며, Slint의 API를 외부에 노출하지 않습니다
(바이너리 크레이트의 `gui` 모듈은 비공개입니다).

### 라이선스 전문

원문: `LICENSES/LicenseRef-Slint-Royalty-free-2.0.md` (slint-ui/slint), 아래는 그 전문입니다.

> # Slint Royalty-free Desktop, Mobile, and Web Applications License
>
> Version 2.0
>
> ## Preamble
>
> Slint is a toolkit that can be used to build user interfaces for applications. Slint (hereafter referred to as **Software**) is made available under different licenses by SixtyFPS GmbH incorporated at Oranienburger Str. 44, 16540 Hohen Neuendorf, Germany (**SixtyFPS**). The **Slint Royalty-free Desktop, Mobile, and Web Applications License** is suitable for those who develop desktop, mobile, or web applications and do not want to use open source components under copyleft licenses.
>
> ## 1. Grant of Rights
>
> SixtyFPS hereby grants You a world-wide, royalty-free, non-exclusive license to use, reproduce, make available, modify, display, perform, distribute the Software as part of a Desktop, Mobile, or Web Application.
>
> A **Desktop Application** is a computer program that is designed to run on a general-purpose computer (PC or notebook), typically installed and executed locally on the computer's operating system.
>
> A **Mobile Application** is a computer program that is designed to run on a general-purpose mobile computer (mobile phone or tablet), typically installed and executed locally on the computer's operating system.
>
> A **Web Application** is a computer program that is designed to run in the sandbox environment provided by a web browser.
>
> Desktop Application, Mobile Application, and Web Application are hereafter referred to as **Application**.
>
> ## 2. License Conditions - Attribution
>
> You may distribute the Software as part of an Application, modified or unmodified, provided that You do either of the following:
>
> (a) Display the [`AboutSlint`](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/misc/aboutslint/) widget in an "About" screen or dialog that is accessible from the top level menu of the Application. In the absence of such a screen or dialog, display the widget in the "Splash Screen" of the Application.
>
> (b) Display the [Slint attribution badge](https://github.com/slint-ui/slint/tree/master/logo/MadeWithSlint-logo-whitebg.png) on a public webpage, preferably where the binaries of your Application can be downloaded from, in such a way that it can be easily found by any visitor to that page.
>
> ## 3. Limitations
>
> The License does not permit to distribute or make the Software publicly available alone and without integration into an Application. For this purpose you may use the Software under the GNU General Public License, version 3.
>
> The License does not permit the use of the Software within Embedded Systems. An **Embedded System** is a computer system designed to perform a specific task within a larger mechanical or electrical system.
>
> The License does not permit the distribution of Application that exposes the APIs, in part or in total, of the Software.
>
> You may not remove or alter any license notices (including copyright notices, disclaimers of warranty, or limitations of liability) contained within the source code form of the Software.
>
> ## 4. Warranty and Liability
>
> SixtyFPS is only liable for conflicting rights of third parties if SixtyFPS was aware of these rights without informing you. Unless required by applicable law or agreed to in writing, SixtyFPS provides the Software on an "as is" basis, without warranties or conditions of any kind, either express or implied, including, without limitation, any warranties or conditions of merchantability, or fitness for a particular purpose.
>
> Unless required by law, SixtyFPS won't be liable for any direct, indirect, incidental, or consequential damages arising in any way out of the use of the Software.

---

## 2. Pretendard (글꼴)

Copyright (c) 2021, Kil Hyung-jin (https://github.com/orioncactus/pretendard),
with Reserved Font Name Pretendard.

SIL Open Font License, Version 1.1 조건으로 바이너리에 내장되어 있습니다.
OFL은 이 고지와 라이선스 전문을 글꼴과 함께 배포할 것을 요구하므로, 전문은 배포본의
**`licenses/LICENSE-Pretendard.txt`** 에 함께 들어 있습니다(저장소 원본은
`crates/nebula/assets/fonts/LICENSE-Pretendard.txt`, 온라인 사본은 https://scripts.sil.org/OFL).
OFL은 글꼴을 단독으로 판매하는 것을 금지하고, 예약된 글꼴 이름(Pretendard)을 유지할 것을 요구합니다.

## 3. Lucide (아이콘)

Lucide icons (https://lucide.dev), ISC License. Copyright (c) 2022 Lucide Contributors.
고지 사본은 배포본의 **`licenses/LICENSE-Lucide.txt`** 에 함께 들어 있습니다.
ISC는 모든 복제본에 아래 저작권 표시와 허가 문구를 함께 실을 것을 요구합니다.

> Permission to use, copy, modify, and/or distribute this software for any purpose
> with or without fee is hereby granted, provided that the above copyright notice
> and this permission notice appear in all copies.
>
> THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
> REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
> FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
> INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
> OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
> TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
> THIS SOFTWARE.

전문 및 Feather(MIT)에서 유래한 부분의 저작권 표기는 https://lucide.dev/license 를 보세요.

## 4. 번들 Java 도구

`libraries/java/` 아래의 `Claritas.jar`, `PackXZExtract.jar`는 각 원 프로젝트의 라이선스를
따릅니다. Nebula는 이 jar들을 바이너리에 내장해 실행 시 임시 폴더로 추출할 뿐이며,
라이선스를 재부여하지 않습니다.

## 5. Rust 크레이트

바이너리는 여러 Rust 크레이트를 정적으로 링크합니다. 정확한 목록과 버전은 이 배포본을
만든 커밋의 `Cargo.lock`에 그대로 기록되어 있으며(재현 가능한 전체 목록),
CI의 `cargo deny check`가 다음 라이선스만 허용합니다 — 저장소 `deny.toml` 참고:

`Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `BSL-1.0`, `ISC`, `MIT`, `MIT-0`,
`Unicode-3.0`, `Zlib`, 그리고 위에 적은 `LicenseRef-Slint-Royalty-free-2.0`.

즉 이 배포본에는 카피레프트(GPL/LGPL 등) 구성 요소가 포함되어 있지 않습니다.
각 크레이트의 라이선스 전문은 crates.io의 해당 버전 페이지에서 확인할 수 있습니다.
