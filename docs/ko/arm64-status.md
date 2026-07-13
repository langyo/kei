# ARM64 지원 현황

## ARM64 지원

ARM64 지원은 Asterinas 프로젝트에 기여되었으며 KEI에서 독립적으로 유지 관리됩니다.

### 현재 기능

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

ARM64 코드는 kei 리포지토리에서 직접 유지 관리됩니다. 이는 다음을 의미합니다:

1. 전체 `ostd/src/arch/aarch64/` 트리가 kei 리포지토리에 존재합니다
2. 모든 파일을 직접 수정할 수 있습니다
3. 상류가 결국 다른 arm64 구현을 병합하면, 새 아키텍처 코드 위에 BSP를 리베이스합니다

## 알려진 문제

| 문제 | 심각도 | kei 조치 |
|-------|----------|------------|
| 코드 감사 및 강화 필요 | 높음 | M2 감사: 모든 파일 검토 |
| 서드파티 GICv3 crate | 중간 | 내장 드라이버로 교체 |
| QEMU 전용 테스트 | 높음 | NanoPi R3S에서 실기기 부팅 |
| SMP/멀티코어 미지원 | 중간 | PSCI 보조 CPU 기동 추가 |

## QEMU 테스트 매트릭스

| QEMU 머신 | CPU | RAM | 부팅 | 비고 |
|-------------|-----|-----|------|-------|
| virt | cortex-a55 | 2GB | ✅ | 주요 테스트 대상 |
| virt | cortex-a72 | 2GB | 🔲 | ARM 코어 간 검증 |
| virt | max | 4GB | 🔲 | 모든 ARM 기능 활성화 |
| sbsa-ref | max | 4GB | 🔲 | 서버 스타일 부팅 |
