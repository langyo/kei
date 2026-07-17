// SPDX-License-Identifier: MPL-2.0

//! Opened Inode-backed File Handle

use core::{fmt::Display, sync::atomic::Ordering};

use aster_rights::Rights;

use super::{
    AccessMode, AtomicStatusFlags, CreationFlags, FileLike, InodeType, Mappable, StatusFlags,
    file_table::FdFlags, flock::FlockItem,
};
use crate::{
    events::IoEvents,
    fs::{
        pipe::PipeHandle,
        utils::DirentVisitor,
        vfs::{
            inode::{FallocMode, FileOps},
            inode_ext::InodeExt,
            path::Path,
            range_lock::{FileRange, OFFSET_MAX, RangeLockItem, RangeLockType},
        },
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
    util::ioctl::RawIoctl,
};

pub struct InodeHandle {
    path: Path,
    /// `open_file` is similar to the `file_private` field in Linux's `file` structure. If
    /// `open_file` is `Some(_)`, typical file operations including `read`, `write`, `poll`,
    /// and `ioctl` will be provided by the per-open file object instead of `path`.
    open_file: Option<Box<dyn PerOpenFileOps>>,
    offset: Mutex<usize>,
    status_flags: AtomicStatusFlags,
    rights: Rights,
}

impl InodeHandle {
    pub fn new(path: Path, access_mode: AccessMode, status_flags: StatusFlags) -> Result<Self> {
        let inode = path.inode();
        if !status_flags.contains(StatusFlags::O_PATH) {
            // "Opening a file or directory with the O_PATH flag requires no permissions on the
            // object itself".
            // Reference: <https://man7.org/linux/man-pages/man2/openat.2.html>
            inode.check_permission(access_mode.into())?;
        }

        Self::new_unchecked_access(path, access_mode, status_flags)
    }

    pub fn new_unchecked_access(
        path: Path,
        access_mode: AccessMode,
        status_flags: StatusFlags,
    ) -> Result<Self> {
        let inode = path.inode();
        let (open_file, rights) = if status_flags.contains(StatusFlags::O_PATH) {
            (None, Rights::empty())
        } else if inode.type_() == InodeType::Dir && access_mode.is_writable() {
            return_errno_with_message!(Errno::EISDIR, "a directory cannot be opened writable");
        } else {
            let open_file = inode.open(access_mode, status_flags).transpose()?;
            let rights = Rights::from(access_mode);
            (open_file, rights)
        };

        Ok(Self {
            path,
            open_file,
            offset: Mutex::new(0),
            status_flags: AtomicStatusFlags::new(status_flags),
            rights,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn offset(&self) -> usize {
        let offset = self.offset.lock();
        *offset
    }

    pub(in crate::fs) fn rights(&self) -> Rights {
        self.rights
    }

    /// Returns whether I/O should use and advance the file offset.
    ///
    /// Calls `is_offset_aware()` directly through the `dyn PerOpenFileOps`
    /// vtable — no trait upcast needed.
    fn is_offset_aware_impl(&self) -> bool {
        if let Some(ref open_file) = self.open_file {
            return open_file.is_offset_aware();
        }

        // Fallback: use the inode directly. This is the normal path for
        // inode-backed files without a per-open file object (regular files,
        // directories), and also for device nodes whose char device lookup
        // failed during open (device not registered or ID mismatch).
        self.path.inode().type_().is_seekable()
    }

    /// Ensures that positional I/O (`pread`/`pwrite`) is supported.
    fn ensure_positional_io(&self) -> Result<()> {
        if let Some(ref open_file) = self.open_file {
            return open_file.check_positional_io();
        }

        let inode = self.path.inode();
        if !inode.type_().is_seekable() {
            return_errno_with_message!(
                Errno::ESPIPE,
                "the inode cannot be read or written at a specific offset"
            );
        }
        Ok(())
    }

    /// Reads at the given offset, dispatching to the per-open file or inode.
    ///
    /// Calls `FileOps::read_at` directly through the `dyn PerOpenFileOps`
    /// vtable when `open_file` is set, avoiding the broken trait upcasting
    /// codegen on aarch64.
    fn read_at_impl(
        &self,
        offset: usize,
        writer: &mut VmWriter,
        status_flags: StatusFlags,
    ) -> Result<usize> {
        if let Some(ref open_file) = self.open_file {
            open_file.read_at(offset, writer, status_flags)
        } else {
            self.path
                .inode()
                .as_ref()
                .read_at(offset, writer, status_flags)
        }
    }

    /// Writes at the given offset, dispatching to the per-open file or inode.
    fn write_at_impl(
        &self,
        offset: usize,
        reader: &mut VmReader,
        status_flags: StatusFlags,
    ) -> Result<usize> {
        if let Some(ref open_file) = self.open_file {
            open_file.write_at(offset, reader, status_flags)
        } else {
            self.path
                .inode()
                .as_ref()
                .write_at(offset, reader, status_flags)
        }
    }

    /// Reads directory entries at the given offset.
    fn readdir_at_impl(&self, offset: usize, visitor: &mut dyn DirentVisitor) -> Result<usize> {
        if let Some(ref open_file) = self.open_file {
            open_file.readdir_at(offset, visitor)
        } else {
            self.path.inode().as_ref().readdir_at(offset, visitor)
        }
    }

    pub fn readdir(&self, visitor: &mut dyn DirentVisitor) -> Result<usize> {
        if !self.rights.contains(Rights::READ) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened readable");
        }

        let mut offset = self.offset.lock();
        let read_cnt = self.readdir_at_impl(*offset, visitor)?;
        *offset += read_cnt;
        Ok(read_cnt)
    }

    pub fn test_range_lock(&self, mut lock: RangeLockItem) -> Result<RangeLockItem> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        let Some(range_lock_list) = self
            .path
            .inode()
            .fs_lock_context()
            .map(|c| c.range_lock_list())
        else {
            // The lock list is not present. So nothing is locked.
            lock.set_type(RangeLockType::Unlock);
            return Ok(lock);
        };

        let req_lock = range_lock_list.test_lock(lock);
        Ok(req_lock)
    }

    pub fn set_range_lock(&self, lock: &RangeLockItem, is_nonblocking: bool) -> Result<()> {
        match lock.type_() {
            RangeLockType::ReadLock => {
                if !self.rights.contains(Rights::READ) {
                    return_errno_with_message!(Errno::EBADF, "the file is not opened readable");
                }
            }
            RangeLockType::WriteLock => {
                if !self.rights.contains(Rights::WRITE) {
                    return_errno_with_message!(Errno::EBADF, "the file is not opened writable");
                }
            }
            RangeLockType::Unlock => {
                if self.rights.is_empty() {
                    return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
                }
            }
        }

        if RangeLockType::Unlock == lock.type_() {
            self.unlock_range_lock(lock);
            return Ok(());
        }

        let range_lock_list = self
            .path
            .inode()
            .fs_lock_context_or_init()
            .range_lock_list();
        range_lock_list.set_lock(lock, is_nonblocking)
    }

    pub fn release_range_locks(&self) {
        let range_lock = RangeLockItem::new(
            RangeLockType::Unlock,
            FileRange::new(0, OFFSET_MAX).unwrap(),
        );
        self.unlock_range_lock(&range_lock);
    }

    fn unlock_range_lock(&self, lock: &RangeLockItem) {
        if let Some(range_lock_list) = self
            .path
            .inode()
            .fs_lock_context()
            .map(|c| c.range_lock_list())
        {
            range_lock_list.unlock(lock);
        }
    }

    pub fn set_flock(&self, lock: FlockItem, is_nonblocking: bool) -> Result<()> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        let flock_list = self.path.inode().fs_lock_context_or_init().flock_list();
        flock_list.set_lock(lock, is_nonblocking)
    }

    pub fn unlock_flock(&self) -> Result<()> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        if let Some(flock_list) = self.path.inode().fs_lock_context().map(|c| c.flock_list()) {
            flock_list.unlock(self);
        }

        Ok(())
    }

    pub fn downcast_open_file<T: 'static>(&self) -> Result<Option<&T>> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        let Some(open_file) = self.open_file.as_ref() else {
            return Ok(None);
        };

        Ok(open_file.as_any().downcast_ref::<T>())
    }
}

impl Pollable for InodeHandle {
    fn poll(&self, mask: IoEvents, poller: Option<&mut PollHandle>) -> IoEvents {
        if let Some(ref open_file) = self.open_file {
            return open_file.poll(mask, poller);
        }

        if self.rights.is_empty() {
            IoEvents::NVAL
        } else {
            let events = IoEvents::IN | IoEvents::OUT;
            events & mask
        }
    }
}

impl FileLike for InodeHandle {
    fn read(&self, writer: &mut VmWriter) -> Result<usize> {
        if !self.rights.contains(Rights::READ) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened readable");
        }

        let status_flags = self.status_flags();
        let is_offset_aware = self.is_offset_aware_impl();

        if !is_offset_aware {
            return self.read_at_impl(0, writer, status_flags);
        }

        let mut offset = self.offset.lock();

        let len = self.read_at_impl(*offset, writer, status_flags)?;
        *offset += len;

        Ok(len)
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        if !self.rights.contains(Rights::WRITE) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened writable");
        }

        let status_flags = self.status_flags();
        let is_offset_aware = self.is_offset_aware_impl();

        if !is_offset_aware {
            return self.write_at_impl(0, reader, status_flags);
        }

        let mut offset = self.offset.lock();

        // O_APPEND: for page-cache-backed files, atomically seek to EOF
        // before writing. For per-open-file descriptors (device nodes,
        // sockets, etc.) the offset is passed through to write_at_impl
        // along with status_flags — the implementation decides whether
        // O_APPEND changes semantics (most device drivers ignore it).
        if status_flags.contains(StatusFlags::O_APPEND) && self.open_file.is_none() {
            *offset = self.path.size();
        }

        let len = self.write_at_impl(*offset, reader, status_flags)?;
        *offset += len;

        Ok(len)
    }

    fn read_at(&self, offset: usize, writer: &mut VmWriter) -> Result<usize> {
        if !self.rights.contains(Rights::READ) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened readable");
        }
        self.ensure_positional_io()?;
        let status_flags = self.status_flags();
        self.read_at_impl(offset, writer, status_flags)
    }

    fn write_at(&self, mut offset: usize, reader: &mut VmReader) -> Result<usize> {
        if !self.rights.contains(Rights::WRITE) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened writable");
        }
        self.ensure_positional_io()?;
        let status_flags = self.status_flags();

        // O_APPEND: for page-cache-backed files, override the caller's
        // offset to EOF. For per-open-file descriptors the offset is
        // passed through — see positional write() for rationale.
        if status_flags.contains(StatusFlags::O_APPEND) && self.open_file.is_none() {
            offset = self.path.size();
        }

        self.write_at_impl(offset, reader, status_flags)
    }

    fn ioctl(&self, raw_ioctl: RawIoctl) -> Result<i32> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        if let Some(ref open_file) = self.open_file {
            return open_file.ioctl(raw_ioctl);
        }

        return_errno_with_message!(Errno::ENOTTY, "ioctl is not supported");
    }

    fn mappable(&self) -> Result<Mappable> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        let inode = self.path.inode();
        if let Some(ref page_cache) = inode.page_cache() {
            // If the inode has a page cache, it is a file-backed mapping and
            // we return the VMO as the mappable object.
            Ok(Mappable::Vmo(page_cache.as_vmo().clone()))
        } else if let Some(ref open_file) = self.open_file {
            // Otherwise, it is a special file (e.g. device file) and we should
            // return the file-specific mappable object.
            open_file.mappable()
        } else {
            return_errno_with_message!(Errno::ENODEV, "the file is not mappable");
        }
    }

    fn resize(&self, new_size: usize) -> Result<()> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }
        if !self.rights.contains(Rights::WRITE) {
            return_errno_with_message!(Errno::EINVAL, "the file is not opened writable");
        }

        // Linux allows ftruncate on O_APPEND files — the flag only
        // affects write offset, not resize operations.
        self.path.inode().resize(new_size)
    }

    fn status_flags(&self) -> StatusFlags {
        self.status_flags.load(Ordering::Relaxed)
    }

    fn set_status_flags(&self, new_status_flags: StatusFlags) -> Result<()> {
        // TODO: Pipes currently require a special status flag check because
        // "packet" mode is not yet supported. Remove this check once "packet"
        // mode is implemented.
        if self
            .open_file
            .as_ref()
            .and_then(|open_file| open_file.as_any().downcast_ref::<PipeHandle>())
            .is_some()
        {
            crate::fs::pipe::check_status_flags(new_status_flags)?;
        }

        self.status_flags.store(new_status_flags, Ordering::Relaxed);

        Ok(())
    }

    fn access_mode(&self) -> AccessMode {
        self.rights.into()
    }

    fn seek(&self, pos: SeekFrom) -> Result<usize> {
        if self.rights.is_empty() {
            return_errno_with_message!(Errno::EBADF, "the file is opened as a path");
        }

        if let Some(ref open_file) = self.open_file {
            open_file.check_seekable()?;
            if open_file.is_offset_aware() {
                return do_seek_util(&self.offset, pos, open_file.seek_end()?);
            } else {
                return Ok(0);
            }
        }

        let inode = self.path.inode();
        if !inode.type_().is_seekable() {
            return_errno_with_message!(Errno::ESPIPE, "seek is not supported");
        }
        do_seek_util(&self.offset, pos, inode.seek_end())
    }

    fn fallocate(&self, mode: FallocMode, offset: usize, len: usize) -> Result<()> {
        if !self.rights.contains(Rights::WRITE) {
            return_errno_with_message!(Errno::EBADF, "the file is not opened writable");
        }

        let inode = self.path.inode().as_ref();
        let inode_type = inode.type_();

        // TODO: `fallocate` on pipe files also fails with `ESPIPE`.
        if inode_type == InodeType::NamedPipe {
            return_errno_with_message!(Errno::ESPIPE, "the inode is a FIFO file");
        }
        if !(inode_type == InodeType::File || inode_type == InodeType::Dir) {
            return_errno_with_message!(
                Errno::ENODEV,
                "the inode is not a regular file or a directory"
            );
        }

        let status_flags = self.status_flags();
        if status_flags.contains(StatusFlags::O_APPEND)
            && (mode == FallocMode::PunchHoleKeepSize
                || mode == FallocMode::CollapseRange
                || mode == FallocMode::InsertRange)
        {
            return_errno_with_message!(
                Errno::EPERM,
                "the flags do not work on the append-only file"
            );
        }
        if status_flags.contains(StatusFlags::O_DIRECT)
            || status_flags.contains(StatusFlags::O_PATH)
        {
            return_errno_with_message!(
                Errno::EBADF,
                "currently fallocate file with O_DIRECT or O_PATH is not supported"
            );
        }

        inode.fallocate(mode, offset, len)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn dump_proc_fdinfo(self: Arc<Self>, fd_flags: FdFlags) -> Box<dyn Display> {
        struct FdInfo {
            inner: Arc<InodeHandle>,
            fd_flags: FdFlags,
        }

        impl Display for FdInfo {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let mut flags = self.inner.status_flags().bits() | self.inner.access_mode() as u32;
                if self.fd_flags.contains(FdFlags::CLOEXEC) {
                    flags |= CreationFlags::O_CLOEXEC.bits();
                }

                writeln!(f, "pos:\t{}", self.inner.offset())?;
                writeln!(f, "flags:\t0{:o}", flags)?;
                writeln!(f, "mnt_id:\t{}", self.inner.path.mount_node().id())?;
                writeln!(f, "ino:\t{}", self.inner.path.inode().ino())
            }
        }

        Box::new(FdInfo {
            inner: self,
            fd_flags,
        })
    }
}

impl Drop for InodeHandle {
    fn drop(&mut self) {
        self.release_range_locks();
        let _ = self.unlock_flock();
    }
}

impl Debug for InodeHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("InodeHandle")
            .field("path", &self.path)
            .field("offset", &self.offset())
            .field("status_flags", &self.status_flags())
            .field("rights", &self.rights)
            .finish_non_exhaustive()
    }
}

/// Describes the position to seek from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeekFrom {
    Start(usize),
    End(isize),
    Current(isize),
}

/// File operations for one opened file description.
///
/// A per-open file object can hold file-description-specific state and override
/// operations that are not purely inode-backed, such as state and operations for
/// devices, pipes, namespace files, and procfs files.
///
/// # aarch64 vtable dispatch note
///
/// On aarch64 (nightly-2026-05-01), the compiler's trait upcasting codegen
/// produces incorrect vtables when converting `&dyn PerOpenFileOps` to
/// `&dyn FileOps` or `&dyn Any`. To work around this, callers must **not**
/// upcast `&dyn PerOpenFileOps` to a supertrait trait object. Instead:
///
/// * For I/O operations (`read_at`/`write_at`/`readdir_at`): call them
///   directly on `&dyn PerOpenFileOps`. These methods are in the
///   `dyn PerOpenFileOps` vtable via supertrait inheritance and dispatch
///   correctly (same path as `is_offset_aware()`, which is known to work).
/// * For type downcasting: use [`PerOpenFileOps::as_any`] to get a
///   `&dyn Any` for the concrete type, then call `Any::downcast_ref`.
///   `as_any` is a required method (not a default method) — each
///   implementor must supply `self as &dyn Any` directly, which produces
///   a correctly-constructed vtable entry per concrete type (no trait
///   upcasting).
pub trait PerOpenFileOps: Pollable + FileOps + Any + Send + Sync + 'static {
    /// Returns a `&dyn Any` for the concrete implementing type.
    ///
    /// This must be implemented explicitly as `self` (the `&Self` → `&dyn Any`
    /// coercion happens on the concrete type, not through trait upcasting).
    /// The vtable entry generated for each concrete `Self` is therefore
    /// correct, and callers can use the returned `&dyn Any` to call
    /// `Any::downcast_ref`.
    fn as_any(&self) -> &dyn Any;

    /// Checks whether the `seek()` operation should fail.
    fn check_seekable(&self) -> Result<()>;

    /// Returns whether the `read()`/`write()` operation should use and advance the offset.
    ///
    /// If [`PerOpenFileOps::check_seekable`] succeeds but this method returns `false`,
    /// the offset in the `seek()` operation will be ignored.
    /// In that case, the `seek()` operation will do nothing but succeed.
    fn is_offset_aware(&self) -> bool;

    /// Checks whether positional I/O (`pread`/`pwrite`) is supported.
    ///
    /// The default delegates to [`check_seekable`], which is correct for
    /// most files. Override this for files that support positional I/O
    /// but not seeking (e.g., nsfs).
    ///
    /// [`check_seekable`]: PerOpenFileOps::check_seekable
    fn check_positional_io(&self) -> Result<()> {
        self.check_seekable()
    }

    /// Returns the end position for [`SeekFrom::End`].
    ///
    /// This is intentionally separate from `Inode::seek_end`. Both `Inode`
    /// and [`PerOpenFileOps`] need `SEEK_END` support, but `Inode::seek_end`
    /// has an inode-specific default implementation, so the two cannot be
    /// cleanly unified under [`FileOps`].
    fn seek_end(&self) -> Result<Option<usize>> {
        Ok(None)
    }

    // See `FileLike::mappable`.
    fn mappable(&self) -> Result<Mappable> {
        return_errno_with_message!(Errno::EINVAL, "the file is not mappable");
    }

    fn ioctl(&self, _raw_ioctl: RawIoctl) -> Result<i32> {
        return_errno_with_message!(Errno::ENOTTY, "ioctl is not supported");
    }
}

fn do_seek_util(offset: &Mutex<usize>, pos: SeekFrom, end: Option<usize>) -> Result<usize> {
    let mut offset = offset.lock();

    let new_offset = match pos {
        SeekFrom::Start(off) => off,
        SeekFrom::End(diff) => {
            if let Some(end) = end {
                end.wrapping_add_signed(diff)
            } else {
                return_errno_with_message!(
                    Errno::EINVAL,
                    "seeking the file from the end is not supported"
                );
            }
        }
        SeekFrom::Current(diff) => offset.wrapping_add_signed(diff),
    };

    // Invariant: `*offset <= isize::MAX as usize`.
    // TODO: Investigate whether `read`/`write` can break this invariant.
    if new_offset.cast_signed() < 0 {
        return_errno_with_message!(Errno::EINVAL, "the file offset cannot be negative");
    }

    *offset = new_offset;
    Ok(new_offset)
}
