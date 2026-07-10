# kei QEMU Demo

**End-to-end demo of the kei wire protocol running on an emulated ARM Cortex-M4 MCU.**

This crate contains:
- **Firmware** (`src/main.rs`) — a bare-metal `#![no_std]` sensor node that runs under QEMU's `mps2-an386` (Cortex-M4) machine. It uses the kei wire protocol to send temperature telemetry over CMSDK APB UART.
- **Host Gateway** (`examples/host_gateway.rs`) — a std binary that decodes the wire frames from QEMU's serial output.

## Quick start

```bash
# 1. Build the firmware (thumbv7em-none-eabi)
cargo build --release

# 2. Build the host gateway
cargo build --example host_gateway --target x86_64-unknown-linux-gnu

# 3. Run end-to-end (QEMU → gateway)
qemu-system-arm -M mps2-an386 -cpu cortex-m4 -m 16M \
    -display none -serial stdio \
    -kernel target/thumbv7em-none-eabi/release/kei-qemu-demo \
    | target/x86_64-unknown-linux-gnu/debug/examples/host_gateway
```

Or use the helper script:
```bash
./run_demo.sh
```

## Expected output

```
[gateway] STATUS     station=1 state=Boot detail=kei-qemu-demo v0.1
[gateway] TELEMETRY  station=1 reg=0x0100 value=20.0 unit=°C t=1
[gateway] TELEMETRY  station=1 reg=0x0100 value=20.0 unit=°C t=2
...
```

## Architecture

```
QEMU (mps2-an386, Cortex-M4)
┌───────────────────────────────┐
│ kei-qemu-demo firmware        │
│  kei::wire::Node              │
│    ↓ send_telemetry()         │
│  CMSDK APB UART0 (0x40004000) │
└───────────┬───────────────────┘
            │ -serial stdio
            ↓
┌───────────────────────────────┐
│ host_gateway (std)            │
│  kei::wire::Gateway           │
│    recv() → Telemetry/Status  │
└───────────────────────────────┘
```

## Files

| File | Description |
|------|-------------|
| `src/main.rs` | Firmware entry point — initializes UART, heap, runs Node loop |
| `src/uart.rs` | CMSDK APB UART driver (raw register access, NOT PL011) |
| `src/transport.rs` | `kei::hal::Transport` impl over the UART |
| `memory.x` | Linker script for mps2-an386 (4MB flash, 4MB RAM) |
| `examples/host_gateway.rs` | Host-side frame decoder |
| `.cargo/config.toml` | QEMU runner config |

## Requirements

- `rustup target add thumbv7em-none-eabi`
- `qemu-system-arm` (on Linux: `apt install qemu-system-arm`)
