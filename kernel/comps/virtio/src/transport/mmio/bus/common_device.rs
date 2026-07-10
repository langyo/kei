// SPDX-License-Identifier: MPL-2.0

//! MMIO device common definitions or functions.

use int_to_c_enum::TryFromInt;
use ostd::{
    Error, Result, info,
    io::IoMem,
    irq::IrqLine,
    mm::{HasPaddr, VmIoOnce},
};

use super::arch::MappedIrqLine;

/// A MMIO common device.
#[derive(Debug)]
pub struct MmioCommonDevice {
    io_mem: IoMem,
    irq: MappedIrqLine,
}

impl MmioCommonDevice {
    pub(super) fn new(io_mem: IoMem, irq: MappedIrqLine) -> Self {
        let this = Self { io_mem, irq };
        // On aarch64 without kernel page table switch, IoMem's KVirtArea
        // mapping doesn't work. But the boot page table's linear mapping
        // (0xffff800000000000 + paddr) already covers MMIO regions.
        // We rely on that mapping for all MMIO reads/writes.
        #[cfg(target_arch = "aarch64")]
        ostd::early_println!("[virtio-mmio] MmioCommonDevice::new: paddr={:#x} irq={}", this.io_mem.paddr(), this.irq.num());

        this
    }

    /// Returns a reference to the I/O memory.
    pub fn io_mem(&self) -> &IoMem {
        &self.io_mem
    }

    /// Returns the physical address of the MMIO region (aarch64 debug).
    pub fn io_mem_paddr(&self) -> usize {
        self.io_mem.paddr()
    }

    /// Reads the device ID from the I/O memory.
    pub fn read_device_id(&self) -> Result<u32> {
        mmio_read_device_id(&self.io_mem)
    }

    /// Reads the version number from the I/O memory.
    pub fn read_version(&self) -> Result<VirtioMmioVersion> {
        VirtioMmioVersion::try_from(mmio_read_version(&self.io_mem)?)
            .map_err(|_| Error::InvalidArgs)
    }

    /// Returns an immutable reference to the IRQ line.
    pub fn irq(&self) -> &IrqLine {
        &self.irq
    }
}

/// Virtio MMIO version.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, TryFromInt)]
pub enum VirtioMmioVersion {
    /// Legacy
    Legacy = 1,
    /// Modern
    Modern = 2,
}

const OFFSET_TO_MAGIC: usize = 0;
const OFFSET_TO_VERSION: usize = 4;
const OFFSET_TO_DEVICE_ID: usize = 8;

pub(super) fn mmio_check_magic(io_mem: &IoMem) -> bool {
    const MAGIC_VALUE: u32 = 0x74726976;
    io_mem
        .read_once::<u32>(OFFSET_TO_MAGIC)
        .is_ok_and(|val| val == MAGIC_VALUE)
}
fn mmio_read_version(io_mem: &IoMem) -> Result<u32> {
    io_mem.read_once(OFFSET_TO_VERSION)
}
pub(super) fn mmio_read_device_id(io_mem: &IoMem) -> Result<u32> {
    io_mem.read_once(OFFSET_TO_DEVICE_ID)
}
