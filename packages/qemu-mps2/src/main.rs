//! kei QEMU demo firmware — mps2-an386 (Cortex-M4).
//!
//! A minimal sensor node that uses the kei wire protocol to report
//! temperature telemetry over QEMU's UART0. The "temperature" is a
//! simulated counter (no real ADC on MPS2).
//!
//! The gateway (host side) can read these telemetry frames via
//! QEMU's -serial stdio or -serial pty.
//!
//! This firmware is deliberately **blocking** (no embassy async) to keep
//! the dependency surface minimal and prove the wire protocol end-to-end.

#![no_std]
#![no_main]

extern crate alloc;

mod timer;
mod transport;
mod uart;

use core::panic::PanicInfo;

use cortex_m_rt::entry;

use kei::hal::{DeviceError, SensorDevice, Transport};
use kei::manifest::{RawValue, RegisterMode, ScaleTransform, SensorUnit};
use kei::wire::{decode_frame, encode_frame, Frame, MsgType, Node, Request};

// ── Heap allocator (kei's wire protocol uses Vec<u8>) ────────────────────────-

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

/// Static heap region: 64KB.
static mut HEAP: [u8; 64 * 1024] = [0; 64 * 1024];

// ── Panic handler ────────────────────────────────────────────────────────────-

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    uart::write_str("\r\n*** PANIC ***\r\n");
    loop {
        cortex_m::asm::bkpt();
    }
}

// ── Simulated temperature sensor ────────────────────────────────────────────-

/// A fake temperature sensor. Register 0x0100 = temperature (read-only),
/// increasing by 0.1°C each read. No real ADC on MPS2.
struct SimSensor {
    temp: f32,
}

impl SensorDevice for SimSensor {
    fn read_register(&mut self, register: u16) -> Result<RawValue, DeviceError> {
        match register {
            0x0100 => {
                self.temp += 0.1;
                Ok(RawValue::F32(self.temp))
            }
            _ => Err(DeviceError::InvalidRegister),
        }
    }

    fn write_register(&mut self, _register: u16, _value: f32) -> Result<(), DeviceError> {
        Err(DeviceError::PermissionDenied)
    }

    fn register_count(&self) -> u16 {
        1
    }

    fn unit_for(&self, register: u16) -> SensorUnit {
        if register == 0x0100 {
            SensorUnit::Celsius
        } else {
            SensorUnit::Dimensionless
        }
    }

    fn mode_for(&self, _register: u16) -> RegisterMode {
        RegisterMode::ReadOnly
    }
}

// ── Simple busy-wait delay ───────────────────────────────────────────────────-

/// Crude delay using a busy loop. At ~25MHz, ~25000 nops ≈ 1ms.
fn delay_ms(ms: u32) {
    for _ in 0..ms * 25_000 {
        cortex_m::asm::nop();
    }
}

// ── Main entry ───────────────────────────────────────────────────────────────-

#[entry]
fn main() -> ! {
    // Initialize heap.
    unsafe {
        ALLOCATOR.lock().init(HEAP.as_mut_ptr(), HEAP.len());
    }

    // Initialize UART.
    uart::init();
    uart::write_str("\r\n=== kei QEMU demo firmware (mps2-an386) ===\r\n");

    // Create the transport + node.
    let t = transport::UartTransport::new();
    let mut node = Node::new(t, 1); // station ID = 1
    let mut sensor = SimSensor { temp: 20.0 };

    // Announce boot.
    uart::write_str("[node] boot complete, station_id=1\r\n");
    let _ = node.send_status(kei::wire::NodeState::Boot, "kei-qemu-demo v0.1", 0);
    delay_ms(100);

    // ── Benchmark: measure kei operations in CMSDK timer ticks (25 MHz) ────────
    timer::init();
    uart::write_str("\r\n=== BENCHMARK (CMSDK TIMER @ 25MHz, 1 tick = 40ns) ===\r\n");

    let frame = Frame::telemetry(1, 0x0100, 23.5, SensorUnit::Celsius);
    let iterations: u32 = 1000;

    // 1. Frame encode (postcard + CRC16)
    let t0 = timer::now();
    for _ in 0..iterations {
        let w = frame.encode();
        core::hint::black_box(&w);
    }
    let t1 = timer::now();
    let ticks_encode = timer::elapsed(t0, t1) / iterations;
    uart::write_str("frame_encode:       ");
    uart::write_uint(ticks_encode);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_encode / 25);
    uart::write_str(" us\r\n");

    // 2. Frame decode (CRC verify + postcard deserialize)
    let wire_bytes = frame.encode();
    let t0 = timer::now();
    for _ in 0..iterations {
        let f = decode_frame(core::hint::black_box(&wire_bytes));
        core::hint::black_box(&f);
    }
    let t1 = timer::now();
    let ticks_decode = timer::elapsed(t0, t1) / iterations;
    uart::write_str("frame_decode:       ");
    uart::write_uint(ticks_decode);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_decode / 25);
    uart::write_str(" us\r\n");

    // 3. CRC16 only (64 bytes)
    let crc_input: [u8; 64] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
    ];
    let t0 = timer::now();
    for _ in 0..iterations {
        let c = kei::wire::frame::crc16_modbus(core::hint::black_box(&crc_input));
        core::hint::black_box(c);
    }
    let t1 = timer::now();
    let ticks_crc = timer::elapsed(t0, t1) / iterations;
    uart::write_str("crc16_64bytes:      ");
    uart::write_uint(ticks_crc);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_crc / 25);
    uart::write_str(" us\r\n");

    // 4. ScaleTransform: Linear
    let linear = ScaleTransform::Linear {
        factor: 0.1,
        offset: -50.0,
        unit: None,
    };
    let t0 = timer::now();
    for _ in 0..iterations {
        let v = linear.apply(core::hint::black_box(1532.0_f64));
        core::hint::black_box(v);
    }
    let t1 = timer::now();
    let ticks_linear = timer::elapsed(t0, t1) / iterations;
    uart::write_str("scale_linear:       ");
    uart::write_uint(ticks_linear);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_linear / 25);
    uart::write_str(" us\r\n");

    // 5. ScaleTransform: Polynomial (degree 2)
    let poly = ScaleTransform::Polynomial {
        coeffs: alloc::vec![1.0, 2.0, 3.0],
        unit: None,
    };
    let t0 = timer::now();
    for _ in 0..iterations {
        let v = poly.apply(core::hint::black_box(500.0_f64));
        core::hint::black_box(v);
    }
    let t1 = timer::now();
    let ticks_poly = timer::elapsed(t0, t1) / iterations;
    uart::write_str("scale_polynomial:   ");
    uart::write_uint(ticks_poly);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_poly / 25);
    uart::write_str(" us\r\n");

    // 6. Full encode+decode combined
    let t0 = timer::now();
    for _ in 0..iterations {
        let f = Frame::telemetry(1, 0x0100, 23.5, SensorUnit::Celsius);
        let w = f.encode();
        let _ = decode_frame(core::hint::black_box(&w));
    }
    let t1 = timer::now();
    let ticks_round = timer::elapsed(t0, t1) / iterations;
    uart::write_str("encode+decode:      ");
    uart::write_uint(ticks_round);
    uart::write_str(" ticks = ");
    uart::write_uint(ticks_round / 25);
    uart::write_str(" us\r\n");

    uart::write_str("=== BENCHMARK COMPLETE ===\r\n\r\n");

    let mut tick: u32 = 0;

    loop {
        tick += 1;

        // Send unsolicited telemetry each cycle.
        let temp = sensor.temp;
        let _ = node.send_telemetry(0x0100, temp, SensorUnit::Celsius, tick as u64);

        // Human-readable debug line (the wire frame above is binary).
        uart::write_str("[node] tick=");
        uart::write_uint(tick);
        uart::write_str(" temp=");
        uart::write_uint(temp as u32);
        uart::write_str("\r\n");

        // Check for incoming gateway requests (non-blocking: only poll
        // if a byte is already in the UART RX buffer).
        if uart::try_read_byte().is_some() {
            uart::write_str("[node] incoming data, polling...\r\n");
            match node.poll(&mut sensor) {
                Ok(Request::ReadRegister(req)) => {
                    uart::write_str("[node] read req reg=0x");
                    uart::write_hex(req.register);
                    uart::write_str("\r\n");
                    let raw = sensor
                        .read_register(req.register)
                        .unwrap_or(RawValue::F32(0.0));
                    let value = raw.as_f64() as f32;
                    let _ = node.send_telemetry(
                        req.register,
                        value,
                        sensor.unit_for(req.register),
                        tick as u64,
                    );
                    uart::write_str("[node] sent telemetry\r\n");
                }
                Ok(Request::Discover(_)) => {
                    let _ = node.send_discover_response("kei-qemu-temp-sensor", 1);
                    uart::write_str("[node] sent discover response\r\n");
                }
                Ok(_) => {}
                Err(_) => {
                    uart::write_str("[node] poll error\r\n");
                }
            }
        }

        delay_ms(2000);
    }
}
