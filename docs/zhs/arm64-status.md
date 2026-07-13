# ARM64 支持状态

## ARM64 支持

ARM64 支持已贡献给 Asterinas 项目，并在 KEI 中独立维护。

### 当前能力

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

ARM64 代码直接在 kei 仓库中维护。这意味着：

1. 完整的 `ostd/src/arch/aarch64/` 目录树存在于 kei 仓库中
2. 我们可以直接修改任意文件
3. 当上游最终合并不同的 arm64 实现时，我们将 BSP 变基到新的架构代码之上

## 已知问题

| 问题 | 严重程度 | kei 处理方式 |
|-------|----------|------------|
| 代码需审查与加固 | 高 | M2 审计：逐文件审查 |
| GICv3 第三方 crate | 中 | 替换为内置驱动 |
| 仅 QEMU 测试 | 高 | 在 NanoPi R3S 上真机启动 |
| 无 SMP/多核 | 中 | 添加 PSCI 次级 CPU 启动 |

## QEMU 测试矩阵

| QEMU 机器 | CPU | RAM | 启动 | 备注 |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | 主要测试目标 |
| virt | cortex-a72 | 2GB | 🔲 | 跨 ARM 核心验证 |
| virt | max | 4GB | 🔲 | 启用所有 ARM 特性 |
| sbsa-ref | max | 4GB | 🔲 | 服务器风格启动 |
