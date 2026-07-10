# ARM64 サポート状況

## 上流追跡

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

### この PR が追加するもの

**OSTD (`ostd/src/arch/aarch64/`):**
- `boot/` — BSP エントリ、ブートページテーブル
- `mm/` — ARM64 ページテーブル（4レベルページング）、MMU セットアップ
- `task/` — コンテキストスイッチ、FPU/SIMD の保存/復元
- `irq/` — GICv3 割り込みコントローラ（サードパーティ crate 使用）
- `timer/` — ARM ジェネリックタイマ（EL1 フィジカル）
- `trap/` — EL1 例外処理（sync/IRQ/FIQ/SError）
- `cpu/` — CPU 機能、PSCI による SMP
- `iommu/` — IOMMU スタブ
- `device/` — FDT によるデバイス検出
- `io/` — MMIO 抽象化
- `power.rs` — PSCI 電源管理（シャットダウン/リブート）
- `serial.rs` — PL011 UART コンソール

**カーネル (`kernel/src/arch/aarch64/`):**
- プロセス / スレッドサポート
- システムコールテーブル（EL0 → EL1）
- TLS 処理（TPIDR_EL0）
- PCI 列挙
- VirtIO サポート
- `KVirtArea::drop()` の TLB フラッシュバグ修正

**OSDK:**
- QEMU Linux ブートプロトコル用の生 ARM64 `Image` フォーマット
- `OSDK.toml` の Arm64 QEMU スキーム
- arm64 の lint + コンパイル用 GitHub Actions CI

## kei の戦略

kei はこのブランチを git でマージします（パッチではありません）。これは次を意味します：

1. 完全な `ostd/src/arch/aarch64/` ツリーが kei のリポジトリに存在します
2. 任意のファイルを直接変更できます
3. 上流同期は `git merge` であり、`quilt push` ではありません
4. 上流が最終的に異なる arm64 実装をマージした場合、新しいアーキテクチャコードの上に BSP をリベースします

## arm64-support ブランチの既知の問題

| Issue | Severity | kei Action |
|-------|----------|------------|
| All code LLM-generated | High | M2 audit: review every file, fix artifacts |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |
| Stale (behind upstream main) | Low | Regular sync rebase |
| LLM-style verbose comments | Low | Clean up during audit |

## QEMU テストマトリクス

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
