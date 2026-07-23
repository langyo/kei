// Auto-generated from configs/nanopi-r3s.toml

pub mod gpio {
    pub const GPIO0: usize = 0xFDD60000;
    pub const GPIO1: usize = 0xFE740000;
    pub const GPIO2: usize = 0xFE750000;
    pub const GPIO3: usize = 0xFE760000;
    pub const GPIO4: usize = 0xFE770000;
}

pub mod gpio_reg {
    pub const EXT_PORT: usize = 0x0050;
    pub const SWPORT_DDR: usize = 0x0004;
    pub const SWPORT_DR: usize = 0x0000;
}

pub struct LedDef {
    pub name: &'static str,
    pub ctrl: &'static str,
    pub pin: u32,
    pub active_low: bool,
}

pub static LEDS: &[LedDef] = &[
    LedDef { name: "power", ctrl: "GPIO3", pin: 18, active_low: false },
    LedDef { name: "lan", ctrl: "GPIO3", pin: 19, active_low: false },
    LedDef { name: "wan", ctrl: "GPIO0", pin: 15, active_low: false },
];

pub const FB_DEFAULT_WIDTH: u32 = 1920;
pub const FB_DEFAULT_HEIGHT: u32 = 1080;
pub const FB_DEFAULT_BPP: u32 = 32;

pub const SERIAL_UART2_BASE: usize = 0xFE660000;
pub const SERIAL_REG_SHIFT: u32 = 2;
pub const SERIAL_IO_WIDTH: u32 = 4;

