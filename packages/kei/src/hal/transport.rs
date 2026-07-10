//! Transport trait — abstract byte-level I/O for the wire protocol.
//!
//! Embassy nodes implement this against `embassy_uart`, `embassy_usb`,
//! `embassy_spi`, etc. The gateway (evernight) implements it against
//! `tokio_serial` / `tokio::net::TcpStream`. The wire protocol is
//! transport-agnostic.

/// A bidirectional byte transport (UART, USB-CDC, SPI, TCP, etc.).
///
/// This is a **synchronous** trait (no async) so it can be implemented
/// equally by blocking kernel code (kei-kernel) and embassy async code
/// (via the embassy adapter in `kei::hal::embassy_adapter`, which wraps
/// async embassy I/O into the sync interface using embassy's blocking
/// primitives, or callers use the async API directly).
///
/// For embassy async usage, see the example in `examples/embassy_node.rs`
/// which uses the async `send_bytes` / `recv_byte` pattern directly rather
/// than this sync trait.
pub trait Transport {
    /// Send raw bytes. Returns the number actually sent.
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError>;

    /// Receive up to `buf.len()` bytes. Returns the number received.
    /// Blocks until at least 1 byte is available (or error).
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;
}

/// Error from a transport operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    /// I/O error (hardware fault, timeout, etc.).
    Io,
    /// The transport is closed / disconnected.
    Closed,
    /// Buffer too small for the operation.
    BufferTooSmall,
}

/// An **async** bidirectional byte transport.
///
/// This is the async counterpart of [`Transport`]. Embassy nodes implement
/// it natively against `embassy_uart::Uart`, `embassy_usb::UsbDevice`, etc.
/// Host gateways can implement it against `tokio::io::AsyncReadExt` or
/// `async_std`.
///
/// The trait uses native `async fn` (stabilized in Rust 1.75) without
/// `Send` bounds, so it works on single-threaded embassy executors.
#[allow(async_fn_in_trait)]
pub trait AsyncTransport {
    /// Send raw bytes asynchronously.
    #[allow(async_fn_in_trait)]
    async fn send(&mut self, data: &[u8]) -> Result<usize, TransportError>;

    /// Receive up to `buf.len()` bytes asynchronously.
    /// Resolves when at least 1 byte is available (or error).
    #[allow(async_fn_in_trait)]
    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// Convenience: send an entire slice, retrying until all bytes are sent.
    async fn send_all(&mut self, mut data: &[u8]) -> Result<(), TransportError> {
        while !data.is_empty() {
            let n = self.send(data).await?;
            data = &data[n..];
        }
        Ok(())
    }
}

/// Blanket impl: any sync `Transport` is also an `AsyncTransport` via
/// polling (non-blocking yield). This is useful for testing async code
/// with the in-memory `PipeTransport`.
///
/// Note: this does NOT actually yield to the executor — it's a busy-wait.
/// For real async I/O, implement `AsyncTransport` directly against your
/// async HAL.
#[allow(async_fn_in_trait)]
pub trait AsyncTransportExt: AsyncTransport {
    /// Receive exactly `buf.len()` bytes (fills the buffer).
    async fn recv_exact(&mut self, buf: &mut [u8]) -> Result<(), TransportError> {
        let mut filled = 0;
        while filled < buf.len() {
            let n = self.recv(&mut buf[filled..]).await?;
            filled += n;
        }
        Ok(())
    }
}

impl<T: AsyncTransport + ?Sized> AsyncTransportExt for T {}

// ── Addressed transport (RS-485 multi-drop) ──────────────────────────────────

/// A node address on a multi-drop bus (RS-485, LIN, etc.).
pub type NodeAddr = u8;

/// A transport that tags each message with a destination/source address.
///
/// On RS-485 multi-drop buses, multiple nodes share one physical wire.
/// Each frame must carry an address so the right node picks it up.
/// This trait extends [`Transport`] with address-aware send/recv.
///
/// For point-to-point links (single node per UART), use plain [`Transport`]
/// and ignore addresses — the kei wire protocol already carries a
/// `station_id` in its payload.
///
/// ## When to use
///
/// - RS-485 with multiple sensor nodes on one bus
/// - LIN bus with multiple slaves
/// - Any multi-drop topology where the physical layer needs addressing
///
/// ## When NOT to use
///
/// - Single node per UART (use `Transport`)
/// - TCP/USB-CDC (each connection is point-to-point)
pub trait AddressedTransport {
    /// Send `data` to a specific node address.
    ///
    /// Address 0x00 is typically broadcast (all nodes receive).
    fn send_to(&mut self, addr: NodeAddr, data: &[u8]) -> Result<usize, TransportError>;

    /// Receive data, returning the source address and byte count.
    ///
    /// Blocks until at least 1 byte arrives from any node.
    fn recv_from(&mut self, buf: &mut [u8]) -> Result<(NodeAddr, usize), TransportError>;
}

/// Async variant of [`AddressedTransport`] for embassy multi-drop buses.
pub trait AsyncAddressedTransport {
    /// Send `data` to a specific node address asynchronously.
    #[allow(async_fn_in_trait)]
    async fn send_to(&mut self, addr: NodeAddr, data: &[u8]) -> Result<usize, TransportError>;

    /// Receive data asynchronously, returning source address + byte count.
    #[allow(async_fn_in_trait)]
    async fn recv_from(&mut self, buf: &mut [u8]) -> Result<(NodeAddr, usize), TransportError>;
}
