// SPDX-License-Identifier: MPL-2.0

//! virtio-gpu 2D scanout driver.
//!
//! Brings up a single 2D scanout and publishes it as a blit-backed
//! [`FrameBuffer`] so the kernel's framebuffer console and `/dev/fb0`
//! light up. The 2D flow is:
//!
//! 1. `GET_DISPLAY_INFO` → learn scanout 0 dimensions
//! 2. `RESOURCE_CREATE_2D` → allocate a host-side XRGB8888 resource
//! 3. `RESOURCE_ATTACH_BACKING` → bind a guest DMA buffer to the resource
//! 4. `SET_SCANOUT` → bind the resource to scanout 0
//! 5. On every `flush` → `TRANSFER_TO_HOST_2D` pushes pixels to the host
//!
//! The driver stores its device state in a static so the framebuffer's
//! blit-flush callback (a plain `fn`) can recover it without a closure.
//!
//! All probe-time command/response handling is done synchronously by
//! polling the control queue (IRQs are enabled by the Kthread stage, but
//! polling avoids bring-up ordering subtleties). The IRQ callback only
//! drains the queue at runtime to avoid backing up `TRANSFER_TO_HOST_2D`
//! completion notifications.
#![allow(unused_imports)]

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use aster_framebuffer::framebuffer::{self, BlitBackend, FrameBuffer};
use ostd::arch::trap::TrapFrame;
use ostd::mm::{HasDaddr, HasSize, PAGE_SIZE, dma::DmaCoherent, io::util::HasVmReaderWriter};
use ostd::sync::SpinLock;
use ostd_pod::Pod;

use crate::device::VirtioDeviceError;
use crate::queue::VirtQueue;
use crate::transport::VirtioTransport;

use super::{
    CmdType, GpuFeatures, MemEntry, QUEUE_CONTROL, QUEUE_CURSOR, QUEUE_SIZE, Rect,
    ResourceAttachBacking, ResourceCreate2d, RespDisplayInfo, RespGeneric, RespType, SCANOUT_ID,
    SetScanout, TransferToHost2d, VirtioGpuConfig, VirtioGpuCtrlHdr, VirtioGpuFormat,
};

/// The single live GPU device, recovered by the blit-flush callback.
static LIVE_DEVICE: SpinLock<Option<&'static GpuDevice>> = SpinLock::new(None);

/// The next 2D resource ID to allocate. IDs start at 1 (0 is reserved).
static NEXT_RESOURCE_ID: AtomicU32 = AtomicU32::new(1);

/// The fence ID counter, incremented per control-queue command.
static NEXT_FENCE_ID: AtomicU64 = AtomicU64::new(1);

/// The pixel format we drive (32bpp XRGB). Maps to the framebuffer
/// `BgrReserved` pixel format.
const PIXEL_FORMAT: VirtioGpuFormat = VirtioGpuFormat::B8G8R8X8;

/// Default resolution when `GET_DISPLAY_INFO` reports no enabled scanout.
const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;

/// Bounded spin iterations when polling the control queue for a response.
/// Enough for QEMU's virtio-gpu, which responds within microseconds.
const POLL_ITERS: usize = 1_000_000;

pub struct GpuDevice {
    transport: SpinLock<Box<dyn VirtioTransport>>,
    control_queue: SpinLock<VirtQueue>,
    cursor_queue: SpinLock<VirtQueue>,
    /// The 2D resource ID returned by `RESOURCE_CREATE_2D`.
    resource_id: u32,
    /// Scanout geometry (pixels).
    width: u32,
    height: u32,
    /// Bytes per row (== width * 4 for XRGB8888).
    line_size: usize,
    /// The guest DMA backing buffer holding the pixel data.
    backing: DmaCoherent,
}

impl GpuDevice {
    /// Feature negotiation: the 2D path needs no device-specific features.
    pub(crate) fn negotiate_features(_features: u64) -> u64 {
        let _ = GpuFeatures::empty();
        0
    }

    /// Probe and initialize the device, then publish a framebuffer.
    pub(crate) fn init(mut transport: Box<dyn VirtioTransport>) -> Result<(), VirtioDeviceError> {
        // 1. Set up the two virtqueues.
        let control_queue = SpinLock::new(VirtQueue::new(
            QUEUE_CONTROL,
            QUEUE_SIZE,
            transport.as_mut(),
        )?);
        let cursor_queue = SpinLock::new(VirtQueue::new(
            QUEUE_CURSOR,
            QUEUE_SIZE,
            transport.as_mut(),
        )?);

        // 2. Query scanout geometry using a transient command context (no
        //    backing buffer allocated yet — GET_DISPLAY_INFO needs none).
        let probe = ProbeCtx {
            transport: &transport,
            control_queue: &control_queue,
        };
        let _num_scanouts = {
            let cfg = VirtioGpuConfig::new_manager(&*transport);
            cfg.num_scanouts()
        };
        let (width, height) = probe.query_display_info().unwrap_or_else(|_| {
            ostd::warn!(
                "virtio-gpu: GET_DISPLAY_INFO failed, defaulting to {}x{}",
                DEFAULT_WIDTH,
                DEFAULT_HEIGHT
            );
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        });

        // 3. Allocate the guest backing DMA buffer for the real geometry.
        let line_size = width as usize * 4;
        let fb_bytes = height as usize * line_size;
        let nframes = frames_of(fb_bytes);
        let backing =
            DmaCoherent::alloc(nframes, false).map_err(VirtioDeviceError::ResourceAlloc)?;

        // 4. Create the host 2D resource + attach backing + set scanout.
        let resource_id = NEXT_RESOURCE_ID.fetch_add(1, Ordering::Relaxed);
        probe.create_resource_2d(resource_id, width, height)?;
        probe.attach_backing(resource_id, &backing)?;
        probe.set_scanout(resource_id, width, height)?;

        // 5. Assemble the final device and leak it to `'static` so the flush
        //    callback can recover it. This is the single allocation point;
        //    no in-place mutation afterwards.
        let device: &'static GpuDevice = Box::leak(Box::new(GpuDevice {
            transport: SpinLock::new(transport),
            control_queue,
            cursor_queue,
            resource_id,
            width,
            height,
            line_size,
            backing,
        }));

        // 6. Finalize: register IRQ callbacks + DRIVER_OK.
        {
            let mut transport = device.transport.lock();
            let handle_irq = {
                let dev = device;
                move |_: &TrapFrame| {
                    dev.drain_control_queue();
                }
            };
            let _ = transport.register_queue_callback(QUEUE_CONTROL, Box::new(handle_irq), false);
            let _ = transport.register_cfg_callback(Box::new(|_| {}));
            transport.finish_init();
        }

        // 7. Register the live device and publish the framebuffer.
        *LIVE_DEVICE.lock() = Some(device);
        let fb_size = device.backing.size();
        let base = device.backing.reader().cursor() as usize;
        let blit = BlitBackend::new(base, fb_size, flush_callback);
        let fb = FrameBuffer::new_blit(
            blit,
            width as usize,
            height as usize,
            line_size,
            aster_framebuffer::pixel::PixelFormat::BgrReserved,
        );
        framebuffer::publish(Arc::new(fb));

        ostd::info!(
            "virtio-gpu: 2D scanout up {}x{} (resource {}, {} bytes backing)",
            width,
            height,
            resource_id,
            fb_size
        );
        Ok(())
    }

    /// Pushes a region of the backing buffer to the host resource. Called by
    /// the framebuffer flush callback.
    fn transfer_to_host(&self, x: u32, y: u32, width: u32, height: u32) {
        let fence = NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed);
        let hdr = VirtioGpuCtrlHdr::new(CmdType::TransferToHost2d, fence);
        let offset = (y as usize * self.line_size + x as usize * 4) as u64;
        let body = TransferToHost2d {
            offset,
            r: Rect {
                x,
                y,
                width,
                height,
            },
            resource_id: self.resource_id,
            _padding: 0,
        };
        // Fire-and-forget; the IRQ callback drains the queue. The host
        // processes transfers in command order.
        if let Err(e) = send_ctrl_hdr::<TransferToHost2d, RespGeneric>(
            &self.control_queue,
            hdr,
            &body,
            frames_of(core::mem::size_of::<RespGeneric>()),
        ) {
            ostd::warn!("virtio-gpu: TRANSFER_TO_HOST_2D failed: {:?}", e);
        }
    }

    /// Drains the control queue of any pending responses (IRQ callback).
    fn drain_control_queue(&self) {
        let mut q = self.control_queue.lock();
        while q.can_pop() {
            if q.pop_used().is_err() {
                break;
            }
        }
    }
}

/// A transient command-sending context borrowing the transport and queue
/// during probe, before the final `GpuDevice` is assembled.
struct ProbeCtx<'a> {
    transport: &'a Box<dyn VirtioTransport>,
    control_queue: &'a SpinLock<VirtQueue>,
}

impl<'a> ProbeCtx<'a> {
    /// `GET_DISPLAY_INFO` → `(width, height)` of scanout 0.
    fn query_display_info(&self) -> Result<(u32, u32), VirtioDeviceError> {
        let fence = NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed);
        let hdr = VirtioGpuCtrlHdr::new(CmdType::GetDisplayInfo, fence);
        // GET_DISPLAY_INFO takes no body; send a zeroed u32 placeholder so
        // the generic send_ctrl_hdr works. The device ignores the body.
        let resp: RespDisplayInfo = send_ctrl_hdr::<u32, RespDisplayInfo>(
            self.control_queue,
            hdr,
            &0u32,
            frames_of(core::mem::size_of::<RespDisplayInfo>()),
        )?;

        if resp.hdr_type != RespType::Ok as u32 && resp.hdr_type != RespType::OkNodata as u32 {
            return Err(VirtioDeviceError::UnsupportedConfig);
        }
        let mode = &resp.pmodes[0];
        if mode.enabled != 0 && mode.r.width != 0 && mode.r.height != 0 {
            Ok((mode.r.width, mode.r.height))
        } else {
            Ok((DEFAULT_WIDTH, DEFAULT_HEIGHT))
        }
    }

    fn create_resource_2d(
        &self,
        resource_id: u32,
        width: u32,
        height: u32,
    ) -> Result<(), VirtioDeviceError> {
        let fence = NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed);
        let hdr = VirtioGpuCtrlHdr::new(CmdType::ResourceCreate2d, fence);
        let body = ResourceCreate2d {
            resource_id,
            format: PIXEL_FORMAT as u32,
            width,
            height,
        };
        let resp: RespGeneric = send_ctrl_hdr::<ResourceCreate2d, RespGeneric>(
            self.control_queue,
            hdr,
            &body,
            frames_of(core::mem::size_of::<RespGeneric>()),
        )?;
        check_ok(resp.hdr_type, "RESOURCE_CREATE_2D")
    }

    fn attach_backing(
        &self,
        resource_id: u32,
        backing: &DmaCoherent,
    ) -> Result<(), VirtioDeviceError> {
        let fence = NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed);
        let hdr = VirtioGpuCtrlHdr::new(CmdType::ResourceAttachBacking, fence);

        // Pack body + one MemEntry contiguously into a single DMA buffer.
        let entry = MemEntry {
            addr: backing.daddr() as u64,
            length: backing.size() as u32,
            _padding: 0,
        };
        let body = ResourceAttachBacking {
            resource_id,
            nentries: 1,
            _padding: 0,
        };

        let hdr_dma = DmaCoherent::alloc(1, false).map_err(VirtioDeviceError::ResourceAlloc)?;
        {
            let mut w = hdr_dma.writer();
            w.write_val(&hdr)
                .map_err(|_| VirtioDeviceError::UnsupportedConfig)?;
        }
        let body_size =
            core::mem::size_of::<ResourceAttachBacking>() + core::mem::size_of::<MemEntry>();
        let body_dma = DmaCoherent::alloc(frames_of(body_size), false)
            .map_err(VirtioDeviceError::ResourceAlloc)?;
        {
            // Write body then entry sequentially via the writer's skip.
            // We use a single writer and advance it manually.
            let mut w = body_dma.writer();
            w.write_val(&body)
                .map_err(|_| VirtioDeviceError::UnsupportedConfig)?;
            // skip returns &mut Self, so chain directly.
            w.skip(core::mem::size_of::<ResourceAttachBacking>())
                .write_val(&entry)
                .map_err(|_| VirtioDeviceError::UnsupportedConfig)?;
        }
        let resp_dma = DmaCoherent::alloc(frames_of(core::mem::size_of::<RespGeneric>()), false)
            .map_err(VirtioDeviceError::ResourceAlloc)?;

        let token = {
            let mut q = self.control_queue.lock();
            q.add_dma_bufs(&[&hdr_dma, &body_dma], &[&resp_dma])
                .map_err(|_| VirtioDeviceError::InvalidQueueArgs)?
        };
        notify(self.control_queue);
        let resp: RespGeneric = poll_response(self.control_queue, token, &resp_dma)?;
        drop(hdr_dma);
        drop(body_dma);
        drop(resp_dma);
        check_ok(resp.hdr_type, "RESOURCE_ATTACH_BACKING")
    }

    fn set_scanout(
        &self,
        resource_id: u32,
        width: u32,
        height: u32,
    ) -> Result<(), VirtioDeviceError> {
        let fence = NEXT_FENCE_ID.fetch_add(1, Ordering::Relaxed);
        let hdr = VirtioGpuCtrlHdr::new(CmdType::SetScanout, fence);
        let body = SetScanout {
            r: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            scanout_id: SCANOUT_ID as u32,
            resource_id,
            _padding: 0,
        };
        let resp: RespGeneric = send_ctrl_hdr::<SetScanout, RespGeneric>(
            self.control_queue,
            hdr,
            &body,
            frames_of(core::mem::size_of::<RespGeneric>()),
        )?;
        check_ok(resp.hdr_type, "SET_SCANOUT")
    }
}

/// The flush callback registered with the framebuffer's `BlitBackend`.
fn flush_callback(_backend: &BlitBackend, x: usize, y: usize, width: usize, height: usize) {
    let guard = LIVE_DEVICE.lock();
    if let Some(device) = *guard {
        device.transfer_to_host(x as u32, y as u32, width as u32, height as u32);
    }
}

// ── Free-function command helpers ───────────────────────────────────────────
//
// These are free functions (not methods on GpuDevice) so the probe-time
// ProbeCtx and the runtime GpuDevice can share them without re-borrowing
// gymnastics.

/// Sends a header + body command, polling for a typed response.
///
/// `Body` is serialized into one DMA buffer, `Resp` is read back from the
/// response DMA buffer. They are distinct types (e.g. body = SetScanout,
/// resp = RespGeneric).
fn send_ctrl_hdr<Body: Pod, Resp: Pod>(
    control_queue: &SpinLock<VirtQueue>,
    hdr: VirtioGpuCtrlHdr,
    body: &Body,
    resp_nframes: usize,
) -> Result<Resp, VirtioDeviceError> {
    let hdr_dma = DmaCoherent::alloc(1, false).map_err(VirtioDeviceError::ResourceAlloc)?;
    {
        let mut w = hdr_dma.writer();
        w.write_val(&hdr)
            .map_err(|_| VirtioDeviceError::UnsupportedConfig)?;
    }
    let body_dma = DmaCoherent::alloc(frames_of(core::mem::size_of::<Body>()), false)
        .map_err(VirtioDeviceError::ResourceAlloc)?;
    {
        let mut w = body_dma.writer();
        w.write_val(body)
            .map_err(|_| VirtioDeviceError::UnsupportedConfig)?;
    }
    let resp_dma =
        DmaCoherent::alloc(resp_nframes, false).map_err(VirtioDeviceError::ResourceAlloc)?;

    let token = {
        let mut q = control_queue.lock();
        q.add_dma_bufs(&[&hdr_dma, &body_dma], &[&resp_dma])
            .map_err(|_| VirtioDeviceError::InvalidQueueArgs)?
    };
    notify(control_queue);
    let result = poll_response::<Resp>(control_queue, token, &resp_dma);
    drop(hdr_dma);
    drop(body_dma);
    drop(resp_dma);
    result
}

fn notify(control_queue: &SpinLock<VirtQueue>) {
    let mut q = control_queue.lock();
    if q.should_notify() {
        q.notify();
    }
}

/// Polls the control queue's used ring for the response to `token`.
///
/// **Limitation (known, accepted for the 2D MVP):** if the queue contains
/// responses from other in-flight commands (e.g. a prior fire-and-forget
/// `TRANSFER_TO_HOST_2D` whose response hasn't been drained), this loop
/// will `pop_used` those responses, find a non-matching token, and discard
/// them — causing the next legitimate waiter to time out. This is safe
/// during probe (commands are strictly serialized), but at runtime the
/// `transfer_to_host` flush path may race with the IRQ-driven drain. A
/// proper fix is a pending-token map keyed by fence_id; deferred until the
/// display path is validated end-to-end.
fn poll_response<T: Pod>(
    control_queue: &SpinLock<VirtQueue>,
    token: u16,
    resp_dma: &DmaCoherent,
) -> Result<T, VirtioDeviceError> {
    for _ in 0..POLL_ITERS {
        let mut q = control_queue.lock();
        if let Ok((used_token, _len)) = q.pop_used() {
            if used_token == token {
                return resp_dma
                    .reader()
                    .read_val::<T>()
                    .map_err(|_| VirtioDeviceError::UnsupportedConfig);
            }
            // Non-matching token: a stale response from a previous
            // fire-and-forget command. Drop it and keep polling.
        }
    }
    Err(VirtioDeviceError::UnsupportedConfig)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Computes the number of 4 KiB frames needed to hold `bytes`.
fn frames_of(bytes: usize) -> usize {
    (bytes + ostd::mm::PAGE_SIZE - 1) / ostd::mm::PAGE_SIZE
}

/// Checks that a response `hdr_type` is `OK`, logging + erroring otherwise.
fn check_ok(hdr_type: u32, cmd_name: &str) -> Result<(), VirtioDeviceError> {
    if hdr_type == RespType::Ok as u32 {
        Ok(())
    } else {
        ostd::warn!(
            "virtio-gpu: {} failed: response type {:#x}",
            cmd_name,
            hdr_type
        );
        Err(VirtioDeviceError::UnsupportedConfig)
    }
}
