// SPDX-License-Identifier: MPL-2.0

//! Platform-abstracted TLS (Thread-Local Storage) initialization for ELF binaries.
//!
//! Provides the [`TlsLayout`] trait that encapsulates the C runtime's thread
//! control block (TCB / struct pthread) layout, and a generic [`setup_tls`]
//! function that works across architectures. Each target architecture
//! implements the trait for its native musl TLS layout.

use core::marker::PhantomData;

use align_ext::AlignExt;

use super::{elf_file::ElfHeaders, relocate::RelocatedRange};
use crate::{prelude::*, vm::perms::VmPerms, vm::vmar::Vmar};

/// Describes the TLS layout used by the statically-linked C runtime (musl).
///
/// For musl, all three architectures (aarch64, riscv64, x86_64) use the
/// **TLS_ABOVE_TP** convention: the thread pointer sits *past* the end of the
/// thread control block, with TLS data occupying lower addresses.
///
/// ```text
/// [TLS data (.tdata + .tbss)] [gap] [struct pthread (TCB)] [dtv] ← TP
/// ```
///
/// The trait encapsulates the arch-specific constants (pthread size, gap) and
/// the TCB field initialization that musl expects for `__pthread_self()`.
pub trait TlsLayout: Sized {
    /// Size of `struct pthread` (the thread control block) in bytes.
    fn pthread_size() -> usize;

    /// Gap between the TLS data region and the TCB (`GAP_ABOVE_TP` in musl).
    fn gap_above_tp() -> usize;

    /// Number of DTV (Dynamic Thread Vector) entries.
    fn dtv_entry_count() -> usize {
        2
    }

    /// Returns the total page-aligned allocation size for a given TLS payload.
    fn total_alloc(tls_data_size: usize, tls_align: usize) -> usize {
        let tls_data_aligned = (tls_data_size + tls_align.max(16) - 1) & !(tls_align.max(16) - 1);
        let dtv_bytes = Self::dtv_entry_count() * core::mem::size_of::<u64>();
        let needed = tls_data_aligned + Self::gap_above_tp() + Self::pthread_size() + dtv_bytes;
        needed.align_up(ostd::mm::PAGE_SIZE)
    }

    /// Computes the thread-pointer value from the layout bases.
    ///
    /// For TLS_ABOVE_TP: `tp = td_addr + pthread_size` (tp points past TCB).
    fn compute_tp(td_addr: Vaddr) -> Vaddr {
        td_addr + Self::pthread_size()
    }

    /// Initialises the TCB fields that musl reads during early startup:
    /// `self`, `dtv` pointer, `locale` pointer.
    ///
    /// The allocated pages are already zeroed; only write the non-zero fields.
    fn init_tcb(vmar: &Vmar, tls_base: Vaddr, td_addr: Vaddr, dtv_addr: Vaddr) -> Result<Vaddr> {
        // self (offset 0x00): pointer to itself.
        write_u64_at(vmar, td_addr, td_addr as u64);

        // dtv (offset 0xc0 for aarch64; we use a tail convention):
        // musl stores the DTV pointer at the end of struct pthread.
        write_u64_at(vmar, td_addr + Self::pthread_size() - 8, dtv_addr as u64);

        // locale (offset varies; we pick an unused area inside pthread and
        // point it there so that musl's locale functions don't deref NULL).
        // A zeroed __locale_struct means C locale — musl treats NULL locale
        // as a fallback, so a zeroed self-referencing region is safe.
        let locale_addr = td_addr + 0x40;
        write_u64_at(vmar, td_addr + 0x98, locale_addr as u64);

        // dtv array: dtv[0] = generation (1), dtv[1] = tls_data pointer.
        write_u64_at(vmar, dtv_addr, 1);
        write_u64_at(vmar, dtv_addr + 8, tls_base as u64);

        Ok(Self::compute_tp(td_addr))
    }
}

/// Write a u64 value to the given virtual address in the target VMAR.
fn write_u64_at(vmar: &Vmar, addr: Vaddr, value: u64) {
    let bytes = value.to_le_bytes();
    let mut reader = ostd::mm::VmReader::from(&bytes[..]).to_fallible();
    let _ = vmar.write_alien(addr, &mut reader);
}

// ---------------------------------------------------------------------------
// Architecture-specific TlsLayout implementations
// ---------------------------------------------------------------------------

/// AArch64 musl TLS_ABOVE_TP layout.
///
/// musl on aarch64 stores the struct pthread at a fixed offset (0xc8 bytes)
/// *above* the TLS data region, with a 16-byte gap.
///
/// Relevant musl source: `arch/aarch64/pthread_arch.h`
pub struct TlsLayoutAarch64;

impl TlsLayout for TlsLayoutAarch64 {
    fn pthread_size() -> usize {
        0xc8
    }

    fn gap_above_tp() -> usize {
        16
    }
}

/// RISC-V 64 musl TLS_ABOVE_TP layout.
///
/// musl on riscv64 has a slightly smaller struct pthread (0xc0 bytes) and
/// the same 16-byte gap as aarch64. TP is stored in the `tp` register.
///
/// Relevant musl source: `arch/riscv64/pthread_arch.h`
pub struct TlsLayoutRiscv64;

impl TlsLayout for TlsLayoutRiscv64 {
    fn pthread_size() -> usize {
        0xc0
    }

    fn gap_above_tp() -> usize {
        16
    }
}

/// x86-64 musl TLS_ABOVE_TP layout.
///
/// musl on x86_64 uses the `%fs` segment register for the thread pointer
/// (set via `WRFSBASE` or `arch_prctl(ARCH_SET_FS)`). The struct pthread
/// size is 0xc8 bytes with a 16-byte gap — identical to aarch64 in layout,
/// but the TP register is different.
///
/// For static binaries, the kernel must initialise the TCB so that musl's
/// `__init_tls` doesn't dereference NULL. The `%fs` register is set later
/// by `set_tls_pointer()` in the UserContext.
///
/// Relevant musl source: `arch/x86_64/pthread_arch.h`
pub struct TlsLayoutX8664;

impl TlsLayout for TlsLayoutX8664 {
    fn pthread_size() -> usize {
        0xc8
    }

    fn gap_above_tp() -> usize {
        16
    }
}

// ---------------------------------------------------------------------------
// Generic TLS setup
// ---------------------------------------------------------------------------

/// Allocates a TLS block and returns the thread pointer value.
///
/// This is the architecture-independent entry point. It:
///
/// 1. Reads PT_TLS from the ELF headers to determine the template size.
/// 2. Allocates a page-aligned block large enough for TLS data + TCB.
/// 3. Initialises the TCB fields (self, dtv, locale).
/// 4. Copies the initialised TLS data (`.tdata`) from the mapped LOAD segment.
/// 5. Returns the thread-pointer value to be stored in the arch register.
///
/// The `load_bias` parameter is the offset between the ELF's link-time
/// virtual addresses and the actual mapping addresses. For non-PIE binaries
/// it is 0; for PIE binaries it is `map_range.start - elf_va_range.start`.
pub fn setup_tls<L: TlsLayout>(
    vmar: &Vmar,
    elf_headers: &ElfHeaders,
    load_bias: i64,
) -> Option<Vaddr> {
    let (tls_memsz, tls_filesz, tls_align, tls_phdr_vaddr) = match elf_headers.tls_phdr() {
        Some(phdr) if phdr.memsz > 0 => (phdr.memsz, phdr.filesz, phdr.align.max(16), phdr.vaddr),
        _ => (0, 0, 16, 0),
    };

    let total = if tls_memsz > 0 {
        L::total_alloc(tls_memsz, tls_align)
    } else {
        // No PT_TLS: still allocate a minimal block for the TCB so that
        // musl's __pthread_self() doesn't dereference NULL.
        let min_tcb = L::pthread_size() + L::gap_above_tp() + L::dtv_entry_count() * 8;
        min_tcb.align_up(ostd::mm::PAGE_SIZE)
    };

    let tls_base = vmar
        .new_map(total, VmPerms::READ | VmPerms::WRITE)
        .ok()?
        .handle_page_faults_around()
        .build()
        .ok()?;

    let tls_data_aligned = if tls_memsz > 0 {
        (tls_memsz + tls_align - 1) & !(tls_align - 1)
    } else {
        0
    };
    let td_addr = tls_base + tls_data_aligned + L::gap_above_tp();
    let dtv_addr = td_addr + L::pthread_size();

    let tp = L::init_tcb(vmar, tls_base, td_addr, dtv_addr).ok()?;

    // Copy .tdata from the LOAD segment to the TLS data area.
    if tls_filesz > 0 && load_bias >= 0 {
        // Compute the relocated virtual address of the TLS template.
        // Without load bias, we'd read from the link-time vaddr, which is
        // only correct for non-PIE binaries (where load_bias == 0).
        let template_va = (tls_phdr_vaddr as i64 + load_bias) as Vaddr;

        // Allocate a temporary buffer, read the template, and write it into the TLS block.
        let mut buf = vec![0u8; tls_filesz];
        let copy_ok = vmar
            .read_alien(
                template_va,
                &mut ostd::mm::VmWriter::from(buf.as_mut_slice()).to_fallible(),
            )
            .is_ok();

        if copy_ok {
            let mut reader = ostd::mm::VmReader::from(buf.as_slice()).to_fallible();
            let _ = vmar.write_alien(tls_base, &mut reader);
        }

        ostd::early_println!(
            "[tls] .tdata: phdr_va={:#x} template_va={:#x} filesz={} bias={:#x} copied={}",
            tls_phdr_vaddr,
            template_va,
            tls_filesz,
            load_bias,
            copy_ok
        );
    } else if tls_filesz > 0 {
        // Negative load_bias: segments mapped below link-time addresses.
        // Fall back to link-time vaddr (works for non-PIE).
        let mut buf = vec![0u8; tls_filesz];
        let copy_ok = vmar
            .read_alien(
                tls_phdr_vaddr,
                &mut ostd::mm::VmWriter::from(buf.as_mut_slice()).to_fallible(),
            )
            .is_ok();

        if copy_ok {
            let mut reader = ostd::mm::VmReader::from(buf.as_slice()).to_fallible();
            let _ = vmar.write_alien(tls_base, &mut reader);
        }

        ostd::early_println!(
            "[tls] .tdata (no bias): phdr_va={:#x} filesz={} copied={}",
            tls_phdr_vaddr,
            tls_filesz,
            copy_ok
        );
    }

    ostd::early_println!(
        "[tls] musl: base={:#x} td={:#x} tp={:#x} dtv={:#x} psz={:#x}",
        tls_base,
        td_addr,
        tp,
        dtv_addr,
        L::pthread_size()
    );

    Some(tp)
}
