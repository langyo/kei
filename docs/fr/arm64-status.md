# Statut du support ARM64

## Suivi de l'amont

### PR #3270 — « Add the initial Arm64 support »

| Champ | Valeur |
|-------|--------|
| PR | [asterinas#3270](https://github.com/asterinas/asterinas/pull/3270) |
| Auteur | [@wanywhn](https://github.com/wanywhn) |
| Branche | [wanywhn/asterinas:arm64-support](https://github.com/wanywhn/asterinas/tree/arm64-support) |
| État | OUVERT, non fusionné |
| Fusionnable | ❌ En conflit (conflits avec le main actuel) |
| Taille | +4 475 / -49 lignes, 80 fichiers, 29 commits |
| Origine du code | Généré par LLM (confirmé par l'auteur) |
| Engagement de l'auteur | Ne maintiendra PAS à long terme |
| Reprise en amont | @lrh2000 prévoit d'intégrer avec son propre port arm |

### Ce que la PR ajoute

**OSTD (`ostd/src/arch/aarch64/`) :**
- `boot/` — Entrée BSP, tables de pages de démarrage
- `mm/` — Tables de pages ARM64 (pagination à 4 niveaux), configuration MMU
- `task/` — Changement de contexte, sauvegarde/restauration FPU/SIMD
- `irq/` — Contrôleur d'interruptions GICv3 (utilise un crate tiers)
- `timer/` — Minuteur générique ARM (physique EL1)
- `trap/` — Gestion des exceptions EL1 (sync/IRQ/FIQ/SError)
- `cpu/` — Fonctionnalités CPU, SMP via PSCI
- `iommu/` — Stub IOMMU
- `device/` — Découverte des périphériques via FDT
- `io/` — Abstraction MMIO
- `power.rs` — Gestion d'alimentation PSCI (extinction/redémarrage)
- `serial.rs` — Console UART PL011

**Noyau (`kernel/src/arch/aarch64/`) :**
- Support des processus / threads
- Table des appels système (EL0 → EL1)
- Gestion TLS (TPIDR_EL0)
- Énumération PCI
- Support VirtIO
- Correctif de vidage TLB dans `KVirtArea::drop()`

**OSDK :**
- Format `Image` ARM64 brut pour le protocole de démarrage Linux QEMU
- Schéma QEMU Arm64 dans `OSDK.toml`
- CI GitHub Actions pour le lint + la compilation arm64

## Stratégie de kei

kei fusionne cette branche via git (pas via des correctifs). Cela signifie que :

1. L'arborescence complète `ostd/src/arch/aarch64/` existe dans le dépôt de kei
2. Nous pouvons modifier n'importe quel fichier directement
3. La synchronisation avec l'amont se fait via `git merge`, pas `quilt push`
4. Quand l'amont finira par fusionner une implémentation arm64 différente, nous
   rebasons notre BSP sur le nouveau code d'architecture

## Problèmes connus dans la branche arm64-support

| Problème | Gravité | Action kei |
|----------|---------|------------|
| Tout le code généré par LLM | Élevée | Audit M2 : examiner chaque fichier, corriger les artefacts |
| Crate GICv3 tiers | Moyenne | Remplacer par un pilote interne |
| Tests uniquement QEMU | Élevée | Démarrage sur matériel réel NanoPi R3S |
| Pas de SMP/multi-cœur | Moyenne | Ajouter le démarrage du CPU secondaire via PSCI |
| Obsolète (en retard sur le main amont) | Faible | Rebase de synchronisation régulier |
| Commentaires verbeux de style LLM | Faible | Nettoyer pendant l'audit |

## Matrice de test QEMU

| Machine QEMU | CPU | RAM | Démarrage | Notes |
|--------------|-----|-----|-----------|-------|
| virt | cortex-a55 | 2 Go | ✅ | Cible de test principale |
| virt | cortex-a72 | 2 Go | 🔲 | Valider sur plusieurs cœurs ARM |
| virt | max | 4 Go | 🔲 | Activer toutes les fonctionnalités ARM |
| sbsa-ref | max | 4 Go | 🔲 | Démarrage de type serveur |
