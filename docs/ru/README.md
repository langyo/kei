<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>Ядро ОС на Rust для промышленных IoT edge-устройств.</strong></p>

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
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
**Русский** ·
[العربية](../ar/README.md)

</div>

## Введение

KEI — ядро ОС на Rust для edge-устройств ARM64 и RISC-V. Включает библиотеку `#![no_std]` для сенсорных узлов embassy.

KEI основан на [Asterinas](https://github.com/asterinas/asterinas), фрейм-ядре на Rust.

## Содержимое

| Компонент | Расположение | Описание |
|-----------|-------------|----------|
| **Ядро KEI** | корень workspace | Ядро ОС Rust ARM64/RISC-V |
| **Библиотека kei** | `packages/kei/` | `#![no_std]` библиотека для embassy |

## Быстрый старт

```bash
just build        # Сборка для платы по умолчанию
just test-all     # Загрузочный тест QEMU
```

## Лицензия

Код KEI: SySL-1.0. Импортированный код Asterinas: MPL-2.0.
