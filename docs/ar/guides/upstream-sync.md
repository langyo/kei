# kei مزامنة المنبع

## نظرة عامة

KEI مشتق من [Asterinas (星绽)](https://github.com/asterinas/asterinas). يتم استيعاب تغييرات المنبع دوريًا عبر vendoring على مستوى الدليل.

## مزامنة سريعة

```bash
just vendor       # استيعاب آخر منبع
just versions     # عرض خط الأساس الحالي للمنبع
```

## التكرار

- روتيني: كل 3–6 أشهر
- إصلاح عاجل: `just vendor-ref <sha>`
