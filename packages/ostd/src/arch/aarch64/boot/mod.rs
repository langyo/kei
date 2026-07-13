// SPDX-License-Identifier: MPL-2.0

//! The ARM64 boot module defines the entrypoints of Asterinas.

pub(crate) mod smp;

use core::arch::global_asm;

use fdt::Fdt;
use spin::Once;

use crate::{
    boot::{
        BootloaderAcpiArg, BootloaderFramebufferArg,
        memory_region::{MemoryRegion, MemoryRegionArray, MemoryRegionType},
    },
    mm::paddr_to_vaddr,
};

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

fn parse_framebuffer_info() -> Option<BootloaderFramebufferArg> {
    None
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
