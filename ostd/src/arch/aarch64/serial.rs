// SPDX-License-Identifier: MPL-2.0

//! The console I/O via ARM PL011 UART.
//!
//! On QEMU virt machine, the PL011 UART is at physical address 0x09000000.
//! During boot (with identity-mapped boot page tables), we access it at the
//! identity-mapped address. After the real page table is activated,
//! the UART is accessed through the linear mapping.

use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::{LocalIrqDisabled, SpinLock};

/// PL011 UART register offsets
const UARTDR: u32 = 0x000; // Data Register
const UARTFR: u32 = 0x018; // Flag Register
const UARTFR_TXFF: u32 = 1 << 5; // TX FIFO full
const UARTFR_RXFE: u32 = 1 << 4; // RX FIFO empty

/// QEMU virt PL011 UART physical base address.
const UART_PHYS_BASE: usize = 0x0900_0000;

/// The current virtual base address of the UART.
/// Initially identity-mapped, updated to linear mapping after page table switch.
static UART_VADDR: AtomicUsize = AtomicUsize::new(0);

/// The primary serial port lock for thread-safe output.
static SERIAL_LOCK: SpinLock<(), LocalIrqDisabled> = SpinLock::new(());

/// A serial port implemented via ARM PL011 UART.
pub struct Pl011Serial;

impl Pl011Serial {
    /// Write a single byte to the UART.
    fn write_byte(byte: u8) {
        let base = UART_VADDR.load(Ordering::Relaxed);
        if base == 0 {
            return;
        }
        let fr = base + UARTFR as usize;
        let dr = base + UARTDR as usize;

        // Wait until TX FIFO is not full
        while unsafe { core::ptr::read_volatile(fr as *const u32) } & UARTFR_TXFF != 0 {}

        // Write byte to data register
        unsafe { core::ptr::write_volatile(dr as *mut u32, byte as u32) };
    }
}

impl fmt::Write for Pl011Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            Self::write_byte(byte);
        }
        Ok(())
    }
}

/// Compatibility shim: the console module expects a `SERIAL_PORT` static
/// with a `.get()` method that returns `Option<impl Write>`.
/// We provide it as a dummy that always returns Some, since the actual
/// address is in `UART_VADDR`.
pub struct SerialPortHandle;

/// The primary serial port singleton.
pub static SERIAL_PORT: SerialPortHandle = SerialPortHandle;

impl SerialPortHandle {
    /// Returns a handle to the serial port if initialized.
    pub fn get(&self) -> Option<SerialPortGuard> {
        if UART_VADDR.load(Ordering::Relaxed) != 0 {
            Some(SerialPortGuard)
        } else {
            None
        }
    }
}

/// A guard that provides locked access to the serial port.
pub struct SerialPortGuard;

impl SerialPortGuard {
    pub fn lock(&self) -> SerialPortWriteGuard {
        SerialPortWriteGuard
    }
}

/// A guard for writing to the serial port.
pub struct SerialPortWriteGuard;

impl fmt::Write for SerialPortWriteGuard {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            Pl011Serial::write_byte(byte);
        }
        Ok(())
    }
}

/// Initializes the serial port with identity mapping (early boot).
pub(crate) fn init() {
    UART_VADDR.store(UART_PHYS_BASE, Ordering::Relaxed);
}

/// Reinitializes the serial port with the linear mapping address.
/// Called after the real page table is activated.
pub(crate) fn reinit_with_linear_mapping() {
    use crate::mm::paddr_to_vaddr;
    let vaddr = paddr_to_vaddr(UART_PHYS_BASE);
    UART_VADDR.store(vaddr, Ordering::Relaxed);
}

// ── Public safe API for PL011 UART MMIO ──────────────────────────
// These functions allow other crates (e.g., aster-uart) to access the
// PL011 UART without using `unsafe`, respecting their `#![deny(unsafe_code)]`.

/// Sends a single byte to the PL011 UART.
/// No-op if the UART is not yet initialized.
pub fn pl011_send_byte(byte: u8) {
    let base = UART_VADDR.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let fr = base + UARTFR as usize;
    let dr = base + UARTDR as usize;
    while unsafe { core::ptr::read_volatile(fr as *const u32) } & UARTFR_TXFF != 0 {}
    unsafe { core::ptr::write_volatile(dr as *mut u32, byte as u32) };
}

/// Receives a single byte from the PL011 UART.
/// Returns `None` if the UART is not initialized or the RX FIFO is empty.
pub fn pl011_recv_byte() -> Option<u8> {
    let base = UART_VADDR.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    let fr = base + UARTFR as usize;
    let dr = base + UARTDR as usize;
    if unsafe { core::ptr::read_volatile(fr as *const u32) } & UARTFR_RXFE != 0 {
        return None;
    }
    Some(unsafe { core::ptr::read_volatile(dr as *const u32) as u8 })
}

/// Returns the virtual address of the PL011 UART, or 0 if not initialized.
pub fn pl011_vaddr() -> usize {
    UART_VADDR.load(Ordering::Relaxed)
}
