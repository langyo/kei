# kei Construcción y despliegue

## Visión general

kei produce `kei-kernel.bin` — el kernel Asterinas habilitado para ARM64
consumido por [aris](https://github.com/celestia-island/aris). Esta guía cubre
la compilación del kernel, pruebas en QEMU y despliegue en hardware físico.

## Pipeline de compilación

```mermaid
flowchart LR
    SRC["Source\nostd/ kernel/ bsp/"] -->|"cargo build\n(aarch64)"| BIN["kei-kernel.bin"]
    BIN --> QEMU["QEMU Test\n(virt/cortex-a55)"]
    QEMU -->|passes| PACK["Package\n(DTB + initramfs)"]
    PACK --> ARIS["aris firmware\n(image.img)"]
    ARIS --> FLASH["Flash SD card"]
    FLASH --> BOARD["NanoPi R3S"]
```

## Requisitos previos

- **Host**: Linux x86_64 o ARM64
- **Rust**: 1.85+ con el target `aarch64-unknown-none-softfloat`
- **QEMU**: ≥ 8.0 para máquina virt con cortex-a55
- **just**: `cargo install just`

## Compilación rápida

```bash
# One-time setup
just setup        # Configure git remotes and Rust targets

# Sync upstream sources
just vendor       # Absorb latest upstream asterinas (squash)
just pull-arm64   # Pull ARM64 code from wanywhn fork (one-time)
just versions     # Show upstream baseline versions

# Build for the NanoPi R3S
just build        # Builds kei-kernel.bin for aarch64/armv8

# Run QEMU boot tests
just test-all     # Boot-tests all supported architectures
```

## Compilación cruzada

Para compilar de forma cruzada de x86_64 a aarch64:

```bash
# Add the ARM64 target (one-time)
rustup target add aarch64-unknown-none-softfloat

# Install GCC cross-toolchain (distribution-dependent)
# Ubuntu / Debian:
sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu

# Build
cargo build --release --target aarch64-unknown-none-softfloat \
  -p kei-kernel
```

El binario del kernel es una imagen ARM64 sin procesar (protocolo de arranque
Linux), no un ELF. Arranca directamente desde U-Boot mediante el comando
`booti`.

## Pruebas en QEMU

Pruebe el kernel en QEMU antes de desplegar en hardware:

```mermaid
flowchart TB
    subgraph Host["Máquina host"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        QEMU["QEMU\n(virt, cortex-a55)"]
    end
    KERN --> QEMU
    DTB --> QEMU
    QEMU -->|"serial output\n(logged)"| LOG["Registro de consola"]
    QEMU -->|"exit 0 = pass"| RESULT["Resultado de prueba"]
```

### Matriz de pruebas

| Máquina QEMU | CPU | RAM | Estado | Comando |
|-------------|-----|-----|--------|---------|
| virt | cortex-a55 | 2GB | ✅ Principal | `just test` |
| virt | cortex-a72 | 2GB | 🔲 Planeado | — |
| virt | max | 4GB | 🔲 Planeado | — |
| sbsa-ref | max | 4GB | 🔲 Planeado | — |

```bash
# Run the primary test target
just test

# Manual QEMU invocation
qemu-system-aarch64 \
  -machine virt,gic-version=3 \
  -cpu cortex-a55 \
  -m 2G \
  -kernel output/kei-kernel.bin \
  -nographic
```

## Despliegue físico

### NanoPi R3S

Desplegando kei en un NanoPi R3S físico:

```mermaid
flowchart TB
    subgraph Build["Host de compilación"]
        KERN["kei-kernel.bin"]
        DTB["nanopi-r3s.dtb"]
        INIT["initramfs.cpio.gz"]
    end
    subgraph Deploy["Despliegue"]
        IMG["image.img"]
        SD["Tarjeta SD"]
        BOARD["NanoPi R3S"]
    end
    KERN --> IMG
    DTB --> IMG
    INIT --> IMG
    IMG -->|"dd / just flash-sd"| SD
    SD --> BOARD
```

### Grabar en tarjeta SD

```bash
# Build the complete firmware image (includes kei-kernel.bin)
# Run from aris repository — aris packages kei as a submodule/dependency
just build-board nanopi-r3s

# Flash to SD card
sudo dd if=output/nanopi-r3s/image.img of=/dev/sdX bs=4M status=progress
sync
```

### Verificación de arranque

Después de insertar la tarjeta SD y encender, conéctese mediante USB-TTL serial
(1500000 baudios, 8N1):

```
U-Boot 2024.01 (Jan 01 2024 - 00:00:00 +0000)
...
## Loading kernel from mmc 0:1
   Image Name:   kei-kernel
   Image Type:   AArch64 Linux Kernel Image
   Data Size:    4194304 Bytes = 4 MiB
   Load Address: 00000000
   Entry Point:  00000000
## Flattened Device Tree blob at 44000000
   Booting using the fdt blob at 0x44000000

kei-kernel booting...
[KEI] initialising GICv3...
[KEI] initialising ARM Generic Timer...
[KEI] starting SMP...
[KEI] 4 cores online
...
aris-core v0.1.0 starting...
evernight daemon starting...
```

### Orden de arranque

```mermaid
flowchart TB
    ROM["Mask ROM"] --> SPL["U-Boot SPL"]
    SPL --> TPL["U-Boot Proper"]
    TPL -->|"load kernel + DTB\nfrom mmc"| KEI["kei-kernel.bin"]
    KEI -->|"Transfer to EL1"| INIT["initramfs\naris-core (PID 1)"]
    INIT --> EVN["evernight daemon"]
    EVN -->|"WebSocket TLS"| ENT["entelecheia"]
```

## Integración con aris

kei entrega el binario del kernel; aris lo empaqueta en una imagen arrancable:

```
aris repository                     kei repository
─────────────────                   ─────────────────
packages/core/        supervisor    kernel/          kernel source
packages/builder/     image builder ostd/            core infra
overlay/              rootfs files  bsp/             board support
scripts/              build + flash board/           board configs
│                                    │
│  just build-board                  │  just build
│    ├── cross-compile aris-core     │    └── cargo build (aarch64)
│    ├── fetch kei-kernel.bin        │
│    ├── assemble image.img          │
│    └── just flash-sd /dev/sdX      │
```

Validar la integración:

```bash
# In aris repo: build with kei kernel
just build-board nanopi-r3s

# Boot in QEMU with the full image
just test-qemu

# Verify kei kernel version in boot log
grep "kei-kernel" output/boot.log
```

## Solución de problemas

| Síntoma | Causa probable | Acción |
|---------|-------------|--------|
| Sin salida serial | Velocidad de baudios incorrecta | Use 1500000, no 115200 |
| Fallo en inicio de GICv3 | Tipo de máquina QEMU | Use `virt,gic-version=3` |
| Fallo de SMP | Falta PSCI en DTB | Verifique el nodo `/cpus` en el device tree |
| Kernel panic | Artefacto de código generado por LLM | Audite `ostd/src/arch/aarch64/` |
| U-Boot no encuentra el kernel | Offset de partición incorrecto | Verifique el offset en `boot.scr` |
| evernight no puede conectar | Red no configurada | Verifique `/data/network.toml` |
