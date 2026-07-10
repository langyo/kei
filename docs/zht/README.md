<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>面向物聯網的作業系統核心 —— 基於 Asterinas 的 RTOS 級設施，兼顧 Linux 生態接入</strong></p>

<div align="center">

[![License: SySL](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![License: MPL-2.0](https://img.shields.io/badge/vendored-MPL--2.0-blue)](../../LICENSE-MPL)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/kei/ci.yml)](https://github.com/celestia-island/kei/actions/workflows/ci.yml)

</div>

<div align="center">

[English](../en/README.md) ·
[简体中文](../zhs/README.md) ·
**繁體中文** ·
[日本語](../ja/README.md) ·
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

## 簡介

KEI 是為工業物聯網打造的作業系統核心。它在 Asterinas 之上做成一套 RTOS 風格的設施——小、即時、可稽核——同時保留通往 Linux 生態的橋樑，讓既有的驅動、工具與二進位仍觸手可及。它既不是 Linux 發行版，也不是原版 Asterinas。最接近的類比是「一個恰好會說 Linux 的 RTOS」：需要即時確定性的負載得到即時確定性，其餘一切享有 Linux 級的軟體相容性。

## 分支模式

KEI **不是**追蹤上游的分支。它是一個獨立分支，按自己的節奏定期吸收上游變更 ——
與 Apple 維護其 LLVM 分支採用相同的模式。

```mermaid
flowchart LR
    UP["asterinas/asterinas\n（活躍上游）"] -->|vendor-upstream.sh\n每 N 個月壓縮一次| KEI["kei（本倉庫）\n完全獨立"]
    WNY["wanywhn/asterinas\n（arm64-support）"] -->|pull-arm64.sh\n一次性快照| KEI
```

KEI 獨立維護 `ostd/src/arch/aarch64/`、`kernel/src/arch/aarch64/`、
`bsp/`、`board/`、`configs/` 以及 `docs/`。

## 快速開始

```bash
just setup        # Configure git remotes
just vendor       # Absorb latest upstream asterinas (squash)
just pull-arm64   # Pull ARM64 code from wanywhn fork (one-time)
just versions     # Show what upstream versions we're based on
just build        # Build kernel for nanopi-r3s (aarch64)
just test-all     # Boot-test all architectures in QEMU
```

## 各目錄職責

| 目錄 | 來源 | 維護方式 |
|-----------|--------|-------------|
| `ostd/` | 上游 asterinas | 定期引入，缺陷就地修復 |
| `ostd/src/arch/aarch64/` | wanywhn 分支（PR #3270） | **獨立** —— 由我們維護 |
| `kernel/` | 上游 asterinas | 定期引入 |
| `kernel/src/arch/aarch64/` | wanywhn 分支（PR #3270） | **獨立** —— 由我們維護 |
| `osdk/` | 上游 asterinas | 定期引入 |
| `bsp/` | kei | **100% 自研** —— 板級支援包 |
| `board/` `configs/` | kei | **100% 自研** —— 板級定義 |
| `scripts/` `docs/` | kei | **100% 自研** —— 工具與文件 |

## 支援的架構

| 架構 | 狀態 | QEMU 測試 |
|------|--------|-----------|
| x86_64 | 上游 Tier 1 | ✅ q35 |
| aarch64 | kei 維護（源自 PR #3270） | ✅ virt/cortex-a55 |
| riscv64 | 上游 Tier 2 | ⚠️ virt/rv64 |
| loongarch64 | 上游 Tier 3 | ⚠️ virt/max |

## 授權條款

SySL-1.0（Synthetic Source License）適用於 KEI 自身程式碼 —— 見 [LICENSE](../../LICENSE)。引入的 Asterinas 程式碼（`ostd/`、`kernel/`、`osdk/`）仍適用 MPL-2.0 —— 見 [LICENSE-MPL](../../LICENSE-MPL)。
