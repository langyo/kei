<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/kei/master/docs/logo.webp" alt="KEI" width="240" /></p>

<h1 align="center">KEI</h1>

<p align="center"><strong>Noyau OS Rust pour l'IoT industriel — dérivé d'Asterinas (星绽), avec bibliothèque no_std pour nœuds capteurs.</strong></p>

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
**Français** ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

## Introduction

KEI est un noyau OS Rust pour appareils edge ARM64 et RISC-V. Il exécute le broker [evernight](https://github.com/celestia-island/evernight) et fournit l'ABI syscall pour [aris](https://github.com/celestia-island/aris) (le moteur de navigateur). Il inclut aussi une bibliothèque `#![no_std]` pour les nœuds capteurs embassy.

KEI est dérivé d'[Asterinas (星绽)](https://github.com/asterinas/asterinas), un framekernel Rust.

## Contenu

| Composant | Emplacement | Description |
|-----------|-------------|-------------|
| **Noyau KEI** | racine workspace | Noyau OS Rust ARM64/RISC-V |
| **Bibliothèque kei** | `packages/kei/` | Bibliothèque `#![no_std]` pour embassy |

## Démarrage rapide

```bash
just build        # Compiler pour la carte par défaut
just test-all     # Test de démarrage QEMU
```

## Écosystème

- **[aris](https://github.com/celestia-island/aris)** — moteur de navigateur
- **[evernight](https://github.com/celestia-island/evernight)** — broker de protocoles industriels
- **[entelecheia](https://github.com/celestia-island/entelecheia)** — plateforme d'agents IA

## Licence

Code KEI : SySL-1.0. Code Asterinas importé : MPL-2.0.
