<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>نواة نظام تشغيل بـ Rust لأجهزة حافة إنترنت الأشياء الصناعي.</strong></p>

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
[Русский](../ru/README.md) ·
**العربية**

</div>

## مقدمة

KEI هو نواة نظام تشغيل بـ Rust لأجهزة الحافة ARM64 و RISC-V. يتضمن أيضًا مكتبة `#![no_std]` لعقد المستشعرات embassy.

KEI مشتق من [Asterinas](https://github.com/asterinas/asterinas)، نواة إطار بـ Rust.

## المحتويات

| المكون | الموقع | الوصف |
|--------|--------|------|
| **نواة KEI** | جذر workspace | نواة نظام تشغيل Rust لـ ARM64/RISC-V |
| **مكتبة kei** | `packages/kei/` | مكتبة `#![no_std]` لـ embassy |

## البدء السريع

```bash
just build        # بناء للوحة الافتراضية
just test-all     # اختبار إقلاع QEMU
```

## الترخيص

كود KEI: SySL-1.0. كود Asterinas المستورد: MPL-2.0.
