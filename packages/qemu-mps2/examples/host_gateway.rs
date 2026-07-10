//! Host-side Gateway demo — connects to QEMU's serial output and decodes
//! kei wire protocol frames from the emulated sensor node.
//!
//! This is an example binary — compiles with `cargo build --example host_gateway`.
//! The firmware (`src/main.rs`) is not compiled when building examples.
//!
//! Usage:
//!   1. Build the firmware:  cargo build --release
//!   2. Start QEMU with a PTY:  (see run_qemu.sh)
//!   3. Run this gateway:  cargo run --bin host_gateway -- <serial-device>
//!
//! On Linux the serial device is /dev/pts/N (printed by QEMU on stderr).
//! On Windows use a named pipe (\\.\pipe\...) or QEMU's -serial stdio
//! and pipe stdin/stdout directly.
//!
//! For the simplest demo, use `run_qemu.sh` which launches QEMU with
//! -serial stdio and pipes it to this gateway.

use std::env;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;

#[cfg(unix)]
use std::os::unix::io::FromRawFd;

use kei::hal::{Transport, TransportError};
use kei::wire::{Gateway, Incoming};

/// A Transport backed by a raw file descriptor (the QEMU serial pipe).
struct FdTransport {
    stdin: std::io::Stdin,
    stdout: std::io::Stdout,
}

impl Transport for FdTransport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        self.stdout
            .write_all(data)
            .map(|_| data.len())
            .map_err(|_| TransportError::Io)
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        self.stdin.read(buf).map_err(|_| TransportError::Io)
    }
}

fn main() {
    println!("kei QEMU Gateway demo\n");
    println!("Connect this to QEMU's UART via:");
    println!("  cargo build --release && ./run_qemu.sh | cargo run --bin host_gateway\n");

    let mut gateway = Gateway::new(FdTransport {
        stdin: io::stdin(),
        stdout: io::stdout(),
    });

    println!("[gateway] waiting for frames from node...\n");

    loop {
        match gateway.recv() {
            Ok(Incoming::Telemetry(t)) => {
                println!(
                    "[gateway] TELEMETRY  station={} reg=0x{:04X} value={:.1} unit={} t={}",
                    t.station_id,
                    t.register,
                    t.value,
                    t.unit.as_str(),
                    t.timestamp_ms
                );
            }
            Ok(Incoming::Status(s)) => {
                println!(
                    "[gateway] STATUS     station={} state={:?} detail={}",
                    s.station_id, s.state, s.detail
                );
            }
            Ok(Incoming::DiscoverResponse(d)) => {
                println!(
                    "[gateway] DISCOVER   station={} name={} registers={}",
                    d.station_id, d.name, d.register_count
                );
            }
            Ok(Incoming::Alarm(a)) => {
                println!(
                    "[gateway] ALARM      station={} reg=0x{:04X} level={:?} msg={}",
                    a.station_id, a.register, a.level, a.message
                );
            }
            Ok(Incoming::Nack(n)) => {
                println!(
                    "[gateway] NACK       station={} code={} msg={}",
                    n.station_id, n.error_code, n.message
                );
            }
            Err(e) => {
                eprintln!("[gateway] error: {:?}", e);
                // Don't exit — the stream may recover after re-sync.
            }
        }
    }
}
