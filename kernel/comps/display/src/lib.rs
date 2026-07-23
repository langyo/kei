// SPDX-License-Identifier: MPL-2.0

//! Display driver framework for kei.
//!
//! Provides trait-based abstractions for display controllers and HDMI
//! transmitters, with per-SoC implementations. Integrates with
//! `aster-framebuffer` to publish the scanout buffer as `/dev/fb0`.

#![no_std]
// Note: Display driver implementations use unsafe for MMIO register access.
// The public API (DisplayController/HdmiTransmitter traits) is safe.

// Set this crate's log prefix for `ostd::log`.
macro_rules! __log_prefix {
    () => { "" };
}

extern crate alloc;

pub mod chips;

use alloc::sync::Arc;
use core::fmt::Debug;
use aster_framebuffer::framebuffer::{self, FrameBuffer};
use ostd::{
    io::IoMem,
    mm::CachePolicy,
};

// ── Display Mode ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub pixel_clock_khz: u32,
    pub hsync_start: u32,
    pub hsync_end: u32,
    pub htotal: u32,
    pub vsync_start: u32,
    pub vsync_end: u32,
    pub vtotal: u32,
}

impl DisplayMode {
    pub const HDMI_1080P60: Self = Self {
        width: 1920, height: 1080, refresh_hz: 60,
        pixel_clock_khz: 148500,
        hsync_start: 2008, hsync_end: 2052, htotal: 2200,
        vsync_start: 1084, vsync_end: 1089, vtotal: 1125,
    };

    pub const HDMI_720P60: Self = Self {
        width: 1280, height: 720, refresh_hz: 60,
        pixel_clock_khz: 74250,
        hsync_start: 1390, hsync_end: 1430, htotal: 1650,
        vsync_start: 725, vsync_end: 730, vtotal: 750,
    };
}

// ── Traits ──────────────────────────────────────────────────────

pub trait DisplayController: Debug + Send + Sync {
    fn init(&mut self) -> Result<(), &'static str>;
    fn set_framebuffer(&mut self, phys_addr: usize, width: u32, height: u32, stride_bytes: u32) -> Result<usize, &'static str>;
    fn set_enabled(&mut self, enabled: bool);
    fn framebuffer_addr(&self) -> Option<usize>;
    fn current_mode(&self) -> Option<DisplayMode>;
}

pub trait HdmiTransmitter: Debug + Send + Sync {
    fn init(&mut self) -> Result<(), &'static str>;
    fn set_mode(&mut self, mode: &DisplayMode) -> Result<(), &'static str>;
    fn set_enabled(&mut self, enabled: bool);
}

// ── Component Init ──────────────────────────────────────────────

pub fn init() -> Result<(), &'static str> {
    if let Some(mut ctrl) = chips::probe_display_controller() {
        ctrl.init()?;

        if let Some(addr) = ctrl.framebuffer_addr() {
            let mode = ctrl.current_mode().unwrap_or(DisplayMode::HDMI_1080P60);
            let stride = mode.width as usize * 4;
            let size = mode.height as usize * stride;

            let io_mem = IoMem::acquire_with_cache_policy(
                addr..addr + size,
                CachePolicy::WriteCombining,
            )
            .map_err(|_| "failed to acquire IoMem for framebuffer")?;

            let fb = FrameBuffer::new_mmio(
                io_mem,
                mode.width as usize,
                mode.height as usize,
                stride,
                aster_framebuffer::pixel::PixelFormat::BgrReserved,
            );

            framebuffer::publish(Arc::new(fb));
        }
    } else {
    }

    Ok(())
}
