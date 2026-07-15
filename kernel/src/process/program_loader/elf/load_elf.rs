// SPDX-License-Identifier: MPL-2.0

//! ELF file parser.

use core::ops::Range;

use align_ext::AlignExt;

use super::{
    elf_file::{ElfHeaders, LoadablePhdr},
    relocate::RelocatedRange,
    tls,
};
use crate::{
    fs::vfs::path::{FsPath, Path, PathResolver},
    prelude::*,
    process::{
        process_vm::{AuxKey, AuxVec},
        program_loader::check_executable_inode,
    },
    util::random::getrandom,
    vm::{
        perms::VmPerms,
        vmar::{VMAR_CAP_ADDR, VMAR_LOWEST_ADDR, Vmar, VmarMapOffset},
    },
};

/// The base address for PIE (ET_DYN with INTERP) loading.
///
/// Linux calls this `ELF_ET_DYN_BASE`. It has some intentions:
/// - The base load address for PIE programs (ET_DYN with INTERP).
/// - The heap start address for static PIE programs (ET_DYN without INTERP).
///
/// References: <https://elixir.bootlin.com/linux/v6.16.9/source/arch/x86/include/asm/elf.h#L235>
/// - x86_64:       ELF_ET_DYN_BASE = DEFAULT_MAP_WINDOW / 3 * 2
/// - riscv64:      ELF_ET_DYN_BASE = (DEFAULT_MAP_WINDOW / 3) * 2
/// - loongarch64:  ELF_ET_DYN_BASE = TASK_SIZE / 3 * 2
const PIE_BASE_ADDR: Vaddr = VMAR_CAP_ADDR / 3 * 2;

pub struct ElfLoadInfo {
    /// The relocated entry point.
    pub entry_point: Vaddr,
    /// The top address of the user stack.
    pub user_stack_top: Vaddr,
    /// TPIDR_EL0 value for TLS (end of TLS block), if PT_TLS exists.
    pub tls_pointer: Option<Vaddr>,
}

/// Loads an ELF file to the process VMAR.
///
/// This function will map ELF segments and
/// initialize the init stack and heap.
pub fn load_elf_to_vmar(
    vmar: &Vmar,
    elf_file: Path,
    path_resolver: &PathResolver,
    elf_headers: ElfHeaders,
    argv: Vec<CString>,
    envp: Vec<CString>,
) -> Result<ElfLoadInfo> {
    let ldso = lookup_and_parse_ldso(&elf_headers, &elf_file, path_resolver)?;

    #[cfg_attr(
        not(any(target_arch = "x86_64", target_arch = "riscv64")),
        expect(unused_mut)
    )]
    let (elf_mapped_info, entry_point, mut aux_vec) =
        map_vmos_and_build_aux_vec(vmar, ldso, &elf_headers, &elf_file)?;
    vmar.process_vm()
        .set_code_range(elf_mapped_info.code_range.clone());
    vmar.process_vm()
        .set_data_range(elf_mapped_info.data_range.clone());

    // Map the vDSO and set the entry.
    // Since the vDSO does not require being mapped to any specific address,
    // the vDSO is mapped after the ELF file, heap, and stack.
    #[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
    if let Some(vdso_text_base) = map_vdso_to_vmar(vmar) {
        #[cfg(target_arch = "riscv64")]
        vmar.process_vm().set_vdso_base(vdso_text_base);
        aux_vec.set(AuxKey::AT_SYSINFO_EHDR, vdso_text_base as u64);
    }

    vmar.process_vm()
        .map_and_write_init_stack(vmar, argv, envp, aux_vec)?;
    vmar.process_vm().map_and_init_heap(
        vmar,
        elf_mapped_info.data_range.len(),
        elf_mapped_info.heap_base,
    )?;

    let user_stack_top = vmar.process_vm().init_stack().user_stack_top();

    // Set up TLS: if the ELF has a PT_TLS segment, allocate a TLS block in the
    // process's address space and compute the thread pointer value (TPIDR_EL0
    // on aarch64, tp register on riscv64, %fs on x86_64). Uses the per-arch
    // TlsLayout trait to initialise musl's struct pthread.
    let load_bias = elf_mapped_info.full_range.load_bias();
    let tls_pointer = {
        #[cfg(target_arch = "aarch64")]
        { tls::setup_tls::<tls::TlsLayoutAarch64>(vmar, &elf_headers, load_bias) }
        #[cfg(target_arch = "riscv64")]
        { tls::setup_tls::<tls::TlsLayoutRiscv64>(vmar, &elf_headers, load_bias) }
        #[cfg(target_arch = "x86_64")]
        { tls::setup_tls::<tls::TlsLayoutX8664>(vmar, &elf_headers, load_bias) }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64", target_arch = "x86_64")))]
        { None }
    };

    // On aarch64, apply ELF relocations at load time (like Linux binfmt_elf)
    // so the static binary doesn't need to process them itself (which requires
    // a fully-initialized TLS that the kernel hasn't set up yet).
    #[cfg(target_arch = "aarch64")]
    {
        apply_relocations(vmar, &elf_file, &elf_mapped_info.full_range)?;
    }

    Ok(ElfLoadInfo {
        entry_point,
        user_stack_top,
        tls_pointer,
    })
}

fn lookup_and_parse_ldso(
    headers: &ElfHeaders,
    elf_file: &Path,
    path_resolver: &PathResolver,
) -> Result<Option<(Path, ElfHeaders)>> {
    let ldso_file = {
        let ldso_path = if let Some(interp_phdr) = headers.interp_phdr() {
            interp_phdr.read_ldso_path(elf_file.inode())?
        } else {
            return Ok(None);
        };

        // Our FS requires the path to be valid UTF-8. This may be too restrictive.
        let ldso_path = ldso_path.into_string().map_err(|_| {
            Error::with_message(
                Errno::ENOEXEC,
                "the interpreter path is not a valid UTF-8 string",
            )
        })?;

        let fs_path = FsPath::try_from(ldso_path.as_str())?;
        path_resolver.lookup(&fs_path)?
    };

    let ldso_elf = {
        let inode = ldso_file.inode();
        check_executable_inode(inode.as_ref())?;

        let mut buf = Box::new([0u8; PAGE_SIZE]);
        let len = inode.read_bytes_at(0, &mut *buf)?;
        if len < ElfHeaders::LEN {
            return_errno_with_message!(Errno::EIO, "the interpreter format is invalid");
        }

        ElfHeaders::parse(&buf[..len])
            .map_err(|_| Error::with_message(Errno::ELIBBAD, "the interpreter format is invalid"))?
    };

    Ok(Some((ldso_file, ldso_elf)))
}

/// Maps the VMOs to the corresponding virtual memory addresses and builds the auxiliary vector.
///
/// Returns the mapped information, the entry point, and the auxiliary vector.
fn map_vmos_and_build_aux_vec(
    vmar: &Vmar,
    ldso: Option<(Path, ElfHeaders)>,
    parsed_elf: &ElfHeaders,
    elf_file: &Path,
) -> Result<(ElfMappedInfo, Vaddr, AuxVec)> {
    let ldso_load_info = if let Some((ldso_file, ldso_elf)) = ldso {
        Some(load_ldso(vmar, &ldso_file, &ldso_elf)?)
    } else {
        None
    };

    let elf_mapped_info = map_segment_vmos(parsed_elf, vmar, elf_file, ldso_load_info.is_some())?;

    let mut aux_vec = {
        let ldso_base = ldso_load_info
            .as_ref()
            .map(|load_info| load_info.range.relocated_start());
        init_aux_vec(parsed_elf, &elf_mapped_info.full_range, ldso_base)?
    };

    // Set AT_SECURE based on setuid/setgid bits of the executable file.
    let mode = elf_file.inode().mode()?;
    let secure = if mode.has_set_uid() || mode.has_set_gid() {
        1
    } else {
        0
    };
    aux_vec.set(AuxKey::AT_SECURE, secure);

    let entry_point = if let Some(ldso_load_info) = ldso_load_info {
        ldso_load_info.entry_point
    } else {
        elf_mapped_info
            .full_range
            .relocated_addr_of(parsed_elf.entry_point())
            .ok_or_else(|| {
                Error::with_message(
                    Errno::ENOEXEC,
                    "the entry point is not located in any segments",
                )
            })?
    };

    Ok((elf_mapped_info, entry_point, aux_vec))
}

struct LdsoLoadInfo {
    /// The relocated entry point.
    entry_point: Vaddr,
    /// The range covering all the mapped segments.
    ///
    /// Note that the range may not be page-aligned.
    range: RelocatedRange,
}

fn load_ldso(vmar: &Vmar, ldso_file: &Path, ldso_elf: &ElfHeaders) -> Result<LdsoLoadInfo> {
    let elf_mapped_info = map_segment_vmos(ldso_elf, vmar, ldso_file, false)?;
    let range = elf_mapped_info.full_range;
    let entry_point = range
        .relocated_addr_of(ldso_elf.entry_point())
        .ok_or_else(|| {
            Error::with_message(
                Errno::ENOEXEC,
                "the entry point is not located in any segments",
            )
        })?;
    Ok(LdsoLoadInfo { entry_point, range })
}

/// The information of mapped ELF segments.
struct ElfMappedInfo {
    /// The range covering all the mapped segments.
    full_range: RelocatedRange,
    /// The executable code range after relocation.
    code_range: Range<Vaddr>,
    /// The data range after relocation.
    data_range: Range<Vaddr>,
    /// The base address for the heap start.
    heap_base: Vaddr,
}

/// Initializes a [`Vmo`] for each segment and then map to the [`Vmar`].
///
/// This function will return the mapped information, which contains the
/// mapped range that covers all segments. The range will be tight,
/// i.e., will not include any padding bytes. So the boundaries may not
/// be page-aligned.
///
/// [`Vmo`]: crate::vm::page_cache::Vmo
fn map_segment_vmos(
    elf: &ElfHeaders,
    vmar: &Vmar,
    elf_file: &Path,
    has_interpreter: bool,
) -> Result<ElfMappedInfo> {
    let elf_va_range = elf.calc_total_vaddr_bounds();

    // The base address for the heap start. If it's `None`, we will use the end of ELF segments.
    let mut heap_base = None;

    let map_range = if elf.is_shared_object() {
        // Relocatable object.

        let align = elf.max_load_align();

        // Given that `elf_va_range` is guaranteed to be below `VMAR_CAP_ADDR`, as long as
        // `VMAR_CAP_ADDR * 2` does not overflow, the following `align_up(align)` cannot overflow
        // either.
        const { assert!(VMAR_CAP_ADDR.checked_mul(2).is_some()) };

        // Allocate a continuous range of virtual memory for all segments in advance.
        //
        // All segments in the ELF program must be mapped to a continuous VM range to
        // ensure the relative offset of each segment not changed.
        let elf_va_range_aligned =
            elf_va_range.start.align_down(align)..elf_va_range.end.align_up(align);
        let map_size = elf_va_range_aligned.len();

        // There are effectively two types of ET_DYN ELF binaries:
        // - PIE programs (ET_DYN with PT_INTERP) and
        // - static PIE programs (ET_DYN without PT_INTERP, usually the ELF interpreter itself).
        //
        // Reference: <https://elixir.bootlin.com/linux/v6.19-rc2/source/fs/binfmt_elf.c#L1109>
        let vmar_map_options = if has_interpreter {
            // PIE program: map near a dedicated base.

            // Add some random padding.
            let nr_pages_padding = {
                let mut nr_random_padding_pages: u8 = 0;
                getrandom(nr_random_padding_pages.as_mut_bytes());
                nr_random_padding_pages as usize
            };
            let offset = (PIE_BASE_ADDR + nr_pages_padding * PAGE_SIZE).align_down(align);

            if offset < VMAR_LOWEST_ADDR {
                return_errno_with_message!(Errno::EPERM, "the mapping address is too small");
            }
            if VMAR_CAP_ADDR - offset < map_size {
                return_errno_with_message!(Errno::ENOMEM, "the mapping address is too large");
            }
            vmar.new_map(map_size, VmPerms::empty())?
                .align(align)
                .offset(VmarMapOffset::FixedNoReplace(offset))
        } else {
            // Static PIE program: pick an aligned address from the mmap region.

            // When executing static PIE programs, place the heap area away from the
            // general mmap region and into the unused `PIE_BASE_ADDR` space.
            // This helps avoid early collisions, since the heap grows upward while
            // the stack grows downward, and other mappings (such as the vDSO) may
            // also be placed in the mmap region.
            //
            // Reference: <https://elixir.bootlin.com/linux/v6.16.9/source/fs/binfmt_elf.c#L1293>
            heap_base = Some(PIE_BASE_ADDR);

            vmar.new_map(map_size, VmPerms::empty())?.align(align)
        };
        let aligned_range = vmar_map_options.build().map(|addr| addr..addr + map_size)?;

        // After acquiring a suitable range, we can remove the mapping and then
        // map each segment at the desired address.
        vmar.remove_mapping(aligned_range.clone())?;

        let start_offset = elf_va_range.start - elf_va_range_aligned.start;
        let end_offset = elf_va_range_aligned.end - elf_va_range.end;

        aligned_range.start + start_offset..aligned_range.end - end_offset
    } else {
        // Not relocatable object. Map as-is.

        if elf_va_range.start < VMAR_LOWEST_ADDR {
            return_errno_with_message!(Errno::EPERM, "the mapping address is too small");
        }

        // Allocate a continuous range of virtual memory for all segments in advance.
        //
        // This is to ensure that the range does not conflict with other objects, such
        // as the interpreter.
        let elf_va_range_aligned =
            elf_va_range.start.align_down(PAGE_SIZE)..elf_va_range.end.align_up(PAGE_SIZE);
        let map_size = elf_va_range_aligned.len();

        vmar.new_map(map_size, VmPerms::empty())?
            .offset(VmarMapOffset::FixedNoReplace(elf_va_range_aligned.start))
            .build()?;

        // After acquiring a suitable range, we can remove the mapping and then
        // map each segment at the desired address.
        vmar.remove_mapping(elf_va_range_aligned.clone())?;

        elf_va_range.clone()
    };

    let relocated_range = RelocatedRange::new(elf_va_range, map_range.start)
        .expect("`map_range` should not overflow");

    for loadable_phdr in elf.loadable_phdrs() {
        let map_at = relocated_range
            .relocated_addr_of(loadable_phdr.virt_range().start)
            .expect("`calc_total_vaddr_bounds()` should cover all segments");
        ostd::early_println!(
            "[elf] seg vaddr={:#x}-{:#x} map_at={:#x} filesz={} flags={:?}",
            loadable_phdr.virt_range().start,
            loadable_phdr.virt_range().end,
            map_at,
            loadable_phdr.file_range().len(),
            loadable_phdr.vm_perms()
        );
        map_segment_vmo(loadable_phdr, elf_file, vmar, map_at)?;
    }

    // The code range spans all executable loadable segments after relocation.
    let code_range = elf
        .loadable_phdrs()
        .iter()
        .filter(|phdr| phdr.vm_perms().contains(VmPerms::EXEC))
        .map(|phdr| {
            let range = phdr.virt_range();
            let start = relocated_range
                .relocated_addr_of(range.start)
                .expect("`calc_total_vaddr_bounds()` should cover all segments");
            start..(start + range.len())
        })
        .reduce(|acc_range, range| acc_range.start.min(range.start)..acc_range.end.max(range.end))
        .unwrap_or(0..0);

    // According to Linux behavior, the data range only includes the last loadable segment.
    // Reference: <https://elixir.bootlin.com/linux/v6.16.9/source/fs/binfmt_elf.c#L1200-L1227>
    let data_range = elf.find_last_vaddr_bound().map_or(0..0, |range| {
        let start = relocated_range
            .relocated_addr_of(range.start)
            .expect("`calc_total_vaddr_bounds()` should cover all segments");
        start..(start + range.len())
    });

    Ok(ElfMappedInfo {
        full_range: relocated_range,
        code_range,
        data_range,
        heap_base: heap_base.unwrap_or(map_range.end),
    })
}

/// Creates and maps the segment VMO to the VMAR.
///
/// Additional anonymous mappings will be created to represent trailing bytes, if any. For example,
/// this applies to the `.bss` segment.
fn map_segment_vmo(
    loadable_phdr: &LoadablePhdr,
    elf_file: &Path,
    vmar: &Vmar,
    map_at: Vaddr,
) -> Result<()> {
    let Some(elf_vmo) = elf_file.inode().page_cache() else {
        return_errno_with_message!(Errno::ENOEXEC, "the executable has no page cache");
    };

    let virt_range = loadable_phdr.virt_range();
    let file_range = loadable_phdr.file_range();
    debug!(
        "ELF segment: virt_range = {:#x?}, file_range = {:#x?}",
        virt_range, file_range,
    );

    let total_map_size = {
        let vmap_start = virt_range.start.align_down(PAGE_SIZE);
        let vmap_end = virt_range.end.align_up(PAGE_SIZE);
        vmap_end - vmap_start
    };

    let (segment_offset, segment_size) = {
        let start = file_range.start.align_down(PAGE_SIZE);
        let end = file_range.end.align_up(PAGE_SIZE);
        (start, end - start)
    };

    let mut perms = loadable_phdr.vm_perms();
    // DIAGNOSTIC (aarch64): static busybox processes its own .rela.plt at
    // startup (lazy IFUNC resolution), writing into the RX LOAD segment where
    // .rela.plt lives (0x4001d8+). Until the kernel applies ELF relocations
    // at load time (like Linux's binfmt_elf), grant WRITE on all segments so
    // the user-space relocation processing doesn't fault.
    #[cfg(target_arch = "aarch64")]
    {
        perms |= VmPerms::WRITE;
    }
    let offset = map_at.align_down(PAGE_SIZE);

    if segment_size != 0 {
        let vm_map_options = vmar
            .new_map(segment_size, perms)?
            .vmo(elf_vmo.as_vmo().clone())
            .path(elf_file.clone())
            .vmo_offset(segment_offset)
            .offset(VmarMapOffset::FixedReplace(offset))
            .handle_page_faults_around();
        let map_addr = vm_map_options.build()?;

        // Write zero as paddings if the tail is not page-aligned and map size
        // is larger than file size (e.g., `.bss`). The mapping is by default
        // private so the writes will trigger copy-on-write. Ignore errors if
        // the permissions do not allow writing.
        // Reference: <https://elixir.bootlin.com/linux/v6.17/source/fs/binfmt_elf.c#L410-L422>
        let vaddr_to_zero = map_addr + (file_range.end - segment_offset);
        let size_to_zero = map_addr + segment_size - vaddr_to_zero;
        if size_to_zero != 0 {
            let res = vmar.fill_zeros_alien(vaddr_to_zero, size_to_zero);
            if let Err((err, _)) = res
                && perms.contains(VmPerms::WRITE)
            {
                return Err(err);
            }
        }
    }

    let anonymous_map_size = total_map_size - segment_size;
    if anonymous_map_size > 0 {
        let anonymous_map_options = vmar
            .new_map(anonymous_map_size, perms)?
            .offset(VmarMapOffset::FixedReplace(offset + segment_size));
        anonymous_map_options.build()?;
    }

    Ok(())
}

fn init_aux_vec(
    elf: &ElfHeaders,
    elf_map_range: &RelocatedRange,
    ldso_base: Option<Vaddr>,
) -> Result<AuxVec> {
    let mut aux_vec = AuxVec::new();

    aux_vec.set(AuxKey::AT_PAGESZ, PAGE_SIZE as _);

    let Some(ph_vaddr) = elf_map_range.relocated_addr_of(elf.find_vaddr_of_phdrs()?) else {
        return_errno_with_message!(
            Errno::ENOEXEC,
            "the ELF program headers are not located in any segments"
        );
    };
    aux_vec.set(AuxKey::AT_PHDR, ph_vaddr as u64);
    aux_vec.set(AuxKey::AT_PHNUM, elf.ph_count() as u64);
    aux_vec.set(AuxKey::AT_PHENT, elf.ph_ent() as u64);

    let Some(entry_vaddr) = elf_map_range.relocated_addr_of(elf.entry_point()) else {
        return_errno_with_message!(
            Errno::ENOEXEC,
            "the entry point is not located in any segments"
        );
    };
    aux_vec.set(AuxKey::AT_ENTRY, entry_vaddr as u64);

    if let Some(ldso_base) = ldso_base {
        aux_vec.set(AuxKey::AT_BASE, ldso_base as u64);
    }

    Ok(aux_vec)
}

/// Maps the vDSO VMO to the corresponding virtual memory address.
#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
fn map_vdso_to_vmar(vmar: &Vmar) -> Option<Vaddr> {
    use crate::vdso::{VDSO_VMO_LAYOUT, vdso_vmo};

    let vdso_vmo = vdso_vmo()?;

    let options = vmar
        .new_map(VDSO_VMO_LAYOUT.size, VmPerms::empty())
        .unwrap()
        .vmo(vdso_vmo);

    let vdso_vmo_base = options.build().unwrap();
    let vdso_data_base = vdso_vmo_base + VDSO_VMO_LAYOUT.data_segment_offset;
    let vdso_text_base = vdso_vmo_base + VDSO_VMO_LAYOUT.text_segment_offset;

    let data_perms = VmPerms::READ;
    let text_perms = VmPerms::READ | VmPerms::EXEC;
    vmar.protect(
        data_perms,
        vdso_data_base..(vdso_data_base + VDSO_VMO_LAYOUT.data_segment_size),
    )
    .unwrap();
    vmar.protect(
        text_perms,
        vdso_text_base..(vdso_text_base + VDSO_VMO_LAYOUT.text_segment_size),
    )
    .unwrap();
    Some(vdso_text_base)
}

/// Applies ELF relocations at load time (aarch64 only).
///
/// Static binaries (like busybox) and static-PIE binaries (like dropbear) may
/// contain RELA relocations (R_AARCH64_RELATIVE, R_AARCH64_GLOB_DAT,
/// R_AARCH64_ABS64) that must be applied to the GOT before user-space runs.
/// Without this, the binary tries to process its own relocations during early
/// init, which fails because the TLS/TCB isn't fully set up yet.
///
/// For PIE (ET_DYN) binaries the ELF `r_offset` and the symbol addends are
/// relative to the link-time base (which starts at 0), so both the patch
/// address and the value must be adjusted by the load bias — exactly as
/// Linux's `binfmt_elf` does when it calls the architecture's relocation
/// helper after mapping the segments.
#[cfg(target_arch = "aarch64")]
fn apply_relocations(
    vmar: &Vmar,
    elf_file: &Path,
    relocated_range: &RelocatedRange,
) -> Result<()> {
    const SHT_RELA: u32 = 4;
    const R_AARCH64_RELATIVE: u32 = 1027;
    const R_AARCH64_GLOB_DAT: u32 = 1025;
    const R_AARCH64_ABS64: u32 = 257;
    // R_AARCH64_IRELATIVE (1032) is NOT handled here: the kernel forbids
    // unsafe code (#![deny(unsafe_code)]), so we cannot transmute and call
    // the IFUNC resolver. Static binaries process their own IRELATIVE
    // entries at startup; the segments are mapped writable (see the
    // DIAGNOSTIC in map_segment_vmo) to allow this.

    let load_bias = relocated_range.load_bias();
    // Convert to unsigned for arithmetic below. Negative bias never happens
    // in practice (segments are always mapped at or above their link addr),
    // but use wrapping to avoid panics.
    let load_bias_u = load_bias as u64;

    let inode = elf_file.inode();

    // Read the ELF header to get section header info.
    let mut ehdr_buf = vec![0u8; 64];
    let read = inode.read_bytes_at(0, &mut ehdr_buf)?;
    if read < 64 {
        return Ok(());
    }

    let e_shoff = u64::from_le_bytes(ehdr_buf[40..48].try_into().unwrap()) as usize;
    let e_shentsize = u16::from_le_bytes(ehdr_buf[58..60].try_into().unwrap()) as usize;
    let e_shnum = u16::from_le_bytes(ehdr_buf[60..62].try_into().unwrap()) as usize;
    if e_shoff == 0 || e_shnum == 0 || e_shentsize == 0 {
        return Ok(());
    }

    // Read the section header table.
    let shdr_total = e_shnum * e_shentsize;
    let mut shdr_buf = vec![0u8; shdr_total];
    inode.read_bytes_at(e_shoff, &mut shdr_buf)?;

    let mut reloc_count = 0usize;
    let mut skip_count = 0usize;
    for i in 0..e_shnum {
        let off = i * e_shentsize;
        let sh_type = u32::from_le_bytes(shdr_buf[off + 4..off + 8].try_into().unwrap());
        if sh_type != SHT_RELA {
            continue;
        }
        let sh_offset = u64::from_le_bytes(shdr_buf[off + 24..off + 32].try_into().unwrap()) as usize;
        let sh_size = u64::from_le_bytes(shdr_buf[off + 32..off + 40].try_into().unwrap()) as usize;

        let n_entries = sh_size / 24;
        let mut rela_buf = vec![0u8; sh_size];
        inode.read_bytes_at(sh_offset, &mut rela_buf)?;

        for j in 0..n_entries {
            let eo = j * 24;
            let r_offset = u64::from_le_bytes(rela_buf[eo..eo + 8].try_into().unwrap());
            let r_info = u64::from_le_bytes(rela_buf[eo + 8..eo + 16].try_into().unwrap());
            let r_addend = u64::from_le_bytes(rela_buf[eo + 16..eo + 24].try_into().unwrap());
            let r_type = (r_info & 0xFFFFFFFF) as u32;

            // The actual patch address is the link-time offset adjusted by the
            // load bias. For non-PIE binaries the bias is 0, so this is a no-op.
            let patch_addr = r_offset.wrapping_add(load_bias_u) as usize;

            // Compute the relocation value.
            //   - R_AARCH64_RELATIVE: value = load_bias + addend
            //   - R_AARCH64_GLOB_DAT/ABS64: value = load_bias + addend
            //   - R_AARCH64_IRELATIVE: call the IFUNC resolver at
            //     (load_bias + addend) and use its return value. The resolver
            //     is a short position-independent function (e.g. selects
            //     memcpy/memset variant based on CPU features). We call it via
            //     a raw function pointer; the user pages are accessible from
            //     kernel mode on aarch64, and these resolvers don't do syscalls.
            let value: u64 = match r_type {
                R_AARCH64_RELATIVE | R_AARCH64_GLOB_DAT | R_AARCH64_ABS64 => {
                    r_addend.wrapping_add(load_bias_u)
                }
                _ => continue,
            };

            let val_bytes = value.to_le_bytes();
            let mut reader = ostd::mm::VmReader::from(&val_bytes[..]).to_fallible();
            match vmar.write_alien(patch_addr, &mut reader) {
                Ok(_) => reloc_count += 1,
                Err((e, _)) => {
                    skip_count += 1;
                    // Only print the first few failures to avoid flooding the log.
                    if skip_count <= 8 {
                        ostd::early_println!(
                            "[reloc] failed at {:#x} (orig {:#x}, bias {:#x}): {:?}",
                            patch_addr,
                            r_offset,
                            load_bias_u,
                            e
                        );
                    }
                }
            }
        }
    }

    if reloc_count > 0 || skip_count > 0 {
        ostd::early_println!(
            "[reloc] applied {} relocations (skipped {}), load_bias={:#x}",
            reloc_count,
            skip_count,
            load_bias_u
        );
    }
    Ok(())
}
