// SPDX-License-Identifier: MPL-2.0

//! Multi-UART console registration for aarch64.
//!
//! Auto-detects the UART type (PL011 / DW 8250) from the ostd serial
//! module and registers the appropriate console device with `aster_console`,
//! enabling user-space TTY output via /dev/ttyS0.

use alloc::{string::ToString, sync::Arc, vec::Vec};

use aster_console::{AnyConsoleDevice, ConsoleCallback};
use ostd::{
    arch::serial::{uart_recv_byte, uart_send_byte},
    sync::{LocalIrqDisabled, SpinLock},
};

use crate::CONSOLE_NAME;

/// A generic UART console device (works with PL011, DW 8250, etc.).
pub(super) struct UartConsole {
    callbacks: SpinLock<Vec<&'static ConsoleCallback>, LocalIrqDisabled>,
}

impl core::fmt::Debug for UartConsole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UartConsole").finish_non_exhaustive()
    }
}

impl UartConsole {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            callbacks: SpinLock::new(Vec::new()),
        })
    }
}

impl AnyConsoleDevice for UartConsole {
    fn send(&self, buf: &[u8]) {
        for &byte in buf {
            if byte == b'\n' {
                uart_send_byte(b'\r');
            }
            uart_send_byte(byte);
        }
    }

    fn register_callback(&self, callback: &'static ConsoleCallback) {
        self.callbacks.lock().push(callback);
    }
}

pub(super) fn init() {
    let uart_type = ostd::arch::serial::uart_kind();
    let console = UartConsole::new();
    aster_console::register_device(CONSOLE_NAME.to_string(), console);
    let type_str = match uart_type {
        Some(ostd::arch::serial::UartKind::Pl011) => "PL011",
        Some(ostd::arch::serial::UartKind::Dw8250 { .. }) => "DW8250",
        None => "unknown",
    };
    ostd::info!("Registered {} UART as console", type_str);
}
