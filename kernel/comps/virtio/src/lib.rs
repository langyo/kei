// SPDX-License-Identifier: MPL-2.0

//! The virtio of Asterinas.
#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

#[cfg(target_arch = "aarch64")]
pub mod aarch64_raw_gpu_probe;

#[macro_use]
extern crate ostd_pod;

use alloc::boxed::Box;
use core::hint::spin_loop;

use aster_block::MajorIdOwner;
use bitflags::bitflags;
use component::{ComponentInitError, init_component};
use device::{
    VirtioDeviceType, block::device::BlockDevice, console::device::ConsoleDevice,
    entropy::device::EntropyDevice, filesystem::device::FileSystemDevice, gpu::device::GpuDevice,
    input::device::InputDevice, network::device::NetworkDevice, socket::device::SocketDevice,
};
use ostd::{error, warn};
use spin::Once;
use transport::{DeviceStatus, mmio::VIRTIO_MMIO_DRIVER, pci::VIRTIO_PCI_DRIVER};

use crate::transport::VirtioTransport;

// Set this crate's log prefix for `ostd::log`.
macro_rules! __log_prefix {
    () => {
        "virtio: "
    };
}

pub mod device;
mod dma_buf;
mod id_alloc;
mod queue;
mod transport;

static VIRTIO_BLOCK_MAJOR_ID: Once<MajorIdOwner> = Once::new();

/// Public init function for manual invocation (aarch64 bypass path).
pub fn virtio_component_init_pub() -> Result<(), ComponentInitError> {
    virtio_component_init_inner()
}

#[init_component]
fn virtio_component_init() -> Result<(), ComponentInitError>
{
    virtio_component_init_inner()
}

fn virtio_component_init_inner() -> Result<(), ComponentInitError> {
    ostd::early_println!("[virtio] allocating major ID...");
    VIRTIO_BLOCK_MAJOR_ID.call_once(|| aster_block::allocate_major().unwrap());

    ostd::early_println!("[virtio] transport::init...");
    // Find all devices and register them to the corresponding crate
    transport::init();
    ostd::early_println!("[virtio] transport::init done");

    ostd::early_println!("[virtio] entropy::init...");
    device::entropy::init();
    ostd::early_println!("[virtio] network::init...");
    device::network::init();
    ostd::early_println!("[virtio] socket::init...");
    device::socket::init();
    ostd::early_println!("[virtio] device sub-inits done");

    // On aarch64, the IoMem KVirtArea mapping doesn't work without the
    // kernel page table switch. Instead, manually probe each MMIO device
    // using raw volatile reads through the linear mapping.
    #[cfg(target_arch = "aarch64")]
    {
        crate::aarch64_raw_gpu_probe::probe();
    }

    let mut dev_idx = 0;
    while let Some(mut transport) = pop_device_transport() {
        dev_idx += 1;
        ostd::early_println!("[virtio] processing device #{}", dev_idx);
        // Reset device
        ostd::early_println!("[virtio] dev #{}: resetting...", dev_idx);
        transport
            .write_device_status(DeviceStatus::empty())
            .unwrap();
        while transport.read_device_status() != DeviceStatus::empty() {
            spin_loop();
        }

        // Set to acknowledge
        transport
            .write_device_status(DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER)
            .unwrap();
        // negotiate features
        negotiate_features(&mut transport);

        if !transport.is_legacy_version() {
            // change to features ok status
            let status =
                DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER | DeviceStatus::FEATURES_OK;
            transport.write_device_status(status).unwrap();
        }

        let device_type = transport.device_type();
        ostd::early_println!("[virtio] dev #{}: type={:?}", dev_idx, device_type);
        let res = match transport.device_type() {
            VirtioDeviceType::Block => BlockDevice::init(transport),
            VirtioDeviceType::Console => ConsoleDevice::init(transport),
            VirtioDeviceType::Entropy => EntropyDevice::init(transport),
            VirtioDeviceType::Gpu => GpuDevice::init(transport),
            VirtioDeviceType::Input => InputDevice::init(transport),
            VirtioDeviceType::Network => NetworkDevice::init(transport),
            VirtioDeviceType::Socket => SocketDevice::init(transport),
            VirtioDeviceType::FileSystem => FileSystemDevice::init(transport),
            _ => {
                warn!("Found unimplemented device: {:?}", device_type);
                Ok(())
            }
        };
        ostd::early_println!("[virtio] dev #{} init result: {:?}", dev_idx, res.as_ref().err());
        if res.is_err() {
            error!(
                "Device initialization error: {:?}, device type: {:?}",
                res, device_type
            );
        }
    }
    Ok(())
}

fn pop_device_transport() -> Option<Box<dyn VirtioTransport>> {
    if let Some(device) = VIRTIO_PCI_DRIVER.get().unwrap().pop_device_transport() {
        return Some(device);
    }
    if let Some(device) = VIRTIO_MMIO_DRIVER.get().unwrap().pop_device_transport() {
        return Some(Box::new(device));
    }
    None
}

fn negotiate_features(transport: &mut Box<dyn VirtioTransport>) {
    let features = transport.read_device_features();
    let mask = ((1u64 << 24) - 1) | (((1u64 << 24) - 1) << 50);
    let device_specified_features = features & mask;
    let device_support_features = match transport.device_type() {
        VirtioDeviceType::Network => NetworkDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::Block => BlockDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::Input => InputDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::Console => ConsoleDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::Gpu => GpuDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::Socket => SocketDevice::negotiate_features(device_specified_features),
        VirtioDeviceType::FileSystem => {
            FileSystemDevice::negotiate_features(device_specified_features)
        }
        _ => device_specified_features,
    };
    let mut support_feature = Feature::from_bits_truncate(features);
    support_feature.remove(Feature::RING_EVENT_IDX);
    transport
        .write_driver_features(features & (support_feature.bits | device_support_features))
        .unwrap();
}

bitflags! {
    /// all device features, bits 0~23 and 50~63 are specified by device.
    /// if using this struct to translate u64, use from_bits_truncate function instead of from_bits
    ///
    struct Feature: u64 {

        // device independent
        const NOTIFY_ON_EMPTY       = 1 << 24; // legacy
        const ANY_LAYOUT            = 1 << 27; // legacy
        const RING_INDIRECT_DESC    = 1 << 28;
        const RING_EVENT_IDX        = 1 << 29;
        const UNUSED                = 1 << 30; // legacy
        const VERSION_1             = 1 << 32; // detect legacy

        // since virtio v1.1
        const ACCESS_PLATFORM       = 1 << 33;
        const RING_PACKED           = 1 << 34;
        const IN_ORDER              = 1 << 35;
        const ORDER_PLATFORM        = 1 << 36;
        const SR_IOV                = 1 << 37;
        const NOTIFICATION_DATA     = 1 << 38;
        const NOTIF_CONFIG_DATA     = 1 << 39;
        const RING_RESET            = 1 << 40;
    }
}
