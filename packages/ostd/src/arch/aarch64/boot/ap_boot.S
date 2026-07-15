/* SPDX-License-Identifier: MPL-2.0 */

// The boot routine executed by application processors (APs) on AArch64.
//
// Each AP is started via PSCI CPU_ON and enters at `ap_boot_start` in
// physical address space. We re-use the kernel page table already set up
// by the BSP (pointed to by `__ap_boot_page_table_pointer`).

PAGE_SHIFT              = 12
TCR_T0SZ                = (16 << 0)     // 16 zero bits → VA space size 2^(64-16)
TCR_T1SZ                = (16 << 16)
TCR_IRGN0_WBWA          = (1 << 8)      // Inner cacheable WBWA
TCR_ORGN0_WBWA          = (1 << 10)     // Outer cacheable WBWA
TCR_SH0_INNER           = (3 << 12)     // Inner shareable
TCR_IRGN1_WBWA          = (1 << 24)
TCR_ORGN1_WBWA          = (1 << 26)
TCR_SH1_INNER           = (3 << 28)
TCR_TG0_4K              = (0 << 14)     // TTBR0 granule = 4 KiB
TCR_TG1_4K              = (2 << 30)     // TTBR1 granule = 4 KiB
TCR_EPD0                = (1 << 7)      // Disable TTBR0 walks (kernel only)
TCR_A1                  = (0 << 22)     // Use TTBR0_EL1 for ASID
TCR_IPS_48BIT           = (5 << 32)     // IPA size = 48 bits

TCR_VALUE               = TCR_T0SZ | TCR_T1SZ | TCR_IRGN0_WBWA | TCR_ORGN0_WBWA | TCR_SH0_INNER | TCR_IRGN1_WBWA | TCR_ORGN1_WBWA | TCR_SH1_INNER | TCR_TG0_4K | TCR_TG1_4K | TCR_EPD0 | TCR_A1 | TCR_IPS_48BIT

MAIR_VALUE              = (0x44 << 0) | (0xff << 8)  // Normal WriteBack / Normal

SCTLR_EL1_MMU_ENABLED   = (1 << 0)      // M bit: enable MMU
SCTLR_EL1_I_CACHE       = (1 << 12)     // I bit: instruction cache
SCTLR_EL1_SPAN          = (1 << 23)     // SPAN (PAN if 0)
SCTLR_RESERVED          = (3 << 28) | (3 << 22) | (1 << 20) | (1 << 11)

.section ".ap_boot", "awx", @progbits

.balign 4096
.global ap_boot_start
ap_boot_start:
    // At this point:
    //   - We're running at the physical address given to PSCI CPU_ON.
    //   - The kernel page table and PerApRawInfo have been set up by the BSP.
    //   - TTBR1_EL1 and TTBR0_EL1 need to be loaded from the boot pointers.

    // Set up MAIR_EL1 for memory attribute indirection.
    ldr     x2, =MAIR_VALUE
    msr     mair_el1, x2

    // Set up TCR_EL1 for translation control.
    ldr     x2, =TCR_VALUE
    msr     tcr_el1, x2
    isb

    // Load the page table physical address and set TTBR1_EL1.
    // The AP boot page table covers the full kernel VMA space.
    adrp    x0, __ap_boot_page_table_pointer
    add     x0, x0, #:lo12:__ap_boot_page_table_pointer
    ldr     x1, [x0]                   // Load page table physical address.
    msr     ttbr1_el1, x1
    msr     ttbr0_el1, xzr             // Zero TTBR0 (no user mappings).
    isb

    // Invalidate TLBs before enabling MMU.
    tlbi    vmalle1
    dsb     nsh
    isb

    // Enable MMU, I-cache, and SPAN.
    mrs     x2, sctlr_el1
    orr     x2, x2, #SCTLR_EL1_MMU_ENABLED
    orr     x2, x2, #SCTLR_EL1_I_CACHE
    orr     x2, x2, #SCTLR_EL1_SPAN
    msr     sctlr_el1, x2
    isb

    // Jump to virtual address space.
    ldr     x1, =ap_boot_virt
    br      x1

.text

ap_boot_virt:
    // Now running with virtual addresses.

    // Set VBAR_EL1 for this AP. The BSP's VBAR address is already a kernel
    // virtual address and present in the kernel page table.
    ldr     x5, =trap_vectors
    msr     vbar_el1, x5
    isb

    // Atomically increment the CPU ID counter and get our ID.
    // CPU ID 0 is the BSP; APs get 1, 2, 3, ...
    adrp    x0, __ap_boot_cpu_id_tail
    add     x0, x0, #:lo12:__ap_boot_cpu_id_tail
    mov     x3, x0                      // Save address for retry loop.
ap_cmpxchg_load:
    ldxr    x1, [x0]                    // Load current tail.
    add     x2, x1, #1                  // Increment.
    stxr    w4, x2, [x0]                // Try to store.
    cbnz    w4, ap_cmpxchg_load          // Retry if store failed.

    // x1 = our CPU ID (1, 2, 3, ...).

    // Get the AP boot info array pointer.
    adrp    x2, __ap_boot_info_array_pointer
    add     x2, x2, #:lo12:__ap_boot_info_array_pointer
    ldr     x2, [x2]                    // Load pointer to PerApRawInfo array.

    // Each PerApRawInfo is 16 bytes (2 × 8-byte pointers).
    // CPU ID 1 → offset 0, CPU ID 2 → offset 16, etc.
    sub     x3, x1, #1                  // cpu_id - 1 (0-based index).
    lsl     x3, x3, #4                  // Multiply by 16.
    add     x3, x3, x2                  // x3 = &per_ap_raw_info[cpu_id - 1].

    // Load stack top and CPU local pointer.
    ldp     x4, x5, [x3]                // x4 = stack_top, x5 = cpu_local.
    mov     sp, x4                      // Set stack pointer.
    msr     tpidr_el1, x5               // Set CPU-local storage base.
    isb

    // Clear frame pointer for clean stack traces.
    mov     x29, xzr

    // Jump to Rust AP entry point.
    // x0 = cpu_id (first argument to aarch64_ap_early_entry).
    mov     x0, x1
    mov     x1, xzr                     // context_id = 0 (not used).
    ldr     x2, =aarch64_ap_early_entry
    br      x2

ap_unreachable:
    wfe
    b       ap_unreachable

// ---------------------------------------------------------------------------
// Data: filled by the BSP before starting APs.
// ---------------------------------------------------------------------------

.section ".ap_boot", "awx", @progbits

.balign 8
.global __ap_boot_page_table_pointer
__ap_boot_page_table_pointer:
    .quad 0

.global __ap_boot_info_array_pointer
__ap_boot_info_array_pointer:
    .quad 0

// Atomic counter: CPU ID 0 is the BSP. The first AP gets 1, second gets 2, etc.
__ap_boot_cpu_id_tail:
    .quad 1
