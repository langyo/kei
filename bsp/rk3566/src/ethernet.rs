//! Ethernet driver for the Rockchip RK3566.
//!
//! RK3566 integrates two Synopsys DesignWare GMAC (stmmac) controllers:
//! - GMAC0: RGMII interface, typically WAN
//! - GMAC1: RGMII interface, typically LAN
//!
//! External PHY: Realtek RTL8211F (NanoPi R3S)

use alloc::vec::Vec;

/// Ethernet controller instance.
pub struct EthernetController {
    /// Controller index (0 or 1)
    index: u8,
    /// PHY address on MDIO bus
    phy_addr: u8,
}

/// Initialize Ethernet controllers.
pub fn init() -> Vec<EthernetController> {
    // TODO: probe device tree, find gmac nodes,
    //       reset PHY, configure RGMII, enable clocks
    alloc::vec![
        EthernetController {
            index: 0,
            phy_addr: 0
        },
        EthernetController {
            index: 1,
            phy_addr: 1
        },
    ]
}

impl EthernetController {
    /// Check if link is up.
    pub fn link_up(&self) -> bool {
        // TODO: read PHY status register via MDIO
        false
    }

    /// Get MAC address from eFuse or device tree.
    pub fn mac_address(&self) -> [u8; 6] {
        // TODO: read from RK3566 OTP eFuse or DTB
        [0; 6]
    }

    /// Send a network packet.
    pub fn send(&self, _data: &[u8]) {
        // TODO: DMA descriptor ring, TX path
    }

    /// Receive network packets (called from interrupt handler).
    pub fn receive(&self) -> Option<&[u8]> {
        // TODO: DMA descriptor ring, RX path
        None
    }
}
