# kei no_std Library

The `kei` library (`packages/kei/`) is a `#![no_std]` Rust crate that provides
the shared contract between embassy-based sensor nodes and the evernight
gateway broker.

## What it provides

- **Wire protocol** (`kei::wire`) — compact binary framing (magic + length + type + payload + CRC16) for UART/USB-CDC/SPI. Includes `Node` (sensor side) and `Gateway` (broker side) high-level APIs.
- **Manifest schema** (`kei::manifest`) — hardware descriptions: register maps, alarm rules, station configs, scale transforms (Linear / Table interpolation / Polynomial).
- **HAL traits** (`kei::hal`) — `Transport`, `AsyncTransport`, `AddressedTransport` — embassy-implementable abstractions.
- **Protocol type identifiers** (`kei::codec`) — `ProtocolKind` enum and `ProtocolFrame` opaque container.

## Quick start

```bash
cd packages/kei
cargo test --all-features              # 20 tests
cargo bench --bench wire_bench         # criterion benchmarks
cargo run --example host_demo          # host-side wire protocol demo
```

## QEMU demo (Cortex-M4)

The `examples/qemu-mps2/` directory contains a bare-metal firmware that runs
under QEMU's `mps2-an386` machine (Cortex-M4) and demonstrates the wire
protocol end-to-end.

```bash
cd examples/qemu-mps2
cargo build --release --target thumbv7em-none-eabi
qemu-system-arm -M mps2-an386 -cpu cortex-m4 -m 16M \
    -display none -serial stdio \
    -kernel target/thumbv7em-none-eabi/release/kei-qemu-mps2
```

A host-side gateway (`examples/host_gateway.rs`) decodes the wire frames:

```bash
cargo build --example host_gateway --target x86_64-unknown-linux-gnu
qemu-system-arm ... | target/.../debug/examples/host_gateway
```

## Using kei in your embassy project

Add to your `Cargo.toml`:

```toml
[dependencies]
kei = { git = "https://github.com/celestia-island/kei.git", branch = "dev", default-features = false, features = ["wire", "manifest", "hal"] }
```

Implement `Transport` for your UART, then use `Node`:

```rust
use kei::wire::{Node, Request};
let mut node = Node::new(your_uart_transport, 1); // station_id = 1
loop {
    let req = node.poll(&mut sensor);
    // handle req, send telemetry back
}
```
