//! Embassy-based sensor node example (NOT compilable on host).
//!
//! This is a reference implementation showing how to use `kei` on a real
//! MCU (STM32 / nRF52 / RP2040) with embassy. It does NOT compile in the
//! kei workspace (which targets host); copy it into your embassy project
//! and adapt the HAL imports for your specific board.
//!
//! ## What it does
//!
//! - Implements `kei::hal::Transport` over `embassy_uart::UartRx`/`UartTx`
//! - Implements `kei::hal::SensorDevice` reading from an ADC (temperature)
//! - Runs the `kei::wire::Node` main loop: polls for gateway requests,
//!   reads the sensor, sends telemetry back
//!
//! ## Wiring
//!
//! ```text
//! STM32 PA9 (TX) ──┬── RS-485 transceiver ──┬── gateway / kei-kernel
//! STM32 PA10 (RX) ─┘                        │
//! STM32 PA0 (ADC) ─── NTC thermistor ───────┘
//! ```

// These imports are placeholders — replace with your board's HAL:
// use embassy_executor::Spawner;
// use embassy_stm32::usart::{Config as UartConfig, Uart};
// use embassy_stm32::adc::{Adc, SampleTime};
// use embassy_stm32::Peripherals;

// The kei imports below are inside the commented main() to avoid
// "unused import" errors when this template compiles on host.
// In your real embassy project, uncomment them.

// ── Embassy Transport adapter ───────────────────────────────────────────────
//
// This wraps embassy's async UART into kei's sync Transport trait.
// On embassy, you'd call `embassy_futures::block_on` inside `send`/`recv`
// to bridge async→sync, OR (preferred) use `Node::recv_frame` / `send_frame`
// directly in an async context. The sync trait is provided for callers that
// prefer a simpler blocking loop (common in sensor nodes that do nothing else).

// pub struct EmbassyUartTransport {
//     rx: UartRx<'static>,
//     tx: UartTx<'static>,
// }
//
// impl Transport for EmbassyUartTransport {
//     fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
//         embassy_futures::block_on(self.tx.write(data))
//             .map(|_| data.len())
//             .map_err(|_| TransportError::Io)
//     }
//
//     fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
//         embassy_futures::block_on(self.rx.read(buf))
//             .map_err(|_| TransportError::Io)
//     }
// }

// ── ADC-backed temperature sensor ──────────────────────────────────────────-

/// A temperature sensor backed by an ADC pin.
/// Register 0x0100 = current temperature (read-only).
/// Register 0x0200 = alarm threshold (read-write).
///
/// Uncomment the `impl SensorDevice` block and the `use kei::hal::*` imports
/// in your real embassy project.
/*
pub struct AdcTempSensor {
    pub adc_raw: u16,
    pub alarm_threshold: f32,
}

impl AdcTempSensor {
    fn raw_to_celsius(&self) -> f32 {
        let v = self.adc_raw as f32 / 4095.0 * 3.3;
        let r = 10000.0 * (3.3 / v - 1.0);
        let t0 = 298.15_f32;
        let r0 = 10000.0_f32;
        let beta = 3950.0_f32;
        let temp_k = 1.0 / (1.0 / t0 + (r / r0).ln() / beta);
        temp_k - 273.15
    }
}

impl kei::hal::SensorDevice for AdcTempSensor {
    fn read_register(&mut self, register: u16) -> Result<kei::manifest::RawValue, kei::hal::DeviceError> {
        match register {
            0x0100 => Ok(kei::manifest::RawValue::F32(self.raw_to_celsius())),
            0x0200 => Ok(kei::manifest::RawValue::F32(self.alarm_threshold)),
            _ => Err(kei::hal::DeviceError::InvalidRegister),
        }
    }
    fn write_register(&mut self, register: u16, value: f32) -> Result<(), kei::hal::DeviceError> {
        match register {
            0x0200 => { self.alarm_threshold = value; Ok(()) }
            _ => Err(kei::hal::DeviceError::PermissionDenied),
        }
    }
    fn register_count(&self) -> u16 { 2 }
    fn unit_for(&self, reg: u16) -> kei::manifest::SensorUnit {
        if reg == 0x0100 { kei::manifest::SensorUnit::Celsius } else { kei::manifest::SensorUnit::Dimensionless }
    }
    fn mode_for(&self, reg: u16) -> kei::manifest::RegisterMode {
        if reg == 0x0200 { kei::manifest::RegisterMode::ReadWrite } else { kei::manifest::RegisterMode::ReadOnly }
    }
}
*/

// ── Embassy main task ─────────────────────────────────────────────────────--
//
// Uncomment and adapt for your board:
//
// #[embassy_executor::main]
// async fn main(_spawner: Spawner) {
//     let p = embassy_stm32::init(Default::default());
//
//     // Configure UART (RS-485 transceiver on PA9/PA10).
//     let mut uart_config = UartConfig::default();
//     uart_config.baudrate = 115200;
//     let (tx, rx) = Uart::new(p.USART1, p.PA10, p.PA9, Irqs, p.DMA1_CH2, p.DMA1_CH3, uart_config).split();
//
//     // Configure ADC (NTC thermistor on PA0).
//     let mut adc = Adc::new(p.ADC1, &mut Delay);
//     let mut temp_pin = p.PA0;
//
//     // Create the kei node.
//     let transport = EmbassyUartTransport { rx, tx };
//     let mut node = Node::new(transport, 1); // station ID = 1
//     let mut sensor = AdcTempSensor { adc_raw: 0, alarm_threshold: 50.0 };
//
//     // Announce boot.
//     node.send_status(kei::wire::NodeState::Boot, "firmware v0.1", 0).ok();
//
//     // Main loop.
//     loop {
//         // Read ADC.
//         sensor.adc_raw = adc.read(&mut temp_pin).await;
//
//         // Check for alarm.
//         let temp = sensor.raw_to_celsius();
//         if temp > sensor.alarm_threshold {
//             node.send_alarm(0x0100, kei::manifest::AlarmLevel::High,
//                 "temperature exceeded threshold", 0).ok();
//         }
//
//         // Poll for gateway requests (blocks until a frame arrives).
//         match node.poll(&mut sensor) {
//             Ok(Request::ReadRegister(req)) => {
//                 let raw = sensor.read_register(req.register).unwrap_or(RawValue::F32(0.0));
//                 let value = raw.as_f64() as f32;
//                 node.send_telemetry(req.register, value, sensor.unit_for(req.register), 0).ok();
//             }
//             Ok(Request::Discover(_)) => {
//                 node.send_discover_response("ntc-temp-sensor", 2).ok();
//             }
//             Ok(_) => {}
//             Err(_) => {
//                 // Transport error — re-init or reset.
//             }
//         }
//
//         // Heartbeat every N cycles (optional).
//         // node.send_status(kei::wire::NodeState::Heartbeat, "", 0).ok();
//     }
// }

fn main() {
    // This file is a reference template — it does not compile on host.
    // Copy it to your embassy project and enable the commented sections.
    eprintln!("embassy_node.rs is a reference template, not runnable on host.");
    eprintln!("See examples/host_demo.rs for a runnable demo.");
}
