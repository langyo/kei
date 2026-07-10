// SPDX-License-Identifier: MPL-2.0

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use ostd::{
    Error, Result,
    boot::boot_info,
    io::IoMem,
    mm::{CachePolicy, HasPaddr, HasSize, VmIo},
    sync::{Mutex, SpinLock},
};

use crate::pixel::{Pixel, PixelFormat, RenderedPixel};

/// Maximum number of colormap entries (standard 8-bit palette)
pub const MAX_CMAP_SIZE: usize = 256;

/// The backing storage for a framebuffer's pixel data.
///
/// A framebuffer is either backed by a memory-mapped I/O region (the classic
/// EFI/VBE linear framebuffer exposed by firmware at a fixed physical address)
/// or by an in-kernel DMA buffer that must be blitted to a host resource on
/// every flush (the virtio-gpu 2D scanout model).
#[derive(Debug)]
pub enum FrameBufferBackend {
    /// A directly-mapped linear framebuffer (EFI/VBE/Bochs VGA).
    /// Writes are immediately visible on screen; no flush is needed.
    Mmio(IoMem),
    /// A guest-allocated DMA buffer that must be blitted to a host-side
    /// scanout resource via a flush callback. Used by virtio-gpu.
    Blit(BlitBackend),
}

/// The blit-flush callback signature. Given a dirty rect (in pixels), the
/// driver pushes the corresponding region of the backing buffer to the host
/// scanout resource. Registered by the virtio-gpu driver at probe time.
pub type FlushCallback = fn(&BlitBackend, x: usize, y: usize, width: usize, height: usize);

/// The in-kernel backing for a blit-based framebuffer.
///
/// Holds a pointer to the DMA buffer's pixel data and the driver-specific
/// flush routine. The flush callback is a plain `fn` (not a closure) to keep
/// the struct `Debug`-derivable and avoid lifetime/HRTB complexity; the
/// virtio-gpu driver stores its device state in a static and the callback
/// recovers it from there.
///
/// Raw pointer I/O is confined to a `#[allow(unsafe_code)]` module because
/// accessing a DMA backing buffer is inherently unsafe (the pointer is
/// obtained from the virtio-gpu driver and outlives this struct). The rest
/// of this crate remains `#![deny(unsafe_code)]`.
#[derive(Debug)]
pub struct BlitBackend {
    /// Virtual address of the pixel backing buffer (coherent DMA region).
    /// The buffer is `width * height * bytes_per_pixel` bytes, tightly packed
    /// (`line_size == width * bpp/8`).
    pub base: usize,
    /// Total size of the backing buffer in bytes.
    pub size: usize,
    /// The driver-provided flush routine. `None` means flushing is a no-op
    /// (e.g. during early construction before the driver wires it up).
    pub flush: Option<FlushCallback>,
    /// Marker so the struct can derive Debug despite containing a raw pointer
    /// equivalent (the `usize` base). No actual marker field is needed.
    _phantom: (),
}

impl BlitBackend {
    /// Creates a new blit backend over a coherent DMA pixel buffer.
    ///
    /// `base` is the kernel-virtual address of the buffer, `size` its
    /// byte length. `flush` is called by `FrameBuffer::flush` to push
    /// pixels to the host scanout.
    pub fn new(base: usize, size: usize, flush: FlushCallback) -> Self {
        Self {
            base,
            size,
            flush: Some(flush),
            _phantom: (),
        }
    }

    /// Returns the kernel-virtual address of the DMA pixel buffer.
    pub fn base_va(&self) -> usize {
        self.base
    }
}

/// Unsafe I/O helpers for [`BlitBackend`], isolated so the rest of the crate
/// can keep `#![deny(unsafe_code)]`.
#[allow(unsafe_code)]
mod unsafe_io {
    use super::BlitBackend;

    impl BlitBackend {
        /// Reads `bytes.len()` bytes at the given byte offset from the backing
        /// buffer.
        pub(crate) fn read_bytes(&self, offset: usize, bytes: &mut [u8]) {
            debug_assert!(offset + bytes.len() <= self.size);
            // SAFETY: `base` points to a valid coherent DMA buffer of `size`
            // bytes that remains valid for the lifetime of this backend. The
            // caller guarantees `offset + len <= size`.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (self.base as *const u8).add(offset),
                    bytes.as_mut_ptr(),
                    bytes.len(),
                );
            }
        }

        /// Writes `bytes` at the given byte offset into the backing buffer.
        pub(crate) fn write_bytes(&self, offset: usize, bytes: &[u8]) {
            debug_assert!(offset + bytes.len() <= self.size);
            // SAFETY: as above; the caller ensures bounds.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    (self.base as *mut u8).add(offset),
                    bytes.len(),
                );
            }
        }
    }
}

/// The framebuffer used for text or graphical output.
///
/// # Notes
///
/// It is highly recommended to use a synchronization primitive, such as a `SpinLock`, to
/// lock the framebuffer before performing any operation on it.
/// Failing to properly synchronize access can result in corrupted framebuffer content
/// or unspecified behavior during rendering.
#[derive(Debug)]
pub struct FrameBuffer {
    backend: FrameBufferBackend,
    width: usize,
    height: usize,
    line_size: usize,
    pixel_format: PixelFormat,
    cmap: Mutex<FbCmap>,
}

/// A single entry in the color map with 16-bit color values.
///
/// Linux framebuffer colormap uses 16-bit values (0-65535) for each color channel
/// to support high precision color mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorMapEntry {
    /// Red color value (16-bit)
    pub red: u16,
    /// Green color value (16-bit)
    pub green: u16,
    /// Blue color value (16-bit)
    pub blue: u16,
    /// Transparency value (16-bit)
    pub transp: u16,
}

/// Internal framebuffer colormap structure.
#[derive(Clone, Debug)]
struct FbCmap {
    /// Color map entries
    entries: Vec<ColorMapEntry>,
}

/// The process-wide framebuffer singleton.
///
/// This holds an `Option<Arc<FrameBuffer>>` rather than a `Once` so that a
/// late-arriving display device (e.g. virtio-gpu, which is probed during the
/// Kthread component stage, long after early boot) can publish a framebuffer
/// via [`publish`]. The x86 EFI/VBE path populates it during the Bootstrap
/// component stage via [`init`].
static FRAMEBUFFER: SpinLock<Option<Arc<FrameBuffer>>> = SpinLock::new(None);

/// Guards against a second `publish` overwriting an already-installed framebuffer.
static PUBLISHED: AtomicBool = AtomicBool::new(false);

/// Returns a clone of the process-wide framebuffer, if one has been installed.
///
/// Replaces the old `FRAMEBUFFER.get()` (which returned `Option<&Arc<_>>`).
/// Callers that previously wrote `FRAMEBUFFER.get().clone()` should call this
/// directly.
pub fn get() -> Option<Arc<FrameBuffer>> {
    FRAMEBUFFER.lock().clone()
}

/// Installs a framebuffer published by a late-arriving display device.
///
/// Called by the virtio-gpu driver once it has attached a 2D scanout resource.
/// The first caller wins; subsequent calls are logged and ignored to match the
/// old `Once::call_once` semantics. The framebuffer is cleared before install.
pub fn publish(fb: Arc<FrameBuffer>) {
    if PUBLISHED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        ostd::warn!("FrameBuffer already published, ignoring late publish");
        return;
    }
    fb.clear();
    *FRAMEBUFFER.lock() = Some(fb);
}

/// The boot-time init path: reads the bootloader-provided framebuffer arg and
/// installs a `Mmio`-backed framebuffer. No-op on architectures where the
/// bootloader provides no framebuffer (e.g. aarch64 before virtio-gpu).
pub(crate) fn init() {
    let Some(framebuffer_arg) = boot_info().framebuffer_arg else {
        ostd::warn!("Framebuffer not found");
        return;
    };

    if framebuffer_arg.address == 0 {
        ostd::error!("Framebuffer address is zero");
        return;
    }

    // FIXME: There are several pixel formats that have the same BPP. We lost the information
    // during the boot phase, so here we guess the pixel format on a best effort basis.
    let pixel_format = match framebuffer_arg.bpp {
        8 => PixelFormat::Grayscale8,
        16 => PixelFormat::Rgb565,
        24 => PixelFormat::Rgb888,
        32 => PixelFormat::BgrReserved,
        _ => {
            ostd::error!(
                "Unsupported framebuffer pixel format: {} bpp",
                framebuffer_arg.bpp
            );
            return;
        }
    };

    let line_size = framebuffer_arg
        .width
        .checked_mul(pixel_format.nbytes())
        .unwrap();
    let fb_size = framebuffer_arg.height.checked_mul(line_size).unwrap();

    let fb_base = framebuffer_arg.address;
    // Use write-combining for framebuffer to enable faster write operations.
    // Write-combining allows the CPU to combine multiple writes into fewer bus transactions,
    // which is ideal for framebuffer access patterns (sequential writes).
    let io_mem = IoMem::acquire_with_cache_policy(
        fb_base..fb_base.checked_add(fb_size).unwrap(),
        CachePolicy::WriteCombining,
    )
    .unwrap();

    let framebuffer = FrameBuffer::new_mmio(
        io_mem,
        framebuffer_arg.width,
        framebuffer_arg.height,
        line_size,
        pixel_format,
    );

    publish(Arc::new(framebuffer));
}

impl FrameBuffer {
    /// Creates an Mmio-backed framebuffer (the classic EFI/VBE linear FB).
    ///
    /// `line_size` is the byte stride between rows (may exceed
    /// `width * bpp/8` for aligned framebuffers).
    pub fn new_mmio(
        io_mem: IoMem,
        width: usize,
        height: usize,
        line_size: usize,
        pixel_format: PixelFormat,
    ) -> Self {
        Self {
            backend: FrameBufferBackend::Mmio(io_mem),
            width,
            height,
            line_size,
            pixel_format,
            cmap: Mutex::new(FbCmap {
                entries: Vec::new(),
            }),
        }
    }

    /// Creates a blit-backed framebuffer over a guest DMA pixel buffer.
    ///
    /// Used by virtio-gpu: the driver allocates a coherent DMA region for the
    /// pixel data and provides a flush callback that issues
    /// `TRANSFER_TO_HOST_2D` to push dirty regions to the host scanout.
    /// `line_size` is the byte stride (typically `width * 4` for XRGB8888).
    pub fn new_blit(
        backing: BlitBackend,
        width: usize,
        height: usize,
        line_size: usize,
        pixel_format: PixelFormat,
    ) -> Self {
        Self {
            backend: FrameBufferBackend::Blit(backing),
            width,
            height,
            line_size,
            pixel_format,
            cmap: Mutex::new(FbCmap {
                entries: Vec::new(),
            }),
        }
    }

    /// Returns the width of the framebuffer in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the framebuffer in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Returns the line size (byte stride) of the framebuffer.
    pub fn line_size(&self) -> usize {
        self.line_size
    }

    /// Returns the backing buffer size in bytes.
    ///
    /// For Mmio backends this is the IoMem size; for Blit backends the DMA
    /// buffer size.
    pub fn buffer_size(&self) -> usize {
        match &self.backend {
            FrameBufferBackend::Mmio(io) => io.size(),
            FrameBufferBackend::Blit(b) => b.size,
        }
    }

    /// Returns the physical base address of the backing store, if any.
    ///
    /// For Mmio backends this is the IoMem physical address. For Blit
    /// backends, this computes the physical address from the kernel-virtual
    /// address by subtracting the linear-mapping base (0xffff_8000_0000_0000
    /// on aarch64). Returns `None` if the VA is not in the linear range.
    pub fn physical_address(&self) -> Option<usize> {
        match &self.backend {
            FrameBufferBackend::Mmio(io) => Some(io.paddr()),
            FrameBufferBackend::Blit(b) => {
                // The DMA buffer is a kernel static (in .bss) mapped via
                // the boot page table's linear mapping. Convert VA → PA.
                const LINEAR_BASE: usize = 0xffff_8000_0000_0000;
                let va = b.base_va();
                if va >= LINEAR_BASE {
                    Some(va - LINEAR_BASE)
                } else {
                    None
                }
            }
        }
    }

    /// Returns a reference to the `IoMem` of an Mmio-backed framebuffer.
    ///
    /// Returns `None` for Blit-backed framebuffers. Used by `/dev/fb0` mmap.
    pub fn io_mem(&self) -> Option<&IoMem> {
        match &self.backend {
            FrameBufferBackend::Mmio(io) => Some(io),
            FrameBufferBackend::Blit(_) => None,
        }
    }

    /// Returns whether this framebuffer is backed by a directly-mapped MMIO
    /// region (vs. a blit-based DMA buffer).
    pub fn is_mmio(&self) -> bool {
        matches!(self.backend, FrameBufferBackend::Mmio(_))
    }

    /// Returns the pixel format of the framebuffer.
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Renders the pixel according to the pixel format of the framebuffer.
    pub fn render_pixel(&self, pixel: Pixel) -> RenderedPixel {
        pixel.render(self.pixel_format)
    }

    /// Calculates the offset of a pixel at the specified position.
    pub fn calc_offset(&self, x: usize, y: usize) -> PixelOffset<'_> {
        PixelOffset {
            fb: self,
            offset: (x * self.pixel_format.nbytes() + y * self.line_size) as isize,
        }
    }

    /// Writes a pixel at the specified position.
    pub fn write_pixel_at(&self, offset: PixelOffset, pixel: RenderedPixel) -> Result<()> {
        self.write_bytes_at(offset.as_usize(), pixel.as_slice())
    }

    /// Writes raw bytes at the specified byte offset.
    ///
    /// For Mmio backends writes go directly to the linear framebuffer; for
    /// Blit backends writes go to the guest DMA buffer (a subsequent
    /// [`flush`] pushes them to the host scanout).
    pub fn write_bytes_at(&self, offset: usize, bytes: &[u8]) -> Result<()> {
        match &self.backend {
            FrameBufferBackend::Mmio(io) => io.write_bytes(offset, bytes),
            FrameBufferBackend::Blit(b) => {
                b.write_bytes(offset, bytes);
                Ok(())
            }
        }
    }

    /// Reads raw bytes at the specified byte offset.
    pub fn read_bytes_at(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        match &self.backend {
            FrameBufferBackend::Mmio(io) => io.read_bytes(offset, buf),
            FrameBufferBackend::Blit(b) => {
                b.read_bytes(offset, buf);
                Ok(())
            }
        }
    }

    /// Flushes a dirty region to the host scanout, for Blit-backed framebuffers.
    ///
    /// No-op for Mmio-backed framebuffers (writes are already on screen).
    /// Callers should invoke this after modifying pixels via
    /// [`write_bytes_at`] / [`write_pixel_at`] to make the changes visible.
    pub fn flush(&self, x: usize, y: usize, width: usize, height: usize) {
        if let FrameBufferBackend::Blit(b) = &self.backend {
            if let Some(flush) = b.flush {
                flush(b, x, y, width, height);
            }
        }
    }

    /// Flushes the entire framebuffer.
    pub fn flush_all(&self) {
        self.flush(0, 0, self.width, self.height);
    }

    /// Clears the framebuffer with default color (black).
    pub fn clear(&self) {
        let size = self.buffer_size();
        // Allocate once; for large framebuffers this is a single memset.
        let frame = alloc::vec![0u8; size];
        self.write_bytes_at(0, &frame).unwrap();
        self.flush_all();
    }

    /// Sets color map entries starting from the given index.
    ///
    /// For efifb devices, hardware color map is not supported, so we maintain
    /// an in-memory map for software emulation.
    pub fn set_color_map(&self, start: usize, entries: &[ColorMapEntry]) -> Result<()> {
        if start > MAX_CMAP_SIZE || entries.len() > MAX_CMAP_SIZE - start {
            return Err(Error::InvalidArgs);
        }

        let mut cmap = self.cmap.lock();
        let required_len = start + entries.len();

        // Ensure the colormap has enough space
        if cmap.entries.len() < required_len {
            cmap.entries.resize(
                required_len,
                ColorMapEntry {
                    red: 0,
                    green: 0,
                    blue: 0,
                    transp: 0,
                },
            );
        }

        // Copy the entries
        cmap.entries[start..start + entries.len()].copy_from_slice(entries);

        Ok(())
    }

    /// Gets color map entries from the given range.
    pub fn get_color_map(&self, start: usize, len: usize) -> Option<Vec<ColorMapEntry>> {
        let cmap = self.cmap.lock();

        if start >= cmap.entries.len() || len > cmap.entries.len() - start {
            return None;
        }

        Some(cmap.entries[start..start + len].to_vec())
    }
}

/// The offset of a pixel in the framebuffer.
#[derive(Clone, Copy, Debug)]
pub struct PixelOffset<'a> {
    fb: &'a FrameBuffer,
    offset: isize,
}

impl PixelOffset<'_> {
    /// Adds the specified delta to the x coordinate.
    pub fn x_add(&mut self, x_delta: isize) {
        let delta = x_delta * self.fb.pixel_format.nbytes() as isize;
        self.offset += delta;
    }

    /// Adds the specified delta to the y coordinate.
    pub fn y_add(&mut self, y_delta: isize) {
        let delta = y_delta * self.fb.line_size as isize;
        self.offset += delta;
    }

    /// Returns the offset value as a `usize`.
    pub fn as_usize(&self) -> usize {
        self.offset as usize
    }
}
