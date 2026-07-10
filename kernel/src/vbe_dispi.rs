// SPDX-License-Identifier: MPL-2.0

//! VBE (VESA BIOS Extensions) DISPI driver for QEMU Bochs VGA.
//!
//! QEMU's `-vga std` implements the Bochs VGA with VBE DISPI interface.
//! We can set a graphics mode by writing to I/O ports 0x1CE/0x1CF,
//! without needing INT 10h (which requires real mode).
//!
//! The linear framebuffer is at PCI BAR0 of the VGA device (typically
//! 0xE0000000 on QEMU). After setting the mode, we provide this info
//! to the Asterinas framebuffer component.

#![allow(unsafe_code)]

/// VBE DISPI I/O ports (Bochs/QEMU VGA)
const VBE_DISPI_IOPORT_INDEX: u16 = 0x1CE;
const VBE_DISPI_IOPORT_DATA: u16 = 0x1CF;

/// VBE DISPI register indices
const VBE_DISPI_INDEX_ID: u16 = 0;
const VBE_DISPI_INDEX_XRES: u16 = 1;
const VBE_DISPI_INDEX_YRES: u16 = 2;
const VBE_DISPI_INDEX_BPP: u16 = 3;
const VBE_DISPI_INDEX_ENABLE: u16 = 4;
const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 6;
const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 7;
const VBE_DISPI_INDEX_X_OFFSET: u16 = 8;
const VBE_DISPI_INDEX_Y_OFFSET: u16 = 9;

const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;
const VBE_DISPI_NOCLEARMEM: u16 = 0x80;

/// QEMU Bochs VGA PCI device info
const BOCHS_VGA_PCI_ADDR: u32 = 0x80000000; // PCI config address port base

/// Linear Frame Buffer address on QEMU (PCI BAR0 of VGA device)
/// QEMU's `-vga std` maps this at 0xFD000000 or 0xE0000000
/// We read it from PCI config space at runtime.

#[cfg(target_arch = "x86_64")]
unsafe fn port_write16(port: u16, val: u16) {
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") val);
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_read16(port: u16) -> u16 {
    let val: u16;
    unsafe {
        core::arch::asm!("in ax, dx", out("ax") val, in("dx") port);
    }
    val
}

unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x80000000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32);
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") addr);
        let val: u32;
        core::arch::asm!("in eax, dx", out("eax") val, in("dx") 0xCFCu16);
        val
    }
}

/// Find the Bochs VGA device on PCI bus 0 and return its BAR0 (LFB address).
unsafe fn find_vga_framebuffer() -> Option<usize> {
    // Scan PCI bus 0 for VGA device (class 0x030000)
    for dev in 0..32u8 {
        let vendor_device = unsafe { pci_read32(0, dev, 0, 0) };
        if vendor_device == 0xFFFFFFFF {
            continue;
        }

        let class_code = unsafe { pci_read32(0, dev, 0, 0x08) };
        let base_class = (class_code >> 24) & 0xFF;
        let sub_class = (class_code >> 16) & 0xFF;

        // VGA compatible controller (class 0x03, subclass 0x00)
        if base_class == 0x03 {
            // Read BAR0 (offset 0x10)
            let bar0 = unsafe { pci_read32(0, dev, 0, 0x10) };
            let lfb_addr = (bar0 & 0xFFFFFFF0) as usize;
            return Some(lfb_addr);
        }
    }
    None
}

/// Set VBE graphics mode via DISPI interface.
///
/// This programs the QEMU Bochs VGA to a graphics mode with a linear
/// framebuffer, without needing INT 10h (BIOS calls).
pub fn set_graphics_mode(
    width: u16,
    height: u16,
    bpp: u16,
) -> Option<(usize, usize, usize, usize)> {
    unsafe {
        // Check VBE DISPI ID
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_ID);
        let id = port_read16(VBE_DISPI_IOPORT_DATA);
        if id < 0xB0C0 {
            return None; // No VBE DISPI support
        }

        // Disable VBE
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_ENABLE);
        port_write16(VBE_DISPI_IOPORT_DATA, VBE_DISPI_DISABLED);

        // Set resolution
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_XRES);
        port_write16(VBE_DISPI_IOPORT_DATA, width);
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_YRES);
        port_write16(VBE_DISPI_IOPORT_DATA, height);

        // Set BPP
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_BPP);
        port_write16(VBE_DISPI_IOPORT_DATA, bpp);

        // Set virtual resolution (same as physical)
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_VIRT_WIDTH);
        port_write16(VBE_DISPI_IOPORT_DATA, width);
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_VIRT_HEIGHT);
        port_write16(VBE_DISPI_IOPORT_DATA, height);

        // Set offsets to 0
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_X_OFFSET);
        port_write16(VBE_DISPI_IOPORT_DATA, 0);
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_Y_OFFSET);
        port_write16(VBE_DISPI_IOPORT_DATA, 0);

        // Enable VBE with LFB
        port_write16(VBE_DISPI_IOPORT_INDEX, VBE_DISPI_INDEX_ENABLE);
        port_write16(
            VBE_DISPI_IOPORT_DATA,
            VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
        );

        // Find the linear framebuffer address via PCI
        let fb_addr = find_vga_framebuffer()?;

        Some((fb_addr, width as usize, height as usize, bpp as usize))
    }
}

/// Draw a filled rectangle on the framebuffer (for testing).
pub fn draw_rect(
    fb_addr: usize,
    fb_width: usize,
    fb_height: usize,
    bpp: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    let bytes_per_pixel = bpp / 8;
    let pitch = fb_width * bytes_per_pixel;

    // Convert fb_addr to virtual address (linear mapping)
    let linear_base = 0xFFFF_8000_0000_0000usize;
    let fb_va = linear_base + fb_addr;

    for row in y..(y + h).min(fb_height) {
        for col in x..(x + w).min(fb_width) {
            let offset = row * pitch + col * bytes_per_pixel;
            unsafe {
                let ptr = (fb_va + offset) as *mut u8;
                if bpp == 32 {
                    // BGRA format
                    core::ptr::write_volatile(ptr, b);
                    core::ptr::write_volatile(ptr.add(1), g);
                    core::ptr::write_volatile(ptr.add(2), r);
                    core::ptr::write_volatile(ptr.add(3), 0xFF);
                }
            }
        }
    }
}
