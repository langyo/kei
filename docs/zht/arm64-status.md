# ARM64 支援狀態

## ARM64 支援

ARM64 支援已貢獻給 Asterinas 專案，並在 KEI 中獨立維護。

### 當前能力

**OSTD (`ostd/src/arch/aarch64/`):**
- `boot/` — BSP 入口，啟動頁表
- `mm/` — ARM64 頁表（四級分頁），MMU 設定
- `task/` — 上下文切換，FPU/SIMD 儲存/還原
- `irq/` — GICv3 中斷控制器（使用第三方 crate）
- `timer/` — ARM 通用計時器（EL1 實體計時器）
- `trap/` — EL1 例外處理（sync/IRQ/FIQ/SError）
- `cpu/` — CPU 特性，透過 PSCI 實現 SMP
- `iommu/` — IOMMU 樁
- `device/` — 透過 FDT 發現裝置
- `io/` — MMIO 抽象
- `power.rs` — PSCI 電源管理（關機/重啟）
- `serial.rs` — PL011 UART 主控台

**核心 (`kernel/src/arch/aarch64/`):**
- 行程 / 執行緒支援
- 系統呼叫表（EL0 → EL1）
- TLS 處理（TPIDR_EL0）
- PCI 列舉
- VirtIO 支援
- `KVirtArea::drop()` 中的 TLB 清空修復

**OSDK:**
- 面向 QEMU Linux 啟動協定的原始 ARM64 `Image` 格式
- `OSDK.toml` 中的 Arm64 QEMU 方案
- 用於 arm64 lint 與編譯的 GitHub Actions CI

## kei 的策略

ARM64 程式碼直接在 kei 倉庫中維護。這意味著：

1. 完整的 `ostd/src/arch/aarch64/` 目錄樹存在於 kei 倉庫中
2. 我們可以直接修改任意檔案
3. 當上游最終合併不同的 arm64 實作時，我們將 BSP 變基到新的架構程式碼之上

## 已知問題

| Issue | Severity | kei Action |
|-------|----------|------------|
| Code needs audit and hardening | High | M2 audit: review every file |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |

## QEMU 測試矩陣

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
