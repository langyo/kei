// SPDX-License-Identifier: MPL-2.0

//! Multiprocessor Boot Support (aarch64).
//!
//! Uses PSCI `CPU_ON` to wake secondary CPUs and a dedicated AP boot
//! assembly entry point (`ap_boot.S`) to initialise each AP's MMU, stack
//! pointer, and CPU-local storage before jumping into Rust.

use core::arch::global_asm;

use super::DEVICE_TREE;
use crate::{
    boot::smp::{PerApRawInfo, ap_early_entry},
    mm::{Paddr, paddr_to_vaddr},
};

global_asm!(include_str!("ap_boot.S"));

pub(crate) fn count_processors() -> Option<u32> {
    let Some(fdt) = DEVICE_TREE.get() else {
        return Some(1);
    };

    let mut count = 0u32;
    fdt.cpus().for_each(|cpu_node| {
        if cpu_node
            .property("device_type")
            .map_or(false, |p| p.as_str() == Some("cpu"))
            && cpu_node.property("reg").is_some()
        {
            count += 1;
        }
    });

    if count == 0 { None } else { Some(count) }
}

/// Brings up all application processors via PSCI `CPU_ON`.
///
/// For each AP we:
/// 1. Read its MPIDR from the FDT `/cpus/cpu@N` node's `reg` property.
/// 2. Compute the physical address of `ap_boot_start` (the AP entry point).
/// 3. Call PSCI `CPU_ON(mpidr, entry_paddr, context_id=0)`.
///
/// # Safety
///
/// The caller must ensure that
///  1. we're in the boot context of the BSP,
///  2. all APs have not yet been booted, and
///  3. the arguments are valid to boot APs.
pub(crate) unsafe fn bringup_all_aps(info_ptr: *const PerApRawInfo, pt_ptr: Paddr, num_cpus: u32) {
    if num_cpus <= 1 {
        return;
    }

    // SAFETY: These statics are accessed exclusively during boot, before
    // any AP has started executing.
    unsafe {
        fill_boot_info_ptr(info_ptr);
        fill_boot_page_table_ptr(pt_ptr);
    }

    let bsp_mpidr = read_mpidr_el1();
    crate::info!(
        "Bootstrapping CPU (mpidr={:#x}), booting {} APs",
        bsp_mpidr,
        num_cpus - 1
    );

    // Collect all non-BSP MPIDRs from the FDT.
    let ap_mpidrs = collect_ap_mpidrs(bsp_mpidr);

    let entry_paddr = get_ap_boot_start_addr();

    for (cpu_id, mpidr) in ap_mpidrs.iter().enumerate() {
        // cpu_id here is 0-based for APs (0 = first AP, 1 = second, etc.)
        // The actual CPU ID passed to ap_early_entry is cpu_id + 1 (BSP is 0).
        let cpu_id = cpu_id as u32 + 1;

        crate::info!(
            "Starting CPU {} (mpidr={:#x}, entry={:#x})",
            cpu_id,
            mpidr,
            entry_paddr
        );

        // SAFETY: Each MPIDR is unique and the entry point is valid.
        let result = unsafe { psci_cpu_on(*mpidr, entry_paddr as u64, cpu_id as u64) };

        if result == 0 {
            crate::debug!("PSCI CPU_ON success for CPU {}", cpu_id);
        } else {
            crate::error!(
                "PSCI CPU_ON failed for CPU {} (mpidr={:#x}): code={}",
                cpu_id,
                mpidr,
                result
            );
        }
    }
}

// ---------------------------------------------------------------------------
// PSCI CPU_ON
// ---------------------------------------------------------------------------

/// PSCI v0.2 function ID for CPU_ON.
const PSCI_0_2_FN_CPU_ON: u64 = 0xC4000003;

/// PSCI return codes.
const PSCI_RET_SUCCESS: u64 = 0;

/// Invokes PSCI `CPU_ON` to start an AP at the given entry point.
///
/// Returns 0 on success (PSCI_RET_SUCCESS).
///
/// # Safety
///
/// The caller must ensure the MPIDR refers to a valid, offline CPU and
/// `entry_paddr` is a valid physical address containing boot code.
unsafe fn psci_cpu_on(mpidr: u64, entry_paddr: u64, context_id: u64) -> u64 {
    // Reuse the HVC/SMC mechanism from power.rs (same conduit).
    // PSCI uses the same conduit for all function calls.
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "hvc #0",
            inout("x0") PSCI_0_2_FN_CPU_ON => ret,
            in("x1") mpidr,
            in("x2") entry_paddr,
            in("x3") context_id,
            out("x4") _, out("x5") _, out("x6") _, out("x7") _,
            out("x8") _, out("x9") _, out("x10") _, out("x11") _,
            out("x12") _, out("x13") _, out("x14") _, out("x15") _,
            out("x16") _, out("x17") _,
        );
    }
    ret
}

// ---------------------------------------------------------------------------
// MPIDR helpers
// ---------------------------------------------------------------------------

fn read_mpidr_el1() -> u64 {
    let val: u64;
    unsafe {
        core::arch::asm!("mrs {0}, mpidr_el1", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Collects MPIDR values for all CPUs except the BSP from the device tree.
fn collect_ap_mpidrs(bsp_mpidr: u64) -> alloc::vec::Vec<u64> {
    let mut ap_mpidrs = alloc::vec::Vec::new();

    let Some(fdt) = DEVICE_TREE.get() else {
        return ap_mpidrs;
    };

    fdt.cpus().for_each(|cpu_node| {
        if let Some(reg) = cpu_node.property("reg") {
            let mpidr = reg.as_usize().unwrap_or(0) as u64;
            if mpidr != bsp_mpidr {
                ap_mpidrs.push(mpidr);
            }
        }
    });

    ap_mpidrs
}

// ---------------------------------------------------------------------------
// Boot info / page table pointer helpers (mirrors RISC-V)
// ---------------------------------------------------------------------------

unsafe fn fill_boot_info_ptr(info_ptr: *const PerApRawInfo) {
    // Use absolute addressing (movz+movk) instead of ADRP because
    // __ap_boot_info_array_pointer is at a physical address while
    // this code runs at kernel VMA — ADRP can't span ±4GB.
    let ptr_addr: u64;
    unsafe {
        core::arch::asm!(
            "movz {0}, #:abs_g3:__ap_boot_info_array_pointer
             movk {0}, #:abs_g2_nc:__ap_boot_info_array_pointer
             movk {0}, #:abs_g1_nc:__ap_boot_info_array_pointer
             movk {0}, #:abs_g0_nc:__ap_boot_info_array_pointer",
            out(reg) ptr_addr,
            options(pure, nomem, nostack),
        );
    }
    let ptr = ptr_addr as *mut *const PerApRawInfo;
    unsafe { *ptr = info_ptr; }
}

unsafe fn fill_boot_page_table_ptr(pt_ptr: Paddr) {
    // Use absolute addressing (movz+movk) instead of ADRP — same reason.
    let ptr_addr: u64;
    unsafe {
        core::arch::asm!(
            "movz {0}, #:abs_g3:__ap_boot_page_table_pointer
             movk {0}, #:abs_g2_nc:__ap_boot_page_table_pointer
             movk {0}, #:abs_g1_nc:__ap_boot_page_table_pointer
             movk {0}, #:abs_g0_nc:__ap_boot_page_table_pointer",
            out(reg) ptr_addr,
            options(pure, nomem, nostack),
        );
    }
    let ptr = ptr_addr as *mut Paddr;
    unsafe { *ptr = pt_ptr; }
}

fn get_ap_boot_start_addr() -> Paddr {
    // ap_boot_start is linked at a physical address (VMA == LMA, no offset).
    // Since the calling code is at kernel VMA (~0xffff80004...), ADRP can't
    // reach ±4GB. Use movz+movk to construct the absolute 64-bit address.
    let addr: u64;
    unsafe {
        core::arch::asm!(
            "movz {0}, #:abs_g3:ap_boot_start
             movk {0}, #:abs_g2_nc:ap_boot_start
             movk {0}, #:abs_g1_nc:ap_boot_start
             movk {0}, #:abs_g0_nc:ap_boot_start",
            out(reg) addr,
            options(pure, nomem, nostack),
        );
    }
    addr as Paddr
}

// ---------------------------------------------------------------------------
// AP Rust entry point
// ---------------------------------------------------------------------------

/// # Safety
///
/// - This function must be called only once on each AP by the AP boot assembly
///   code, before any other Rust code runs on this CPU.
/// - The caller must follow C calling conventions and put the right arguments
///   in registers (cpu_id in x0, context_id in x1).
// SAFETY: The name does not collide with other symbols.
#[unsafe(no_mangle)]
unsafe extern "C" fn aarch64_ap_early_entry(cpu_id: u32, _context_id: u64) -> ! {
    // SAFETY: This is the first Rust code on this AP. All AP state is
    // uninitialized; ap_early_entry handles the full bootstrap.
    unsafe { ap_early_entry(cpu_id) }
}
