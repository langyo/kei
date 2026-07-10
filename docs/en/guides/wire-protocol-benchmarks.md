# Wire Protocol Benchmarks

Performance measurements of the kei wire protocol, both on host (x86_64)
and on an emulated Cortex-M4 MCU (QEMU mps2-an386 @ 25 MHz).

## Host benchmarks (criterion, release build)

| Operation | Time |
|-----------|------|
| Frame encode (postcard + CRC16) | 159 ns |
| Frame decode (CRC verify + deserialize) | 130 ns |
| CRC16-Modbus (64 bytes) | 593 ns |
| CRC16-Modbus (256 bytes) | 2.36 µs |
| Streaming decoder (byte-at-a-time) | 223 ns |
| ScaleTransform / Linear | 5.4 ns |
| ScaleTransform / Table interpolation (25 pts) | 8.8 ns |
| ScaleTransform / Polynomial (degree 2) | 1.4 ns |
| Encode + decode round-trip | 469 ns |

Run with:
```bash
cd packages/kei
cargo bench --bench wire_bench
```

## On-chip benchmarks (Cortex-M4 @ 25 MHz, release build)

Measured via the CMSDK APB Timer (TIMER1) in the QEMU firmware
(`examples/qemu-mps2/`), using `core::hint::black_box` to prevent
optimizer elimination.

| Operation | Ticks | Time |
|-----------|-------|------|
| Frame encode (postcard + CRC16) | 14 | 560 ns |
| Frame decode (CRC verify + deserialize) | 11 | 440 ns |
| CRC16-Modbus (64 bytes) | 7 | 280 ns |
| ScaleTransform / Linear | 12 | 480 ns |
| ScaleTransform / Polynomial | 15 | 600 ns |
| Encode + decode round-trip | 43 | 1.7 µs |

## Analysis

At 115200 baud UART, a 23-byte wire frame takes ~2 ms to transmit.
The protocol overhead (encode + decode = 1.7 µs) is **under 0.1%** of
physical I/O time. Even at 921600 baud (250 µs per frame), the protocol
accounts for less than 1%.

The bottleneck is always the physical layer, never the protocol.
