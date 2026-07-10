# ARM64 지원 현황

## 상류 추적

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

### 이 PR이 추가하는 내용

**OSTD (`ostd/src/arch/aarch64/`):**
- `boot/` — BSP 진입점, 부트 페이지 테이블
- `mm/` — ARM64 페이지 테이블(4단계 페이징), MMU 설정
- `task/` — 컨텍스트 스위칭, FPU/SIMD 저장/복원
- `irq/` — GICv3 인터럽트 컨트롤러(서드파티 crate 사용)
- `timer/` — ARM 제네릭 타이머(EL1 물리)
- `trap/` — EL1 예외 처리(sync/IRQ/FIQ/SError)
- `cpu/` — CPU 기능, PSCI를 통한 SMP
- `iommu/` — IOMMU 스텁
- `device/` — FDT를 통한 디바이스 검색
- `io/` — MMIO 추상화
- `power.rs` — PSCI 전원 관리(종료/재부팅)
- `serial.rs` — PL011 UART 콘솔

**커널 (`kernel/src/arch/aarch64/`):**
- 프로세스 / 스레드 지원
- 시스콜 테이블(EL0 → EL1)
- TLS 처리(TPIDR_EL0)
- PCI 열거
- VirtIO 지원
- `KVirtArea::drop()`의 TLB 플러시 버그 수정

**OSDK:**
- QEMU Linux 부트 프로토콜용 원시 ARM64 `Image` 포맷
- `OSDK.toml`의 Arm64 QEMU 스킴
- arm64 lint + 컴파일용 GitHub Actions CI

## kei의 전략

kei는 이 브랜치를 git으로 병합합니다(패치가 아님). 이는 다음을 의미합니다:

1. 전체 `ostd/src/arch/aarch64/` 트리가 kei 리포지토리에 존재합니다
2. 모든 파일을 직접 수정할 수 있습니다
3. 상류 동기화는 `quilt push`가 아닌 `git merge`입니다
4. 상류가 결국 다른 arm64 구현을 병합하면, 새 아키텍처 코드 위에 BSP를 리베이스합니다

## arm64-support 브랜치의 알려진 문제

| Issue | Severity | kei Action |
|-------|----------|------------|
| All code LLM-generated | High | M2 audit: review every file, fix artifacts |
| Third-party GICv3 crate | Medium | Replace with in-tree driver |
| QEMU-only testing | High | Real hardware boot on NanoPi R3S |
| No SMP/multi-core | Medium | Add PSCI secondary CPU bring-up |
| Stale (behind upstream main) | Low | Regular sync rebase |
| LLM-style verbose comments | Low | Clean up during audit |

## QEMU 테스트 매트릭스

| QEMU Machine | CPU | RAM | Boot | Notes |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | Primary test target |
| virt | cortex-a72 | 2GB | 🔲 | Validate across ARM cores |
| virt | max | 4GB | 🔲 | Enable all ARM features |
| sbsa-ref | max | 4GB | 🔲 | Server-style boot |
