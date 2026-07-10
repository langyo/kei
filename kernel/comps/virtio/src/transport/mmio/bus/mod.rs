// SPDX-License-Identifier: MPL-2.0

//! Virtio over MMIO

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
    ostd::early_println!("[virtio-mmio] try_register: acquiring IoMem {:#x}..{:#x}", start_addr, mmio_range.end);
    let Ok(io_mem) = IoMem::acquire(mmio_range) else {
        ostd::early_println!("[virtio-mmio] IoMem::acquire FAILED at {:#x}", start_addr);
        return Err(MmioRegisterError::MmioUnavailable);
    };
    ostd::early_println!("[virtio-mmio] IoMem acquired OK, checking magic...");

    // The kernel page table is now activated (commit dfd7324), so IoMem reads
    // work correctly on aarch64. No more debug skips.
    let magic_ok = mmio_check_magic(&io_mem);

    if !magic_ok {
        ostd::early_println!("[virtio-mmio] magic mismatch at {:#x}", start_addr);
        return Err(MmioRegisterError::MagicMismatch);
    }
    ostd::early_println!("[virtio-mmio] magic OK, reading device ID...");

    match mmio_read_device_id(&io_mem) {
        Err(_) | Ok(0) => {
            ostd::early_println!("[virtio-mmio] no device at {:#x}", start_addr);
            return Err(MmioRegisterError::NoDevice);
        }
        Ok(id) => {
            ostd::early_println!("[virtio-mmio] device ID = {} at {:#x}", id, start_addr);
        }
    }

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
