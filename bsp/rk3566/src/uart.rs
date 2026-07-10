//! UART driver for the Rockchip RK3566.
//!
//! RK3566 has up to 10 UART controllers based on Synopsys DW_apb_uart
//! (8250-compatible). The debug console uses UART2 on NanoPi R3S.

use core::fmt;

/// UART port identifier.
pub struct UartPort(u8);

/// Initialize UART console on the specified port.
pub fn init_console(port: u8) -> UartPort {
    // TODO: configure baud rate (1500000), 8N1, enable FIFO
    UartPort(port)
}

impl UartPort {
    /// Write a single byte to the UART.
    pub fn putc(&self, _c: u8) {
        // TODO: wait for THR empty, write to DATA register
    }

    /// Read a single byte from the UART (blocking).
    pub fn getc(&self) -> u8 {
        // TODO: wait for RDR not empty, read from DATA register
        0
    }

    /// Check if a byte is available to read.
    pub fn has_data(&self) -> bool {
        false
    }
}

impl fmt::Write for UartPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            self.putc(b);
        }
        Ok(())
    }
}
