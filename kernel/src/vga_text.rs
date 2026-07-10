// SPDX-License-Identifier: MPL-2.0

//! Minimal VGA text mode driver for early display output.
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicUsize, Ordering};

const VGA_PA: usize = 0xB8000;
/// On x86_64 with 48-bit VA, LINEAR_MAPPING_BASE_VADDR = 0xFFFF_8000_0000_0000
/// So VGA virtual address = LINEAR_MAPPING_BASE_VADDR + 0xB8000
const VGA_VA: usize = 0xFFFF_8000_000B_8000;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const VGA_ATTR: u8 = 0x0A; // Light green on black

static CURSOR: AtomicUsize = AtomicUsize::new(0);

/// No-op init — VGA virtual address is a compile-time constant.
pub fn init() {}

fn vga_base() -> usize {
    VGA_VA
}

/// Write a string to the VGA text display.
pub fn vga_print(s: &str) {
    for byte in s.bytes() {
        vga_write_byte(byte);
    }
}

pub fn vga_println(s: &str) {
    vga_print(s);
    vga_newline();
}

fn vga_write_byte(byte: u8) {
    if byte == b'\n' {
        vga_newline();
        return;
    }
    let pos = CURSOR.fetch_add(1, Ordering::Relaxed);
    let col = pos % VGA_WIDTH;
    let row = pos / VGA_WIDTH;

    if row >= VGA_HEIGHT {
        vga_scroll();
        return;
    }

    let base = vga_base();
    if base == 0 {
        return;
    }
    let idx = row * VGA_WIDTH + col;
    unsafe {
        let ptr = (base + idx * 2) as *mut u8;
        core::ptr::write_volatile(ptr, byte);
        core::ptr::write_volatile(ptr.add(1), VGA_ATTR);
    }
}

fn vga_newline() {
    let pos = CURSOR.load(Ordering::Relaxed);
    let col = pos % VGA_WIDTH;
    CURSOR.fetch_add(VGA_WIDTH - col, Ordering::Relaxed);

    let new_pos = CURSOR.load(Ordering::Relaxed);
    if new_pos / VGA_WIDTH >= VGA_HEIGHT {
        vga_scroll();
    }
}

fn vga_scroll() {
    let base = vga_base();
    if base == 0 {
        return;
    }
    for row in 1..VGA_HEIGHT {
        for col in 0..VGA_WIDTH {
            let src = (row * VGA_WIDTH + col) * 2;
            let dst = ((row - 1) * VGA_WIDTH + col) * 2;
            unsafe {
                let c = core::ptr::read_volatile((base + src) as *const u8);
                let a = core::ptr::read_volatile((base + src + 1) as *const u8);
                core::ptr::write_volatile((base + dst) as *mut u8, c);
                core::ptr::write_volatile((base + dst + 1) as *mut u8, a);
            }
        }
    }
    for col in 0..VGA_WIDTH {
        let idx = ((VGA_HEIGHT - 1) * VGA_WIDTH + col) * 2;
        unsafe {
            core::ptr::write_volatile((base + idx) as *mut u8, b' ');
            core::ptr::write_volatile((base + idx + 1) as *mut u8, VGA_ATTR);
        }
    }
    CURSOR.store((VGA_HEIGHT - 1) * VGA_WIDTH, Ordering::Relaxed);
}

pub fn vga_clear() {
    let base = vga_base();
    if base == 0 {
        return;
    }
    for i in 0..(VGA_WIDTH * VGA_HEIGHT) {
        unsafe {
            core::ptr::write_volatile((base + i * 2) as *mut u8, b' ');
            core::ptr::write_volatile((base + i * 2 + 1) as *mut u8, VGA_ATTR);
        }
    }
    CURSOR.store(0, Ordering::Relaxed);
}

pub fn print_banner() {
    init();
    vga_clear();
    vga_println("");
    vga_println("    ___   _______  _  _______  __   _______  __");
    vga_println("   / _ | / __/ _ \\/ \\/ / ___/ / /  / __/ _ \\/ /");
    vga_println("  / __ |/ _// , _/\\  / /__/ -_) /__/ _// , _/ /__");
    vga_println(" /_/ |_/___/_/|_| /_/\\___/\\__/____/___/_/|_|/____/");
    vga_println("");
    vga_println("         kei — Asterinas ARM64 Fork");
    vga_println("");
    vga_println("  Kernel booted on x86_64 QEMU.");
    vga_println("  ostd framework: 100% initialized");
    vga_println("  Display: VGA text mode (80x25)");
    vga_println("");
    vga_println("  Component init: in progress...");
}
