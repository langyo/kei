// SPDX-License-Identifier: MPL-2.0

//! PL011 UART console registration for aarch64.
//!
//! Registers the PL011 UART (at 0x09000000 on QEMU virt) as a console
//! device with `aster_console`, enabling user-space TTY output via
//! /dev/ttyS0.

use alloc::{string::ToString, sync::Arc, vec::Vec};

use aster_console::{AnyConsoleDevice, ConsoleCallback};
use ostd::{
    arch::serial::{pl011_recv_byte, pl011_send_byte},
    mm::VmReader,
    sync::{LocalIrqDisabled, SpinLock},
};

use crate::CONSOLE_NAME;

/// A PL011 UART console device.
pub(super) struct Pl011Console {
    callbacks: SpinLock<Vec<&'static ConsoleCallback>, LocalIrqDisabled>,
}

impl core::fmt::Debug for Pl011Console {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pl011Console").finish_non_exhaustive()
    }
}

impl Pl011Console {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            callbacks: SpinLock::new(Vec::new()),
        })
    }
}

impl AnyConsoleDevice for Pl011Console {
    fn send(&self, buf: &[u8]) {
        for &byte in buf {
            // Translate \n → \r\n for terminal compatibility
            if byte == b'\n' {
                pl011_send_byte(b'\r');
            }
            pl011_send_byte(byte);
        }
    }

    fn register_callback(&self, callback: &'static ConsoleCallback) {
        self.callbacks.lock().push(callback);
    }
}

pub(super) fn init() {
    // The PL011 UART is already initialized by OSTD's early serial init.
    // We just need to register it as a console device.
    let console = Pl011Console::new();
    aster_console::register_device(CONSOLE_NAME.to_string(), console);
    ostd::info!("Registered PL011 UART as console");
}
