# kei Upstream Sync

## Overview

KEI is derived from [Asterinas (星绽)](https://github.com/asterinas/asterinas).
It periodically absorbs upstream changes through directory-level vendoring rather
than `git merge`, giving us full control over when and how upstream lands.

## Quick Sync

```bash
just vendor       # Absorb latest upstream asterinas
just versions     # Show current upstream baselines
```

After vendoring, fix any API breaks, test, and commit:

```bash
cargo check
just test-all
git add -A
git commit -m "vendor: absorb asterinas <sha>"
```

## What's Vendored

| Path | Source | On vendor |
|------|--------|-----------|
| `ostd/` (except `arch/aarch64/`) | upstream | Replaced |
| `kernel/` (except `arch/aarch64/`) | upstream | Replaced |
| `osdk/` `test/` `tools/` | upstream | Replaced |
| `ostd/src/arch/aarch64/` `kernel/src/arch/aarch64/` | kei | **Preserved** |
| `bsp/` `board/` `configs/` `scripts/` `docs/` | kei | **Preserved** |

## When to Vendor

- Routine: every 3–6 months
- Critical fix: vendor a pinned ref (`just vendor <sha>`)

## See Also

- [Building & Deploying](./deployment.md)
- [ARM64 Support Status](../arm64-status.md)
