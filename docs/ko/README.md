<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>산업용 IoT 엣지 디바이스를 위한 Rust OS 커널.</strong></p>

<div align="center">

[![License: SySL](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![License: MPL-2.0](https://img.shields.io/badge/vendored-MPL--2.0-blue)](../../LICENSE-MPL)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/kei/ci.yml)](https://github.com/celestia-island/kei/actions/workflows/ci.yml)

</div>

<div align="center">

[English](../en/README.md) ·
[简体中文](../zhs/README.md) ·
[繁體中文](../zht/README.md) ·
[日本語](../ja/README.md) ·
**한국어** ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

## 소개

KEI는 ARM64 및 RISC-V 엣지 디바이스를 위한 Rust OS 커널입니다. embassy 센서 노드용 `#![no_std]` 라이브러리도 포함합니다.

KEI는 [Asterinas](https://github.com/asterinas/asterinas)에서 파생된 Rust 프레임커널입니다.

## 저장소 내용

| 컴포넌트 | 위치 | 설명 |
|---------|------|------|
| **KEI 커널** | workspace root | ARM64/RISC-V Rust OS 커널 |
| **kei 라이브러리** | `packages/kei/` | embassy용 `#![no_std]` 라이브러리 |

## 빠른 시작

```bash
just build        # 기본 보드 빌드
just test-all     # QEMU 부트 테스트
```

## 라이선스

KEI 자체 코드는 SySL-1.0. 도입된 Asterinas 코드는 MPL-2.0.
