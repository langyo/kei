<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>面向工業物聯網邊緣設備的 Rust OS 核心。</strong></p>

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

KEI 是面向 ARM64 和 RISC-V 邊緣設備的 Rust OS 核心。同時附帶面向 embassy 感測器節點的 `#![no_std]` 庫。

KEI 源自 [Asterinas（星綻）](https://github.com/asterinas/asterinas)，一個 Rust 框架核心。KEI 在其基礎上增加了 ARM64 板級支援、virtio-gpu 顯示、工業驅動和感測器節點通訊協定。

## 倉庫內容

| 組件 | 位置 | 說明 |
|------|------|------|
| **KEI 核心** | workspace root | ARM64/RISC-V Rust OS 核心 |
| **kei 庫** | `packages/kei/` | 面向 embassy 的 `#![no_std]` 庫 |

## 快速開始

```bash
just build        # 構建預設板卡
just test-all     # QEMU 啟動測試
```

## 授權條款

KEI 自身程式碼適用 SySL-1.0。引入的 Asterinas 程式碼適用 MPL-2.0。
