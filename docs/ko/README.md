<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>IoT 지향 OS 커널 — Asterinas 위의 RTOS 규율, Linux 생태계 접근 포함</strong></p>

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

KEI는 산업용 IoT를 위해 목적 구축된 운영체제 커널입니다. Asterinas를 가져와 RTOS 스타일의 설비 — 작고, 실시간이며, 감사 가능한 — 로 빚어내면서도, Linux 생태계로의 다리를 유지하여 기존 드라이버·도구·바이너리를 계속 손에 둡니다. Linux 배포판도 아니고 날것의 Asterinas도 아닙니다. 가장 가까운 유사품은 '우연히 Linux를 구사하는 RTOS'입니다 — 필요한 워크로드에는 실시간 결정론을, 그 외 모든 곳에는 Linux 급 소프트웨어 호환성을 제공합니다.

## 포크 모델

kei는 상류를 추적하는 브랜치가 **아닙니다**. 독립적인 포크로서 자체 일정에 따라 주기적으로
상류 변경 사항을 흡수합니다 —— Apple이 자사 LLVM 포크에 사용하는 모델과 동일합니다.

```mermaid
flowchart LR
    UP["asterinas/asterinas\n(활성 상류)"] -->|vendor-upstream.sh\nN개월마다 스쿼시| KEI["kei(이 리포지토리)\n완전히 독립"]
    WNY["wanywhn/asterinas\n(arm64-support)"] -->|pull-arm64.sh\n일회성 스냅샷| KEI
```

kei는 `ostd/src/arch/aarch64/`, `kernel/src/arch/aarch64/`,
`bsp/`, `board/`, `configs/`, `docs/`를 독자적으로 유지 관리합니다.

## 빠른 시작

```bash
just setup        # Configure git remotes
just vendor       # Absorb latest upstream asterinas (squash)
just pull-arm64   # Pull ARM64 code from wanywhn fork (one-time)
just versions     # Show what upstream versions we're based on
just build        # Build kernel for nanopi-r3s (aarch64)
just test-all     # Boot-test all architectures in QEMU
```

## 디렉토리 안내

| 디렉토리 | 출처 | 유지 보수 |
|-----------|--------|-------------|
| `ostd/` | 상류 asterinas | 주기적 벤더링, 버그는 즉석 수정 |
| `ostd/src/arch/aarch64/` | wanywhn 포크(PR #3270) | **독립** —— 직접 관리 |
| `kernel/` | 상류 asterinas | 주기적 벤더링 |
| `kernel/src/arch/aarch64/` | wanywhn 포크(PR #3270) | **독립** —— 직접 관리 |
| `osdk/` | 상류 asterinas | 주기적 벤더링 |
| `bsp/` | kei | **100% 자체** —— 보드 지원 패키지 |
| `board/` `configs/` | kei | **100% 자체** —— 보드 정의 |
| `scripts/` `docs/` | kei | **100% 자체** —— 도구 및 문서 |

## 지원 아키텍처

| 아키텍처 | 상태 | QEMU 테스트 |
|------|--------|-----------|
| x86_64 | 상류 Tier 1 | ✅ q35 |
| aarch64 | kei 관리(PR #3270 기반) | ✅ virt/cortex-a55 |
| riscv64 | 상류 Tier 2 | ⚠️ virt/rv64 |
| loongarch64 | 상류 Tier 3 | ⚠️ virt/max |

## 라이선스

SySL-1.0（Synthetic Source License）이 KEI 자체 코드에 적용됩니다 —— [LICENSE](../../LICENSE) 참조. 벤더링된 Asterinas 코드（`ostd/`, `kernel/`, `osdk/`）는 MPL-2.0으로 유지됩니다 —— [LICENSE-MPL](../../LICENSE-MPL) 참조.
