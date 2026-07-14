# kei 상류 동기화

## 개요

KEI는 [Asterinas(星绽)](https://github.com/asterinas/asterinas)에서 파생되었습니다. `git merge` 대신 디렉터리 수준 vendoring으로 주기적으로 상류 변경 사항을 흡수합니다.

## 빠른 동기화

```bash
just vendor       # 최신 상류 흡수
just versions     # 현재 상류 기준 표시
```

## 동기화 빈도

- 정기: 3~6개월마다
- 긴급 수정: `just vendor <sha>`
