//! Watchdog timer driver for the Rockchip RK3566.
//!
//! Based on the Synopsys DesignWare Watchdog Timer (DW WDT).
//! Integrated into the RK3566 power management unit.

/// Watchdog timer controller.
pub struct Watchdog;

/// Initialize and start the watchdog with a timeout in seconds.
///
/// If not fed within the timeout period, the SoC will reset.
pub fn init(timeout_secs: u32) -> Watchdog {
    // TODO: map WDT registers, disable before configuring,
    //       set timeout counter, enable
    let _ = timeout_secs;
    Watchdog
}

impl Watchdog {
    /// Feed the watchdog to prevent timeout.
    ///
    /// Write the restart key (0x76) to the counter restart register.
    pub fn feed(&self) {
        // TODO: write WDT_CRR register with 0x76 magic
    }

    /// Disable the watchdog.
    pub fn disable(&self) {
        // TODO: write WDT_CR register to disable
    }
}
