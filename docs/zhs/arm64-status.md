# ARM64 支持状态

## 上游跟踪

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

### 该 PR 新增的内容

**OSTD (`ostd/src/arch/aarch64/`):**
- `boot/` — BSP 入口，启动页表
- `mm/` — ARM64 页表（四级分页），MMU 设置
- `task/` — 上下文切换，FPU/SIMD 保存/恢复
- `irq/` — GICv3 中断控制器（使用第三方 crate）
- `timer/` — ARM 通用定时器（EL1 物理定时器）
- `trap/` — EL1 异常处理（sync/IRQ/FIQ/SError）
- `cpu/` — CPU 特性，通过 PSCI 实现 SMP
- `iommu/` — IOMMU 桩
- `device/` — 通过 FDT 发现设备
- `io/` — MMIO 抽象
- `power.rs` — PSCI 电源管理（关机/重启）
- `serial.rs` — PL011 UART 控制台

**内核 (`kernel/src/arch/aarch64/`):**
- 进程 / 线程支持
- 系统调用表（EL0 → EL1）
- TLS 处理（TPIDR_EL0）
- PCI 枚举
- VirtIO 支持
- `KVirtArea::drop()` 中的 TLB 刷新修复

**OSDK:**
- 面向 QEMU Linux 启动协议的原始 ARM64 `Image` 格式
- `OSDK.toml` 中的 Arm64 QEMU 方案
- 用于 arm64 lint 与编译的 GitHub Actions CI

## kei 的策略

kei 通过 git 合并该分支（而非补丁）。这意味着：

1. 完整的 `ostd/src/arch/aarch64/` 目录树存在于 kei 仓库中
2. 我们可以直接修改任意文件
3. 上游同步使用 `git merge`，而非 `quilt push`
4. 当上游最终合并不同的 arm64 实现时，我们将 BSP 变基到新的架构代码之上

## arm64-support 分支中的已知问题

| Issue | Severity | kei Action |
|-------|----------|------------|
| All code LLM-generated | High | M2 audit: review every file, fix artifacts |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |
| Stale (behind upstream main) | Low | Regular sync rebase |
| LLM-style verbose comments | Low | Clean up during audit |

## QEMU 测试矩阵

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
