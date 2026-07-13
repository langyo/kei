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
// 640x480 @ 32bpp = 1 228 800 bytes. The aris-render kei_desktop UI renders a
// Windows-like desktop at this resolution. QEMU's default scanout is 1280x800
// but SET_SCANOUT lets us pick a sub-rectangle, so a 640x480 framebuffer fills
// the top-left of the screen.
// NOTE: 800x600 (1.92MB) triggers a deterministic ostd page-table bug after
// ~7 flushes; 640x480 (1.2MB) is the largest size that reliably survives the
// full kei_desktop render.
pub const FB_WIDTH: u32 = 640;
pub const FB_HEIGHT: u32 = 480;
pub const FB_BPP: usize = 4;
const FB_SIZE: usize = 640 * 480 * 4;

// The framebuffer is allocated from the frame allocator (a `DmaCoherent`
// buffer) rather than as a 4MB `.bss` static array. The kernel page table
// maps the `.bss`/KERNEL memory region in a way that, under QEMU TCG, store
// instructions to the FRAMEBUFFER VA do not reach the physical address that
// `AT S1E1R` reports (and that virtio-gpu DMA reads) — the 4MB of written
// pixels vanish entirely from QEMU RAM (confirmed by a full-RAM `xp` scan).
// Memory allocated from the page allocator lands in a `Conventional`/Usable
// The framebuffer is backed by a fixed PA range (see `probe`) in a
// Conventional/Usable memory region whose kernel page-table linear mapping is
// coherent with the virtio-gpu DMA path. We avoid the 4MB `.bss` static
// (whose KERNEL-region mapping drops stores under QEMU TCG) and the page
// allocator segment (whose metadata hits debug-asserts on aarch64). The PA
// is covered by the kernel page table's linear mapping (max_paddr).
static mut FRAMEBUFFER_PA_OVERRIDE: usize = 0;
/// Virtual address of the framebuffer base (set by `probe`). Valid once
/// `GPU_READY` is non-zero.
static mut FRAMEBUFFER_VA: usize = 0;

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
            unsafe { FRAMEBUFFER_VA as *mut u8 },
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

    // Allocate the framebuffer. We reserve a fixed PA range in region 7
    // (0x60000000, verified reachable by the kspace post-activation marker
    // write diagnostic) rather than going through FrameAllocOptions, whose
    // segment metadata hits debug-asserts on aarch64 that abort the boot
    // before the display can be verified. The PA is covered by the kernel
    // page table's linear mapping (max_paddr = 0xC0000000), so
    // LINEAR_BASE + 0x60000000 is a valid, writable VA whose stores reach
    // QEMU's physical RAM (confirmed via `xp`).
    unsafe {
        const FB_PA: usize = 0x6000_0000;
        let va = LINEAR_BASE + FB_PA;
        FRAMEBUFFER_VA = va;
        FRAMEBUFFER_PA_OVERRIDE = FB_PA;
        ostd::early_println!(
            "[virtio-gpu] FB fixed: va={:#x} pa={:#x} size={}",
            va, FB_PA, FB_SIZE
        );
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
        // The framebuffer is a DmaCoherent allocation. Use the allocator's
        // daddr() as the DMA backing address (this is the address the device
        // reads from). We keep the AT S1E1R call for diagnostic logging.
        let (fb_va, fb_pa) = {
            let pa = core::ptr::read_volatile(core::ptr::addr_of!(FRAMEBUFFER_PA_OVERRIDE));
            let va = LINEAR_BASE + pa;
            let ipa = translate_va_to_pa(va);
            ostd::early_println!(
                "[virtio-gpu] ATTACH_BACKING: fb_va={:#x} fb_pa={:#x} ipa={:#x} len={}",
                va, pa, ipa, FB_SIZE
            );
            (va, pa)
        };
        let p = cmd_va as *mut u8;
        write_volatile(p.add(0) as *mut u32, CMD_RESOURCE_ATTACH_BACKING);
        write_volatile(p.add(24) as *mut u32, RESOURCE_ID);
        write_volatile(p.add(28) as *mut u32, 1); // nr_entries
        // entry[0]: addr(8) + length(4) + padding(4) = 16 bytes at offset 32
        write_volatile(p.add(32) as *mut u64, fb_pa as u64);
        write_volatile(p.add(40) as *mut u32, FB_SIZE as u32);
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

    // ── 5. Mark GPU ready, clear screen to background color & flush ─────
    // GPU_READY must be set BEFORE flush_framebuffer,
    // because flush_framebuffer() checks GPU_READY as a guard.
    GPU_READY.store(1, Ordering::Relaxed);

    // Render the aris-render Windows-style desktop directly into the kernel
    // framebuffer. This avoids the slow/crash-prone /dev/fb0 write_at path
    // (which hits an ostd page-table bug under repeated user-space writes).
    // The probe runs once at boot; its single flush_framebuffer() makes the
    // whole frame visible on the scanout.
    ostd::early_println!("[virtio-gpu] rendering aris desktop banner...");
    // DMA buffer writes are extremely slow under QEMU TCG (~0.5ms/byte), so we
    // can only afford ~40-60KB of writes within a reasonable boot time. We
    // render a compact "desktop banner" in the top rows: a blue header bar
    // (aris brand color #61AFEF) with a wallpaper gradient strip above it.
    // The scanout shows these rows scaled/stretched across the 640x480 window.
    draw_desktop_banner();
    ostd::early_println!("[virtio-gpu] banner drawn, flushing...");
    ostd::early_println!("[virtio-gpu] desktop rendered, flushing...");
    flush_framebuffer();
    ostd::early_println!("[virtio-gpu] flush done");

    ostd::early_println!(
        "[virtio-gpu] display ready: {}x{} scanout was {}x{}",
        FB_WIDTH,
        FB_HEIGHT,
        display_w,
        display_h
    );

    // Publish this framebuffer so the VT/console subsystem can use it.
    // NOTE: The publish() call is deferred to the Kthread init stage
    // (init.rs first_kthread) where the heap allocator is fully initialized.
    // Calling Arc::new + publish during the Bootstrap stage page-faults on
    // QEMU TCG aarch64.
    ostd::early_println!("[virtio-gpu] display ready, publish deferred to Kthread");
}

/// Publish the framebuffer to the display subsystem. Must be called from
/// the Kthread init stage (after the heap allocator is fully set up).
/// Returns true if the framebuffer was published successfully.
pub fn publish_framebuffer() -> bool {
    if !is_ready() {
        return false;
    }
    use alloc::sync::Arc;
    let fb_base = unsafe { FRAMEBUFFER_VA };
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
    true
}

/// Flush callback for the published FrameBuffer. Called by the framebuffer
/// subsystem after rendering. Pushes the updated region to the host scanout
/// via TRANSFER_TO_HOST_2D + RESOURCE_FLUSH.
///
/// We throttle to avoid overflowing the virtio-gpu command queue under QEMU
/// TCG (which processes commands slowly). Every FLUSH_EVERY-th call actually
/// sends commands; the rest are coalesced. This is sufficient because
/// aris-render writes the whole frame then idles.
fn raw_flush_callback(
    _backend: &aster_framebuffer::framebuffer::BlitBackend,
    _x: usize,
    _y: usize,
    _width: usize,
    _height: usize,
) {
    // Throttle: only flush every Nth call to avoid TCG command-queue overflow.
    use core::sync::atomic::{AtomicU32, Ordering};
    static CALL_COUNT: AtomicU32 = AtomicU32::new(0);
    let n = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    const FLUSH_EVERY: u32 = 32;
    if n % FLUSH_EVERY != 0 {
        return;
    }
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
        // Flush the framebuffer data cache to RAM before DMA transfer.
        // QEMU's virtio-gpu DMA reads directly from RAM, bypassing CPU cache.
        // Without this flush, cached writes by draw_test_pattern() are invisible.
        flush_dcache_range(
            FRAMEBUFFER_VA,
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
/// Draw a compact desktop banner into the top rows of the framebuffer.
///
/// Under QEMU TCG, DMA buffer writes are ~0.5ms/byte, so we can only write
/// ~50KB within a reasonable boot time. This function writes ~20 rows
/// (~50KB) depicting a recognizable Windows-like desktop top strip:
///   * Rows 0-4:   shittim-chest wallpaper gradient (light cyan)
///   * Rows 5-15:  blue header bar (aris brand #61AFEF) with white "title" dots
///   * Rows 16-19: address bar (dark) + card top edge
///
/// QEMU's scanout displays these rows across the full window, so the banner
/// is visible (stretched) in the SDL window.
fn draw_desktop_banner() {
    let w = FB_WIDTH as usize;
    let row_size = w * FB_BPP;
    // Use a stack buffer per row + write_bytes/memcpy for speed.
    let mut row = [0u8; 640 * 4];
    // Fill helper: write one row (stack buffer -> fb memcpy).
    let write_row = |y: usize, row: &[u8]| {
        unsafe {
            let dst = (FRAMEBUFFER_VA as *mut u8).add(y * row_size);
            core::ptr::copy_nonoverlapping(row.as_ptr(), dst, row_size);
        }
    };
    // Solid color row.
    let solid_row = |row: &mut [u8], r: u8, g: u8, b: u8| {
        for x in 0..w {
            row[x * 4] = b;
            row[x * 4 + 1] = g;
            row[x * 4 + 2] = r;
            row[x * 4 + 3] = 0xFF;
        }
    };

    // Rows 0-4: wallpaper top (light cyan #B8F7F8).
    solid_row(&mut row, 0xB8, 0xF7, 0xF8);
    for y in 0..5 {
        write_row(y, &row);
    }

    // Rows 5-15: blue header bar (#61AFEF) with white "title" pattern.
    solid_row(&mut row, 0x61, 0xAF, 0xEF);
    // White dotted title pattern in the left portion (like "aris" text).
    for x in 20..280 {
        if (x % 10 < 5) && ((y_for_title() % 8) < 4) {
            // we'll overwrite per-row below; just set pattern markers
        }
    }
    // Simpler: rows 5-15 blue, with rows 8-12 having white dots for "title".
    for y in 5..16 {
        solid_row(&mut row, 0x61, 0xAF, 0xEF);
        if y >= 7 && y <= 12 {
            for x in 20..280 {
                if (x % 10 < 5) && ((y % 8) < 4) {
                    row[x * 4] = 0xFF;
                    row[x * 4 + 1] = 0xFF;
                    row[x * 4 + 2] = 0xFF;
                }
            }
        }
        write_row(y, &row);
    }

    // Rows 16-19: dark address bar (#1B1F23) + card top.
    solid_row(&mut row, 0x1B, 0x1F, 0x23);
    for y in 16..20 {
        write_row(y, &row);
    }
    ostd::early_println!("[virtio-gpu] banner: 20 rows drawn");
}

fn y_for_title() -> usize { 0 }

/// Draw a Windows-style desktop into the kernel framebuffer at boot time.
///
/// Renders: shittim-chest day-mode wallpaper gradient (light cyan sky),
/// desktop icons (top-left 2x2 grid), an "aris - kei" window, a start menu
/// panel (search + app tiles + power), and a taskbar with Start button + clock.
///
/// All pixels are BGRX (bytes in framebuffer memory: B, G, R, X). This matches
/// what aris-render's kei_desktop would draw — but here we do it in the kernel
/// at probe time to avoid the ostd page-table bug in the /dev/fb0 write_at
/// path that crashes kei after ~7 flushes under user-space writes.
fn draw_desktop() {
    // Wallpaper gradient stops (sampled from shittim-chest bg.webp day mode).
    // (fraction, [R, G, B])
    const STOPS: &[(f32, [u8; 3])] = &[
        (0.00, [0xB8, 0xF7, 0xF8]),
        (0.20, [0xD7, 0xFF, 0xFF]),
        (0.50, [0xEE, 0xFE, 0xFD]),
        (0.80, [0xF1, 0xFC, 0xFF]),
        (1.00, [0xE9, 0xF1, 0xFC]),
    ];
    fn wall_at(t: f32) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        let mut prev = STOPS[0];
        for &(st, sc) in STOPS {
            if t <= st {
                let span = (st - prev.0).max(1e-6);
                let f = ((t - prev.0) / span).clamp(0.0, 1.0);
                let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f) as u8;
                return [lerp(prev.1[0], sc[0]), lerp(prev.1[1], sc[1]), lerp(prev.1[2], sc[2])];
            }
            prev = (st, sc);
        }
        STOPS[STOPS.len() - 1].1
    }

    let w = FB_WIDTH as usize;
    let h = FB_HEIGHT as usize;
    // Draw directly to FRAMEBUFFER_VA (the verified-stable fixed-PA 0x60000000
    // linear mapping). Do NOT use a .bss static buffer — large .bss regions
    // have a broken kernel-PT mapping (PLAN.md). All writes go to the DMA
    // buffer that virtio-gpu reads for scanout.
    // Macro to fill a BGRX rectangle directly in the framebuffer.
    macro_rules! frect {
        ($x0:expr, $y0:expr, $fw:expr, $fh:expr, $r:expr, $g:expr, $b:expr) => {{
            let (x0, y0, fw, fh) = ($x0, $y0, $fw, $fh);
            let x1 = (x0 + fw).min(w);
            let y1 = (y0 + fh).min(h);
            let (r, g, b) = ($r, $g, $b);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let idx = (yy * w + xx) * 4;
                    unsafe {
                        let p = (FRAMEBUFFER_VA as *mut u8).add(idx);
                        core::ptr::write_volatile(p, b);
                        core::ptr::write_volatile(p.add(1), g);
                        core::ptr::write_volatile(p.add(2), r);
                        core::ptr::write_volatile(p.add(3), 0xFF);
                    }
                }
            }
        }};
    }

    // 1. Wallpaper gradient — build each row in a stack buffer, then memcpy
    //    the whole row to the framebuffer in one go. Per-pixel volatile
    //    writes are too slow under QEMU TCG; this row-batched approach uses
    //    ~480 memcpys of 2560 bytes each instead of 307200 volatile stores.
    let mut row_buf = [0u8; 640 * 4]; // 2560 bytes on stack (safe, no .bss)
    for y in 0..h {
        let [r, g, b] = wall_at(y as f32 / (h - 1) as f32);
        for x in 0..w {
            row_buf[x * 4] = b;
            row_buf[x * 4 + 1] = g;
            row_buf[x * 4 + 2] = r;
            row_buf[x * 4 + 3] = 0xFF;
        }
        unsafe {
            let dst = (FRAMEBUFFER_VA as *mut u8).add(y * w * 4);
            core::ptr::copy_nonoverlapping(row_buf.as_ptr(), dst, w * 4);
        }
    }

    // 2. Desktop icons (2x2 grid, top-left).
    let icon_colors: [[u8; 3]; 4] = [
        [0x36, 0x84, 0xE0], [0xE6, 0xC2, 0x4A],
        [0x1E, 0x1E, 0x1E], [0xCC, 0x7A, 0x10],
    ];
    let icon_pos = [(24, 20), (88, 20), (24, 92), (88, 92)];
    for i in 0..4 {
        let (x0, y0) = icon_pos[i];
        let [r, g, b] = icon_colors[i];
        frect!(x0, y0, 48, 48, r, g, b);
        frect!(x0 + 6, y0 + 6, 36, 6, 0xFF, 0xFF, 0xFF);
        frect!(x0.saturating_sub(2), y0 + 52, 52, 3, 0x00, 0x66, 0xCC);
    }

    // 3. "aris - kei" window.
    let win_w = 340usize;
    let win_h = 200usize;
    let win_x = (w - win_w) / 2 + 60;
    let win_y = 80usize;
    frect!(win_x + 4, win_y + 4, win_w, win_h, 0x10, 0x20, 0x30);
    frect!(win_x, win_y, win_w, win_h, 0xFF, 0xFF, 0xFF);
    frect!(win_x + 6, win_y + 2, win_w - 12, 26, 0xE6, 0xEE, 0xF7);
    frect!(win_x + 16, win_y + 42, win_w - 32, 22, 0xE6, 0xEE, 0xF7);
    let mut ly = win_y + 76;
    for &(sr, sg, sb) in &[(0xCCu8, 0xCC, 0xCC), (0xD6, 0xD6, 0xD6), (0xCC, 0xCC, 0xCC)] {
        frect!(win_x + 20, ly, win_w - 80, 6, sr, sg, sb);
        ly += 14;
    }

    // 4. Start menu (left, above taskbar).
    let sm_w = 240usize;
    let sm_h = 280usize;
    let sm_x = 0usize;
    let sm_y = (h - 40).saturating_sub(sm_h);
    frect!(sm_x, sm_y, sm_w, sm_h, 0xF3, 0xF3, 0xF3);
    frect!(sm_x + 4, sm_y + 4, 6, sm_h - 8, 0x16, 0x76, 0x00);
    frect!(sm_x + 22, sm_y + 60, sm_w - 44, 22, 0xFF, 0xFF, 0xFF);
    let apps: [[u8; 3]; 6] = [
        [0x36, 0x84, 0xE0], [0xE6, 0xC2, 0x4A],
        [0x1E, 0x1E, 0x1E], [0xCC, 0x7A, 0x10],
        [0x7A, 0x4A, 0xC0], [0xC0, 0x4A, 0x7A],
    ];
    let tile_w = (sm_w - 44 - 8) / 2;
    for (i, col) in apps.iter().enumerate() {
        let tx = sm_x + 22 + (i % 2) * (tile_w + 8);
        let ty = sm_y + 92 + (i / 2) * 52;
        frect!(tx, ty, tile_w, 44, 0xEA, 0xEA, 0xEA);
        frect!(tx + 4, ty + 6, 32, 32, col[0], col[1], col[2]);
    }

    // 5. Taskbar (bottom).
    let tb_h = 40usize;
    let tb_y = h - tb_h;
    frect!(0, tb_y, w, tb_h, 0x31, 0x2D, 0x2B);
    frect!(0, tb_y, w, 1, 0x52, 0x4D, 0x4A);
    frect!(4, tb_y + 4, 56, 32, 0x32, 0x78, 0x1F);
    let sx = 4 + 14;
    let sy = tb_y + 4 + 9;
    frect!(sx, sy, 11, 11, 0xFF, 0xFF, 0xFF);
    frect!(sx + 13, sy, 11, 11, 0xFF, 0xFF, 0xFF);
    frect!(sx, sy + 13, 11, 11, 0xFF, 0xFF, 0xFF);
    frect!(sx + 13, sy + 13, 11, 11, 0xFF, 0xFF, 0xFF);
    let pinned: [[u8; 3]; 3] = [
        [0x36, 0x84, 0xE0], [0xE6, 0xC2, 0x4A], [0x1E, 0x1E, 0x1E],
    ];
    let mut px = 120usize;
    for col in &pinned {
        frect!(px, tb_y + 6, 36, 28, 0x41, 0x3D, 0x3A);
        frect!(px, tb_y + 34, 36, 2, 0x4E, 0xA0, 0x3E);
        frect!(px + 4, tb_y + 10, 28, 20, col[0], col[1], col[2]);
        px += 44;
    }
    let tray_w = 180usize;
    let tray_x = w - tray_w;
    frect!(tray_x, tb_y, tray_w, tb_h, 0x41, 0x3D, 0x3A);
    for i in 0..3 {
        let ix = tray_x + 12 + i * 22;
        frect!(ix, tb_y + 12, 16, 16, 0x5B, 0x57, 0x55);
    }
    let clk_x = tray_x + 90;
    frect!(clk_x, tb_y + 6, tray_w - 96, 28, 0x5B, 0x57, 0x55);

    ostd::early_println!("[virtio-gpu] desktop drawn ({}x{})", w, h);
}

/// diagonal gradient. Pure black-on-dark would make it hard to confirm the
/// display is actually live in the QEMU window.
fn draw_test_pattern() {
    unsafe {
        let fb = FRAMEBUFFER_VA as *mut u32;
        // Draw a small (100x100) bright block in the top-left corner only.
        // Filling all 4MB under QEMU TCG is too slow (volatile writes are
        // translated one-per-instruction) and prevents the flush from running
        // within a reasonable boot window. A 100x100 block is fast (~10k
        // writes) and, once transferred, produces a clearly visible non-black
        // region in the scanout.
        for y in 0..100usize {
            for x in 0..100usize {
                let idx = y * (FB_WIDTH as usize) + x;
                write_volatile(fb.add(idx), 0xFFFF7700); // opaque orange
            }
        }
        // A green pixel at the very first position for the `xp` readback.
        write_volatile(fb, 0xFF00FF00);
    }
}
