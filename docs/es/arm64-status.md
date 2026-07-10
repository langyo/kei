# Estado del soporte ARM64

## Seguimiento del upstream

### PR #3270 — « Add the initial Arm64 support »

| Campo | Valor |
|-------|-------|
| PR | [asterinas#3270](https://github.com/asterinas/asterinas/pull/3270) |
| Autor | [@wanywhn](https://github.com/wanywhn) |
| Rama | [wanywhn/asterinas:arm64-support](https://github.com/wanywhn/asterinas/tree/arm64-support) |
| Estado | ABIERTO, no fusionado |
| Fusionable | ❌ En conflicto (conflictos con el main actual) |
| Tamaño | +4.475 / -49 líneas, 80 archivos, 29 commits |
| Origen del código | Generado por LLM (confirmado por el autor) |
| Compromiso del autor | NO lo mantendrá a largo plazo |
| Toma de control upstream | @lrh2000 planea integrarlo con su propio port arm |

### Lo que añade la PR

**OSTD (`ostd/src/arch/aarch64/`) :**
- `boot/` — Entrada BSP, tablas de páginas de arranque
- `mm/` — Tablas de páginas ARM64 (paginación de 4 niveles), configuración MMU
- `task/` — Cambio de contexto, guardado/restauración FPU/SIMD
- `irq/` — Controlador de interrupciones GICv3 (usa un crate de terceros)
- `timer/` — Temporizador genérico ARM (físico EL1)
- `trap/` — Manejo de excepciones EL1 (sync/IRQ/FIQ/SError)
- `cpu/` — Funciones de CPU, SMP vía PSCI
- `iommu/` — Stub de IOMMU
- `device/` — Descubrimiento de dispositivos vía FDT
- `io/` — Abstracción MMIO
- `power.rs` — Gestión de energía PSCI (apagado/reinicio)
- `serial.rs` — Consola UART PL011

**Kernel (`kernel/src/arch/aarch64/`) :**
- Soporte de procesos / hilos
- Tabla de syscalls (EL0 → EL1)
- Manejo de TLS (TPIDR_EL0)
- Enumeración PCI
- Soporte VirtIO
- Corrección de bug de vaciado TLB en `KVirtArea::drop()`

**OSDK :**
- Formato `Image` ARM64 en bruto para el protocolo de arranque Linux de QEMU
- Esquema QEMU Arm64 en `OSDK.toml`
- CI de GitHub Actions para lint + compilación arm64

## Estrategia de kei

kei fusiona esta rama vía git (no mediante parches). Esto significa que :

1. El árbol completo `ostd/src/arch/aarch64/` existe en el repo de kei
2. Podemos modificar cualquier archivo directamente
3. La sincronización con upstream es `git merge`, no `quilt push`
4. Cuando upstream eventualmente fusiona una implementación arm64 diferente,
   hacemos rebase de nuestro BSP sobre el nuevo código de arquitectura

## Problemas conocidos en la rama arm64-support

| Problema | Gravedad | Acción de kei |
|----------|----------|---------------|
| Todo el código generado por LLM | Alta | Auditoría M2: revisar cada archivo, corregir artefactos |
| Crate GICv3 de terceros | Media | Reemplazar por driver interno |
| Tests solo en QEMU | Alta | Arranque en hardware real NanoPi R3S |
| Sin SMP/multi-núcleo | Media | Añadir arranque de CPU secundario vía PSCI |
| Desactualizado (por detrás del main upstream) | Baja | Rebase de sincronización regular |
| Comentarios verbosos de estilo LLM | Baja | Limpiar durante la auditoría |

## Matriz de tests QEMU

| Máquina QEMU | CPU | RAM | Arranque | Notas |
|--------------|-----|-----|----------|-------|
| virt | cortex-a55 | 2 GB | ✅ | Objetivo de test principal |
| virt | cortex-a72 | 2 GB | 🔲 | Validar en varios núcleos ARM |
| virt | max | 4 GB | 🔲 | Activar todas las funciones ARM |
| sbsa-ref | max | 4 GB | 🔲 | Arranque tipo servidor |
