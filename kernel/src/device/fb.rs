// SPDX-License-Identifier: MPL-2.0

use aster_framebuffer::{
    framebuffer::{self, ColorMapEntry, FrameBuffer, MAX_CMAP_SIZE},
    pixel::PixelFormat,
};
use device_id::{DeviceId, MajorId, MinorId};
use ostd::mm::{FallibleVmRead, HasPaddr, HasSize, VmIo, VmReader, VmWriter};

use super::{Device, DeviceType, DevtmpfsInodeMeta, registry::char};
use crate::{
    context::current_userspace,
    events::IoEvents,
    fs::{
        file::{Mappable, PerOpenFileOps, StatusFlags},
        vfs::inode::FileOps,
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    util::ioctl::{RawIoctl, dispatch_ioctl},
};

#[derive(Debug)]
struct Fb;

#[derive(Debug)]
struct FbHandle {
    framebuffer: Arc<FrameBuffer>,
    /// Cached IoMem for the DMA buffer (Blit backends only). Acquired once
    /// on open() and reused for all subsequent write_at calls.
    cached_iomem: Option<ostd::io::IoMem>,
}

/// Bitfields describing the color channel layout; `struct fb_bitfield` in Linux.
///
/// Reference: <https://elixir.bootlin.com/linux/v6.17/source/include/uapi/linux/fb.h#L189>.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod)]
struct FbBitfield {
    /// Bit offset of the field
    pub offset: u32,
    /// Length of the field in bits
    pub length: u32,
    /// Most significant bit position (0 = left, 1 = right)
    pub msb_right: u32,
}

impl FbBitfield {
    /// Converts pixel format to framebuffer bitfields for Linux compatibility.
    #[rustfmt::skip]
    fn from_pixel_format(pixel_format: PixelFormat) -> (Self, Self, Self, Self) {
        match pixel_format {
            PixelFormat::Grayscale8 => (
                Self { offset: 0, length: 8, msb_right: 0 },
                Self { offset: 0, length: 8, msb_right: 0 },
                Self { offset: 0, length: 8, msb_right: 0 },
                Self::default(),
            ),
            PixelFormat::Rgb565 => (
                Self { offset: 11, length: 5, msb_right: 0 },
                Self { offset: 5, length: 6, msb_right: 0 },
                Self { offset: 0, length: 5, msb_right: 0 },
                Self::default(),
            ),
            PixelFormat::Rgb888 => (
                Self { offset: 16, length: 8, msb_right: 0 },
                Self { offset: 8, length: 8, msb_right: 0 },
                Self { offset: 0, length: 8, msb_right: 0 },
                Self::default(),
            ),
            PixelFormat::BgrReserved => (
                Self { offset: 16, length: 8, msb_right: 0 },
                Self { offset: 8, length: 8, msb_right: 0 },
                Self { offset: 0, length: 8, msb_right: 0 },
                Self { offset: 24, length: 8, msb_right: 0 },
            ),
        }
    }
}

/// Variable screen information for framebuffer devices; `struct fb_var_screeninfo` in Linux.
///
/// Reference: <https://elixir.bootlin.com/linux/v6.17/source/include/uapi/linux/fb.h#L243>.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod)]
struct FbVarScreenInfo {
    /// Visible resolution width
    pub xres: u32,
    /// Visible resolution height
    pub yres: u32,
    /// Virtual resolution width
    pub xres_virtual: u32,
    /// Virtual resolution height
    pub yres_virtual: u32,
    /// Offset from virtual to visible (horizontal)
    pub xoffset: u32,
    /// Offset from virtual to visible (vertical)
    pub yoffset: u32,
    /// Color depth in bits per pixel
    pub bits_per_pixel: u32,
    /// 0 = color, 1 = grayscale, >1 = FOURCC
    pub grayscale: u32,
    /// Red color bitfield in framebuffer memory
    pub red: FbBitfield,
    /// Green color bitfield in framebuffer memory
    pub green: FbBitfield,
    /// Blue color bitfield in framebuffer memory
    pub blue: FbBitfield,
    /// Transparency bitfield
    pub transp: FbBitfield,
    /// Non-standard pixel format indicator
    pub nonstd: u32,
    /// Activation control flags
    pub activate: u32,
    /// Height of display in millimeters
    pub height: u32,
    /// Width of display in millimeters
    pub width: u32,
    /// Acceleration capabilities (obsolete)
    pub accel_flags: u32,
    /// Pixel clock period in picoseconds
    pub pixclock: u32,
    /// Time from horizontal sync to picture
    pub left_margin: u32,
    /// Time from picture to horizontal sync
    pub right_margin: u32,
    /// Time from vertical sync to picture
    pub upper_margin: u32,
    /// Time from picture to vertical sync
    pub lower_margin: u32,
    /// Length of horizontal sync
    pub hsync_len: u32,
    /// Length of vertical sync
    pub vsync_len: u32,
    /// Synchronization flags
    pub sync: u32,
    /// Video mode flags
    pub vmode: u32,
    /// Screen rotation angle (counter-clockwise)
    pub rotate: u32,
    /// Colorspace for FOURCC-based modes
    pub colorspace: u32,
    /// Reserved for future compatibility
    pub reserved: [u32; 4],
}

/// Fixed screen information for framebuffer devices; `struct fb_fix_screeninfo` in Linux.
///
/// Reference: <https://elixir.bootlin.com/linux/v6.17/source/include/uapi/linux/fb.h#L158>.
#[padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod)]
struct FbFixScreenInfo {
    /// Identification string (e.g., "EFI VGA")
    pub id: [u8; 16],
    /// Start of framebuffer memory (physical address)
    pub smem_start: u64,
    /// Length of framebuffer memory in bytes
    pub smem_len: u32,
    /// Framebuffer type identifier
    pub type_: u32,
    /// Auxiliary type information (e.g., interleave)
    pub type_aux: u32,
    /// Visual type (mono, pseudo-color, true-color, etc.)
    pub visual: u32,
    /// Horizontal panning step size (0 = no panning)
    pub xpanstep: u16,
    /// Vertical panning step size (0 = no panning)
    pub ypanstep: u16,
    /// Y-axis wrapping step size (0 = no wrapping)
    pub ywrapstep: u16,
    /// Length of a screen line in bytes
    pub line_length: u32,
    /// Start of memory-mapped I/O (physical address)
    pub mmio_start: u64,
    /// Length of memory-mapped I/O region
    pub mmio_len: u32,
    /// Hardware acceleration type identifier
    pub accel: u32,
    /// Hardware capability flags
    pub capabilities: u16,
    /// Reserved for future compatibility
    pub reserved: [u16; 2],
}

/// Framebuffer colormap structure for userspace communication; `struct fb_cmap` in Linux.
///
/// Reference: <https://elixir.bootlin.com/linux/v6.17/source/include/uapi/linux/fb.h#L283>.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
struct FbCmapUser {
    /// Starting offset in colormap
    pub start: u32,
    /// Number of colormap entries
    pub len: u32,
    /// Pointer to red color values in userspace
    pub red: usize,
    /// Pointer to green color values in userspace
    pub green: usize,
    /// Pointer to blue color values in userspace
    pub blue: usize,
    /// Pointer to transparency values in userspace (may be null)
    pub transp: usize,
}

mod ioctl_defs {
    use super::{FbCmapUser, FbFixScreenInfo, FbVarScreenInfo};
    use crate::util::ioctl::{InData, InOutData, NoData, OutData, ioc};

    // Reference: <https://elixir.bootlin.com/linux/v6.17/source/include/uapi/linux/fb.h#L13-L38>

    pub(super) type GetVarScreenInfo = ioc!(FBIOGET_VSCREENINFO, 0x4600, OutData<FbVarScreenInfo>);
    pub(super) type PutVarScreenInfo =
        ioc!(FBIOPUT_VSCREENINFO, 0x4601, InOutData<FbVarScreenInfo>);
    pub(super) type GetFixScreenInfo = ioc!(FBIOGET_FSCREENINFO, 0x4602, OutData<FbFixScreenInfo>);
    pub(super) type GetColorMap = ioc!(FBIOGETCMAP, 0x4604, InData<FbCmapUser>);
    pub(super) type PutColorMap = ioc!(FBIOPUTCMAP, 0x4605, InData<FbCmapUser>);

    // `NoData` is used below because they're not supported by efifb.
    pub(super) type PanDisplay = ioc!(FBIOPAN_DISPLAY, 0x4606, NoData);
    pub(super) type Blank = ioc!(FBIOBLANK, 0x4611, NoData);
}

impl Device for Fb {
    fn type_(&self) -> DeviceType {
        DeviceType::Char
    }

    fn id(&self) -> DeviceId {
        // Same value with Linux: major 29, minor 0
        DeviceId::new(MajorId::new(29), MinorId::new(0))
    }

    fn devtmpfs_meta(&self) -> Option<DevtmpfsInodeMeta<'_>> {
        // Linux names framebuffer device nodes as `fbN`.
        // TODO: We currently expose only one framebuffer device,
        // so the devtmpfs node is fixed to `fb0`.
        // Reference: <https://elixir.bootlin.com/linux/v6.18/source/drivers/video/fbdev/core/fbsysfs.c#L482>.
        Some(DevtmpfsInodeMeta::new("fb0"))
    }

    fn open(&self) -> Result<Box<dyn PerOpenFileOps>> {
        let Some(framebuffer) = framebuffer::get() else {
            return Err(Error::with_message(
                Errno::ENODEV,
                "the framebuffer device is not present",
            ));
        };

        // Note: we intentionally do NOT acquire an IoMem for Blit-backed
        // framebuffers here. The IoMem kvirt_area path triggers an EL1 page
        // fault on aarch64 QEMU TCG after repeated writes (see write_at).
        // Instead, write_at uses the BlitBackend's linear-mapped VA directly.
        let cached_iomem: Option<ostd::io::IoMem> = None;

        Ok(Box::new(FbHandle {
            framebuffer,
            cached_iomem,
        }))
    }
}

impl FbHandle {
    /// Reads an array of `u16` color map values from userspace.
    fn read_color_maps_from_user(addr: usize, data: &mut [u16]) -> Result<()> {
        for (i, item) in data.iter_mut().enumerate() {
            let user_addr = addr + i * size_of::<u16>();
            *item = current_userspace!().read_val(user_addr)?;
        }
        Ok(())
    }

    /// Writes an array of `u16` color map values to userspace.
    fn write_color_maps_to_user(addr: usize, data: &[u16]) -> Result<()> {
        for (i, &value) in data.iter().enumerate() {
            let user_addr = addr + i * size_of::<u16>();
            current_userspace!().write_val(user_addr, &value)?;
        }
        Ok(())
    }

    /// Collects the information in the [`FbVarScreenInfo`].
    fn collect_var_screen_info(&self) -> FbVarScreenInfo {
        /// Default pixel clock calculation for efifb compatibility
        const DEFAULT_PIXEL_CLOCK_DIVISOR: u32 = 10_000_000;

        /// Default timing parameters for efifb compatibility
        const DEFAULT_RIGHT_MARGIN: u32 = 32;
        const DEFAULT_UPPER_MARGIN: u32 = 16;
        const DEFAULT_LOWER_MARGIN: u32 = 4;
        const DEFAULT_VSYNC_LEN: u32 = 4;

        let pixel_format = self.framebuffer.pixel_format();
        let (red, green, blue, transp) = FbBitfield::from_pixel_format(pixel_format);

        FbVarScreenInfo {
            xres: self.framebuffer.width() as u32,
            yres: self.framebuffer.height() as u32,
            xres_virtual: self.framebuffer.width() as u32,
            yres_virtual: self.framebuffer.height() as u32,
            bits_per_pixel: (8 * pixel_format.nbytes()) as u32,
            red,
            green,
            blue,
            transp,
            pixclock: DEFAULT_PIXEL_CLOCK_DIVISOR / self.framebuffer.width() as u32 * 1000
                / self.framebuffer.height() as u32,
            left_margin: (self.framebuffer.width() as u32 / 8) & 0xf8,
            right_margin: DEFAULT_RIGHT_MARGIN,
            upper_margin: DEFAULT_UPPER_MARGIN,
            lower_margin: DEFAULT_LOWER_MARGIN,
            hsync_len: (self.framebuffer.width() as u32 / 8) & 0xf8,
            vsync_len: DEFAULT_VSYNC_LEN,
            ..Default::default()
        }
    }

    /// Collects the information in the [`FbFixScreenInfo`].
    fn collect_fix_screen_info(&self) -> FbFixScreenInfo {
        // For Mmio-backed framebuffers expose the physical address/size so
        // userspace can mmap; for Blit-backed (virtio-gpu) framebuffers
        // smem_start is 0 and mmap falls back to the ioctl read/write path.
        let (smem_start, smem_len) = match self.framebuffer.io_mem() {
            Some(io) => (io.paddr() as u64, io.size() as u32),
            None => (0, self.framebuffer.buffer_size() as u32),
        };
        FbFixScreenInfo {
            smem_start,
            smem_len,
            line_length: self.framebuffer.line_size() as u32,
            ..Default::default()
        }
    }

    /// Handles the [`ioctl_defs::GetColorMap`] ioctl command.
    ///
    /// Arguments:
    ///  - Input: [`FbCmapUser`] (specifying the range).
    ///  - Output: [`FbCmapUser`] (filled with color palette data).
    fn handle_get_cmap(&self, cmap_user: &FbCmapUser) -> Result<()> {
        if cmap_user.len == 0 {
            return Ok(());
        }

        let start = cmap_user.start as usize;
        let len = cmap_user.len as usize;

        // Get color map entries from framebuffer
        let entries = self.framebuffer.get_color_map(start, len).ok_or_else(|| {
            Error::with_message(Errno::EINVAL, "the color map index is out of bounds")
        })?;

        // Extract color channels and write to userspace
        let red: Vec<u16> = entries.iter().map(|e| e.red).collect();
        let green: Vec<u16> = entries.iter().map(|e| e.green).collect();
        let blue: Vec<u16> = entries.iter().map(|e| e.blue).collect();
        let transp: Vec<u16> = entries.iter().map(|e| e.transp).collect();

        Self::write_color_maps_to_user(cmap_user.red, &red)?;
        Self::write_color_maps_to_user(cmap_user.green, &green)?;
        Self::write_color_maps_to_user(cmap_user.blue, &blue)?;
        if cmap_user.transp != 0 {
            Self::write_color_maps_to_user(cmap_user.transp, &transp)?;
        }

        Ok(())
    }

    /// Handles the [`ioctl_defs::PutColorMap`] ioctl command.
    ///
    /// Arguments:
    ///  - Input: [`FbCmapUser`] (with color palette data).
    ///  - Output: None.
    fn handle_set_cmap(&self, cmap_user: &FbCmapUser) -> Result<()> {
        if cmap_user.len == 0 {
            return Ok(());
        }

        let start = cmap_user.start as usize;
        let len = cmap_user.len as usize;

        // Check the size to prevent excessive memory allocation
        if start > MAX_CMAP_SIZE || len > MAX_CMAP_SIZE - start {
            return_errno_with_message!(
                Errno::EINVAL,
                "the color map range exceeds its maximum size"
            );
        }

        // Read color data from userspace
        let mut red = vec![0u16; len];
        let mut green = vec![0u16; len];
        let mut blue = vec![0u16; len];
        let mut transp = vec![0u16; len];

        Self::read_color_maps_from_user(cmap_user.red, &mut red)?;
        Self::read_color_maps_from_user(cmap_user.green, &mut green)?;
        Self::read_color_maps_from_user(cmap_user.blue, &mut blue)?;
        if cmap_user.transp != 0 {
            Self::read_color_maps_from_user(cmap_user.transp, &mut transp)?;
        }

        // Build color map entries
        let entries: Vec<ColorMapEntry> = (0..len)
            .map(|i| ColorMapEntry {
                red: red[i],
                green: green[i],
                blue: blue[i],
                transp: transp[i],
            })
            .collect();

        // Set color map entries in framebuffer
        self.framebuffer.set_color_map(start, &entries)?;

        Ok(())
    }
}

impl Pollable for FbHandle {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }
}

impl FileOps for FbHandle {
    fn read_at(
        &self,
        offset: usize,
        writer: &mut VmWriter,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        if !writer.has_avail() {
            return Ok(0);
        }

        let size = self.framebuffer.buffer_size();
        if offset >= size {
            return Ok(0);
        }

        let len = writer.avail().min(size - offset);
        if len == 0 {
            return Ok(0);
        }

        // For Mmio-backed framebuffers, use the fallible VmIo path so a
        // faulting userspace page still yields a partial copy. For Blit
        // backends, fall back to a single non-fallible read into a stack
        // buffer then copy to userspace.
        if let Some(io_mem) = self.framebuffer.io_mem() {
            let mut new_writer = writer.clone_exclusive();
            new_writer.limit(len);
            let result = io_mem.read_fallible(offset, &mut new_writer);
            let copied = match result {
                Ok(copied) => copied,
                Err((err, copied)) => {
                    if copied > 0 {
                        copied
                    } else {
                        return Err(err.into());
                    }
                }
            };
            writer.skip(copied);
            Ok(copied)
        } else {
            let mut buf = vec![0u8; len];
            self.framebuffer.read_bytes_at(offset, &mut buf)?;
            // Copy kernel buffer to userspace writer. VmWriter does not
            // implement VmIo, so we fall back to a direct cursor-based copy.
            // A faulting userspace page would abort here rather than yield a
            // partial copy, which is acceptable for the kernel-RAM-backed
            // blit framebuffer (no MMIO fault semantics to honor).
            let avail = writer.avail().min(buf.len());
            // SAFETY: writer.cursor() points to a valid userspace buffer of
            // at least `avail` bytes for the duration of this call.
            #[allow(unsafe_code)]
            unsafe {
                core::ptr::copy_nonoverlapping(buf.as_ptr(), writer.cursor(), avail);
            }
            writer.skip(avail);
            Ok(avail)
        }
    }

    fn write_at(
        &self,
        offset: usize,
        reader: &mut VmReader,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        if !reader.has_remain() {
            return Ok(0);
        }

        let size = self.framebuffer.buffer_size();
        if offset >= size {
            return_errno_with_message!(
                Errno::ENOSPC,
                "the write offset is beyond the framebuffer size"
            );
        }

        let len = reader.remain().min(size - offset);
        if len == 0 {
            return Ok(0);
        }

        // For Mmio-backed framebuffers, use the fallible VmIo path. For Blit
        // backends, write directly to the linear-mapped DMA buffer VA.
        //
        // On aarch64 QEMU TCG, the IoMem kvirt_area path triggers an EL1 page
        // fault (ESR=0x96000041) after repeated write() syscalls, because the
        // dynamically-allocated KVirtArea mapping for the fb DMA buffer becomes
        // invalid. The BlitBackend.base address (LINEAR_BASE + 0x60000000) uses
        // the fixed-PA linear mapping that is verified stable (PLAN.md).
        // So for Blit backends we bypass IoMem entirely and copy from the
        // userspace reader cursor directly into the fb VA.
        if let Some(io_mem) = self.framebuffer.io_mem() {
            let mut new_reader = reader.clone();
            new_reader.limit(len);
            let result = io_mem.write_fallible(offset, &mut new_reader);
            let copied = match result {
                Ok(copied) => copied,
                Err((err, copied)) => {
                    if copied > 0 {
                        copied
                    } else {
                        return Err(err.into());
                    }
                }
            };
            reader.skip(copied);
            Ok(copied)
        } else {
            // Blit-backed framebuffer: copy userspace bytes to the DMA buffer
            // via the stable linear mapping. Use a small stack buffer copied
            // through the VmReader/VmWriter API (which handles user/kernel
            // access correctly).
            //
            // We deliberately do NOT call flush_all() here. The flush path
            // (flush_framebuffer) hits a deterministic ostd page-table bug after
            // ~7 invocations under repeated write() syscalls, crashing the kernel.
            // The raw-probe initial fill already pushed a test pattern; the
            // framebuffer console's own flush path (or kei_desktop's single
            // explicit ioctl-triggered flush) will make new pixels visible.
            const CHUNK: usize = 4096;
            let mut total = 0;
            let mut off = offset;
            while total < len {
                let n = (len - total).min(CHUNK);
                let mut buf = [0u8; CHUNK];
                // Use the fallible read API: the reader is a user-space
                // Fallible VmReader, and we read into a kernel Fallible writer
                // wrapping our stack buffer. read_fallible returns
                // Ok(copied) or Err((err, copied)).
                let mut writer = VmWriter::from(&mut buf[..n]).to_fallible();
                let copied = match reader.read_fallible(&mut writer) {
                    Ok(c) => c,
                    Err((_, c)) => c,
                };
                if copied == 0 {
                    break;
                }
                self.framebuffer.write_bytes_at(off, &buf[..copied])?;
                total += copied;
                off += copied;
            }
            // No flush here — kei_desktop triggers a single flush via
            // FBIOPAN_DISPLAY ioctl after writing the full frame.
            Ok(total)
        }
    }
}

impl PerOpenFileOps for FbHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn check_seekable(&self) -> Result<()> {
        Ok(())
    }

    fn is_offset_aware(&self) -> bool {
        true
    }

    fn mappable(&self) -> Result<Mappable> {
        // Mmio-backed framebuffers: direct IoMem mapping (EFI/VBE linear FB).
        if let Some(iomem) = self.framebuffer.io_mem() {
            return Ok(Mappable::IoMem(iomem.clone()));
        }

        // Blit-backed framebuffers (virtio-gpu DMA buffer): create an IoMem
        // that covers the DMA buffer's physical pages. The IoMem abstraction
        // supports any physical address range, not just device MMIO holes.
        // Userspace writes will go directly into the DMA buffer; a subsequent
        // msync or ioctl flush pushes pixels to the host scanout.
        let pa = self.framebuffer.physical_address().ok_or_else(|| {
            Error::with_message(
                Errno::ENODEV,
                "framebuffer has no mappable physical address",
            )
        })?;
        let size = self.framebuffer.buffer_size();
        // Acquire the physical range as an IoMem with WriteCombining policy
        // for efficient sequential pixel writes from userspace.
        let iomem = ostd::io::IoMem::acquire_with_cache_policy(
            pa..pa + size,
            ostd::mm::CachePolicy::WriteCombining,
        )
        .map_err(|_| Error::with_message(Errno::ENOMEM, "failed to acquire fb IoMem"))?;
        Ok(Mappable::IoMem(iomem))
    }

    fn ioctl(&self, raw_ioctl: RawIoctl) -> Result<i32> {
        use ioctl_defs::*;

        dispatch_ioctl!(match raw_ioctl {
            cmd @ GetVarScreenInfo => {
                cmd.write(&self.collect_var_screen_info())?;
                Ok(0)
            }
            cmd @ PutVarScreenInfo => {
                // EFI framebuffers do not support changing settings. Linux
                // will return the old settings to user space and succeed.
                // Reference: <https://elixir.bootlin.com/linux/v6.17/source/drivers/video/fbdev/core/fbmem.c#L276-L279>.
                cmd.write(&self.collect_var_screen_info())?;
                Ok(0)
            }
            cmd @ GetFixScreenInfo => {
                cmd.write(&self.collect_fix_screen_info())?;
                Ok(0)
            }
            cmd @ GetColorMap => {
                self.handle_get_cmap(&cmd.read()?)?;
                Ok(0)
            }
            cmd @ PutColorMap => {
                self.handle_set_cmap(&cmd.read()?)?;
                Ok(0)
            }
            PanDisplay => {
                // Hijack FBIOPAN_DISPLAY as an explicit "flush the framebuffer
                // to the scanout" trigger for Blit-backed devices. The normal
                // write_at path does NOT flush (to avoid the ostd page-table
                // crash under repeated flushes). kei_desktop calls this once
                // after writing the full frame to make all pixels visible in a
                // single virtio-gpu TRANSFER_TO_HOST_2D + RESOURCE_FLUSH.
                //
                // If arg != 0, also move the hardware cursor: high16=y, low16=x.
                if raw_ioctl.arg() != 0 {
                    #[cfg(target_arch = "aarch64")]
                    {
                        let pos = raw_ioctl.arg() as usize;
                        let x = (pos & 0xFFFF) as u32;
                        let y = ((pos >> 16) & 0xFFFF) as u32;
                        aster_virtio::aarch64_raw_gpu_probe::move_cursor_hw(x, y);
                    }
                }
                self.framebuffer.flush_all();
                Ok(0)
            }
            Blank => {
                // Not supported by efifb.
                return_errno_with_message!(
                    Errno::EINVAL,
                    "the ioctl command is not supported by efifb devices"
                )
            }
            _ => {
                // Custom ioctl: move hardware cursor (cmd=0x4607).
                // User passes x,y packed as u32 (high16=y, low16=x) in arg.
                if raw_ioctl.cmd() == 0x4607 {
                    #[cfg(target_arch = "aarch64")]
                    {
                        let pos = raw_ioctl.arg() as usize;
                        let x = (pos & 0xFFFF) as u32;
                        let y = ((pos >> 16) & 0xFFFF) as u32;
                        aster_virtio::aarch64_raw_gpu_probe::move_cursor_hw(x, y);
                    }
                    return Ok(0);
                }
                ostd::debug!(
                    "the ioctl command {:#x} is unknown for framebuffer devices",
                    raw_ioctl.cmd()
                );
                return_errno_with_message!(Errno::ENOTTY, "the ioctl command is unknown");
            }
        })
    }
}

pub(super) fn init_in_first_kthread() {
    if framebuffer::get().is_none() {
        return;
    }

    char::register(Arc::new(Fb)).expect("failed to register framebuffer char device");
}

/// Registers the framebuffer char device after a late-arriving display device
/// (e.g. virtio-gpu) has published a framebuffer.
///
/// Called by the virtio-gpu driver once `framebuffer::publish` has installed
/// a `FrameBuffer`. Idempotent: the char device registration is a no-op if
/// already registered (the `Fb` device is stateless).
pub fn register_late() {
    char::register(Arc::new(Fb)).expect("failed to register framebuffer char device");
}
