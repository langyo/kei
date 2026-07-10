# kei 上游同步

## 概述

KEI 源自 [Asterinas（星绽）](https://github.com/asterinas/asterinas)。定期通过目录级 vendoring 吸收上游变更，而非 `git merge`。

## 快速同步

```bash
just vendor       # 吸收最新上游
just versions     # 查看当前上游基线
```

同步后修复 API 问题并提交：

```bash
cargo check
just test-all
git add -A
git commit -m "vendor: absorb asterinas <sha>"
```

## 同步频率

- 常规：每 3–6 个月
- 紧急修复：`just vendor-ref <sha>`
