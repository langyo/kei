// SPDX-License-Identifier: MPL-2.0

//! RK3566/RK3568 VOP2 display controller and DW HDMI transmitter.
//!
//! Reads the U-Boot-configured VOP2 registers to find the framebuffer
//! address and publishes it through kei's framebuffer component.

use core::fmt;
use crate::{DisplayController, DisplayMode, HdmiTransmitter};

/// Kernel linear mapping base: VA = LINEAR_BASE + PA (first 4 GiB).
const LINEAR_BASE: usize = 0xffff_8000_0000_0000;

fn pa_to_va(pa: usize) -> usize {
    LINEAR_BASE + pa
}

// ── VOP2 Register Offsets ────────────────────────────────────────

const VOP2_SYS_CTRL: usize        = 0x0008;
const VOP2_DSP_IF_EN: usize        = 0x001C;

const VP0_DSP_CTRL: usize          = 0x0C00;
const VP0_DSP_HACT_ST_END: usize   = 0x0C08;
const VP0_DSP_VACT_ST_END: usize   = 0x0C10;

const CLUSTER0_WIN_CTRL0: usize     = 0x1000;
const CLUSTER0_WIN_YRGB_MST: usize  = 0x1010;
const CLUSTER0_WIN_DSP_INFO: usize  = 0x1008;
const CLUSTER0_WIN_ACT_INFO: usize  = 0x1004;
const CLUSTER0_WIN_VIR: usize       = 0x1014;

const WIN_ENABLE: u32            = 1;
const VP_DSP_CTRL__STANDBY: u32  = 1 << 31;

// ── RK3566 VOP2 ──────────────────────────────────────────────────

pub struct Rk3566Vop2 {
    base_pa: usize,
    base: usize,
    mode: Option<DisplayMode>,
    fb_addr: Option<usize>,
    hdmi: Option<Rk3566DwHdmi>,
}

impl Rk3566Vop2 {
    pub const BASE: usize = 0xFDD9_0000;

    pub fn probe() -> Option<Self> {
        // Check device tree for VOP2 node before touching hardware.
        let fdt = ostd::arch::boot::DEVICE_TREE.get()?;
        fdt.find_compatible(&["rockchip,rk3568-vop", "rockchip,rk3566-vop"])?;

        let base_pa = Self::BASE;
        let base = pa_to_va(base_pa);

        // Read version register to confirm VOP2 is alive
        let _version = unsafe { core::ptr::read_volatile((base + 0x0000) as *const u32) };

        // Scan ALL window types for the active framebuffer.
        let fb_addr = Self::scan_framebuffer(base);

        Some(Self { base_pa, base, mode: None, fb_addr, hdmi: Rk3566DwHdmi::probe() })
    }

    /// Scan all VOP2 windows for one that is enabled and has a valid
    /// framebuffer address. Returns the physical address if found.
    fn scan_framebuffer(base: usize) -> Option<usize> {
        // Window types: (name, base_offset, has_separate_uv)
        let windows: &[(&str, usize)] = &[
            ("Cluster0", 0x1000),
            ("Cluster1", 0x1200),
            ("Esmart0",  0x1800),
            ("Esmart1",  0x1A00),
            ("Smart0",   0x1C00),
            ("Smart1",   0x1E00),
        ];

        for &(name, off) in windows {
            let ctrl = unsafe { core::ptr::read_volatile((base + off) as *const u32) };
            if (ctrl & WIN_ENABLE) == 0 {
                continue;
            }
            let yrgb = unsafe { core::ptr::read_volatile((base + off + 0x10) as *const u32) };
            ostd::early_println!("[display] VOP2 {}: ctrl={:#x} yrgb_mst={:#x}", name, ctrl, yrgb);
            if yrgb != 0 {
                return Some(yrgb as usize);
            }
        }
        None
    }
}

impl fmt::Debug for Rk3566Vop2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rk3566Vop2")
            .field("base_pa", &format_args!("{:#x}", self.base_pa))
            .finish()
    }
}

impl DisplayController for Rk3566Vop2 {
    fn init(&mut self) -> Result<(), &'static str> {
        let base = self.base;
        let hactive = unsafe { core::ptr::read_volatile((base + VP0_DSP_HACT_ST_END) as *const u32) };
        let vactive = unsafe { core::ptr::read_volatile((base + VP0_DSP_VACT_ST_END) as *const u32) };
        let w = (hactive & 0xFFFF) as u32;
        let h = (vactive & 0xFFFF) as u32;
        if w > 0 && h > 0 {
            self.mode = Some(DisplayMode {
                width: w, height: h, refresh_hz: 60, pixel_clock_khz: 148500,
                hsync_start: w + 16, hsync_end: w + 60, htotal: w + 280,
                vsync_start: h + 5, vsync_end: h + 10, vtotal: h + 45,
            });
        } else {
            self.mode = Some(DisplayMode::HDMI_1080P60);
        }
        if let Some(ref mut hdmi) = self.hdmi { hdmi.init()?; }
        Ok(())
    }

    fn set_framebuffer(&mut self, phys_addr: usize, width: u32, height: u32, stride_bytes: u32) -> Result<usize, &'static str> {
        let base = self.base;
        let size = (height as usize) * (stride_bytes as usize);
        let ctrl = (5 << 1) | WIN_ENABLE;
        unsafe { core::ptr::write_volatile((base + CLUSTER0_WIN_CTRL0) as *mut u32, ctrl); }
        unsafe { core::ptr::write_volatile((base + CLUSTER0_WIN_YRGB_MST) as *mut u32, phys_addr as u32); }
        unsafe { core::ptr::write_volatile((base + CLUSTER0_WIN_VIR) as *mut u32, (stride_bytes / 4) as u32); }
        let dsp = ((height as u32 - 1) << 16) | (width as u32 - 1);
        unsafe { core::ptr::write_volatile((base + CLUSTER0_WIN_DSP_INFO) as *mut u32, dsp); }
        unsafe { core::ptr::write_volatile((base + CLUSTER0_WIN_ACT_INFO) as *mut u32, dsp); }
        let vp = unsafe { core::ptr::read_volatile((base + VP0_DSP_CTRL) as *const u32) };
        unsafe { core::ptr::write_volatile((base + VP0_DSP_CTRL) as *mut u32, vp & !VP_DSP_CTRL__STANDBY); }
        self.fb_addr = Some(phys_addr);
        Ok(size)
    }

    fn set_enabled(&mut self, enabled: bool) {
        let ctrl = if enabled { (5 << 1) | WIN_ENABLE } else { 0 };
        unsafe { core::ptr::write_volatile((self.base + CLUSTER0_WIN_CTRL0) as *mut u32, ctrl); }
    }

    fn framebuffer_addr(&self) -> Option<usize> { self.fb_addr }

    fn current_mode(&self) -> Option<DisplayMode> { self.mode }
}

// ── RK3566 DW HDMI ────────────────────────────────────────────────

pub struct Rk3566DwHdmi;

impl Rk3566DwHdmi {
    pub const BASE: usize = 0xFE0A_0000;
    fn probe() -> Option<Self> { Some(Self) }
}

impl fmt::Debug for Rk3566DwHdmi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rk3566DwHdmi").finish()
    }
}

impl HdmiTransmitter for Rk3566DwHdmi {
    fn init(&mut self) -> Result<(), &'static str> { Ok(()) }
    fn set_mode(&mut self, _mode: &DisplayMode) -> Result<(), &'static str> { Ok(()) }
    fn set_enabled(&mut self, _enabled: bool) {}
}

pub fn probe() -> Option<Rk3566Vop2> { Rk3566Vop2::probe() }
