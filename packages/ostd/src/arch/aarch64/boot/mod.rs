// SPDX-License-Identifier: MPL-2.0

//! The ARM64 boot module defines the entrypoints of Asterinas.

pub(crate) mod smp;

use core::arch::global_asm;

use fdt::Fdt;
use spin::Once;

use crate::{
    arch::serial::{UartKind, UartProbe},
    boot::{
        BootloaderAcpiArg, BootloaderFramebufferArg,
        memory_region::{MemoryRegion, MemoryRegionArray, MemoryRegionType},
    },
    mm::paddr_to_vaddr,
};

fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}

/// Blink all three LEDs on NanoPi R3S (RK3566) to prove the kernel entered
/// Rust code. GPIO addresses are from the board TOML config, compiled in
/// via build.rs → board_config.rs.
/// Blink all board LEDs on boot to prove the kernel entered Rust code.
/// Uses the board_config.rs generated from TOML for GPIO addresses.
#[cfg(not(feature = "cvm_guest"))]
fn blink_led_rk3566() {
    include!("board_config.rs");

    let get_gpio_base = |name: &str| -> usize {
        match name {
            "GPIO0" => gpio::GPIO0,
            "GPIO1" => gpio::GPIO1,
            "GPIO2" => gpio::GPIO2,
            "GPIO3" => gpio::GPIO3,
            "GPIO4" => gpio::GPIO4,
            _ => gpio::GPIO0,
        }
    };

    let ddr = gpio_reg::SWPORT_DDR;
    let dr = gpio_reg::SWPORT_DR;
    const DELAY: usize = 2_000_000;

    for led in LEDS {
        let base = paddr_to_vaddr(get_gpio_base(led.ctrl));
        let bit = 1u32 << led.pin;
        unsafe { core::ptr::write_volatile((base + ddr) as *mut u32, bit); }
    }

    for led in LEDS {
        let base = paddr_to_vaddr(get_gpio_base(led.ctrl));
        let bit = 1u32 << led.pin;
        unsafe { core::ptr::write_volatile((base + dr) as *mut u32, bit); }
        for _ in 0..DELAY { core::hint::spin_loop(); }
        unsafe { core::ptr::write_volatile((base + dr) as *mut u32, 0); }
        for _ in 0..DELAY { core::hint::spin_loop(); }
    }
}

/// Public LED debug signal (real hardware only). On QEMU/cvm_guest, no-op.
/// Both LEDs blink together (separator), then WAN=N/10 times, LAN=N%10 times.
#[cfg(not(feature = "cvm_guest"))]
pub fn led_debug(code: u32) {
    include!("board_config.rs");
    let get_base = |name: &str| -> usize {
        match name { "GPIO0" => gpio::GPIO0, "GPIO3" => gpio::GPIO3, _ => gpio::GPIO0 }
    };
    let ddr = gpio_reg::SWPORT_DDR;
    let dr = gpio_reg::SWPORT_DR;
    const D: usize = 400_000;
    let wan = LEDS.iter().find(|l| l.name == "wan").map(|l| (get_base(l.ctrl), 1u32 << l.pin));
    let lan = LEDS.iter().find(|l| l.name == "lan").map(|l| (get_base(l.ctrl), 1u32 << l.pin));
    // Separator
    for led in LEDS {
        let (b, m) = (get_base(led.ctrl), 1u32 << led.pin);
        unsafe { core::ptr::write_volatile((b + ddr) as *mut u32, m); }
        unsafe { core::ptr::write_volatile((b + dr) as *mut u32, m); }
        for _ in 0..D { core::hint::spin_loop(); }
        unsafe { core::ptr::write_volatile((b + dr) as *mut u32, 0); }
        for _ in 0..D { core::hint::spin_loop(); }
    }
    for _ in 0..D*3 { core::hint::spin_loop(); }
    let tens = code / 10; let ones = code % 10;
    if let Some((b, m)) = wan {
        unsafe { core::ptr::write_volatile((b + ddr) as *mut u32, m); }
        for _ in 0..tens {
            unsafe { core::ptr::write_volatile((b + dr) as *mut u32, m); }
            for _ in 0..D { core::hint::spin_loop(); }
            unsafe { core::ptr::write_volatile((b + dr) as *mut u32, 0); }
            for _ in 0..D/2 { core::hint::spin_loop(); }
        }
    }
    for _ in 0..D { core::hint::spin_loop(); }
    if let Some((b, m)) = lan {
        unsafe { core::ptr::write_volatile((b + ddr) as *mut u32, m); }
        for _ in 0..ones {
            unsafe { core::ptr::write_volatile((b + dr) as *mut u32, m); }
            for _ in 0..D { core::hint::spin_loop(); }
            unsafe { core::ptr::write_volatile((b + dr) as *mut u32, 0); }
            for _ in 0..D/2 { core::hint::spin_loop(); }
        }
    }
    for _ in 0..D*3 { core::hint::spin_loop(); }
}

#[cfg(feature = "cvm_guest")]
pub fn led_debug(_code: u32) {}

global_asm!(include_str!("bsp_boot.S"));

/// The Flattened Device Tree of the platform.
pub static DEVICE_TREE: Once<Fdt> = Once::new();

/// FDT physical address and size, saved for reserving its memory region.
pub static FDT_PHYS: Once<(usize, usize)> = Once::new();

fn parse_bootloader_name() -> &'static str {
    "QEMU virt"
}

fn parse_kernel_commandline() -> &'static str {
    DEVICE_TREE.get().unwrap().chosen().bootargs().unwrap_or("")
}

fn parse_initramfs() -> Option<&'static [u8]> {
    let (start, end) = parse_initramfs_range()?;

    let base_va = paddr_to_vaddr(start);
    let length = end - start;
    Some(unsafe { core::slice::from_raw_parts(base_va as *const u8, length) })
}

fn parse_acpi_arg() -> BootloaderAcpiArg {
    BootloaderAcpiArg::NotProvided
}

/// Probe the UART from the FDT.
/// Called early in boot to replace the QEMU PL011 default with the
/// correct hardware UART (e.g., DW 8250 on Rockchip/Broadcom/Allwinner).
fn probe_uart_from_fdt(devicetree: &Fdt) -> UartProbe {
    // Strategy: find the first UART node by compatible string,
    // preferring the /chosen stdout-path alias.

    // 1. Check /chosen stdout-path for a serial alias (e.g. "serial2:1500000n8")
    if let Some(chosen) = devicetree.find_node("/chosen") {
        if let Some(stdout) = chosen.property("stdout-path") {
            if let Ok(path_str) = core::str::from_utf8(stdout.value) {
                // Extract the alias part (before ':')
                let alias = path_str.split(':').next().unwrap_or(path_str);
                // Resolve the alias: e.g., "serial2" → look up "/aliases/serial2"
                if let Some(aliases) = devicetree.find_node("/aliases") {
                    if let Some(alias_prop) = aliases.property(alias) {
                        if let Ok(alias_path) = core::str::from_utf8(alias_prop.value) {
                            if let Some(node) = devicetree.find_node(alias_path.trim_end_matches('\0')) {
                                if let Some(probe) = parse_uart_node(node) {
                                    return probe;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Search by compatible strings in order of preference:
    // Standard 8250-compatible, then Rockchip-specific, then PL011 (QEMU fallback)
    let compat_lists: &[&[&str]] = &[
        &["snps,dw-apb-uart"],       // Synopsys DesignWare 8250
        &["ns16550a", "ns16550"],    // Standard 8250/16550
        &["arm,pl011"],              // ARM PL011 (QEMU)
    ];

    for compat_list in compat_lists {
        if let Some(node) = devicetree.find_compatible(compat_list) {
            if let Some(probe) = parse_uart_node(node) {
                return probe;
            }
        }
    }

    // 3. Fallback: PL011 at QEMU default address
    UartProbe::default_qemu()
}

/// Parse a UART device tree node into a UartProbe.
fn parse_uart_node(node: fdt::node::FdtNode) -> Option<UartProbe> {
    let reg = node.property("reg")?.value;
    if reg.len() < 8 {
        return None;
    }
    // FDT reg is two u32 cells (address_high, address_low) or two u64 cells
    let base = if reg.len() >= 16 {
        u64::from_be_bytes(reg[0..8].try_into().ok()?) as usize
    } else {
        // Two u32 cells
        let high = u32::from_be_bytes(reg[0..4].try_into().ok()?) as u64;
        let low = u32::from_be_bytes(reg[4..8].try_into().ok()?) as u64;
        ((high << 32) | low) as usize
    };

    let compat = node.property("compatible")?.value;
    let kind = if has_compat(compat, b"arm,pl011") {
        UartKind::Pl011
    } else {
        let reg_shift_raw = node.property("reg-shift").and_then(|s| s.as_usize()).unwrap_or(0);
        let io_width_raw = node.property("reg-io-width").and_then(|w| w.as_usize()).unwrap_or(1);
        UartKind::Dw8250 {
            reg_shift: reg_shift_raw as u32,
            io_width: io_width_raw as u32,
        }
    };

    Some(UartProbe { base, kind })
}

/// Check if a compatible bytes string contains the given exact match.
fn has_compat(compat_bytes: &[u8], target: &[u8]) -> bool {
    let s = compat_bytes;
    let t = target;
    if s.len() < t.len() {
        return false;
    }
    // Simple substring search for null-terminated compatible strings
    for start in 0..s.len() - t.len() {
        if &s[start..start + t.len()] == t {
            // Check word boundary
            let before_ok = start == 0 || s[start - 1] == 0;
            let after_ok = start + t.len() >= s.len() || s[start + t.len()] == 0;
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

fn parse_framebuffer_info() -> Option<BootloaderFramebufferArg> {
    let devicetree = DEVICE_TREE.get().unwrap();

    // Try the standard simple-framebuffer node.
    if let Some(node) = devicetree.find_node("/reserved-memory/framebuffer")
        .or_else(|| devicetree.find_node("/framebuffer"))
        .or_else(|| devicetree.find_node("/simple-framebuffer"))
    {
        if let Some(fb_info) = parse_simplefb_node(node) {
            crate::early_println!("[kei] framebuffer from FDT: {}x{} bpp={} @ {:#x}",
                fb_info.width, fb_info.height, fb_info.bpp, fb_info.address);
            return Some(fb_info);
        }
    }

    // Bootloader may inject framebuffer info into /chosen node.
    if let Some(chosen) = devicetree.find_node("/chosen") {
        if let Some(fb_info) = parse_simplefb_node(chosen) {
            crate::early_println!("[kei] framebuffer from /chosen: {}x{} bpp={} @ {:#x}",
                fb_info.width, fb_info.height, fb_info.bpp, fb_info.address);
            return Some(fb_info);
        }
    }

    None
}

/// Parse a `simple-framebuffer` compatible node from the FDT.
fn parse_simplefb_node(node: fdt::node::FdtNode) -> Option<BootloaderFramebufferArg> {
    let reg = node.property("reg")?.value;
    if reg.len() < 8 {
        return None;
    }
    // reg is typically two u32 pairs (address, size) for 32-bit or two u64 for 64-bit.
    // U-Boot on aarch64 typically uses 64-bit values.
    let address = if reg.len() >= 16 {
        u64::from_be_bytes(reg[0..8].try_into().ok()?) as usize
    } else {
        u32::from_be_bytes(reg[0..4].try_into().ok()?) as usize
    };

    let width = node.property("width")?.as_usize()?;
    let height = node.property("height")?.as_usize()?;
    let stride = node.property("stride").and_then(|s| s.as_usize());

    let format = node
        .property("format")
        .and_then(|s| core::str::from_utf8(s.value).ok());
    let bpp = match format {
        Some("a8r8g8b8") | Some("x8r8g8b8") => 32,
        Some("r5g6b5") => 16,
        Some("a8b8g8r8") | Some("x8b8g8r8") => 32,
        _ => {
            // Fall back to stride-based BPP guess.
            if let Some(stride) = stride {
                if width > 0 { (stride / width) * 8 } else { 32 }
            } else {
                32 // Default to 32bpp (most HDMI framebuffers)
            }
        }
    };

    Some(BootloaderFramebufferArg {
        address,
        width,
        height,
        bpp,
    })
}

fn parse_memory_regions() -> MemoryRegionArray {
    let mut regions = MemoryRegionArray::new();

    for region in DEVICE_TREE.get().unwrap().memory().regions() {
        if region.size.unwrap_or(0) > 0 {
            regions
                .push(MemoryRegion::new(
                    region.starting_address as usize,
                    region.size.unwrap(),
                    MemoryRegionType::Usable,
                ))
                .unwrap();
        }
    }

    // Add the kernel region.
    regions.push(MemoryRegion::kernel()).unwrap();

    // Add the initramfs region.
    if let Some((start, end)) = parse_initramfs_range() {
        regions
            .push(MemoryRegion::new(
                start,
                end - start,
                MemoryRegionType::Module,
            ))
            .unwrap();
    }

    // Reserve FDT memory region, like Linux's memblock_reserve(dtb_start, dtb_size).
    // QEMU places the FDT in usable RAM; without reserving it, the frame allocator
    // will reclaim that memory and overwrite the FDT data.
    if let Some((fdt_paddr, fdt_size)) = FDT_PHYS.get() {
        regions
            .push(MemoryRegion::new(
                *fdt_paddr,
                *fdt_size,
                MemoryRegionType::Reserved,
            ))
            .unwrap();
    }

    // Reserve framebuffer region so the frame allocator doesn't reclaim the
    // buffer set up by the bootloader (U-Boot simple-framebuffer).
    if let Some(fb) = parse_framebuffer_info() {
        let fb_size = align_up(fb.width * fb.height * fb.bpp / 8, 4096);
        regions
            .push(MemoryRegion::new(
                fb.address,
                fb_size,
                MemoryRegionType::Reserved,
            ))
            .unwrap();
    }

    regions.into_non_overlapping()
}

fn parse_initramfs_range() -> Option<(usize, usize)> {
    let chosen = DEVICE_TREE.get().unwrap().find_node("/chosen").unwrap();
    let initrd_start = chosen.property("linux,initrd-start")?.as_usize()?;
    let initrd_end = chosen.property("linux,initrd-end")?.as_usize()?;
    Some((initrd_start, initrd_end))
}

/// The entry point of the Rust code portion of Asterinas.
///
/// # Safety
///
/// - This function must be called only once at a proper timing in the BSP's boot assembly code.
/// - The caller must follow C calling conventions and put the right arguments in registers.
#[unsafe(no_mangle)]
unsafe extern "C" fn aarch64_boot(fdt_paddr: usize) -> ! {
    // Blink the power LED BEFORE any serial init, so we have proof of life
    // even if UART/display never work. On NanoPi R3S (RK3566):
    //   GPIO0 base = 0xFDD60000, power LED = GPIO0_PB7 (bit 15).
    // Uses the boot page table identity mapping (PA 0xC0000000-0xFFFFFFFF).
    #[cfg(not(feature = "cvm_guest"))]
    blink_led_rk3566();

    // Initialize early serial console FIRST, before any output.
    crate::arch::serial::init();

    crate::early_println!("[kei] aarch64_boot: entering Rust code");
    crate::early_println!("[kei] FDT physical address: {:#x}", fdt_paddr);

    // QEMU's `-kernel` only programs x0 = FDT address for non-ELF (ARM64
    // Image) kernels. When an ELF kernel is loaded, QEMU jumps to the ELF
    // entry with x0 = 0, but it still generates the device tree blob and
    // loads it somewhere in RAM. Recover it by scanning low RAM for the FDT
    // magic (0xd00dfeed). The boot page table maps the first 4 GiB of RAM
    // via 1 GiB blocks, so the scan is safe.
    // Ref: https://stackoverflow.com/questions/78957741/no-fdt-bootparam-in-aarch64-virt
    let fdt_paddr = if fdt_paddr == 0 {
        const FDT_MAGIC: u32 = 0xD00DFEED;
        let ram_base = 0x4000_0000usize;
        // Scan RAM for the FDT magic. QEMU places the DTB near the top of RAM
        // (just below the top-of-RAM, after the initrd). For a 2G guest
        // (base 0x40000000, top 0xC0000000), the DTB sits a few hundred KB
        // below the top. Scan the full low-RAM range downward so we find it
        // regardless of exact placement, but start near the top (fast path).
        let scan_top = 0xBFFF_F000usize;
        let scan_bottom = ram_base + 0x0020_0000; // skip the first 2 MiB (kernel image)
        let page_size = 4096usize;
        let mut found = 0usize;
        let mut addr = scan_top;
        loop {
            let ptr = paddr_to_vaddr(addr) as *const u32;
            let val = unsafe { core::ptr::read_volatile(ptr) };
            if val.to_le() == FDT_MAGIC {
                let size_ptr = paddr_to_vaddr(addr + 4) as *const u8;
                let mut sz = [0u8; 4];
                unsafe { core::ptr::copy_nonoverlapping(size_ptr, sz.as_mut_ptr(), 4) };
                let totalsize = u32::from_be_bytes(sz);
                if totalsize > 0 && totalsize < (4 << 20) {
                    found = addr;
                    break;
                }
            }
            if addr <= scan_bottom {
                break;
            }
            addr -= page_size;
        }
        crate::early_println!("[kei] FDT scan (x0=0): found at {:#x}", found);
        found
    } else {
        fdt_paddr
    };

    if fdt_paddr == 0 {
        crate::early_println!("[kei] FATAL: no FDT found, hanging");
        loop {
            core::hint::spin_loop();
        }
    }

    let fdt_ptr = paddr_to_vaddr(fdt_paddr) as *const u8;
    crate::early_println!("[kei] FDT virtual address: {:#x}", fdt_ptr as usize);

    let fdt = unsafe { Fdt::from_ptr(fdt_ptr).unwrap() };
    crate::early_println!("[kei] FDT parsed successfully, size={}", fdt.total_size());

    // Re-probe the UART from the FDT. On real hardware this detects
    // the correct UART type and address (e.g., DW 8250 on RK3566).
    // On QEMU virt, this falls back to PL011 at 0x09000000.
    let uart = probe_uart_from_fdt(&fdt);
    let uart_base = uart.base;
    crate::arch::serial::init_with_probe(uart);
    crate::early_println!("[kei] UART re-initialized (type={:?}, base={:#x})",
        crate::arch::serial::uart_kind(), uart_base);

    // Save FDT physical address and size for memory reservation.
    FDT_PHYS.call_once(|| (fdt_paddr, fdt.total_size()));
    DEVICE_TREE.call_once(|| fdt);
    crate::early_println!("[kei] DEVICE_TREE initialized");

    use crate::boot::{EARLY_INFO, EarlyBootInfo, start_kernel};

    crate::early_println!("[kei] parsing boot info...");
    EARLY_INFO.call_once(|| EarlyBootInfo {
        bootloader_name: parse_bootloader_name(),
        kernel_cmdline: parse_kernel_commandline(),
        initramfs: parse_initramfs(),
        acpi_arg: parse_acpi_arg(),
        framebuffer_arg: parse_framebuffer_info(),
        memory_regions: parse_memory_regions(),
    });
    crate::early_println!("[kei] boot info parsed, calling start_kernel()");

    unsafe { start_kernel() };
}
