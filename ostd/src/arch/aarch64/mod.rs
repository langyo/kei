// SPDX-License-Identifier: MPL-2.0

//! Platform-specific code for the ARM64 (AArch64) platform.

#![expect(dead_code)]

pub mod boot;
pub mod cpu;
pub mod device;
pub(crate) mod io;
pub(crate) mod iommu;
pub mod irq;
pub mod mm;
mod power;
pub mod serial;
pub(crate) mod task;
pub(crate) mod timer;
pub mod trap;

/// Architecture-specific initialization on the bootstrapping processor.
///
/// It should be called when the heap and frame allocators are available.
///
/// # Safety
///
/// 1. This function must be called only once in the boot context of the
///    bootstrapping processor.
/// 2. This function must be called after the kernel page table is activated on
///    the bootstrapping processor.
pub(crate) unsafe fn late_init_on_bsp() {
    crate::early_println!("[arch] trap::init_on_cpu");
    unsafe { trap::init_on_cpu() };

    crate::early_println!("[arch] io_mem_builder");
    let io_mem_builder = unsafe { io::construct_io_mem_allocator_builder() };

    crate::early_println!("[arch] irq::chip::init_on_bsp (GIC)");
    unsafe { irq::chip::init_on_bsp(&io_mem_builder) };

    crate::early_println!("[arch] irq::ipi::init_on_bsp");
    unsafe { irq::ipi::init_on_bsp() };

    crate::early_println!("[arch] timer::init_on_bsp");
    unsafe { timer::init_on_bsp() };

    crate::early_println!("[arch] smp::boot_all_aps");
    unsafe { crate::boot::smp::boot_all_aps() };

    crate::early_println!("[arch] io::init");
    unsafe { crate::io::init(io_mem_builder) };

    crate::early_println!("[arch] power::init");
    power::init();
    crate::early_println!("[arch] late_init_on_bsp done");
}

/// Initializes application-processor-specific state.
///
/// # Safety
///
/// 1. This function must be called only once on each application processor.
/// 2. This function must be called after the BSP's call to [`late_init_on_bsp`]
///    and before any other architecture-specific code in this module is called
///    on this AP.
pub(crate) unsafe fn init_on_ap() {
    // SAFETY: The safety is upheld by the caller.
    unsafe { trap::init_on_cpu() };

    // SAFETY: The safety is upheld by the caller.
    unsafe { irq::chip::init_on_ap() };

    // SAFETY: This is called before any harts can send IPIs to this AP.
    unsafe { irq::ipi::init_on_ap() };

    // SAFETY: The caller ensures that this is only called once on this AP.
    unsafe { timer::init_on_ap() };
}

/// Returns the frequency of the architected timer. The unit is Hz.
pub fn tsc_freq() -> u64 {
    timer::get_timer_freq()
}

/// Reads the current value of the architected timer counter.
pub fn read_tsc() -> u64 {
    timer::read_counter()
}

/// Reads the current TPIDR_EL0 (thread pointer / TLS pointer).
///
/// On aarch64, TPIDR_EL0 holds the user-space thread pointer. The kernel
/// needs to read it when forking (to inherit the child's TLS pointer),
/// since the trap frame does not save it automatically.
pub fn read_tpidr_el0() -> usize {
    let val: usize;
    unsafe { core::arch::asm!("mrs {0}, tpidr_el0", out(reg) val, options(nomem, nostack)) };
    val
}

/// Reads a hardware generated 64-bit random value.
///
/// Returns `None` if no random value was generated.
pub fn read_random() -> Option<u64> {
    // FIXME: Implement a hardware random number generator on ARM64 platforms.
    None
}

pub(crate) fn enable_cpu_features() {
    cpu::extension::init();
}
