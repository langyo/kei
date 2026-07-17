// SPDX-License-Identifier: MPL-2.0

//! Minimal serial console device for user-space I/O on aarch64/riscv64.

use alloc::sync::Arc;
use core::fmt::Display;

#[cfg(target_arch = "aarch64")]
use ostd::arch::serial::{pl011_recv_byte, pl011_send_byte};
use ostd::mm::{FallibleVmRead, FallibleVmWrite};

/// Writes one byte to the debug serial console.
///
/// aarch64 uses the PL011 UART MMIO; riscv64 goes through the SBI console
/// (the same backend as `ostd::early_print!`).
#[cfg(target_arch = "aarch64")]
fn serial_send_byte(byte: u8) {
    pl011_send_byte(byte);
}

/// Reads one byte from the debug serial console, if available.
#[cfg(target_arch = "aarch64")]
fn serial_recv_byte() -> Option<u8> {
    pl011_recv_byte()
}

#[cfg(target_arch = "riscv64")]
fn serial_send_byte(byte: u8) {
    let buf = [byte];
    let s = core::str::from_utf8(&buf).unwrap_or("?");
    ostd::early_print!("{}", s);
}

#[cfg(target_arch = "riscv64")]
fn serial_recv_byte() -> Option<u8> {
    // The SBI debug console is write-only on this boot path; the serial
    // backend is a log file, so there is nothing to read anyway.
    None
}

use crate::{
    events::IoEvents,
    fs::{
        file::{AccessMode, FileLike, file_table::FdFlags},
        pseudofs::AnonInodeFs,
        vfs::path::Path,
    },
    prelude::*,
    process::signal::{PollHandle, Pollable},
};

#[derive(Debug)]
pub struct SerialConsole {
    access_mode: AccessMode,
    pseudo_path: Path,
}

impl SerialConsole {
    pub fn new(access_mode: AccessMode) -> Self {
        let pseudo_path = AnonInodeFs::new_path(|_| "anon_inode:[serial]".to_string());
        Self {
            access_mode,
            pseudo_path,
        }
    }
}

impl Pollable for SerialConsole {
    fn poll(&self, mask: IoEvents, _poller: Option<&mut PollHandle>) -> IoEvents {
        let mut events = IoEvents::empty();
        if mask.contains(IoEvents::OUT) {
            events |= IoEvents::OUT;
        }
        if mask.contains(IoEvents::IN) {
            events |= IoEvents::IN;
        }
        events
    }
}

impl FileLike for SerialConsole {
    fn read(&self, writer: &mut VmWriter) -> Result<usize> {
        if !self.access_mode.is_readable() {
            return_errno_with_message!(Errno::EBADF, "serial console not readable");
        }
        let mut buf = [0u8; 256];
        let mut total = 0;
        while total < buf.len() {
            match serial_recv_byte() {
                Some(byte) => {
                    buf[total] = byte;
                    total += 1;
                }
                None => break,
            }
        }
        if total == 0 {
            return_errno_with_message!(Errno::EAGAIN, "no data available");
        }
        // Copy kernel buffer to user-space writer
        let mut kreader = ostd::mm::VmReader::from(&buf[..total]);
        writer.write_fallible(&mut kreader).map_err(|e| e.0)?;
        Ok(total)
    }

    fn write(&self, reader: &mut VmReader) -> Result<usize> {
        if !self.access_mode.is_writable() {
            return_errno_with_message!(Errno::EBADF, "serial console not writable");
        }
        let mut buf = [0u8; 1024];
        let mut kwriter = ostd::mm::VmWriter::from(&mut buf[..]);
        let n = reader.read_fallible(&mut kwriter).map_err(|e| e.0)?;
        for i in 0..n {
            serial_send_byte(buf[i]);
        }
        Ok(n)
    }

    fn access_mode(&self) -> AccessMode {
        self.access_mode
    }

    fn path(&self) -> &Path {
        &self.pseudo_path
    }

    fn dump_proc_fdinfo(self: Arc<Self>, _fd_flags: FdFlags) -> Box<dyn Display> {
        struct FdInfo;
        impl Display for FdInfo {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "serial console")
            }
        }
        Box::new(FdInfo)
    }
}
