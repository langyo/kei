//! kei wire protocol benchmarks — pure logic performance on host.
//!
//! Measures the hot paths that embassy MCU nodes and evernight gateways
//! hit most frequently:
//!
//! - Frame encode (postcard serialize + CRC16)
//! - Frame decode (parse header + verify CRC + postcard deserialize)
//! - CRC16-Modbus computation
//! - ScaleTransform apply (Linear / Table interpolation / Polynomial)
//! - FrameDecoder streaming state machine (byte-at-a-time)
//! - Full Node → Gateway round-trip (send_telemetry → recv)

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use kei::hal::{DeviceError, SensorDevice, Transport, TransportError};
use kei::manifest::{RawValue, RegisterMode, SensorUnit};
use kei::wire::{decode::FrameDecoder, frame::decode_frame, Frame, Gateway, Node};

// ── In-memory transport (zero-overhead, same as test harness) ────────────────

use std::cell::RefCell;
use std::rc::Rc;

struct Pipe {
    buf: Vec<u8>,
}

struct PipeTransport {
    tx: Rc<RefCell<Pipe>>,
    rx: Rc<RefCell<Pipe>>,
}

impl PipeTransport {
    fn pair() -> (Self, Self) {
        let ab = Rc::new(RefCell::new(Pipe { buf: Vec::new() }));
        let ba = Rc::new(RefCell::new(Pipe { buf: Vec::new() }));
        (
            Self {
                tx: ab.clone(),
                rx: ba.clone(),
            },
            Self { tx: ba, rx: ab },
        )
    }
}

impl Transport for PipeTransport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        self.tx.borrow_mut().buf.extend_from_slice(data);
        Ok(data.len())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        let mut rx = self.rx.borrow_mut();
        if rx.buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(rx.buf.len());
        buf[..n].copy_from_slice(&rx.buf[..n]);
        rx.buf.drain(..n);
        Ok(n)
    }
}

// Stub sensor for Node
struct StubSensor;
impl SensorDevice for StubSensor {
    fn read_register(&mut self, _: u16) -> Result<RawValue, DeviceError> {
        Ok(RawValue::F32(0.0))
    }
    fn write_register(&mut self, _: u16, _: f32) -> Result<(), DeviceError> {
        Ok(())
    }
    fn register_count(&self) -> u16 {
        1
    }
    fn unit_for(&self, _: u16) -> SensorUnit {
        SensorUnit::Celsius
    }
    fn mode_for(&self, _: u16) -> RegisterMode {
        RegisterMode::ReadOnly
    }
}

// ── Benchmarks ───────────────────────────────────────────────────────────────-

fn bench_frame_encode(c: &mut Criterion) {
    let frame = Frame::telemetry(42, 0x0100, 23.5, SensorUnit::Celsius);
    c.bench_function("frame_encode_telemetry", |b| {
        b.iter(|| {
            black_box(frame.encode());
        });
    });
}

fn bench_frame_decode(c: &mut Criterion) {
    let frame = Frame::telemetry(42, 0x0100, 23.5, SensorUnit::Celsius);
    let wire = frame.encode();
    c.bench_function("frame_decode_telemetry", |b| {
        b.iter(|| {
            black_box(decode_frame(black_box(&wire)).unwrap());
        });
    });
}

fn bench_crc16(c: &mut Criterion) {
    let data: Vec<u8> = (0..64u8).collect();
    c.bench_function("crc16_64bytes", |b| {
        b.iter(|| {
            black_box(kei::wire::frame::crc16_modbus(black_box(&data)));
        });
    });

    // Also benchmark larger payloads (256 bytes — typical max wire frame).
    let data256: Vec<u8> = (0..255u8).chain(core::iter::once(0)).collect();
    c.bench_function("crc16_256bytes", |b| {
        b.iter(|| {
            black_box(kei::wire::frame::crc16_modbus(black_box(&data256)));
        });
    });
}

fn bench_decoder_streaming(c: &mut Criterion) {
    let frame = Frame::telemetry(42, 0x0100, 23.5, SensorUnit::Celsius);
    let wire = frame.encode();
    c.bench_function("decoder_streaming_byte_by_byte", |b| {
        b.iter(|| {
            let mut dec = FrameDecoder::new();
            let mut result = None;
            for &byte in &wire {
                if let Ok(f) = dec.push(byte) {
                    result = Some(f);
                    break;
                }
            }
            black_box(result);
        });
    });
}

fn bench_scale_transform(c: &mut Criterion) {
    use kei::manifest::{ScaleTransform, SensorUnit};
    let mut group = c.benchmark_group("scale_transform");
    group.throughput(Throughput::Elements(1));

    let linear = ScaleTransform::Linear {
        factor: 0.1,
        offset: -50.0,
        unit: None,
    };
    group.bench_function("linear", |b| {
        b.iter(|| black_box(linear.apply(black_box(1532.0))));
    });

    // NTC-style lookup table (25 entries, -20°C to 100°C)
    let x: Vec<f32> = (-20..=100i32)
        .map(|t| {
            // Simulated ADC counts for NTC: resistance → ADC
            let r = 10000.0 * libm::exp(3950.0 * (1.0 / (t as f64 + 273.15) - 1.0 / 298.15));
            (4095.0 * 10000.0 / (r + 10000.0)) as f32
        })
        .collect();
    let y: Vec<f32> = (-20..=100i32).map(|t| t as f32).collect();
    let table = ScaleTransform::Table {
        x,
        y,
        unit: Some(SensorUnit::Celsius),
    };
    group.bench_function("table_interpolation_25pts", |b| {
        b.iter(|| black_box(table.apply(black_box(2048.0))));
    });

    // Quadratic polynomial
    let poly = ScaleTransform::Polynomial {
        coeffs: vec![10.0, 0.5, 0.001],
        unit: Some(SensorUnit::Celsius),
    };
    group.bench_function("polynomial_degree2", |b| {
        b.iter(|| black_box(poly.apply(black_box(500.0))));
    });

    group.finish();
}

fn bench_node_gateway_roundtrip(c: &mut Criterion) {
    let (gw_t, node_t) = PipeTransport::pair();
    let mut node = Node::new(node_t, 42);
    let mut gateway = Gateway::new(gw_t);

    c.bench_function("telemetry_round_trip", |b| {
        b.iter(|| {
            // Node sends telemetry.
            node.send_telemetry(0x0100, 23.5, SensorUnit::Celsius, 0)
                .unwrap();
            // Gateway receives it.
            let incoming = gateway.recv().unwrap();
            black_box(incoming);
        });
    });
}

criterion_group!(
    benches,
    bench_frame_encode,
    bench_frame_decode,
    bench_crc16,
    bench_decoder_streaming,
    bench_scale_transform,
    bench_node_gateway_roundtrip,
);
criterion_main!(benches);
