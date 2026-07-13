<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>Un núcleo de SO en Rust para dispositivos edge de IoT industrial.</strong></p>

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
**Español** ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

## Introducción

KEI es un núcleo OS Rust para dispositivos edge ARM64 y RISC-V. También incluye una biblioteca `#![no_std]` para nodos sensores embassy.

KEI deriva de [Asterinas](https://github.com/asterinas/asterinas), un framekernel Rust.

## Contenido

| Componente | Ubicación | Descripción |
|-----------|-----------|-------------|
| **Núcleo KEI** | raíz workspace | Núcleo OS Rust ARM64/RISC-V |
| **Biblioteca kei** | `packages/kei/` | Biblioteca `#![no_std]` para embassy |

## Inicio rápido

```bash
just build        # Compilar para placa por defecto
just test-all     # Prueba de arranque QEMU
```

## Licencia

Código KEI: SySL-1.0. Código Asterinas importado: MPL-2.0.
