# ARM64 Support Status

## Upstream Tracking

### PR #3270 — "Add the initial Arm64 support"

| Field | Value |
|------|-------|
| PR | [asterinas#3270](https://github.com/asterinas/asterinas/pull/3270) |
| Author | [@wanywhn](https://github.com/wanywhn) |
| Branch | [wanywhn/asterinas:arm64-support](https://github.com/wanywhn/asterinas/tree/arm64-support) |
| State | OPEN, not merged |
| Mergeable | ❌ Dirty (conflicts with current main) |
| Size | +4,475 / -49 lines, 80 files, 29 commits |
| Code origin | LLM-generated (author confirmed) |
| Author commitment | Will NOT maintain long-term |
| Upstream takeover | @lrh2000 plans to integrate with his own arm port |

### What the PR Adds

**OSTD (`ostd/src/arch/aarch64/`):**
- `boot/` — BSP entry, boot page tables
- `mm/` — ARM64 page tables (4-level paging), MMU setup
- `task/` — Context switching, FPU/SIMD save/restore
- `irq/` — GICv3 interrupt controller (uses third-party crate)
- `timer/` — ARM Generic Timer (EL1 physical)
- `trap/` — EL1 exception handling (sync/IRQ/FIQ/SError)
- `cpu/` — CPU features, SMP via PSCI
- `iommu/` — IOMMU stub
- `device/` — Device discovery via FDT
- `io/` — MMIO abstraction
- `power.rs` — PSCI power management (shutdown/reboot)
- `serial.rs` — PL011 UART console

**Kernel (`kernel/src/arch/aarch64/`):**
- Process / thread support
- Syscall table (EL0 → EL1)
- TLS handling (TPIDR_EL0)
- PCI enumeration
- VirtIO support
- TLB flush bugfix in `KVirtArea::drop()`

**OSDK:**
- Raw ARM64 `Image` format for QEMU Linux boot protocol
- Arm64 QEMU scheme in `OSDK.toml`
- GitHub Actions CI for arm64 lint + compile

## kei's Strategy

kei merges this branch via git (not patches). This means:

1. The full `ostd/src/arch/aarch64/` tree exists in kei's repo
2. We can modify any file directly
3. Upstream sync is `git merge`, not `quilt push`
4. When upstream eventually merges a different arm64 implementation, we
   rebase our BSP on top of the new arch code

## Known Issues in the arm64-support Branch

| Issue | Severity | kei Action |
|-------|----------|------------|
| All code LLM-generated | High | M2 audit: review every file, fix artifacts |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |
| Stale (behind upstream main) | Low | Regular sync rebase |
| LLM-style verbose comments | Low | Clean up during audit |

## QEMU Test Matrix

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
