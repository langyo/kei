// SPDX-License-Identifier: MPL-2.0

use alloc::borrow::Cow;

use cpio_decoder::{CpioDecoder, CpioEntry, FileMetadata, FileType};
use device_id::{DeviceId, MajorId, MinorId};
use lending_iterator::LendingIterator;
use no_std_io2::io::{Cursor, Read};
use ostd::boot::boot_info;
use zune_inflate::DeflateDecoder;

use super::{
    file::{InodeMode, InodeType},
    vfs::path::{FsPath, PathResolver, is_dot},
};
use crate::{fs::vfs::inode::MknodType, prelude::*};

/// Unpack and prepare the rootfs from the initramfs CPIO buffer.
pub fn init_in_first_kthread(path_resolver: &PathResolver) -> Result<()> {
    let initramfs_buf = boot_info()
        .initramfs
        .ok_or_else(|| Error::with_message(Errno::EINVAL, "no initramfs found"));

    // On aarch64, QEMU `-kernel <ELF>` does not load the `-initrd` file into
    // guest RAM nor write the linux,initrd-start/end properties to the FDT
    // (the ELF boot path bypasses arm_load_kernel's initrd handling). As a
    // fallback, embed the initramfs at compile time. This is gated to aarch64
    // so x86_64 (where QEMU passes initrd normally) is unaffected.
    #[cfg(target_arch = "aarch64")]
    let initramfs_buf: &[u8] = match initramfs_buf {
        Ok(buf) => buf,
        Err(_) => {
            ostd::early_println!("[rootfs] FDT has no initramfs, using embedded copy");
            include_bytes!("../../../tests/initramfs/build/initramfs.cpio.gz")
        }
    };
    #[cfg(not(target_arch = "aarch64"))]
    let initramfs_buf: &[u8] = initramfs_buf?;

    ostd::early_println!(
        "[rootfs] initramfs buf size = {} bytes",
        initramfs_buf.len()
    );

    let (reader, suffix) = match &initramfs_buf[..4] {
        // Gzip magic number: 0x1F 0x8B
        &[0x1F, 0x8B, _, _] => {
            ostd::early_println!("[rootfs] decompressing gzip...");
            let decompressed = DeflateDecoder::new(initramfs_buf)
                .decode_gzip()
                .map_err(|_| Error::with_message(Errno::EINVAL, "gzip decompression failed"))?;
            ostd::early_println!("[rootfs] decompressed {} bytes", decompressed.len());
            (Cow::Owned(decompressed), ".gz")
        }
        _ => (Cow::Borrowed(initramfs_buf), ""),
    };

    ostd::early_println!("[rootfs] unpacking initramfs.cpio{} to rootfs ...", suffix);

    let mut decoder = CpioDecoder::new(Cursor::new(reader));

    let mut entry_count = 0u32;
    while let Some(entry_result) = decoder.next() {
        let mut entry = entry_result?;
        entry_count += 1;
        if let Err(e) = try_append_entry_to_rootfs(&mut entry, path_resolver) {
            ostd::early_println!("[rootfs] failed to add entry {}: {:?}", entry.name(), e);
        }
    }

    ostd::early_println!("[rootfs] rootfs is ready ({} entries)", entry_count);
    Ok(())
}

fn try_append_entry_to_rootfs<R: Read>(
    entry: &mut CpioEntry<R>,
    path_resolver: &PathResolver,
) -> Result<()> {
    // Make sure the name is a relative path, and is not end with "/".
    let entry_name = entry.name().trim_start_matches('/').trim_end_matches('/');
    if entry_name.is_empty() {
        return_errno_with_message!(Errno::EINVAL, "invalid entry name");
    }
    if is_dot(entry_name) {
        return Ok(());
    }

    // Here we assume that the directory referred by "prefix" must has been created.
    // The basis of this assumption is：
    // The mkinitramfs script uses `find` command to ensure that the entries are
    // sorted that a directory always appears before its child directories and files.
    let (parent, name) = if let Some((prefix, last)) = entry_name.rsplit_once('/') {
        (path_resolver.lookup(&FsPath::try_from(prefix)?)?, last)
    } else {
        (path_resolver.root().clone(), entry_name)
    };

    let metadata = entry.metadata();
    let mode = InodeMode::from_bits_truncate(metadata.permission_mode());
    match metadata.file_type() {
        FileType::File => {
            let path = parent.new_fs_child(name, InodeType::File, mode)?;
            entry.read_all(path.inode().writer(0))?;
        }
        FileType::Dir => {
            let _ = parent.new_fs_child(name, InodeType::Dir, mode)?;
        }
        FileType::Link => {
            let path = parent.new_fs_child(name, InodeType::SymLink, mode)?;
            let link_content = {
                let mut link_data: Vec<u8> = Vec::new();
                entry.read_all(&mut link_data)?;
                core::str::from_utf8(&link_data)?.to_string()
            };
            path.inode().write_link(&link_content)?;
        }
        FileType::Char => {
            let device_id = try_device_id_from_metadata(metadata)?;
            parent.mknod(name, mode, MknodType::CharDevice(device_id))?;
        }
        FileType::Block => {
            let device_id = try_device_id_from_metadata(metadata)?;
            parent.mknod(name, mode, MknodType::BlockDevice(device_id))?;
        }
        FileType::FiFo => {
            parent.mknod(name, mode, MknodType::NamedPipe)?;
        }
        FileType::Socket => {
            return_errno_with_message!(Errno::EINVAL, "socket files are not supported in initramfs")
        }
    }

    Ok(())
}

fn try_device_id_from_metadata(metadata: &FileMetadata) -> Result<u64> {
    let major = {
        let dev_maj = u16::try_from(metadata.rdev_maj())?;
        MajorId::try_from(dev_maj).map_err(|msg| Error::with_message(Errno::EINVAL, msg))?
    };
    let minor = MinorId::try_from(metadata.rdev_min())
        .map_err(|msg| Error::with_message(Errno::EINVAL, msg))?;
    Ok(DeviceId::new(major, minor).as_encoded_u64())
}
