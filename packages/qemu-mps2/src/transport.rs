//! kei Transport implementation over the CMSDK APB UART.
//!
//! This bridges kei's `Transport` trait (used by `kei::wire::Node`) to
//! the raw UART driver. The transport is blocking — `recv` spins until
//! a byte arrives. This is fine for a simple sensor node that does
//! nothing else while waiting for a gateway request.

use kei::hal::{Transport, TransportError};

/// A UART-backed transport. Singleton — there's only one UART0 on the bus.
pub struct UartTransport;

impl UartTransport {
    pub fn new() -> Self {
        crate::uart::init();
        Self
    }
}

impl Transport for UartTransport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        crate::uart::write_bytes(data);
        Ok(data.len())
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        // Read at least 1 byte (blocking), then drain any additional
        // bytes that arrived in the meantime (up to buf.len()).
        if buf.is_empty() {
            return Ok(0);
        }
        buf[0] = crate::uart::read_byte();
        let mut count = 1;
        while count < buf.len() {
            match crate::uart::try_read_byte() {
                Some(b) => {
                    buf[count] = b;
                    count += 1;
                }
                None => break,
            }
        }
        Ok(count)
    }
}
