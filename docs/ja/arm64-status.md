# ARM64 サポート状況

## ARM64 サポート

ARM64 サポートは Asterinas プロジェクトに貢献され、KEI で独立してメンテナンスされています。

### 現在の機能

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

ARM64 コードは kei のリポジトリで直接メンテナンスされています。これは次を意味します：

1. 完全な `ostd/src/arch/aarch64/` ツリーが kei のリポジトリに存在します
2. 任意のファイルを直接変更できます
3. 上流が最終的に異なる arm64 実装をマージした場合、新しいアーキテクチャコードの上に BSP をリベースします

## 既知の問題

| 問題 | 重要度 | kei の対応 |
|-------|----------|------------|
| コードの監査と強化が必要 | 高 | M2 監査: 全ファイルをレビュー |
| サードパーティ GICv3 crate | 中 | 内蔵ドライバに置き換え |
| QEMU のみのテスト | 高 | NanoPi R3S での実機起動 |
| SMP/マルチコア未対応 | 中 | PSCI セカンダリ CPU 起動を追加 |

## QEMU テストマトリクス

| QEMU マシン | CPU | RAM | 起動 | 備考 |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | 主要テストターゲット |
| virt | cortex-a72 | 2GB | 🔲 | ARM コア間検証 |
| virt | max | 4GB | 🔲 | 全 ARM 機能を有効化 |
| sbsa-ref | max | 4GB | 🔲 | サーバースタイル起動 |
