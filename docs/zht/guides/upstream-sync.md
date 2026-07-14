# kei 上游同步

## 概述

KEI 源自 [Asterinas（星綻）](https://github.com/asterinas/asterinas)。定期透過目錄級 vendoring 吸收上游變更。

## 快速同步

```bash
just vendor       # 吸收最新上游
just versions     # 查看目前上游基線
```

## 同步頻率

- 常規：每 3–6 個月
- 緊急修復：`just vendor <sha>`
