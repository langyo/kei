//! I2C master driver for the Rockchip RK3566.
//!
//! Based on the RK3x I2C controller.
//! Used for RTC, temperature sensors, EEPROM, etc.

/// I2C bus instance.
pub struct I2cBus(u8);

/// Initialize an I2C bus.
pub fn init_bus(index: u8) -> I2cBus {
    // TODO: enable clock, configure pinmux, set speed
    I2cBus(index)
}

impl I2cBus {
    /// Write data to an I2C device.
    pub fn write(&self, _addr: u8, _data: &[u8]) {
        // TODO: send START, address+W, data bytes, STOP
    }

    /// Read data from an I2C device.
    pub fn read(&self, _addr: u8, _buf: &mut [u8]) {
        // TODO: send START, address+R, data bytes, STOP
    }
}
