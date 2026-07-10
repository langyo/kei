// SPDX-License-Identifier: MPL-2.0

#![doc = include_str!("../README.md")]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(btree_cursors)]
#![feature(core_intrinsics)]
#![feature(linkage)]
#![feature(min_specialization)]
#![feature(negative_impls)]
#![feature(ptr_metadata)]
#![feature(sync_unsafe_cell)]
#![cfg_attr(target_arch = "x86_64", feature(iter_advance_by, macro_metavar_expr))]
#![expect(internal_features)]
#![no_std]
#![warn(missing_docs)]

extern crate alloc;
#[macro_use]
extern crate ostd_pod;

// Set this crate's log prefix for `ostd::log`.
macro_rules! __log_prefix {
    () => {
        ""
    };
}

#[cfg_attr(target_arch = "x86_64", path = "arch/x86/mod.rs")]
#[cfg_attr(target_arch = "aarch64", path = "arch/aarch64/mod.rs")]
#[cfg_attr(target_arch = "riscv64", path = "arch/riscv/mod.rs")]
#[cfg_attr(target_arch = "loongarch64", path = "arch/loongarch/mod.rs")]
pub mod arch;

pub mod boot;
pub mod bus;
pub mod console;
pub mod cpu;
mod error;
mod ex_table;
pub mod io;
pub mod irq;
pub mod log;
pub mod mm;
pub mod panic;
pub mod power;
pub mod prelude;
pub mod smp;
pub mod sync;
pub mod task;
pub mod timer;
pub mod user;
pub mod util;

#[cfg(feature = "coverage")]
mod coverage;

use core::sync::atomic::{AtomicBool, Ordering};

pub use ostd_macros::{
    global_frame_allocator, global_heap_allocator, global_heap_allocator_slot_map, main,
    panic_handler,
};

pub use self::{error::Error, prelude::Result};

/// Initializes OSTD.
///
/// This function represents the first phase booting up the system. It makes
/// all functionalities of OSTD available after the call.
///
/// # Safety
///
/// This function should be called only once and only on the BSP.
//
// TODO: We need to refactor this function to make it more modular and
// make inter-initialization-dependencies more clear and reduce usages of
// boot stage only global variables.
unsafe fn init() {
    crate::early_println!("[ostd] init: enable_cpu_features");
    arch::enable_cpu_features();

    crate::early_println!("[ostd] init: init_early_allocator");
    unsafe { mm::frame::allocator::init_early_allocator() };

    #[cfg(target_arch = "x86_64")]
    arch::if_tdx_enabled!({
    } else {
        arch::serial::init();
    });
    #[cfg(not(target_arch = "x86_64"))]
    arch::serial::init();

    crate::early_println!("[ostd] init: log::init");
    log::init();

    crate::early_println!("[ostd] init: cpu::init_on_bsp");
    unsafe { cpu::init_on_bsp() };

    crate::early_println!("[ostd] init: frame::meta::init");
    let meta_pages = unsafe { mm::frame::meta::init() };

    crate::early_println!("[ostd] init: frame::allocator::init");
    unsafe { mm::frame::allocator::init() };

    crate::early_println!("[ostd] init: kspace::init_kernel_page_table");
    mm::kspace::init_kernel_page_table(meta_pages);

    // Activate the cursor-built kernel page table on all architectures.
    // The kernel is linked at the linear-mapping VMA (aarch64.ld KERNEL_VMA),
    // so all symbol references are upper-half addresses present in
    // KERNEL_PAGE_TABLE — activation is a plain TTBR write + TLB flush with
    // no PC-migration trampoline. VBAR_EL1 is set to trap_vectors (linear VA)
    // in bsp_boot.S before Rust entry, so any fault during activation is
    // handled by our trap handler.
    crate::early_println!("[ostd] init: kspace::activate_kernel_page_table");
    unsafe { mm::kspace::activate_kernel_page_table() };

    // Serial port is reinitialized inside activate_kernel_page_table
    // for aarch64 (the identity mapping is gone after the switch).
    crate::early_println!("[ostd] init: sync::init");
    sync::init();

    crate::early_println!("[ostd] init: boot::init_after_heap");
    boot::init_after_heap();

    crate::early_println!("[ostd] init: arch::late_init_on_bsp");
    unsafe { arch::late_init_on_bsp() };

    #[cfg(target_arch = "x86_64")]
    arch::if_tdx_enabled!({
        arch::serial::init();
    });

    crate::early_println!("[ostd] init: smp::init");
    smp::init();

    crate::early_println!("[ostd] init: boot_pt::dismiss");
    // On aarch64 we skipped page table activation, so dismiss is a no-op
    // (the boot page table is still active and needed).
    #[cfg(not(target_arch = "aarch64"))]
    unsafe { mm::page_table::boot_pt::dismiss() };
    #[cfg(target_arch = "aarch64")]
    crate::early_println!("[ostd] init: boot_pt::dismiss (SKIPPED on aarch64)");

    crate::early_println!("[ostd] init: irq::enable_local");
    arch::irq::enable_local();

    crate::early_println!("[ostd] init: invoke_ffi_init_funcs");
    invoke_ffi_init_funcs();

    IN_BOOTSTRAP_CONTEXT.store(false, Ordering::Relaxed);
    crate::early_println!("[ostd] init: DONE");
}

/// Indicates whether the kernel is in bootstrap context.
pub(crate) static IN_BOOTSTRAP_CONTEXT: AtomicBool = AtomicBool::new(true);

/// Invoke the initialization functions defined in the FFI.
/// The component system uses this function to call the initialization functions of
/// the components.
fn invoke_ffi_init_funcs() {
    unsafe extern "C" {
        fn __sinit_array();
        fn __einit_array();
    }
    let call_len = (__einit_array as *const () as usize - __sinit_array as *const () as usize) / 8;
    crate::early_println!("[ostd] ffi_init: {} functions at [{:#x}..{:#x}]",
        call_len, __sinit_array as usize, __einit_array as usize);
    for i in 0..call_len {
        unsafe {
            let function = (__sinit_array as *const () as usize + 8 * i) as *const fn();
            crate::early_println!("[ostd] ffi_init: calling function {} at {:#x}", i, function as usize);
            (*function)();
        }
    }
    crate::early_println!("[ostd] ffi_init: done ({} called)", call_len);
}

mod feature_validation {
    #[cfg(all(not(target_arch = "riscv64"), feature = "riscv_sv39_mode"))]
    compile_error!(
        "feature \"riscv_sv39_mode\" cannot be specified for architectures other than RISC-V"
    );
}

/// Simple unit tests for the ktest framework.
#[cfg(ktest)]
mod test {
    use crate::prelude::*;

    #[expect(clippy::eq_op)]
    #[ktest]
    fn trivial_assertion() {
        assert_eq!(0, 0);
    }

    #[ktest]
    #[should_panic]
    fn failing_assertion() {
        assert_eq!(0, 1);
    }

    #[ktest]
    #[should_panic(expected = "expected panic message")]
    fn expect_panic() {
        panic!("expected panic message");
    }
}

#[doc(hidden)]
pub mod ktest {
    //! The module re-exports everything from the [`ostd_test`] crate, as well
    //! as the test entry point macro.
    //!
    //! It is rather discouraged to use the definitions here directly. The
    //! `ktest` attribute is sufficient for all normal use cases.

    pub use ostd_macros::{test_main as main, test_panic_handler as panic_handler};
    pub use ostd_test::*;
}
