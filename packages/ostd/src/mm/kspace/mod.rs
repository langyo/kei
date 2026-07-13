// SPDX-License-Identifier: MPL-2.0

//! Kernel memory space management.
//!
//! The kernel memory space is currently managed as follows, if the
//! address width is 48 bits (with 47 bits kernel space).
//!
//! TODO: the cap of linear mapping (the start of vm alloc) are raised
//! to workaround for high IO in TDX. We need actual vm alloc API to have
//! a proper fix.
//!
//! ```text
//! +-+ <- the highest used address (0xffff_ffff_ffff_0000)
//! | |         For the kernel code, 1 GiB.
//! +-+ <- 0xffff_ffff_8000_0000
//! | |
//! | |         Unused hole.
//! +-+ <- 0xffff_e100_0000_0000
//! | |         For frame metadata, 1 TiB.
//! +-+ <- 0xffff_e000_0000_0000
//! | |         For [`KVirtArea`], 32 TiB.
//! +-+ <- the middle of the higher half (0xffff_c000_0000_0000)
//! | |
//! | |
//! | |
//! | |         For linear mappings, 64 TiB.
//! | |         Mapped physical addresses are untracked.
//! | |
//! | |
//! | |
//! +-+ <- the base of high canonical address (0xffff_8000_0000_0000)
//! ```
//!
//! If the address width is (according to [`crate::arch::mm::PagingConsts`])
//! 39 bits or 57 bits, the memory space just adjust proportionally.

#![cfg_attr(target_arch = "loongarch64", expect(unused_imports))]

pub(crate) mod kvirt_area;

use core::ops::Range;

use spin::Once;

#[cfg(ktest)]
mod test;

use super::{
    Frame, HasSize, Paddr, PagingConstsTrait, Vaddr,
    frame::{
        Segment,
        meta::{AnyFrameMeta, MetaPageMeta, mapping},
    },
    page_prop::{CachePolicy, PageFlags, PageProperty, PrivilegedPageFlags},
    page_table::{PageTable, PageTableConfig},
};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    boot::memory_region::MemoryRegionType,
    const_assert, info,
    mm::{HasPaddr, PAGE_SIZE, PagingLevel, frame::FrameRef, page_table::largest_pages},
    task::disable_preempt,
};

// The shortest supported address width is 39 bits. So the literal
// values are written for 39 bits address width and we adjust the values
// by arithmetic left shift.
const_assert!(PagingConsts::ADDRESS_WIDTH >= 39);
const ADDR_WIDTH_SHIFT: usize = PagingConsts::ADDRESS_WIDTH - 39;

/// Start of the kernel address space.
#[cfg(not(target_arch = "loongarch64"))]
pub const KERNEL_BASE_VADDR: Vaddr = 0xffff_ffc0_0000_0000 << ADDR_WIDTH_SHIFT;
#[cfg(target_arch = "loongarch64")]
pub const KERNEL_BASE_VADDR: Vaddr = 0x9000_0000_0000_0000;
/// End of the kernel address space (non inclusive).
pub const KERNEL_END_VADDR: Vaddr = 0xffff_ffff_ffff_0000;

/// The maximum virtual address of user space (non inclusive).
///
/// A typical way to reserve half of the address space for the kernel is
/// to use the highest `ADDRESS_WIDTH`-bit virtual address space.
///
/// Also, the top page is not regarded as usable since it's a workaround
/// for some x86_64 CPUs' bugs. See
/// <https://github.com/torvalds/linux/blob/480e035fc4c714fb5536e64ab9db04fedc89e910/arch/x86/include/asm/page_64.h#L68-L78>
/// for the rationale.
pub const MAX_USERSPACE_VADDR: Vaddr = (0x0000_0040_0000_0000 << ADDR_WIDTH_SHIFT) - PAGE_SIZE;

/// The kernel address space.
///
/// They are the high canonical addresses (i.e., the negative part of the
/// address space, with the most significant bits in the addresses set).
pub const KERNEL_VADDR_RANGE: Range<Vaddr> = KERNEL_BASE_VADDR..KERNEL_END_VADDR;

/// The kernel code is linear mapped to this address.
///
/// FIXME: This offset should be randomly chosen by the loader or the
/// boot compatibility layer. But we disabled it because OSTD
/// doesn't support relocatable kernel yet.
pub fn kernel_loaded_offset() -> usize {
    KERNEL_CODE_BASE_VADDR
}

#[cfg(target_arch = "x86_64")]
const KERNEL_CODE_BASE_VADDR: usize = 0xffff_ffff_8000_0000;
#[cfg(target_arch = "aarch64")]
const KERNEL_CODE_BASE_VADDR: usize = 0xffff_8000_0000_0000;
#[cfg(target_arch = "riscv64")]
const KERNEL_CODE_BASE_VADDR: usize = 0xffff_ffff_0000_0000;
#[cfg(target_arch = "loongarch64")]
const KERNEL_CODE_BASE_VADDR: usize = 0x9000_0000_0000_0000;

const FRAME_METADATA_CAP_VADDR: Vaddr = 0xffff_fff0_8000_0000 << ADDR_WIDTH_SHIFT;
const FRAME_METADATA_BASE_VADDR: Vaddr = 0xffff_fff0_0000_0000 << ADDR_WIDTH_SHIFT;
pub(in crate::mm) const FRAME_METADATA_RANGE: Range<Vaddr> =
    FRAME_METADATA_BASE_VADDR..FRAME_METADATA_CAP_VADDR;

const VMALLOC_BASE_VADDR: Vaddr = 0xffff_ffe0_0000_0000 << ADDR_WIDTH_SHIFT;
pub const VMALLOC_VADDR_RANGE: Range<Vaddr> = VMALLOC_BASE_VADDR..FRAME_METADATA_BASE_VADDR;

/// The base address of the linear mapping of all physical
/// memory in the kernel address space.
#[cfg(not(target_arch = "loongarch64"))]
pub const LINEAR_MAPPING_BASE_VADDR: Vaddr = 0xffff_ffc0_0000_0000 << ADDR_WIDTH_SHIFT;
#[cfg(target_arch = "loongarch64")]
pub const LINEAR_MAPPING_BASE_VADDR: Vaddr = 0x9000_0000_0000_0000;
pub const LINEAR_MAPPING_VADDR_RANGE: Range<Vaddr> = LINEAR_MAPPING_BASE_VADDR..VMALLOC_BASE_VADDR;

/// Convert physical address to virtual address using offset, only available inside `ostd`
pub fn paddr_to_vaddr(pa: Paddr) -> usize {
    debug_assert!(pa < VMALLOC_BASE_VADDR - LINEAR_MAPPING_BASE_VADDR);
    pa + LINEAR_MAPPING_BASE_VADDR
}

/// The kernel page table instance.
///
/// It manages the kernel mapping of all address spaces by sharing the kernel part. And it
/// is unlikely to be activated.
pub(super) static KERNEL_PAGE_TABLE: Once<PageTable<KernelPtConfig>> = Once::new();

#[derive(Clone, Debug)]
pub(super) struct KernelPtConfig {}

// We use the first available PTE bit to mark the frame as tracked.
// SAFETY: `item_raw_info`, `item_into_raw`, `item_from_raw`, and
// `item_ref_from_raw` are correctly implemented with respect to the `Item` and
// `ItemRef` types.
unsafe impl PageTableConfig for KernelPtConfig {
    const TOP_LEVEL_INDEX_RANGE: Range<usize> = 256..512;
    const TOP_LEVEL_CAN_UNMAP: bool = false;

    type E = PageTableEntry;
    type C = PagingConsts;

    type Item = MappedItem;
    type ItemRef<'a> = MappedItemRef<'a>;

    fn item_raw_info(item: &Self::Item) -> (Paddr, PagingLevel, PageProperty) {
        match *item {
            MappedItem::Tracked(ref frame, mut prop) => {
                debug_assert!(!prop.priv_flags.contains(PrivilegedPageFlags::AVAIL1));
                prop.priv_flags |= PrivilegedPageFlags::AVAIL1;
                let level = frame.map_level();
                let paddr = frame.paddr();
                (paddr, level, prop)
            }
            MappedItem::Untracked(ref pa, ref level, mut prop) => {
                debug_assert!(!prop.priv_flags.contains(PrivilegedPageFlags::AVAIL1));
                prop.priv_flags -= PrivilegedPageFlags::AVAIL1;
                (*pa, *level, prop)
            }
        }
    }

    unsafe fn item_from_raw(paddr: Paddr, level: PagingLevel, prop: PageProperty) -> Self::Item {
        if prop.priv_flags.contains(PrivilegedPageFlags::AVAIL1) {
            debug_assert_eq!(level, 1);
            // SAFETY: The caller ensures safety.
            let frame = unsafe { Frame::<dyn AnyFrameMeta>::from_raw(paddr) };
            MappedItem::Tracked(frame, prop)
        } else {
            MappedItem::Untracked(paddr, level, prop)
        }
    }

    unsafe fn item_ref_from_raw<'a>(
        paddr: Paddr,
        level: PagingLevel,
        prop: PageProperty,
    ) -> Self::ItemRef<'a> {
        if prop.priv_flags.contains(PrivilegedPageFlags::AVAIL1) {
            debug_assert_eq!(level, 1);
            // SAFETY: The caller ensures that the frame outlives `'a` and that
            // the type matches the frame.
            let frame = unsafe { FrameRef::<dyn AnyFrameMeta>::borrow_paddr(paddr) };
            MappedItemRef::Tracked(frame, prop)
        } else {
            MappedItemRef::Untracked(paddr, level, prop)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MappedItem {
    Tracked(Frame<dyn AnyFrameMeta>, PageProperty),
    Untracked(Paddr, PagingLevel, PageProperty),
}

#[derive(Debug)]
pub(crate) enum MappedItemRef<'a> {
    #[cfg_attr(not(ktest), expect(dead_code))]
    Tracked(FrameRef<'a, dyn AnyFrameMeta>, PageProperty),
    #[cfg_attr(not(ktest), expect(dead_code))]
    Untracked(Paddr, PagingLevel, PageProperty),
}

/// Initializes the kernel page table.
///
/// This function should be called after:
///  - the page allocator and the heap allocator are initialized;
///  - the memory regions are initialized.
///
/// This function should be called before:
///  - any initializer that modifies the kernel page table.
pub fn init_kernel_page_table(meta_pages: Segment<MetaPageMeta>) {
    crate::early_println!("[kpt] init_kernel_page_table: start");
    let kpt = PageTable::<KernelPtConfig>::new_kernel_page_table();
    let preempt_guard = disable_preempt();

    #[cfg(not(target_arch = "loongarch64"))]
    {
        crate::early_println!("[kpt] linear mapping");
        let max_paddr = crate::mm::frame::max_paddr();
        let from = LINEAR_MAPPING_BASE_VADDR..LINEAR_MAPPING_BASE_VADDR + max_paddr;
        let prop = PageProperty {
            flags: PageFlags::RWX,
            cache: CachePolicy::Writeback,
            priv_flags: PrivilegedPageFlags::GLOBAL,
        };
        let mut cursor = kpt.cursor_mut(&preempt_guard, &from).unwrap();
        for (pa, level) in largest_pages::<KernelPtConfig>(from.start, 0, max_paddr) {
            unsafe { cursor.map(MappedItem::Untracked(pa, level, prop)) };
        }
        crate::early_println!("[kpt] linear mapping done");
    }

    {
        crate::early_println!("[kpt] metadata mapping");
        let start_va = mapping::frame_to_meta::<PagingConsts>(0);
        let from = start_va..start_va + meta_pages.size();
        let prop = PageProperty {
            flags: PageFlags::RW,
            cache: CachePolicy::Writeback,
            priv_flags: PrivilegedPageFlags::GLOBAL,
        };
        let mut cursor = kpt.cursor_mut(&preempt_guard, &from).unwrap();
        let pa_range = meta_pages.into_raw();
        for (pa, level) in
            largest_pages::<KernelPtConfig>(from.start, pa_range.start, pa_range.len())
        {
            unsafe { cursor.map(MappedItem::Untracked(pa, level, prop)) };
        }
        crate::early_println!("[kpt] metadata mapping done");
    }

    // The kernel code is already mapped by the linear mapping above with RWX.
    // No separate kernel code mapping is needed for aarch64 since the kernel
    // runs in the linear mapping address space.
    #[cfg(not(any(target_arch = "loongarch64", target_arch = "aarch64")))]
    {
        crate::early_println!("[kpt] kernel code mapping");
        let regions = &crate::boot::EARLY_INFO.get().unwrap().memory_regions;
        let region = regions
            .iter()
            .find(|r| r.typ() == MemoryRegionType::Kernel)
            .unwrap();
        let offset = kernel_loaded_offset();
        let from = region.base() + offset..region.end() + offset;
        let prop = PageProperty {
            flags: PageFlags::RWX,
            cache: CachePolicy::Writeback,
            priv_flags: PrivilegedPageFlags::GLOBAL,
        };
        let mut cursor = kpt.cursor_mut(&preempt_guard, &from).unwrap();
        for (pa, level) in largest_pages::<KernelPtConfig>(from.start, region.base(), from.len()) {
            unsafe { cursor.map(MappedItem::Untracked(pa, level, prop)) };
        }
        crate::early_println!("[kpt] kernel code mapping done");
    }

    KERNEL_PAGE_TABLE.call_once(|| kpt);
    crate::early_println!("[kpt] init_kernel_page_table: done");
}

/// Activates the kernel page table.
///
/// All address translation of symbols in the boot sections must be manually
/// done from now on.
///
/// # Safety
///
/// This function must only be called once per CPU.
pub unsafe fn activate_kernel_page_table() {
    crate::early_println!("[kpt] activate: getting KERNEL_PAGE_TABLE...");
    let kpt = KERNEL_PAGE_TABLE
        .get()
        .expect("The kernel page table is not initialized yet");
    crate::early_println!("[kpt] activate: calling first_activate_unchecked...");
    // SAFETY: the kernel page table is initialized properly.
    // first_activate_unchecked writes TTBR0/TTBR1 and flushes the TLB. After
    // it returns, the boot page table's identity mapping (incl. the UART at
    // PA 0x09000000) is gone. We MUST reinit the serial port to its linear
    // mapping address before ANY console output — including the debug
    // println below — otherwise the first early_println faults (Data Abort
    // on the identity UART address).
    unsafe {
        kpt.first_activate_unchecked();
    }
    #[cfg(target_arch = "aarch64")]
    crate::arch::serial::reinit_with_linear_mapping();
    // Only now is the console safe to use again.
    crate::early_println!("[kpt] activate: page table switched + serial reinitialized OK");
}
