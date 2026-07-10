// SPDX-License-Identifier: MPL-2.0

pub mod iface;
pub mod socket;
pub mod uts_ns;

pub fn init() {
    iface::init();
    socket::netlink::init();
    socket::vsock::init();
}

/// Lazy init should be called after spawning init thread.
pub fn init_in_first_kthread() {
    iface::init_in_first_kthread();
}

/// Poll all network interfaces. On aarch64, this is used by the
/// accept/read/write blocking path as a workaround for the unreliable
/// timer-based preemption system.
pub(crate) fn poll_ifaces() {
    iface::poll_ifaces();
}
