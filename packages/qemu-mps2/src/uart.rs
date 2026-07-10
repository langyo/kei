//! CMSDK APB UART driver for QEMU mps2-an386.
//!
//! This is NOT a PL011. QEMU wires a `cmsdk-apb-uart` at 0x40004000
//! for AN385/AN386/AN500 boards (verified from QEMU hw/char/cmsdk-apb-uart.c).
//!
//! Register map (32-bit, little-endian, region size 0x1000):
//!   0x00 DATA       — write = TX byte, read = RX byte
//!   0x04 STATE      — bit0 TXFULL, bit1 RXFULL, bit2 TXOVERRUN, bit3 RXOVERRUN
//!   0x08 CTRL       — bit0 TX_EN, bit1 RX_EN, bit2 TX_INTEN, bit3 RX_INTEN
//!   0x0c INTSTATUS  — bit0 TX, bit1 RX (write-1-to-clear)
//!   0x10 BAUDDIV    — 20-bit, MUST be >= 16 or TX is silently dropped
//!
//! The UART has a 1-byte Tx buffer (no FIFO). Poll TXFULL before each write.

use core::ptr::{read_volatile, write_volatile};

const UART0_BASE: usize = 0x4000_4000;

const REG_DATA: *mut u32 = UART0_BASE as *mut u32;
const REG_STATE: *mut u32 = (UART0_BASE + 0x04) as *mut u32;
const REG_CTRL: *mut u32 = (UART0_BASE + 0x08) as *mut u32;
const REG_BAUDDIV: *mut u32 = (UART0_BASE + 0x10) as *mut u32;

const STATE_TXFULL: u32 = 1 << 0;
const STATE_RXFULL: u32 = 1 << 1;
const CTRL_TX_EN: u32 = 1 << 0;
const CTRL_RX_EN: u32 = 1 << 1;

/// Initialize the UART. BAUDDIV=27 gives ~925923 baud at pclk=25MHz.
/// BAUDDIV must be >= 16 or QEMU silently drops all TX.
pub fn init() {
    unsafe {
        write_volatile(REG_BAUDDIV, 27);
        write_volatile(REG_CTRL, CTRL_TX_EN | CTRL_RX_EN);
    }
}

/// Send one byte. Blocks until the 1-byte Tx buffer is drained.
pub fn write_byte(byte: u8) {
    unsafe {
        while read_volatile(REG_STATE) & STATE_TXFULL != 0 {}
        write_volatile(REG_DATA, byte as u32);
    }
}

/// Send a byte slice.
pub fn write_bytes(data: &[u8]) {
    for &b in data {
        write_byte(b);
    }
}

/// Send a string (ASCII only).
pub fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}

/// Try to read one byte if available (non-blocking). Returns None if empty.
pub fn try_read_byte() -> Option<u8> {
    unsafe {
        if read_volatile(REG_STATE) & STATE_RXFULL != 0 {
            Some(read_volatile(REG_DATA) as u8)
        } else {
            None
        }
    }
}

/// Blocking read of one byte.
pub fn read_byte() -> u8 {
    unsafe {
        loop {
            if read_volatile(REG_STATE) & STATE_RXFULL != 0 {
                return read_volatile(REG_DATA) as u8;
            }
        }
    }
}

/// Write an unsigned integer in decimal.
pub fn write_uint(n: u32) {
    if n == 0 {
        write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut len = 0;
    let mut n = n;
    while n > 0 {
        buf[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    for i in (0..len).rev() {
        write_byte(buf[i]);
    }
}

/// Write a u16 in uppercase hexadecimal (4 digits, no 0x prefix).
pub fn write_hex(n: u16) {
    let hex_chars = b"0123456789ABCDEF";
    for shift in (0..4).rev() {
        let nibble = ((n >> (shift * 4)) & 0xF) as usize;
        write_byte(hex_chars[nibble]);
    }
}
