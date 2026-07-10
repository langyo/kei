//! Host-side demo: simulates a sensor node and gateway talking over an
//! in-memory pipe. Runnable on any host with `cargo run --example host_demo`.
//!
//! This demonstrates the kei wire protocol API without any real hardware.
//! For the embassy (bare-metal MCU) version, see `examples/embassy_node.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use kei::hal::{DeviceError, SensorDevice, Transport};
use kei::manifest::{RawValue, RegisterMode, SensorUnit};

// ── In-memory bidirectional pipe (same as the test helper) ──────────────────

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
    fn send(&mut self, data: &[u8]) -> Result<usize, kei::hal::TransportError> {
        self.tx.borrow_mut().buf.extend_from_slice(data);
        Ok(data.len())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, kei::hal::TransportError> {
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

// ── A demo temperature sensor ───────────────────────────────────────────────

struct TemperatureSensor {
    temp: f32,
}

impl SensorDevice for TemperatureSensor {
    fn read_register(&mut self, register: u16) -> Result<RawValue, DeviceError> {
        match register {
            0x0100 => Ok(RawValue::F32(self.temp)),
            _ => Err(DeviceError::InvalidRegister),
        }
    }
    fn write_register(&mut self, register: u16, value: f32) -> Result<(), DeviceError> {
        if register == 0x0200 {
            self.temp = value; // set-point write
            Ok(())
        } else {
            Err(DeviceError::InvalidRegister)
        }
    }
    fn register_count(&self) -> u16 {
        2
    }
    fn unit_for(&self, reg: u16) -> SensorUnit {
        if reg == 0x0100 {
            SensorUnit::Celsius
        } else {
            SensorUnit::Dimensionless
        }
    }
    fn mode_for(&self, reg: u16) -> RegisterMode {
        if reg == 0x0200 {
            RegisterMode::ReadWrite
        } else {
            RegisterMode::ReadOnly
        }
    }
}

// ── Main demo: gateway reads temperature from node ─────────────────────────-

fn main() {
    println!("kei wire protocol demo (host, in-memory pipe)\n");

    let (gw_transport, node_transport) = PipeTransport::pair();
    let mut gateway = kei::wire::Gateway::new(gw_transport);
    let mut node = kei::wire::Node::new(node_transport, 42); // station 42
    let mut sensor = TemperatureSensor { temp: 23.5 };

    // 1. Gateway discovers nodes on the bus.
    println!("[gateway] sending discover probe...");
    gateway.send_discover().unwrap();

    // Node handles the discover probe.
    let req = node.poll(&mut sensor).expect("node poll");
    if let kei::wire::Request::Discover(_) = req {
        println!("[node 42] responding to discover");
        node.send_discover_response("temp-sensor-01", 2).unwrap();
    }
    let incoming = gateway.recv().unwrap();
    if let kei::wire::Incoming::DiscoverResponse(d) = incoming {
        println!(
            "[gateway] found station {} ({}, {} registers)",
            d.station_id, d.name, d.register_count
        );
    }

    // 2. Gateway reads the temperature register.
    println!("\n[gateway] reading register 0x0100 from station 42...");
    gateway.send_read_register(42, 0x0100).unwrap();

    // Node handles the read request and sends telemetry back.
    let req = node.poll(&mut sensor).expect("node poll");
    if let kei::wire::Request::ReadRegister(r) = req {
        println!("[node 42] read request for register 0x{:04X}", r.register);
        let raw = sensor.read_register(r.register).unwrap();
        let value = raw.as_f64() as f32;
        node.send_telemetry(r.register, value, sensor.unit_for(r.register), 0)
            .unwrap();
    }

    // Gateway receives the telemetry.
    let incoming = gateway.recv().unwrap();
    if let kei::wire::Incoming::Telemetry(t) = incoming {
        println!(
            "[gateway] telemetry: station={}, register=0x{:04X}, value={:.1}°C",
            t.station_id, t.register, t.value
        );
    }

    // 3. Gateway writes a set-point.
    println!("\n[gateway] writing 30.0 to register 0x0200 (set-point)...");
    gateway.send_write_register(42, 0x0200, 30.0).unwrap();

    // Node auto-dispatches the write (poll consumes it internally).
    // We then send a new read to verify the value changed.
    gateway.send_read_register(42, 0x0100).unwrap();
    let req = node
        .poll(&mut sensor)
        .expect("node poll (after write dispatch)");
    if let kei::wire::Request::ReadRegister(r) = req {
        let raw = sensor.read_register(r.register).unwrap();
        let value = raw.as_f64() as f32;
        node.send_telemetry(r.register, value, sensor.unit_for(r.register), 0)
            .unwrap();
    }
    let incoming = gateway.recv().unwrap();
    if let kei::wire::Incoming::Telemetry(t) = incoming {
        println!(
            "[gateway] telemetry after set-point: {:.1}°C (expected 30.0)",
            t.value
        );
        assert!((t.value - 30.0).abs() < 0.01);
    }

    println!("\n✓ demo complete — all operations succeeded");
}
