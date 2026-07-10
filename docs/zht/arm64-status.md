# ARM64 支援狀態

## 上游追蹤

### PR #3270 —— "Add the initial Arm64 support"

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

### 該 PR 新增的內容

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

kei 透過 git 合併該分支（而非補丁）。這意味著：

1. 完整的 `ostd/src/arch/aarch64/` 目錄樹存在於 kei 倉庫中
2. 我們可以直接修改任意檔案
3. 上游同步使用 `git merge`，而非 `quilt push`
4. 當上游最終合併不同的 arm64 實作時，我們將 BSP 變基到新的架構程式碼之上

## arm64-support 分支中的已知問題

| Issue | Severity | kei Action |
|-------|----------|------------|
| All code LLM-generated | High | M2 audit: review every file, fix artifacts |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |
| Stale (behind upstream main) | Low | Regular sync rebase |
| LLM-style verbose comments | Low | Clean up during audit |

## QEMU 測試矩陣

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
