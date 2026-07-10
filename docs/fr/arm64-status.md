# Statut du support ARM64

## Support ARM64

Le support ARM64 a été contribué au projet Asterinas et est maintenu indépendamment dans KEI.

### Capacités actuelles

**OSTD (`ostd/src/arch/aarch64/`) :**
- `boot/` — Entrée BSP, tables de pages de démarrage
- `mm/` — Tables de pages ARM64, configuration MMU
- `task/` — Changement de contexte, FPU/SIMD
- `irq/` — Contrôleur GICv3
- `timer/` — Minuteur générique ARM (EL1)
- `trap/` — Gestion des exceptions EL1
- `cpu/` — Fonctionnalités CPU, SMP via PSCI
- `device/` — Découverte via FDT
- `power.rs` — Gestion PSCI
- `serial.rs` — Console PL011 UART

**Noyau (`kernel/src/arch/aarch64/`) :**
- Processus/threads, appels système, TLS, PCI, VirtIO

**OSDK :**
- Format ARM64 `Image`, schéma QEMU Arm64, CI GitHub Actions

## Stratégie KEI

Le code ARM64 est maintenu directement dans le dépôt KEI.

## Problèmes connus

| Problème | Gravité | Action |
|----------|---------|--------|
| Audit et renforcement nécessaires | Élevée | Audit M2 |
| Crate GICv3 tiers | Moyenne | Remplacer par pilote interne |
| Tests QEMU uniquement | Élevée | Démarrage sur NanoPi R3S |
| Pas de SMP/multi-cœur | Moyenne | Ajout PSCI secondaire |

## Matrice de test QEMU

| Machine QEMU | CPU | RAM | Boot | Notes |
|--------------|-----|-----|------|-------|
| virt | cortex-a55 | 2 Go | ✅ | Cible principale |
| virt | cortex-a72 | 2 Go | 🔲 | Validation multi-cœurs |
| virt | max | 4 Go | 🔲 | Toutes fonctionnalités ARM |
| sbsa-ref | max | 4 Go | 🔲 | Démarrage serveur |
