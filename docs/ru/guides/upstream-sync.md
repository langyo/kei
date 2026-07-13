# kei Синхронизация с Upstream

## Обзор

KEI основан на [Asterinas (星绽)](https://github.com/asterinas/asterinas). Изменения из upstream поглощаются периодически через vendoring на уровне директорий.

## Быстрая синхронизация

```bash
just vendor       # Поглотить последний upstream
just versions     # Показать текущую базу upstream
```

## Частота

- Регулярно: каждые 3–6 месяцев
- Срочное исправление: `just vendor-ref <sha>`
