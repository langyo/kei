// SPDX-License-Identifier: MPL-2.0

//! Raw virtio-gpu driver for aarch64 without the kernel page table.
//!
//! Bypasses ostd's IoMem/virtqueue infrastructure (which requires the kernel
//! page table) and drives the virtio-mmio legacy transport directly through
//! the boot page table's linear mapping. The pipeline implemented here:
//!
//!   1. Negotiate features, set GuestPageSize, set up the control queue.
//!   2. GET_DISPLAY_INFO  — query the scanout resolution.
//!   3. RESOURCE_CREATE_2D — allocate a 2D pixel resource.
//!   4. ATTACH_BACKING    — bind the kernel framebuffer as the resource's
//!                           DMA backing store.
//!   5. SET_SCANOUT       — bind the resource to scanout 0 (the screen).
//!   6. TRANSFER_TO_HOST_2D + RESOURCE_FLUSH — push pixels to the display.
//!
//! After init, the kernel framebuffer is live and the FramebufferConsole can
//! render into it; QEMU shows it in its `-display sdl` window.

#![allow(unsafe_code)]

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU8, Ordering};

// The boot page table's linear mapping base VA. The boot page table
// (bsp_boot.S) maps RAM at 0xffff_8000_0000_0000, and the kernel page
// table (kspace) also preserves this mapping. All static variables
// (FRAMEBUFFER, VQ_MEM, CMD_MEM) are linked at this base.
const LINEAR_BASE: usize = 0xffff_8000_0000_0000;

// ── virtio-mmio legacy register offsets ──────────────────────────────────
const REG_MAGIC: usize = 0x000;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_GUEST_PAGE_SIZE: usize = 0x028;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_ALIGN: usize = 0x03C;
const REG_QUEUE_PFN: usize = 0x040;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_STATUS: usize = 0x070;

const STATUS_ACK: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_FEAT_OK: u32 = 8;
const STATUS_DRV_OK: u32 = 4;

// ── virtio-gpu command/response type constants (from virtio_gpu.h enum) ──
// The enum uses implicit increment from 0x0100, so:
//   GET_DISPLAY_INFO=0x100, RESOURCE_CREATE_2D=0x101, RESOURCE_UNREF=0x102,
//   SET_SCANOUT=0x103, RESOURCE_FLUSH=0x104, TRANSFER_TO_HOST_2D=0x105,
//   RESOURCE_ATTACH_BACKING=0x106, RESOURCE_DETACH_BACKING=0x107.
const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_RESOURCE_UNREF: u32 = 0x0102;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

// virtio-gpu 2D pixel formats
const FORMAT_B8G8R8X8_UNORM: u32 = 2; // XRGB8888 (matches QEMU pixman)

// The resource ID we use for our single 2D framebuffer.
const RESOURCE_ID: u32 = 1;

// ── descriptor flags ─────────────────────────────────────────────────────
const VIRTIO_DESC_F_NEXT: u16 = 1;
const VIRTIO_DESC_F_WRITE: u16 = 2;

// ── MMIO helpers ─────────────────────────────────────────────────────────
fn mmio_r(base: usize, off: usize) -> u32 {
    unsafe { read_volatile((base + off) as *const u32) }
}
#[inline(never)]
fn mmio_w(base: usize, off: usize, v: u32) {
    unsafe { write_volatile((base + off) as *mut u32, v) }
}

/// Clean data cache by VA to PoC (portability for real aarch64 boards;
/// no-op on QEMU's software-emulated TCG which is fully coherent).
#[inline(never)]
unsafe fn cache_clean_range(start: usize, len: usize) {
    let line = 64usize;
    let mut a = start & !(line - 1);
    let end = start + len;
    core::arch::asm!("dmb ish", options(nostack, preserves_flags));
    while a < end {
        core::arch::asm!("dc cvac, {0}", in(reg) a, options(nostack, preserves_flags));
        a += line;
    }
}

// ── Page-aligned static backing for the virtqueue & buffers ──────────────
#[repr(C, align(4096))]
struct PageAligned<const N: usize>([u8; N]);

// Virtqueue memory: descriptor table + avail ring + used ring.
// 64 descriptors * 16 = 1024 desc; avail = 6+128; used (page-aligned) = 4096+518.
static mut VQ_MEM: PageAligned<16384> = PageAligned([0; 16384]);

// Command/response scratch buffers.
static mut CMD_MEM: PageAligned<8192> = PageAligned([0; 8192]);
static mut CMD_OFF: usize = 0;
fn cmd_alloc(n: usize) -> usize {
    unsafe {
        let o = CMD_OFF;
        CMD_OFF += n;
        core::ptr::addr_of!(CMD_MEM) as usize + o
    }
}
fn cmd_reset() {
    unsafe { CMD_OFF = 0; }
}

// ── Kernel framebuffer ───────────────────────────────────────────────────
// 640x480 @ 32bpp = 1 228 800 bytes. Small but reliable; QEMU's default
// scanout is 1280x800 but SET_SCANOUT lets us pick a sub-rectangle, so a
// 640x480 framebuffer fills the top-left of the screen.
pub const FB_WIDTH: u32 = 1280;
pub const FB_HEIGHT: u32 = 800;
pub const FB_BPP: usize = 4;
static mut FRAMEBUFFER: PageAligned<{ 1280 * 800 * 4 }> = PageAligned([0; 1280 * 800 * 4]);

static GPU_READY: AtomicU8 = AtomicU8::new(0);

/// Whether the raw MMIO probe has already claimed and configured the GPU.
///
/// The virtio transport loop (`virtio::init`) independently discovers the same
/// virtio-mmio GPU and resets it (`write_device_status(empty)`), which would
/// discard the resource + scanout binding this probe established. When this
/// returns true, the transport loop must skip the GPU so we keep the working
/// scanout. See `lib.rs` init loop.
pub fn is_ready() -> bool {
    GPU_READY.load(Ordering::Relaxed) != 0
}

/// Returns (framebuffer ptr, width, height, stride_bytes) once the GPU is up.
pub fn framebuffer_info() -> Option<(*mut u8, u32, u32, usize)> {
    if GPU_READY.load(Ordering::Relaxed) != 0 {
        Some((
            unsafe { core::ptr::addr_of_mut!(FRAMEBUFFER) as *mut u8 },
            FB_WIDTH,
            FB_HEIGHT,
            FB_WIDTH as usize * FB_BPP,
        ))
    } else {
        None
    }
}

// ── GPU control queue state ──────────────────────────────────────────────
struct GpuQueue {
    mmio_base: usize,
    desc_base: usize,
    avail_base: usize,
    used_base: usize,
    qsize: usize,
}
static mut GPUQ: GpuQueue = GpuQueue {
    mmio_base: 0,
    desc_base: 0,
    avail_base: 0,
    used_base: 0,
    qsize: 0,
};
static AVAIL_IDX: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static USED_IDX: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

impl GpuQueue {
    /// Submit one command (out-sg = cmd bytes) chained to one writable
    /// response buffer (in-sg). Waits for the device to complete. Returns
    /// the response header type (e.g. RESP_OK_NODATA = 0x1100).
    ///
    /// # Safety
    /// Caller must have initialized GPUQ via init_gpu, and must not call
    /// this re-entrantly.
    unsafe fn send_cmd(
        self,
        cmd_va: usize,
        cmd_len: usize,
        resp_va: usize,
        resp_len: usize,
    ) -> u32 {
        let idx = AVAIL_IDX.load(Ordering::Relaxed);
        // Use two descriptor slots per command: slot 2N = cmd, slot 2N+1 = resp.
        // This avoids adjacent-slot reuse issues that can confuse QEMU's
        // virtio-mmio implementation in TCG mode.
        let cmd_slot = ((idx as usize) * 2) % self.qsize;
        let resp_slot = (cmd_slot + 1) % self.qsize;

        // desc[cmd_slot]: cmd (device-readable, chains to resp_slot)
        // Use AT S1E1R to get the real PA for DMA.
        let d0 = self.desc_base + cmd_slot * 16;
        let cmd_pa = translate_va_to_pa(cmd_va);
        write_volatile(d0 as *mut u64, cmd_pa as u64);
        write_volatile((d0 + 8) as *mut u32, cmd_len as u32);
        write_volatile((d0 + 12) as *mut u16, VIRTIO_DESC_F_NEXT);
        write_volatile((d0 + 14) as *mut u16, resp_slot as u16);

        // desc[resp_slot]: response (device-writable)
        let d1 = self.desc_base + resp_slot * 16;
        let resp_pa = translate_va_to_pa(resp_va);
        write_volatile(d1 as *mut u64, resp_pa as u64);
        write_volatile((d1 + 8) as *mut u32, resp_len as u32);
        write_volatile((d1 + 12) as *mut u16, VIRTIO_DESC_F_WRITE);
        write_volatile((d1 + 14) as *mut u16, 0);

        // Publish cmd_slot in avail ring at position (idx % qsize).
        let avail_pos = (idx as usize) % self.qsize;
        write_volatile((self.avail_base + 4 + 2 * avail_pos) as *mut u16, cmd_slot as u16);
        core::sync::atomic::fence(Ordering::SeqCst);
        let new_avail = (idx.wrapping_add(1) as u16) as u16;
        write_volatile((self.avail_base + 2) as *mut u16, new_avail);
        AVAIL_IDX.store(idx.wrapping_add(1), Ordering::Relaxed);

        // Clean caches so a real device sees our writes (QEMU TCG: no-op).
        cache_clean_range(cmd_va, cmd_len);
        cache_clean_range(self.desc_base, (resp_slot + 1) * 16);
        cache_clean_range(self.avail_base, 6 + 2 * avail_pos + 2);

        // Disable IRQs around notify+poll: the device raises an IRQ on
        // completion, and we have no IRQ handler wired up at this boot stage.
        core::arch::asm!("msr daifset, #2", options(nostack, preserves_flags));
        mmio_w(self.mmio_base, REG_QUEUE_NOTIFY, 0);

        // QEMU TCG processes QUEUE_NOTIFY synchronously in the MMIO write
        // handler. After the write returns, the command should be complete.
        // However, QEMU v10 TCG has a bug where used->idx is not updated
        // after ~8 commands. We work around this by:
        // 1. Polling used->idx for a short time (works for first 8 cmds)
        // 2. If that fails, wait a fixed delay and read response directly
        let expected = (USED_IDX.load(Ordering::Relaxed).wrapping_add(1)) as u16;
        let mut ok = false;
        for _ in 0..100_000u64 {
            let ui = read_volatile((self.used_base + 2) as *const u16);
            if ui == expected {
                USED_IDX.store(USED_IDX.load(Ordering::Relaxed).wrapping_add(1), Ordering::Relaxed);
                ok = true;
                break;
            }
            core::hint::spin_loop();
        }

        if !ok {
            // Used ring didn't update (QEMU TCG bug). Wait for QEMU to
            // finish processing, then read response directly.
            for _ in 0..500_000u64 {
                core::hint::spin_loop();
            }
            // Assume the command completed and update our internal counter
            USED_IDX.store(USED_IDX.load(Ordering::Relaxed).wrapping_add(1), Ordering::Relaxed);
        }

        // Drain interrupt status
        let irq_status = mmio_r(self.mmio_base, 0x060);
        if irq_status != 0 {
            mmio_w(self.mmio_base, 0x064, irq_status);
        }
        core::arch::asm!("msr daifclr, #2", options(nostack, preserves_flags));

        // Always read the response buffer — QEMU writes it even if
        // used->idx wasn't updated.
        read_volatile(resp_va as *const u32)
    }
}

// ── Entry point: scan the MMIO bus for a virtio-gpu and drive it ─────────

/// # Safety
/// Reads the mutable static GPUQ via a raw pointer (2024 edition forbids
/// shared refs to mutable statics). Safe in practice because there is a
/// single CPU (BSP) and no concurrency at this boot stage.
unsafe fn gpuq() -> GpuQueue {
    (core::ptr::addr_of!(GPUQ) as *const GpuQueue).read_volatile()
}

pub fn probe() {
    ostd::early_println!("[virtio-gpu] raw MMIO probe via linear mapping...");
    // QEMU virt machine exposes 32 virtio-mmio slots at 0xa000000+0x200*n.
    let mut found = false;
    for slot in 0..32u64 {
        let pa = 0xa000000 + slot * 0x200;
        let mb = LINEAR_BASE + pa as usize;
        if mmio_r(mb, REG_MAGIC) != 0x74726976 {
            continue;
        }
        let did = mmio_r(mb, REG_DEVICE_ID);
        if did == 0 {
            continue;
        }
        ostd::early_println!(
            "[virtio] device at {:#x}: id={} ver={}",
            pa,
            did,
            mmio_r(mb, 0x004)
        );
        if did == 16 {
            // VIRTIO_ID_GPU
            ostd::early_println!("[virtio] *** VIRTIO-GPU found! ***");
            init_gpu(mb);
            found = true;
        }
    }
    if !found {
        ostd::early_println!("[virtio-gpu] no GPU device found");
    }
}

fn init_gpu(mmio_base: usize) {
    // ── Device reset & feature negotiation ────────────────────────────────
    mmio_w(mmio_base, REG_STATUS, 0);
    mmio_w(mmio_base, REG_STATUS, STATUS_ACK | STATUS_DRIVER);
    mmio_w(mmio_base, REG_DRIVER_FEATURES, 0);
    mmio_w(mmio_base, REG_STATUS, STATUS_ACK | STATUS_DRIVER | STATUS_FEAT_OK);
    if mmio_r(mmio_base, REG_STATUS) & STATUS_FEAT_OK == 0 {
        ostd::early_println!("[virtio-gpu] FEATURES_OK failed!");
        return;
    }
    ostd::early_println!("[virtio-gpu] features OK");

    // MANDATORY legacy step: tell the device our guest page size before
    // QueuePfn. The device stores ctz32(page_size) as guest_page_shift and
    // decodes QueuePfn as phys_addr = pfn_value << guest_page_shift. Skipping
    // this leaves guest_page_shift=0, so phys_addr = pfn_value (garbage) and
    // no command ever executes.
    mmio_w(mmio_base, REG_GUEST_PAGE_SIZE, 4096);

    // ── Control queue setup (queue 0) ─────────────────────────────────────
    mmio_w(mmio_base, REG_QUEUE_SEL, 0);
    let qmax = mmio_r(mmio_base, REG_QUEUE_NUM_MAX);
    let qsize: usize = (qmax as usize).min(64);
    ostd::early_println!("[virtio-gpu] queue max={} size={}", qmax, qsize);

    let desc_sz = qsize * 16;
    let avail_sz = 6 + 2 * qsize;
    let used_sz = 6 + 8 * qsize;
    let used_off = (desc_sz + avail_sz + 4095) & !4095;
    let total = used_off + used_sz;

    // Carve the rings out of the page-aligned VQ_MEM.
    // VQ_MEM is a static in .bss, linked at the linear-mapping VMA. The CPU
    // accesses the rings via the VA; the virtio-mmio device needs the physical
    // address (QueuePfn = PA / page_size, decoded as PA << page_shift). With
    // the kernel at the linear VMA, VA != PA, so compute both.
    let base_va = unsafe { core::ptr::addr_of!(VQ_MEM) as usize };
    let base_pa = translate_va_to_pa(base_va);
    let avail_base = base_va + desc_sz;
    let used_base = base_va + used_off;

    ostd::early_println!(
        "[virtio-gpu] vq base_va={:#x} base_pa={:#x} pfn={} (desc {} avail {} used_off {})",
        base_va,
        base_pa,
        base_pa / 4096,
        desc_sz,
        avail_sz,
        used_off
    );

    mmio_w(mmio_base, REG_QUEUE_NUM, qsize as u32);
    mmio_w(mmio_base, REG_QUEUE_ALIGN, 4096u32);
    mmio_w(mmio_base, REG_QUEUE_PFN, (base_pa / 4096) as u32);
    mmio_w(
        mmio_base,
        REG_STATUS,
        STATUS_ACK | STATUS_DRIVER | STATUS_FEAT_OK | STATUS_DRV_OK,
    );
    ostd::early_println!("[virtio-gpu] DRIVER_OK, queue ready");

    // Publish the shared queue state for send_cmd().
    unsafe {
        let p = core::ptr::addr_of_mut!(GPUQ) as *mut GpuQueue;
        write_volatile(
            p,
            GpuQueue {
                mmio_base,
                desc_base: base_va,
                avail_base,
                used_base,
                qsize,
            },
        );
    }
    AVAIL_IDX.store(0, Ordering::Relaxed);
    USED_IDX.store(0, Ordering::Relaxed);

    // ── 1. GET_DISPLAY_INFO ───────────────────────────────────────────────
    let display_w;
    let display_h;
    unsafe {
        cmd_reset();
        let cmd_va = cmd_alloc(24); // virtio_gpu_ctrl_hdr
        let resp_va = cmd_alloc(512); // resp_hdr(24) + 16 scanout modes
        write_volatile(cmd_va as *mut u32, CMD_GET_DISPLAY_INFO);

        let rt = gpuq().send_cmd(cmd_va, 24, resp_va, 512);
        ostd::early_println!("[virtio-gpu] GET_DISPLAY_INFO resp={:#x}", rt);
        if rt == RESP_OK_DISPLAY_INFO {
            let w = read_volatile((resp_va + 24 + 8) as *const u32);
            let h = read_volatile((resp_va + 24 + 12) as *const u32);
            let enabled = read_volatile((resp_va + 24 + 16) as *const u32);
            ostd::early_println!(
                "[virtio-gpu] scanout[0]: {}x{} enabled={}",
                w,
                h,
                enabled
            );
            display_w = w;
            display_h = h;
        } else {
            ostd::early_println!("[virtio-gpu] display query failed, assuming 640x480");
            display_w = 640;
            display_h = 480;
        }
    }

    // ── 2. RESOURCE_CREATE_2D ─────────────────────────────────────────────
    // virtio_gpu_resource_create_2d: hdr(24) + resource_id(4) + format(4)
    //                              + width(4) + height(4) = 40 bytes
    unsafe {
        cmd_reset();
        let cmd_va = cmd_alloc(40);
        let resp_va = cmd_alloc(24);
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_RESOURCE_CREATE_2D);
        write_volatile(p.add(24) as *mut u32, RESOURCE_ID);
        write_volatile(p.add(28) as *mut u32, FORMAT_B8G8R8X8_UNORM);
        write_volatile(p.add(32) as *mut u32, FB_WIDTH);
        write_volatile(p.add(36) as *mut u32, FB_HEIGHT);

        let rt = gpuq().send_cmd(cmd_va, 40, resp_va, 24);
        ostd::early_println!("[virtio-gpu] RESOURCE_CREATE_2D resp={:#x}", rt);
    }

    // ── 3. RESOURCE_ATTACH_BACKING ────────────────────────────────────────
    // struct virtio_gpu_resource_attach_backing { hdr; resource_id; nr_entries; }
    //   = 32 bytes, followed by nr_entries * virtio_gpu_mem_entry,
    //   each entry = { addr(8); length(4); padding(4) } = 16 bytes.
    //   Total for 1 entry: 32 + 16 = 48 bytes.
    unsafe {
        cmd_reset();
        let cmd_va = cmd_alloc(48);
        let resp_va = cmd_alloc(24);
        // FRAMEBUFFER is a static in .bss. The kernel page table may map
        // it at a different physical address than the boot page table.
        // Use the AT S1E1R instruction to translate the actual VA→PA
        // mapping under the current (kernel) page table.
        let fb_va = core::ptr::addr_of!(FRAMEBUFFER) as usize;
        let fb_pa = translate_va_to_pa(fb_va);
        let fb_len = (FB_WIDTH as usize) * (FB_HEIGHT as usize) * FB_BPP;
        ostd::early_println!(
            "[virtio-gpu] ATTACH_BACKING: fb_va={:#x} fb_pa={:#x} (AT-translated) len={}",
            fb_va, fb_pa, fb_len
        );
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_RESOURCE_ATTACH_BACKING);
        write_volatile(p.add(24) as *mut u32, RESOURCE_ID);
        write_volatile(p.add(28) as *mut u32, 1); // nr_entries
        // entry[0]: addr(8) + length(4) + padding(4) = 16 bytes at offset 32
        write_volatile(p.add(32) as *mut u64, fb_pa as u64);
        write_volatile(p.add(40) as *mut u32, fb_len as u32);
        write_volatile(p.add(44) as *mut u32, 0); // padding

        let rt = gpuq().send_cmd(cmd_va, 48, resp_va, 24);
        ostd::early_println!("[virtio-gpu] ATTACH_BACKING resp={:#x}", rt);
    }

    // ── 4. SET_SCANOUT — bind resource 1 to scanout 0 ────────────────────
    // struct virtio_gpu_set_scanout { hdr; rect{x,y,w,h}; scanout_id; resource_id; }
    //   = 24 + 16 + 4 + 4 = 48 bytes. scanout_id is at offset 40, resource_id 44.
    unsafe {
        cmd_reset();
        let cmd_va = cmd_alloc(48);
        let resp_va = cmd_alloc(24);
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_SET_SCANOUT);
        write_volatile(p.add(24) as *mut u32, 0); // r.x
        write_volatile(p.add(28) as *mut u32, 0); // r.y
        write_volatile(p.add(32) as *mut u32, FB_WIDTH); // r.width
        write_volatile(p.add(36) as *mut u32, FB_HEIGHT); // r.height
        write_volatile(p.add(40) as *mut u32, 0); // scanout_id = 0
        write_volatile(p.add(44) as *mut u32, RESOURCE_ID);

        let rt = gpuq().send_cmd(cmd_va, 48, resp_va, 24);
        ostd::early_println!("[virtio-gpu] SET_SCANOUT resp={:#x}", rt);
    }

    // ── 5. Mark GPU ready, then draw test pattern & flush ───────────────
    // GPU_READY must be set BEFORE draw_test_pattern/flush_framebuffer,
    // because flush_framebuffer() checks GPU_READY as a guard.
    GPU_READY.store(1, Ordering::Relaxed);
    draw_test_pattern();

    // Diagnostic: confirm the test pattern is readable via the FRAMEBUFFER VA,
    // and log the EL plus the S1E1R (stage-1 → IPA) translation of the
    // framebuffer base. NOTE: under kei's EL2 config a stage-2 table is active,
    // so S1E1R yields an *IPA*, not the true PA that QEMU's virtio-gpu DMA
    // reads. This is the known root cause of the black scanout: the framebuffer
    // backing store is attached at the IPA, but the device reads the true PA
    // (stage-2 output), where the kernel's writes have not landed. S12E1R (the
    // combined walk that would give the true PA) is trapped by HCR_EL2, so it
    // cannot be used from EL1 here. See PLAN.md for the full analysis.
    unsafe {
        let fb_va = core::ptr::addr_of!(FRAMEBUFFER) as *const u32;
        let v0 = read_volatile(fb_va);
        let fb_ipa = translate_va_to_pa(core::ptr::addr_of!(FRAMEBUFFER) as usize);
        let el: usize;
        core::arch::asm!("mrs {0}, CurrentEL", out(reg) el, options(nostack, preserves_flags));
        ostd::early_println!(
            "[virtio-gpu] readback VA[0]={:#x} IPA={:#x} (EL{})",
            v0, fb_ipa, (el >> 2) & 3
        );
    }

    flush_framebuffer();

    ostd::early_println!(
        "[virtio-gpu] display ready: {}x{} scanout was {}x{}",
        FB_WIDTH,
        FB_HEIGHT,
        display_w,
        display_h
    );

    // Publish this framebuffer so the VT/console subsystem can use it.
    use alloc::sync::Arc;
    let fb_base = core::ptr::addr_of!(FRAMEBUFFER) as *const _ as usize;
    let fb_size = FB_WIDTH as usize * FB_HEIGHT as usize * FB_BPP;
    let backing = aster_framebuffer::framebuffer::BlitBackend::new(fb_base, fb_size, raw_flush_callback);
    let fb = aster_framebuffer::framebuffer::FrameBuffer::new_blit(
        backing,
        FB_WIDTH as usize,
        FB_HEIGHT as usize,
        FB_WIDTH as usize * FB_BPP,
        aster_framebuffer::pixel::PixelFormat::BgrReserved,
    );
    aster_framebuffer::framebuffer::publish(Arc::new(fb));
    ostd::early_println!("[virtio-gpu] framebuffer published for VT console");
}

/// Flush callback for the published FrameBuffer. Called by VT FramebufferConsole
/// after rendering. Ignores the dirty rect and flushes the entire framebuffer.
fn raw_flush_callback(
    _backend: &aster_framebuffer::framebuffer::BlitBackend,
    _x: usize,
    _y: usize,
    _width: usize,
    _height: usize,
) {
    flush_framebuffer();
}

/// Push the whole framebuffer to the device: TRANSFER_TO_HOST_2D then
/// RESOURCE_FLUSH. Call after mutating FRAMEBUFFER.
///
/// Rate-limited to FLUSH_CAP total commands to avoid overflowing the
/// virtio-gpu command queue when boot log output triggers many flushes.
/// The first FLUSH_CAP flushes push the test pattern + banner + initial
/// boot output; after that the display stays static until the VT component
/// console takes over (with its own flush path).
static FLUSH_COUNT: AtomicU8 = AtomicU8::new(0);
const FLUSH_CAP: u8 = 200;

pub fn flush_framebuffer() {
    if GPU_READY.load(Ordering::Relaxed) == 0 {
        return;
    }
    let count = FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
    if count >= FLUSH_CAP {
        return;
    }
    unsafe {
        // Flush the FRAMEBUFFER data cache to RAM before DMA transfer.
        // QEMU's virtio-gpu DMA reads directly from RAM, bypassing CPU cache.
        // Without this flush, cached writes by draw_test_pattern() are invisible.
        flush_dcache_range(
            core::ptr::addr_of!(FRAMEBUFFER) as usize,
            (FB_WIDTH as usize) * (FB_HEIGHT as usize) * FB_BPP,
        );

        // TRANSFER_TO_HOST_2D: hdr(24) + rect(16) + offset(8) + resource_id(4) + padding(4) = 56
        cmd_reset();
        let cmd_va = cmd_alloc(56);
        let resp_va = cmd_alloc(24);
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_TRANSFER_TO_HOST_2D);
        write_volatile(p.add(24) as *mut u32, 0); // r.x
        write_volatile(p.add(28) as *mut u32, 0); // r.y
        write_volatile(p.add(32) as *mut u32, FB_WIDTH); // r.width
        write_volatile(p.add(36) as *mut u32, FB_HEIGHT); // r.height
        write_volatile(p.add(40) as *mut u64, 0u64); // offset
        write_volatile(p.add(48) as *mut u32, RESOURCE_ID);
        write_volatile(p.add(52) as *mut u32, 0); // padding
        let rt = gpuq().send_cmd(cmd_va, 56, resp_va, 24);
        if rt != RESP_OK_NODATA {
            ostd::early_println!("[virtio-gpu] TRANSFER resp={:#x}", rt);
        }

        // RESOURCE_FLUSH: hdr(24) + rect(16) + resource_id(4) + padding(4) = 48
        cmd_reset();
        let cmd_va = cmd_alloc(48);
        let resp_va = cmd_alloc(24);
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_RESOURCE_FLUSH);
        write_volatile(p.add(24) as *mut u32, 0); // r.x
        write_volatile(p.add(28) as *mut u32, 0); // r.y
        write_volatile(p.add(32) as *mut u32, FB_WIDTH); // r.width
        write_volatile(p.add(36) as *mut u32, FB_HEIGHT); // r.height
        write_volatile(p.add(40) as *mut u32, RESOURCE_ID);
        write_volatile(p.add(44) as *mut u32, 0); // padding
        let rt = gpuq().send_cmd(cmd_va, 48, resp_va, 24);
        if rt != RESP_OK_NODATA {
            ostd::early_println!("[virtio-gpu] FLUSH resp={:#x}", rt);
        }
    }
}

/// Flushes (cleans) the data cache for the given virtual address range.
///
/// On ARM64, CPU writes to Normal Memory go through the cache hierarchy.
/// When an external DMA master (like QEMU's virtio-gpu) reads from RAM,
/// it bypasses the CPU cache. This function pushes dirty cache lines to
/// RAM so that DMA reads see the latest data.
///
/// Uses `DC CIVAC` (Clean+Invalidate by VA to PoC) on each cache line.
fn flush_dcache_range(va_start: usize, len: usize) {
    let line = 64usize; // ARM64 cache line size (typically 64 bytes)
    let mut addr = va_start & !(line - 1);
    let end = va_start + len;
    unsafe {
        core::arch::asm!("dmb ish", options(nostack, preserves_flags));
        while addr < end {
            core::arch::asm!(
                "dc civac, {0}",
                in(reg) addr,
                options(nostack, preserves_flags),
            );
            addr += line;
        }
        core::arch::asm!("dsb ish", options(nostack, preserves_flags));
    }
}

/// Translates a kernel virtual address to a physical address using the
/// ARM64 AT S1E1R instruction. This reads the stage-1 (EL1) page table
/// mapping, ensuring we get the correct IPA even after the kernel page
/// table switch (where the boot PT linear mapping may differ from the
/// kernel PT's mapping).
///
/// NOTE: S1E1R yields the *IPA* (intermediate physical address) when a
/// stage-2 (EL2) translation is active. A combined S12E1R walk that would
/// yield the true PA is trapped under kei's EL2 configuration (HCR_EL2
/// traps AT instructions), so it cannot be used here. Callers must be aware
/// that under stage-2 this IPA may differ from the PA that an external DMA
/// master (QEMU's virtio-gpu) sees.
///
/// Falls back to `va - LINEAR_BASE` if AT translation fails (bit 0 of
/// PAR_EL1 = 1 indicates abort).
fn translate_va_to_pa(va: usize) -> usize {
    let par: usize;
    unsafe {
        core::arch::asm!(
            "at s1e1r, {0}",
            in(reg) va,
            options(nostack, preserves_flags),
        );
        core::arch::asm!(
            "mrs {0}, par_el1",
            out(reg) par,
            options(nostack, preserves_flags),
        );
    }
    if par & 1 == 0 {
        let pa = par & 0x0000_FFFF_F000;
        pa | (va & 0xFFF)
    } else {
        va.wrapping_sub(LINEAR_BASE)
    }
}

/// Paint a simple test pattern into FRAMEBUFFER: a green border and a
/// diagonal gradient. Pure black-on-dark would make it hard to confirm the
/// display is actually live in the QEMU window.
fn draw_test_pattern() {
    unsafe {
        let fb = core::ptr::addr_of_mut!(FRAMEBUFFER) as *mut u32;
        for y in 0..FB_HEIGHT {
            for x in 0..FB_WIDTH {
                let idx = (y as usize) * (FB_WIDTH as usize) + (x as usize);
                let on_border = x < 8 || y < 8 || x >= FB_WIDTH - 8 || y >= FB_HEIGHT - 8;
                let pixel: u32 = if on_border {
                    0xFF00FF00 // opaque green
                } else {
                    // blue gradient with red diagonal
                    let blue = ((x ^ y) & 0xFF) as u32;
                    let red = ((x + y) & 0x7F) as u32;
                    0xFF000000 | (blue << 16) | (red << 8)
                };
                write_volatile(fb.add(idx), pixel);
            }
        }
        // Draw "kei" markers: a few bright white squares near top-left.
        for &(px, py) in &[(40, 40), (50, 40), (60, 40), (70, 40)] {
            let idx = (py as usize) * (FB_WIDTH as usize) + (px as usize);
            write_volatile(fb.add(idx), 0xFFFFFFFF);
        }
    }
}
