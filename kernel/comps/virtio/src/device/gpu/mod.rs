// SPDX-License-Identifier: MPL-2.0

//! virtio-gpu device definitions.
//!
//! Implements the 2D subset of the OASIS virtio-gpu specification
//! (without virgl 3D acceleration). The guest allocates a 2D host
//! resource, attaches a guest DMA backing buffer to it, and pushes pixels
//! to the host scanout via `TRANSFER_TO_HOST_2D`.
//!
//! Reference: virtio-v1.2, "5.7 GPU Device", with the 2D commands in
//! "5.7.4" and the command/response wire formats in "5.7.6".
//
// Some command/response structs below are not yet exercised at runtime
// (2D-only driver); they exist for spec completeness.
#![allow(dead_code)]

pub mod device;

use core::mem::offset_of;

use aster_util::safe_ptr::SafePtr;
use bitflags::bitflags;
#[allow(unused_imports)]
use ostd_pod::Pod;

use crate::transport::{ConfigManager, VirtioTransport};

pub const DEVICE_NAME: &str = "Virtio-GPU";

/// The control queue (commands + responses).
pub const QUEUE_CONTROL: u16 = 0;
/// The cursor queue (cursor updates). Unused for 2D-only operation but the
/// device requires it to be set up.
pub const QUEUE_CURSOR: u16 = 1;

/// Virtqueue size — 64 is ample for the synchronous 2D command flow.
pub const QUEUE_SIZE: u16 = 64;

/// The number of scanouts we will drive. 2D kiosk mode uses one.
pub const SCANOUT_ID: u8 = 0;

// ── Feature bits (device-specific, bits 0..24) ──────────────────────────────

bitflags! {
    /// virtio-gpu feature bits (spec 5.7.3).
    pub struct GpuFeatures: u64 {
        /// Virgl 3D acceleration via the Gallium interface. Not supported
        /// (this is a 2D-only driver).
        const VIRGL = 1 << 0;
        /// Extended Display Identification Data. We use `GET_DISPLAY_INFO`
        /// instead, so this is optional.
        const EDID = 1 << 1;
        /// Resource UUID assignment (unused in 2D mode).
        #[expect(dead_code)]
        const RESOURCE_UUID = 1 << 2;
        /// Blob resources (unused in 2D mode).
        #[expect(dead_code)]
        const RESOURCE_BLOB = 1 << 3;
    }
}

// ── 2D resource formats (spec 5.7.4.1) ──────────────────────────────────────

/// The pixel format of a 2D resource. We only use `B8G8R8X8` (32bpp XRGB),
/// which maps cleanly to the framebuffer's `BgrReserved` pixel format.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioGpuFormat {
    /// 32bpp BGRX (X = unused alpha). Matches the kei `BgrReserved` format.
    B8G8R8X8 = 2,
    /// 32bpp BGRA (A = alpha).
    #[expect(dead_code)]
    B8G8R8A8 = 6,
}

// ── Control queue command types (spec 5.7.6.1) ──────────────────────────────

/// virtio-gpu control command `hdr_type` values.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CmdType {
    /// `GET_DISPLAY_INFO` — query the configured scanout geometry.
    GetDisplayInfo = 0x0100,
    /// `RESOURCE_CREATE_2D` — create a host-side 2D resource.
    ResourceCreate2d = 0x0101,
    /// `RESOURCE_UNREF` — destroy a host-side resource.
    #[expect(dead_code)]
    ResourceUnref = 0x0102,
    /// `SET_SCANOUT` — bind a resource to a scanout.
    SetScanout = 0x0103,
    /// `RESOURCE_FLUSH` — flush a region of a host resource to its scanout.
    #[expect(dead_code)]
    ResourceFlush = 0x0104,
    /// `TRANSFER_TO_HOST_2D` — copy guest pixels into a host resource region.
    TransferToHost2d = 0x0105,
    /// `RESOURCE_ATTACH_BACKING` — attach guest DMA storage to a resource.
    ResourceAttachBacking = 0x0106,
    /// `RESOURCE_DETACH_BACKING` — detach guest DMA storage.
    #[expect(dead_code)]
    ResourceDetachBacking = 0x0107,
    /// `GET_CAPSET_INFO` — query available capability sets (virgl).
    #[expect(dead_code)]
    GetCapsetInfo = 0x0108,
    /// `GET_CAPSET` — fetch a capability set (virgl).
    #[expect(dead_code)]
    GetCapset = 0x0109,
    /// `RESOURCE_ASSIGN_UUID` — assign a UUID to a resource.
    #[expect(dead_code)]
    ResourceAssignUuid = 0x010b,
}

/// virtio-gpu response `type` values (spec 5.7.6.3).
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RespType {
    Ok = 0x1100,
    OkNodata = 0x1101,
    ErrUnspec = 0x1200,
    ErrOutOfMemory = 0x1201,
    ErrInvalidScanoutId = 0x1202,
    ErrInvalidResourceId = 0x1203,
    ErrInvalidContextId = 0x1204,
    ErrInvalidParameter = 0x1205,
}

// ── Wire-format structures (all `#[repr(C)] #[derive(Pod)]`) ────────────────

/// The header preceding every control-queue command (spec 5.7.6.1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct VirtioGpuCtrlHdr {
    /// Command type — one of [`CmdType`].
    pub hdr_type: u32,
    /// Status flags (currently zero for 2D commands).
    pub flags: u32,
    /// Fence ID — used to correlate `TRANSFER_TO_HOST_2D` with completion
    /// responses. We increment this per command.
    pub fence_id: u64,
    /// Context ID (zero for 2D mode).
    pub ctx_id: u32,
    /// Padding to 24 bytes.
    pub _padding: u32,
}

impl VirtioGpuCtrlHdr {
    pub const fn new(hdr_type: CmdType, fence_id: u64) -> Self {
        Self {
            hdr_type: hdr_type as u32,
            flags: 0,
            fence_id,
            ctx_id: 0,
            _padding: 0,
        }
    }
}

/// A rectangle used by several 2D commands (spec 5.7.6.4).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// `RESOURCE_CREATE_2D` command body (spec 5.7.6.1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct ResourceCreate2d {
    /// Resource ID (driver-allocated, non-zero).
    pub resource_id: u32,
    /// Pixel format — one of [`VirtioGpuFormat`].
    pub format: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// `SET_SCANOUT` command body (spec 5.7.6.1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct SetScanout {
    /// Scanout rectangle on the display.
    pub r: Rect,
    /// Scanout index (we use [`SCANOUT_ID`]).
    pub scanout_id: u32,
    /// Resource ID to bind to this scanout. Zero detaches.
    pub resource_id: u32,
    /// Padding.
    pub _padding: u32,
}

/// `TRANSFER_TO_HOST_2D` command body (spec 5.7.6.1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct TransferToHost2d {
    /// Offset within the backing buffer (in bytes) where the source region
    /// starts. For a tightly-packed buffer this is `y * line_size + x * bpp`.
    pub offset: u64,
    /// Source rectangle in the host resource (destination of the copy).
    pub r: Rect,
    /// Resource ID to copy into.
    pub resource_id: u32,
    /// Padding.
    pub _padding: u32,
}

/// One entry in the `RESOURCE_ATTACH_BACKING` scatter list (spec 5.7.6.1).
/// `addr` is the **guest physical address** of the segment.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct MemEntry {
    /// Guest physical address of the segment.
    pub addr: u64,
    /// Length of the segment in bytes.
    pub length: u32,
    /// Padding.
    pub _padding: u32,
}

/// `RESOURCE_ATTACH_BACKING` command body (spec 5.7.6.1).
/// Sent immediately after the ctrl header; followed by `nent` [`MemEntry`]
/// entries in the same buffer chain.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct ResourceAttachBacking {
    /// Resource ID to attach storage to.
    pub resource_id: u32,
    /// Number of [`MemEntry`] entries following.
    pub nentries: u32,
    /// Padding.
    pub _padding: u32,
}

/// A single scanout's geometry, as returned by `GET_DISPLAY_INFO`
/// (spec 5.7.6.4, `virtio_gpu_resp_display_info`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct DisplayOne {
    /// Geometry rectangle.
    pub r: Rect,
    /// Enabled flag (1 = scanout enabled).
    pub enabled: u32,
    /// Flags.
    pub flags: u32,
}

/// The response to `GET_DISPLAY_INFO` (spec 5.7.6.4).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct RespDisplayInfo {
    /// Response header `type` — should be [`RespType::OkNodata`] if no
    /// display is configured, else [`RespType::Ok`].
    pub hdr_type: u32,
    /// Number of scanouts present in `pmodes`.
    pub num_scanouts: u32,
    /// Per-scanout geometry. The spec fixes this at 16; we use index 0.
    pub pmodes: [DisplayOne; 16],
}

/// The generic response to a 2D command (spec 5.7.6.3, `virtio_gpu_ctrl_hdr`
/// reused as response). Only `hdr_type` carries the status; the rest is
/// zeroed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Default)]
pub struct RespGeneric {
    /// Response type — one of [`RespType`].
    pub hdr_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub _padding: u32,
}

// ── Device config space (spec 5.7.5) ────────────────────────────────────────

/// The virtio-gpu configuration space layout (spec 5.7.5).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
pub struct VirtioGpuConfig {
    /// Events read (bitmask, currently unused).
    pub events_read: u32,
    /// Number of scanouts the device supports.
    pub num_scanouts: u32,
    /// Number of capability sets (virgl).
    pub num_capsets: u32,
}

impl VirtioGpuConfig {
    /// Build a [`ConfigManager`] over the transport's config space.
    pub(crate) fn new_manager(transport: &dyn VirtioTransport) -> ConfigManager<Self> {
        let safe_ptr = transport
            .device_config_mem()
            .map(|mem| SafePtr::new(mem, 0));
        let bar_space = transport.device_config_bar();
        ConfigManager::new(safe_ptr, bar_space)
    }
}

impl ConfigManager<VirtioGpuConfig> {
    /// Read the number of scanouts the device claims to support.
    pub(crate) fn num_scanouts(&self) -> u32 {
        self.read_once::<u32>(offset_of!(VirtioGpuConfig, num_scanouts))
            .unwrap_or(1)
    }
}
