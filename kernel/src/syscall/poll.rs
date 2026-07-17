// SPDX-License-Identifier: MPL-2.0

use core::{cell::Cell, mem::offset_of, time::Duration};

use ostd::mm::VmIo;

use super::SyscallReturn;
use crate::{
    events::IoEvents,
    fs::file::{
        FileLike,
        file_table::{FileDesc, FileTable},
    },
    prelude::*,
    process::{ResourceType, signal::Poller},
};

pub fn sys_poll(fds: Vaddr, nfds: u32, timeout: i32, ctx: &Context) -> Result<SyscallReturn> {
    let timeout = if timeout >= 0 {
        Some(Duration::from_millis(timeout as _))
    } else {
        None
    };

    do_sys_poll(fds, nfds, timeout, ctx)
}

pub(super) fn do_sys_poll(
    fds: Vaddr,
    nfds: u32,
    timeout: Option<Duration>,
    ctx: &Context,
) -> Result<SyscallReturn> {
    if nfds as u64
        > ctx
            .process
            .resource_limits()
            .get_rlimit(ResourceType::RLIMIT_NOFILE)
            .get_cur()
    {
        return_errno_with_message!(
            Errno::EINVAL,
            "the `nfds` value exceeds the `RLIMIT_NOFILE` value"
        )
    }

    let user_space = ctx.user_space();

    let poll_fds = {
        let mut read_addr = fds;
        let mut poll_fds = Vec::with_capacity(nfds as _);

        for _ in 0..nfds {
            let c_poll_fd = user_space.read_val::<c_pollfd>(read_addr)?;
            read_addr += size_of::<c_pollfd>();

            let poll_fd = PollFd::from(c_poll_fd);
            // Always clear the `revents` field first.
            poll_fd.revents().set(IoEvents::empty());
            poll_fds.push(poll_fd);
        }

        poll_fds
    };

    debug!(
        "poll_fds = {:?}, nfds = {}, timeout = {:?}",
        poll_fds, nfds, timeout
    );

    let result = do_poll(&poll_fds, timeout.as_ref(), ctx);

    // Write back -- even when `do_poll` returns an error
    // because the `revents` field may contain garbage and must be cleared.
    let mut write_addr = fds;
    for pollfd in poll_fds {
        let revents = pollfd.revents().get().bits() as i16;

        // Update the `revents` field only. Keep all other fields unchanged.
        user_space.write_val(write_addr + offset_of!(c_pollfd, revents), &revents)?;
        write_addr += size_of::<c_pollfd>();
    }

    let num_revents = result?;
    Ok(SyscallReturn::Return(num_revents as _))
}

pub(super) fn do_poll(
    poll_fds: &[PollFd],
    timeout: Option<&Duration>,
    ctx: &Context,
) -> Result<usize> {
    let mut file_table = ctx.thread_local.borrow_file_table_mut();
    let file_table = file_table.unwrap();

    let poll_files = if let Some(file_table_inner) = file_table.get() {
        PollFiles::new_borrowed(poll_fds, file_table_inner)
    } else {
        let file_table_locked = file_table.read();
        PollFiles::new_owned(poll_fds, &file_table_locked)
    };

    let poller = match poll_files.register_poller(timeout) {
        PollerResult::Registered(poller) => poller,
        PollerResult::FoundEvents(num_events) => return Ok(num_events),
    };

    // On aarch64, the Pollee cross-thread notification is unreliable (the
    // busy-poll thread's Pollee::notify doesn't wake the select waiter due to
    // observer registration timing). As a workaround, we poll network ifaces
    // directly to process pending packets (including TCP handshakes and
    // timer callbacks) and then check for events. This ensures that connections
    // become visible to select/accept without relying on cross-thread wakeups.
    #[cfg(target_arch = "aarch64")]
    {
        // For zero or very short timeouts, return immediately (non-blocking).
        // For None timeout (infinite), loop forever.
        // For finite timeouts, use a bounded loop.
        let max_iters = match timeout {
            None => usize::MAX,                               // infinite
            Some(d) if d.is_zero() => 1,                      // non-blocking: one check
            Some(d) => (d.as_millis().max(1) * 100) as usize, // ~10 checks per ms
        };

        for _ in 0..max_iters {
            // Poll network interfaces to process pending packets
            // and trigger timer callbacks. This also completes any
            // in-progress TCP handshakes.
            crate::net::poll_ifaces();

            let n = poll_files.count_events();
            if n > 0 {
                return Ok(n);
            }

            // Give other threads a chance to run.
            ostd::task::Task::yield_now();
        }

        // Timeout expired with no ready fds.
        return Ok(0);
    }

    #[cfg(not(target_arch = "aarch64"))]
    loop {
        match poller.wait() {
            Ok(()) => (),
            Err(err) if err.error() == Errno::ETIME => return Ok(0),
            Err(err) => return Err(err),
        };

        let num_events = poll_files.count_events();
        if num_events > 0 {
            return Ok(num_events);
        }
    }
}

struct PollFiles<'a> {
    poll_fds: &'a [PollFd],
    files: CowFiles<'a>,
}

enum CowFiles<'a> {
    Borrowed(&'a FileTable),
    Owned(Vec<Option<Arc<dyn FileLike>>>),
}

impl<'a> PollFiles<'a> {
    /// Creates `PollFiles` by holding the file table reference.
    fn new_borrowed(poll_fds: &'a [PollFd], file_table: &'a FileTable) -> Self {
        Self {
            poll_fds,
            files: CowFiles::Borrowed(file_table),
        }
    }

    /// Creates `PollFiles` by cloning all files that we're going to poll.
    fn new_owned(poll_fds: &'a [PollFd], file_table: &FileTable) -> Self {
        let files = poll_fds
            .iter()
            .map(|poll_fd| {
                poll_fd
                    .fd()
                    .and_then(|fd| file_table.get_file(fd).ok().cloned())
            })
            .collect();
        Self {
            poll_fds,
            files: CowFiles::Owned(files),
        }
    }
}

enum PollerResult {
    Registered(Poller),
    FoundEvents(usize),
}

impl PollFiles<'_> {
    /// Registers the files with a poller, or exits early if some events are detected.
    fn register_poller(&self, timeout: Option<&Duration>) -> PollerResult {
        let mut poller = Poller::new(timeout);

        for (index, poll_fd) in self.poll_fds.iter().enumerate() {
            let events = if let Some(file) = self.file_at(index) {
                file.poll(poll_fd.events(), Some(poller.as_handle_mut()))
            } else {
                poll_fd.revents_for_missing_file()
            };

            if events.is_empty() {
                continue;
            }

            poll_fd.revents().set(events);
            return PollerResult::FoundEvents(1 + self.count_events_from(1 + index));
        }

        PollerResult::Registered(poller)
    }

    /// Counts the number of the ready files.
    fn count_events(&self) -> usize {
        self.count_events_from(0)
    }

    /// Counts the number of the ready files from the given index.
    fn count_events_from(&self, start: usize) -> usize {
        let mut counter = 0;

        for index in start..self.poll_fds.len() {
            let poll_fd = &self.poll_fds[index];

            let events = if let Some(file) = self.file_at(index) {
                file.poll(poll_fd.events(), None)
            } else {
                poll_fd.revents_for_missing_file()
            };

            if events.is_empty() {
                continue;
            }

            poll_fd.revents().set(events);
            counter += 1;
        }

        counter
    }

    fn file_at(&self, index: usize) -> Option<&dyn FileLike> {
        match &self.files {
            CowFiles::Borrowed(table) => self.poll_fds[index]
                .fd()
                .and_then(|fd| table.get_file(fd).ok())
                .map(Arc::as_ref),
            CowFiles::Owned(files) => files[index].as_deref(),
        }
    }
}

// https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/poll.h
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
struct c_pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[derive(Clone, Debug)]
pub struct PollFd {
    fd: Option<FileDesc>,
    events: IoEvents,
    revents: Cell<IoEvents>,
}

impl PollFd {
    pub fn new(fd: Option<FileDesc>, events: IoEvents) -> Self {
        let revents = Cell::new(IoEvents::empty());
        Self {
            fd,
            events,
            revents,
        }
    }

    pub fn fd(&self) -> Option<FileDesc> {
        self.fd
    }

    pub fn events(&self) -> IoEvents {
        self.events
    }

    pub fn revents(&self) -> &Cell<IoEvents> {
        &self.revents
    }

    /// Returns the desired `revents` value if the file does not exist.
    pub(self) fn revents_for_missing_file(&self) -> IoEvents {
        if self.fd.is_some() {
            IoEvents::NVAL
        } else {
            IoEvents::empty()
        }
    }
}

impl From<c_pollfd> for PollFd {
    fn from(raw: c_pollfd) -> Self {
        let fd = FileDesc::try_from(raw.fd).ok();
        let events = IoEvents::from_bits_truncate(raw.events as _);
        let revents = Cell::new(IoEvents::from_bits_truncate(raw.revents as _));
        Self {
            fd,
            events,
            revents,
        }
    }
}
