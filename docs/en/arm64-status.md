# ARM64 Support Status

## ARM64 Support

ARM64 support was contributed to the Asterinas project and is independently maintained in KEI.

### Current Capabilities

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

The ARM64 code is maintained directly in kei's repository. This means:

1. The full `ostd/src/arch/aarch64/` tree exists in kei's repo
2. We can modify any file directly
3. When upstream eventually merges a different arm64 implementation, we
   rebase our BSP on top of the new arch code

## Known Issues

| Issue | Severity | kei Action |
|-------|----------|------------|
| Code needs audit and hardening | High | M2 audit: review every file, fix issues |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |

## QEMU Test Matrix

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
