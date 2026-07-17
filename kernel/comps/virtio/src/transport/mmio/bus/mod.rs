// SPDX-License-Identifier: MPL-2.0

//! Virtio over MMIO

#![allow(unsafe_code)]

use core::ops::Range;

use bus::MmioBus;
use ostd::{debug, io::IoMem, irq::IrqLine, sync::SpinLock};

use crate::transport::mmio::bus::common_device::{
    MmioCommonDevice, mmio_check_magic, mmio_read_device_id,
};

#[cfg_attr(target_arch = "x86_64", path = "arch/x86.rs")]
#[cfg_attr(target_arch = "aarch64", path = "arch/aarch64.rs")]
#[cfg_attr(target_arch = "riscv64", path = "arch/riscv.rs")]
#[cfg_attr(target_arch = "loongarch64", path = "arch/loongarch.rs")]
mod arch;

#[expect(clippy::module_inception)]
pub(super) mod bus;
pub(super) mod common_device;

/// The MMIO bus instance.
pub(super) static MMIO_BUS: SpinLock<MmioBus> = SpinLock::new(MmioBus::new());

pub(super) fn init() {
    #[cfg(target_arch = "x86_64")]
    ostd::if_tdx_enabled!({
        // TODO: support virtio-mmio devices on TDX.
        //
        // Currently, virtio-mmio devices need to acquire sub-page MMIO regions,
        // which are not supported by `IoMem::acquire` in the TDX environment.
    } else {
        arch::probe_for_device();
    });
    #[cfg(not(target_arch = "x86_64"))]
    arch::probe_for_device();
}

/// Tries to validate a potential VirtIO-MMIO device, map it to an IRQ line, and
/// register it as a VirtIO-MMIO device.
///
/// Returns `Ok(())` if the device was registered, or a specific
/// `MmioRegisterError` otherwise.
#[cfg_attr(target_arch = "loongarch64", expect(unused))]
fn try_register_mmio_device<F>(
    mmio_range: Range<usize>,
    map_irq_line: F,
) -> Result<(), MmioRegisterError>
where
    F: FnOnce(IrqLine) -> ostd::Result<arch::MappedIrqLine>,
{
    let start_addr = mmio_range.start;

    // On aarch64 QEMU TCG, IoMem's VMALLOC mapping fails to read device_id
    // (returns 0) even though the magic read works. The raw GPU probe uses
    // the linear mapping (LINEAR_BASE + PA) successfully. We use the same
    // linear-mapping approach here for the initial device check, then create
    // IoMem with Writeback cache for subsequent device operations.
    #[cfg(target_arch = "aarch64")]
    {
        const LINEAR_BASE: usize = 0xffff_8000_0000_0000;
        let va = LINEAR_BASE + start_addr;
        let magic = unsafe { core::ptr::read_volatile(va as *const u32) };
        if magic != 0x74726976 {
            return Err(MmioRegisterError::MagicMismatch);
        }
        let device_id = unsafe { core::ptr::read_volatile((va + 8) as *const u32) };
        if device_id == 0 {
            return Err(MmioRegisterError::NoDevice);
        }
        ostd::early_println!(
            "[virtio-mmio] device ID = {} at {:#x} (linear mapping)",
            device_id,
            start_addr
        );
    }

    // Create IoMem for device operations. On aarch64 use Writeback to match
    // the linear mapping cache attributes (works on QEMU TCG).
    #[cfg(target_arch = "aarch64")]
    let io_mem = {
        use ostd::mm::CachePolicy;
        IoMem::acquire_with_cache_policy(mmio_range, CachePolicy::Writeback)
            .map_err(|_| MmioRegisterError::MmioUnavailable)?
    };
    #[cfg(not(target_arch = "aarch64"))]
    let io_mem = {
        // Acquire once and reuse: the region can only be acquired a single
        // time, and `mmio_range` is moved by `IoMem::acquire`.
        let io_mem = IoMem::acquire(mmio_range).map_err(|_| MmioRegisterError::MmioUnavailable)?;
        if !mmio_check_magic(&io_mem) {
            return Err(MmioRegisterError::MagicMismatch);
        }
        match mmio_read_device_id(&io_mem) {
            Err(_) | Ok(0) => return Err(MmioRegisterError::NoDevice),
            Ok(id) => {
                ostd::early_println!("[virtio-mmio] device ID = {} at {:#x}", id, start_addr);
            }
        }
        io_mem
    };

    ostd::early_println!("[virtio-mmio] allocating IRQ line...");
    let Ok(mapped_irq_line) = IrqLine::alloc().and_then(map_irq_line) else {
        ostd::early_println!("[virtio-mmio] IRQ line unavailable at {:#x}", start_addr);
        return Err(MmioRegisterError::IrqUnavailable);
    };
    ostd::early_println!("[virtio-mmio] IRQ mapped OK, registering device...");

    let device = MmioCommonDevice::new(io_mem, mapped_irq_line);
    MMIO_BUS.lock().register_mmio_device(device);
    ostd::early_println!("[virtio-mmio] device registered OK");

    Ok(())
}

#[derive(Clone, Copy)]
enum MmioRegisterError {
    /// MMIO region not available.
    MmioUnavailable,
    /// Not a VirtIO-MMIO slot.
    MagicMismatch,
    /// No device present.
    NoDevice,
    /// IRQ line not available.
    IrqUnavailable,
}
