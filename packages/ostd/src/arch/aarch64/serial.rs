// SPDX-License-Identifier: MPL-2.0

//! The console I/O via ARM PL011 UART or DesignWare 8250-compatible UART.
//!
//! On QEMU virt machine, the PL011 UART is at physical address 0x09000000.
//! On real hardware (Rockchip RK3566, Allwinner, etc.), DW 8250 UARTs are
//! used at device-specific physical addresses (e.g., 0xFE660000).
//!
//! UART type and base address are probed from the device tree (FDT) if
//! available, falling back to the QEMU PL011 default.

use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::{LocalIrqDisabled, SpinLock};

/// ── UART type detection ───────────────────────────────────────────

/// Supported UART hardware types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UartKind {
    /// ARM PrimeCell PL011 (QEMU virt, some ARM dev boards)
    Pl011,
    /// Synopsys DesignWare 8250-compatible (Rockchip, Allwinner, Broadcom, etc.)
    Dw8250 {
        /// Register spacing shift: reg_offset = (reg_index << reg_shift)
        reg_shift: u32,
        /// Register access width in bytes (1, 2, or 4)
        io_width: u32,
    },
}

/// Result of probing the UART from the FDT or defaults.
pub struct UartProbe {
    /// Physical base address of the UART.
    pub base: usize,
    /// The UART hardware type.
    pub kind: UartKind,
}

impl UartProbe {
    /// Default probe for QEMU virt (PL011 at 0x09000000).
    pub const fn default_qemu() -> Self {
        Self {
            base: 0x0900_0000,
            kind: UartKind::Pl011,
        }
    }
}

/// ── PL011 UART registers ──────────────────────────────────────────

const PL011_UARTDR: u32 = 0x000;
const PL011_UARTFR: u32 = 0x018;
const PL011_UARTFR_TXFF: u32 = 1 << 5;
const PL011_UARTFR_RXFE: u32 = 1 << 4;

/// ── DW 8250 UART register indices ─────────────────────────────────
/// Register offsets = (index << reg_shift). Multiply by io_width for byte offset.

const DW_LSR: u32 = 5;   // Line Status Register (THRE=bit5, DR=bit0)
const DW_DATA: u32 = 0;  // THR (write) / RBR (read)
const DW_LSR_THRE: u32 = 1 << 5;
const DW_LSR_DR: u32 = 1;

/// ── Global UART state ─────────────────────────────────────────────

/// The current virtual base address of the UART.
static UART_VADDR: AtomicUsize = AtomicUsize::new(0);

/// The detected UART kind. Set once during init.
static UART_KIND: AtomicUsize = AtomicUsize::new(0);
// Encoding: 0=uninit, 1=PL011, 2=DW8250 (shift<<8 | width)

/// The primary serial port lock for thread-safe output.
static SERIAL_LOCK: SpinLock<(), LocalIrqDisabled> = SpinLock::new(());

/// ── Public probe / init API ───────────────────────────────────────

/// Initializes the serial port with the probed UART info.
/// Called very early in boot (identity-mapped page tables active).
pub(crate) fn init_with_probe(probe: UartProbe) {
    let kind_code: usize = match probe.kind {
        UartKind::Pl011 => 1,
        UartKind::Dw8250 { reg_shift, io_width } => {
            2 | ((reg_shift as usize) << 8) | ((io_width as usize) << 16)
        }
    };
    UART_KIND.store(kind_code, Ordering::Relaxed);
    UART_VADDR.store(probe.base, Ordering::Relaxed);
}

/// Initializes the serial port with QEMU defaults (PL011 at 0x09000000).
/// Idempotent: does nothing if already initialized via init_with_probe().
pub(crate) fn init() {
    if UART_VADDR.load(Ordering::Relaxed) == 0 {
        init_with_probe(UartProbe::default_qemu());
    }
}

/// Reinitializes the serial port with the linear mapping address.
/// Called after the real page table is activated.
pub(crate) fn reinit_with_linear_mapping() {
    use crate::mm::paddr_to_vaddr;
    let base = UART_VADDR.load(Ordering::Relaxed);
    if base != 0 {
        let vaddr = paddr_to_vaddr(base);
        UART_VADDR.store(vaddr, Ordering::Relaxed);
    }
}

/// Returns the detected UART kind, or None if not initialized.
pub fn uart_kind() -> Option<UartKind> {
    match UART_KIND.load(Ordering::Relaxed) {
        0 => None,
        1 => Some(UartKind::Pl011),
        code => {
            let reg_shift = ((code >> 8) & 0xFF) as u32;
            let io_width = ((code >> 16) & 0xFF) as u32;
            Some(UartKind::Dw8250 { reg_shift, io_width })
        }
    }
}

/// ── Low-level write / receive ─────────────────────────────────────

fn write_byte_raw(kind: UartKind, base: usize, byte: u8) {
    match kind {
        UartKind::Pl011 => {
            let fr = base + PL011_UARTFR as usize;
            let dr = base + PL011_UARTDR as usize;
            while unsafe { core::ptr::read_volatile(fr as *const u32) } & PL011_UARTFR_TXFF != 0 {}
            unsafe { core::ptr::write_volatile(dr as *mut u32, byte as u32) };
        }
        UartKind::Dw8250 { reg_shift, io_width: _ } => {
            let lsr_off = (DW_LSR << reg_shift) as usize;
            let data_off = (DW_DATA << reg_shift) as usize;
            let lsr = base + lsr_off;
            let data = base + data_off;
            while unsafe { core::ptr::read_volatile(lsr as *const u32) } & DW_LSR_THRE == 0 {}
            unsafe { core::ptr::write_volatile(data as *mut u32, byte as u32) };
        }
    }
}

fn recv_byte_raw(kind: UartKind, base: usize) -> Option<u8> {
    match kind {
        UartKind::Pl011 => {
            let fr = base + PL011_UARTFR as usize;
            let dr = base + PL011_UARTDR as usize;
            if unsafe { core::ptr::read_volatile(fr as *const u32) } & PL011_UARTFR_RXFE != 0 {
                return None;
            }
            Some(unsafe { core::ptr::read_volatile(dr as *const u32) as u8 })
        }
        UartKind::Dw8250 { reg_shift, io_width: _ } => {
            let lsr_off = (DW_LSR << reg_shift) as usize;
            let data_off = (DW_DATA << reg_shift) as usize;
            let lsr = base + lsr_off;
            let data = base + data_off;
            if unsafe { core::ptr::read_volatile(lsr as *const u32) } & DW_LSR_DR == 0 {
                return None;
            }
            Some(unsafe { core::ptr::read_volatile(data as *const u32) as u8 })
        }
    }
}

/// ── Pl011Serial (backward-compatible) ─────────────────────────────

/// A serial port implemented via the detected UART hardware.
pub struct Pl011Serial;

impl Pl011Serial {
    fn write_byte(byte: u8) {
        let base = UART_VADDR.load(Ordering::Relaxed);
        if base == 0 {
            return;
        }
        let kind = uart_kind().unwrap_or(UartKind::Pl011);
        write_byte_raw(kind, base, byte);
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

/// ── Serial port handle (for console module) ──────────────────────

pub struct SerialPortHandle;

pub static SERIAL_PORT: SerialPortHandle = SerialPortHandle;

impl SerialPortHandle {
    pub fn get(&self) -> Option<SerialPortGuard> {
        if UART_VADDR.load(Ordering::Relaxed) != 0 {
            Some(SerialPortGuard)
        } else {
            None
        }
    }
}

pub struct SerialPortGuard;

impl SerialPortGuard {
    pub fn lock(&self) -> SerialPortWriteGuard {
        SerialPortWriteGuard
    }
}

pub struct SerialPortWriteGuard;

impl fmt::Write for SerialPortWriteGuard {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            Pl011Serial::write_byte(byte);
        }
        Ok(())
    }
}

// ── Public safe API for UART MMIO ──────────────────────────────────
// These allow other crates (e.g., aster-uart) to access the UART
// without `unsafe`, respecting their `#![deny(unsafe_code)]`.

/// Sends a single byte.
pub fn uart_send_byte(byte: u8) {
    Pl011Serial::write_byte(byte);
}

/// Receives a single byte, or None if no data.
pub fn uart_recv_byte() -> Option<u8> {
    let base = UART_VADDR.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    let kind = uart_kind().unwrap_or(UartKind::Pl011);
    recv_byte_raw(kind, base)
}

/// Returns the virtual address of the UART.
pub fn uart_vaddr() -> usize {
    UART_VADDR.load(Ordering::Relaxed)
}

// ── Backward-compatible public API ─────────────────────────────────

/// Sends a single byte to the PL011 UART.
#[deprecated(note = "use uart_send_byte")]
pub fn pl011_send_byte(byte: u8) {
    uart_send_byte(byte);
}

/// Receives a single byte from the PL011 UART.
#[deprecated(note = "use uart_recv_byte")]
pub fn pl011_recv_byte() -> Option<u8> {
    uart_recv_byte()
}

/// Returns the virtual address of the PL011 UART.
#[deprecated(note = "use uart_vaddr")]
pub fn pl011_vaddr() -> usize {
    uart_vaddr()
}
