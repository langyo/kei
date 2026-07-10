# kei — 项目状态与计划 (PLAN)

> 本文件于 **2026-07-04** 更新，记录项目当前状态、近期进展与后续计划。
> 原有详细计划已保留于文末「既有详细计划（存档）」。

## 1. 项目概述

- **名称**：`kei`
- **简介**：面向物联网的操作系统内核 —— 基于 Asterinas 的 RTOS 级设施，兼顾 Linux 生态接入。
- **远程仓库**：本地仓库（无 origin）
- **技术栈**：Rust / just
- **类别**：firmware

## 2. 当前状态

- **当前分支**：`dev`
- **工作区**：干净
- **最近提交时间**：2026-07-04
- **最近提交**：test: add kei+evernight E2E QEMU ignition test script
- **initramfs**：已构建（`test/initramfs/build/initramfs.cpio.gz`，aarch64 busybox + init）

## 3. 未提交改动

无。

## 4. 近期进展

### kei 内核完整启动 + 用户空间进程（2026-07-04）🎉

**重大里程碑**：kei 内核在 QEMU arm64 上完整启动并成功加载用户空间 ELF 进程。

通过 Docker QEMU 镜像（`qemu-system-aarch64`）在 QEMU virt（cortex-a72, GICv3, 2GB）上验证：

```
[kei] FDT parsed → DEVICE_TREE initialized
[ostd] frame::meta::init: max_paddr=0xC0000000 (正确的 3GB)
[ostd] init: DONE — GIC, timer, SMP, page tables, IRQ 全部通过
[kernel] 组件初始化: arch, thread, driver, net, sched, process, fs, security
[kernel] initramfs.cpio.gz 解包 → rootfs ready
[kernel] spawn_init_process: 用户空间 ELF 加载成功（init=/init）
```

**修复的问题**：
1. **FDT 内存区域溢出**：链接脚本 `KERNEL_VMA` 与 `kernel_loaded_offset()` 不匹配。重装 cargo-osdk 后链接脚本使用正确的 `0xffff800040080000`（线性映射基址），全量重编译后修复。`max_paddr` 从 `0x7fff40080000`（128TB）降至正确的 `0xC0000000`（3GB）。
2. **vbe_dispi x86 模块**：`kernel/src/lib.rs` 中 `mod vbe_dispi` 未门控，添加 `#[cfg(target_arch = "x86_64")]`。
3. **initramfs 架构错误**：`initramfs.py` 使用宿主机 x86-64 busybox。添加 `find_busybox(arch)` 函数，支持按架构选择 busybox。

### evernight 联调（2026-07-04）

与 aris + evernight 进行宿主机联调测试，验证 IoT 网关数据链路：

```
Modbus TCP sim → evernight sensor-poll → WebSocket → evernight-server
```

- evernight 二进制构建成功，device.register + device.telemetry 双向验证通过
- aris `ignition_test.py` 修复：`SENSOR_DATA_DIR` 注入 + Modbus TCP sim 帧解析

### 多架构构建验证（2026-07-04）

全部 4 种架构编译成功，产出有效 ELF 内核二进制：

| 架构 | OSDK Scheme | 状态 | 产物 |
|------|-------------|------|------|
| **aarch64** | `aarch64` | ✅ 完整启动 + 用户空间 | ELF 64-bit ARM aarch64 (7MB) |
| **x86_64** | `microvm` | ✅ 编译通过 | ELF 64-bit x86-64 (需 vDSO) |
| **riscv64** | `riscv` | ✅ 编译通过 | ELF 64-bit RISC-V (需 vDSO) |
| **loongarch64** | `loongarch` | ✅ 编译通过 | ELF 64-bit LoongArch |

> x86_64/riscv64 需要 `VDSO_LIBRARY_DIR` 环境变量指向预构建的 vDSO .so 文件。
> aarch64/loongarch64 不需要 vDSO（vdso 模块仅 x86_64/riscv64 启用）。

### evernight aarch64 交叉编译（2026-07-04）

- **evernight** 交叉编译成功：`aarch64-unknown-linux-musl`，12MB 静态链接 ELF
- 使用 musl.cc 交叉工具链 + `.cargo/config.toml` linker 配置
- 修复 AppContext feature 门控 bug（`capture`/`signaling` 字段未正确 cfg-gated）
- 功能集：`hardware,protocol,serial,sensor,s7comm,bin,api,vault,manifest,tunnel,remote-ssh`

### virtio-gpu 2D 显示驱动（2026-07-06）

**新增 kei 的第一个图形显示路径**：virtio-gpu 2D scanout 驱动，让内核能在 QEMU 窗口显示像素。

- `kernel/comps/virtio/src/device/gpu/`：全新 virtio-gpu 驱动（mod.rs spec 类型 + device.rs 驱动主体），实现 OASIS virtio-gpu 2D 协议
- `kernel/comps/framebuffer/src/framebuffer.rs`：重构 framebuffer 子系统支持 late-init + blit 后端（`FrameBufferBackend::Blit`），让设备探测时（晚于 boot）能 publish framebuffer
- 探测链路：`GET_DISPLAY_INFO` → `RESOURCE_CREATE_2D` → `RESOURCE_ATTACH_BACKING`（绑定客户机 DMA 缓冲区）→ `SET_SCANOUT` → `publish()` blit-backed `FrameBuffer`
- 内核 printk 经 `FramebufferConsole` 自动渲染到 QEMU 窗口
- QEMU 参数更新：`OSDK.toml` aarch64 scheme 加 `-device virtio-gpu-device`，显示后端由 `$QEMU_DISPLAY` 控制（默认 sdl，CI 用 none）
- `cargo osdk check`（x86_64）编译通过，仅剩预存的 inode/set_tls 错误

**已知限制**：
1. `/dev/fb0` mmap 对 blit 后端返回 ENODEV（用户态暂走 read/write ioctl）
2. `poll_response` 运行时 flush 路径有竞态（probe 阶段串行不受影响），已在代码注释记录
3. **aarch64 `cargo osdk build` 受阻于 cargo-osdk 0.18.0 的 base crate 隔离机制**：base crate 用 `ostd = "0.18.0"`（version）声明依赖，虽然 `[patch.crates-io]` 重定向到本地 ostd 源码，但依赖图仍用 crates.io 版本的 metadata（不含 kei fork 的 aarch64 target 依赖如 `fdt`/`unwinding`）。CJK 路径（`/mnt/d/源代码/...`）上 cargo-osdk 的 `get_cargo_metadata` spawn 会 panic。临时绕过：bind-mount 到 ASCII 路径（`/opt/kei`），但 fdt/unwinding 依赖解析仍需修复（可能需要给 base crate 注入 target-specific deps 或 patch cargo-osdk）

### 设备树（FDT）验证（2026-07-04）

QEMU virt FDT 包含标准 Linux 绑定的网络设备节点：

```
virtio_mmio@a000000 {
    dma-coherent;
    interrupts = <0x00 0x10 0x01>;    ← GIC SPI #16
    reg = <0x00 0xa000000 0x00 0x200>; ← MMIO 512 bytes
    compatible = "virtio,mmio";        ← 标准 Linux 绑定
};
```

- 16 个 virtio_mmio 插槽（0xa000000 – 0xa001e00），每个 512 字节
- GICv3 3-cell 中断格式，interrupt-parent 指向 /intc
- kei `aarch64.rs::probe_for_device()` 完整解析 compatible/reg/interrupts
- **完全兼容 Linux 设备树**（使用标准 DTB 绑定，非自定义格式）

### kei 内核用户空间 I/O 打通（2026-07-04）🎉🎉🎉

**kei 内核在 aarch64 QEMU 中实现了完整的用户空间 I/O。**

裸金属 aarch64 init 程序通过 `write(1, msg, 24)` syscall 成功在串口输出：
```
=== kei ignition ===
```

**根因与修复**：
- `dyn PerOpenFileOps` trait object 的 vtable 在 aarch64（nightly-2026-04-03）上无法正确 dispatch `FileOps::write_at` 到 `TtyFile::write_at`
- 修复：`sys_write()` 在 aarch64 上拦截 fd 1/2（stdout/stderr），直接通过 `pl011_send_byte()` 写 PL011 UART，绕过 vtable dispatch

**完整验证链路**：
1. PL011 UART 控制台注册（替换 TODO stub）→ `aster_console` 发现 "Uart-Console" ✅
2. 串口 Tty 设备创建 → `/dev/ttyS0` 在 RamFs 注册 ✅
3. init 进程 fd 0/1/2 连接到 `/dev/ttyS0` ✅
4. 用户空间 `write(1, buf, 24)` syscall → PL011 MMIO → 串口输出 ✅

** celestia-devtools 集成**：
- aris 和 kei 导入 `celestia-devtools.just`
- 共享 recipes：cache-guard、fmt-markdown、prefetch、cross-check
- 宿主机 QEMU/dtc/交叉编译器安装自动化（`setup_env.py`）
- `tests/e2e_qemu_ignition.sh`（177 行）：QEMU arm64 中 kei 内核启动 → evernight sensor-poll → gateway 全链路测试脚本
- evernight-server 作为 mock entelecheia gateway（8443 端口）
- QEMU user-mode NAT 网络（guest 10.0.2.15 ↔ host 10.0.2.2）
- evernight aarch64 二进制嵌入 initramfs（6.2MB cpio.gz）

### 既往提交

- fix: gate vbe_dispi module to x86_64 only (aarch64 build fix)
- feat: fix build/test pipeline + verify aarch64 QEMU boot
- milestone: kei Asterinas kernel FULLY BOOTS on aarch64 QEMU

## 5. 后续计划

### 短期
1. ~~**用户空间串口输出**——init 进程已加载但 stdout 未连接到串口~~ ✅ 已修复（`open_initial_console` 将 /dev/console 分配为 fd 0/1/2）
2. **busybox ELF TLS 加载**——busybox 的 TLS 段触发 `copy_from_slice::len_mismatch_fail`（FileSiz=0x40 vs 分配 buffer），需修复 ELF 加载器 TLS 处理
3. ~~**evernight aarch64 交叉编译**——构建 `aarch64-unknown-linux-musl` evernight 二进制~~ ✅ 已完成
4. ~~**kei + evernight 联调**——QEMU 中 kei 内核启动 → evernight 连接 gateway~~ ✅ 测试脚本已就绪

### 中期
1. M2 ARM64 Hardening：审计 ostd/src/arch/aarch64/，替换第三方 GICv3 crate
2. M2 SMP/PSCI 多核启动
3. M3 RK3566 BSP 驱动（GPIO / stmmac / DW UART）

### 长期
1. M2.4 在 NanoPi R3S 上运行 kei + evernight 全栈
2. 性能基准测试 vs Linux baseline

---

## 6. 桌面系统支持路线图（2026-07-10 制定）

### 6.1 kei 的职责

kei 作为纯内核，在桌面系统中负责：
- **syscall ABI**：Linux 兼容系统调用（已有 ~240 个实现）
- **`/dev/fb0` fbdev 设备**：用户态通过 mmap 直接写帧缓冲像素
- **virtio-gpu 2D scanout**：QEMU SDL 窗口显示
- **帧缓冲控制台**：内核态 ANSI 彩色 + Sixel 内联图像（已实现）

aris 作为系统中间件层，负责渲染引擎（Blitz + Vello CPU）、JS 引擎（Boa）、WASM 运行时（Wasmtime）、Linux ABI 兼容层。kei 不涉及渲染引擎和 ABI 兼容层。

### 6.2 kei 侧需要补全的功能

为了让 aris 的渲染管线能在 kei 上运行，kei 需要以下改动：

#### P0：`/dev/fb0` mmap 支持（阶段 2，~3-5 天）

当前 `/dev/fb0` 对 Blit 后端（virtio-gpu DMA buffer）返回 `ENODEV`。需要实现：

- `mmap` 系统调用对 `/dev/fb0` 文件描述符的支持
- 将 virtio-gpu 的 `FRAMEBUFFER` 静态数组映射到用户态地址空间
- 用户态写入后通过 `MS_SYNC` msync 或自动 flush 触发 `TRANSFER_TO_HOST_2D` + `RESOURCE_FLUSH`
- 文件位置：`kernel/src/device/fb.rs`（修改 mmap handler）+ `kernel/comps/virtio/src/aarch64_raw_gpu_probe.rs`（暴露 DMA buffer 的物理地址）

这是让 Blitz 的 Vello CPU 像素 buffer 能显示在 QEMU SDL 窗口上的关键依赖。

#### P1：缺失 syscall 补全（阶段 2，与 P0 并行）

| syscall | 用途 | 复杂度 |
|---------|------|--------|
| SYSV shm (`shmget/shmat/shmdt/shmctl`) | 部分多媒体/音频库 | 中 |
| `posix_spawn` | 进程创建（musl 会 fallback） | 低 |
| `prctl(PR_SET_VMA)` | 内存命名 | 低 |
| 扩展 `ioctl`（终端/PTY） | shell 支持 | 中 |

kei 已有的核心 syscall 覆盖足够运行 Servo/Blitz + Boa + Wasmtime：mmap/munmap/mprotect/mremap ✅、clone3+完整线程 ✅、futex 全操作 ✅、epoll 全家族 ✅、pipe2/socketpair ✅、TCP/UDP/Unix socket ✅、memfd_create ✅、getrandom ✅、/dev/shm POSIX shm ✅。

#### P2：DRM ioctl 框架（远期，按需）

如果需要 GPU 加速或窗口管理（Wayland compositor），需要实现：
- `/dev/dri/card0` + `/dev/dri/renderD128` 设备节点
- DRM ioctl 处理（`DRM_IOCTL_MODE_*`, `DRM_IOCTL_GEM_*`）
- virtio-gpu 3D 支持（VIRTIO_GPU_CAPSET_*）

阶段 1-5 不需要 DRM——Vello CPU 纯软件渲染 + `/dev/fb0` 足以显示网页。

### 6.3 数据流（kei 视角）

```
用户态（aris render 包）:
  Vello CPU render_to_buffer(&mut [u8] RGBA)
    → mmap /dev/fb0（kei 映射 virtio-gpu DMA buffer）
    → memcpy RGBA buffer 到 mmap 区域
    → msync 或写 ioctl FBIO_WAITFORVSYNC 触发 flush

内核态（kei）:
  /dev/fb0 mmap handler
    → 返回 FRAMEBUFFER 物理页的 user-accessible 映射
  msync / ioctl handler
    → 调用 flush_framebuffer()
    → virtio-gpu TRANSFER_TO_HOST_2D + RESOURCE_FLUSH
    → QEMU SDL 窗口更新
```

### 6.4 里程碑对齐

| 阶段 | kei 工作项 | aris 工作项 | 交付物 |
|------|-----------|------------|--------|
| 阶段 1 | — | aris Linux + WebKitGTK kiosk 验证 | QEMU 中网页截图 |
| 阶段 2 | `/dev/fb0` mmap + syscall 补全 | — | kei 上用户态写 fb0 → SDL 显示 |
| 阶段 3 | — | Blitz + Vello CPU 集成 → fb0 | aris Linux 上 Blitz 渲染 HTML |
| 阶段 4 | — | Boa + Wasmtime + WIT host | kei 上 tairitsu 组件渲染 |
| 阶段 5 | — | evernight 迁移 + 镜像组装 | 可部署 sdcard.img |
| 阶段 6 | DRM 框架（按需） | ABI 完整兼容层 | 任意 Linux 二进制可运行 |

---

# kei — Project Plan

## Goal

Maintain a production-ready Asterinas kernel fork for ARM64 embedded devices,
with comprehensive Board Support Packages and multi-architecture QEMU testing.

## Design: Independent Fork (Apple LLVM Model)

### Why Not Track Upstream?

| Approach | Pro | Con | Verdict |
|----------|-----|-----|---------|
| Regular merge tracking | Catch upstream API breaks early | Constant merge conflicts; resource-heavy | ❌ Too expensive for startup |
| Patch series (quilt) | Clean delta tracking | Fragile for 4475-line arch port; no IDE support | ❌ Wrong tool for scale |
| **Independent fork + squash vendor** | Full control; absorb upstream on our schedule | Must manually detect API breaks at vendor time | ✅ Best fit |

### How Vendoring Works

`scripts/vendor-upstream.sh` does **directory-level replacement**, not git merge:

```
1. Snapshot our code (ostd/src/arch/aarch64/, bsp/, board/, configs/, ...)
2. Delete ostd/, kernel/, osdk/ from kei tree
3. Check out fresh copies from upstream/main
4. Restore our snapshot on top
5. Fix any API breaks (compile errors from changed upstream APIs)
6. Commit as single "vendor: absorb asterinas <sha>"
```

This is exactly how Apple absorbs LLVM upstream: take the whole thing,
overlay Apple-specific changes, commit as one squashed point.

### What We Track vs. What We Own

```
kei tree:
│
├── ostd/                          ← VENDORED (replaced wholesale on upgrade)
│   └── src/arch/
│       ├── x86/                   ← comes with vendoring
│       ├── riscv/                 ← comes with vendoring
│       ├── loongarch/             ← comes with vendoring
│       └── aarch64/               ← OURS (preserved across vendoring)
│
├── kernel/                        ← VENDORED
│   └── src/arch/
│       └── aarch64/               ← OURS (preserved across vendoring)
│
├── osdk/                          ← VENDORED
├── bsp/                           ← OURS (never touched by vendoring)
├── board/ configs/                ← OURS
├── scripts/ docs/                 ← OURS
└── .vendored-upstream             ← tracks which upstream commit we're on
```

### Vendoring Frequency

- **Upstream asterinas**: Every 3-6 months, or when a critical fix lands
- **ARM64 code (wanywhn)**: One-time pull, then independent maintenance.
  Re-pull only if wanywhn makes significant improvements worth absorbing.

## Milestones

### M1 — Fork Bootstrap
- [x] Independent fork structure
- [x] Vendor script (squash/directory-replace model)
- [x] ARM64 pull script (point-in-time snapshot from wanywhn)
- [x] Multi-architecture QEMU test harness
- [x] First successful vendor + arm64 pull + aarch64 boot

> **Status** (2026-07-04): Kernel FULLY BOOTS on QEMU aarch64 (cortex-a72, virt, GICv3).
> All OSTD subsystems initialize successfully. Kernel components (arch, thread,
> driver, net, sched, process, fs, security) all pass. Initramfs unpacked to
> rootfs. User-space ELF process successfully loaded and spawned (init=/init).
> max_paddr = 0xC0000000 (correct 3GB for 2GB RAM + MMIO).
> Previous FDT region 6 overflow bug RESOLVED via linker script fix + clean rebuild.

### M2 — ARM64 Hardening
The wanywhn arm64 code is LLM-generated and QEMU-only. Hardening tasks:
- [x] Fix FDT memory region parsing (region 6 overflows PA space) ← **RESOLVED 2026-07-04**
- [ ] Audit all files in ostd/src/arch/aarch64/, fix LLM artifacts
- [ ] Replace third-party GICv3 crate with in-tree driver
- [ ] SMP / multi-core boot (PSCI secondary bring-up)
- [ ] Real hardware boot on NanoPi R3S (RK3566)
- [ ] Performance benchmarks vs Linux baseline
- [x] QEMU arm64 boot reaches user-space init ← **DONE 2026-07-04**
- [ ] Fix busybox TLS ELF loading (copy_from_slice panic)
- [ ] Connect user-space stdout to serial console

### M3 — RK3566 BSP
- [ ] GPIO (Rockchip GRF pinctrl)
- [ ] Dual Ethernet (stmmac / RK GMAC)
- [ ] UART (DW 8250)
- [ ] SPI / I2C / Watchdog
- [ ] SD/eMMC (DW MMC)

### M4 — Multi-Arch Expansion
- [ ] RISC-V: JH7110 BSP (VisionFive 2)
- [ ] ARMv7 evaluation
- [ ] x86_64: Intel N100 BSP

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| Upstream API breaks at vendor time | Medium | Vendor script + compile test + fix cycle |
| wanywhn arm64 code has subtle bugs | High | M2 audit milestone; real HW testing |
| Falling behind upstream features | Low | Periodic vendoring catches up in batches |
| Upstream ships different arm64 | Low | Evaluate at vendor time; adopt if better |

