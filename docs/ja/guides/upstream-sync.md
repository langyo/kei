# kei 上流同期

## 概要

KEI は [Asterinas（星绽）](https://github.com/asterinas/asterinas) に由来します。`git merge` ではなくディレクトリレベルの vendoring で定期的に上流の変更を取り込みます。

## クイック同期

```bash
just vendor       # 最新の上流を吸収
just versions     # 現在の上流ベースラインを表示
```

## 同期頻度

- 定期: 3–6ヶ月ごと
- 緊急修正: `just vendor-ref <sha>`
