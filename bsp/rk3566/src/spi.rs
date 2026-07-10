//! SPI master driver for the Rockchip RK3566.
//!
//! Based on Synopsys DesignWare SSI (DW_apb_ssi).
//! Used for sensor communication, external flash, etc.

/// SPI bus instance.
pub struct SpiBus(u8);

/// Initialize an SPI bus.
pub fn init_bus(index: u8) -> SpiBus {
    // TODO: enable clock, configure pinmux, set speed/mode
    SpiBus(index)
}

impl SpiBus {
    /// Perform a full-duplex SPI transfer.
    pub fn transfer(&self, _tx: &[u8], _rx: &mut [u8]) {
        // TODO: fill TX FIFO, poll RX FIFO
    }
}
