// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Arc;
use core::time::Duration;

use ostd::{debug, timer::Jiffies};

use super::{Iface, iter_all_ifaces};
use crate::{
    sched::{Nice, SchedPolicy},
    thread::kernel_thread::ThreadOptions,
    time::wait::WaitTimeout,
};

pub fn init_in_first_kthread() {
    for iface in iter_all_ifaces() {
        spawn_background_poll_thread(iface.clone());
    }
}

pub(crate) fn poll_ifaces() {
    for iface in iter_all_ifaces() {
        iface.poll();
    }
}

fn spawn_background_poll_thread(iface: Arc<Iface>) {
    let task_fn = move || {
        debug!("spawn background poll thread for {:?}", iface.name());

        // On aarch64, virtio-mmio IRQ delivery is unreliable (the device may
        // not fire interrupts on packet arrival/departure). Use a busy-poll
        // approach: poll the iface at a fixed 2ms interval AND raise TX/RX
        // softirqs to process the send/recv virtqueues, bypassing IRQ delivery.
        #[cfg(target_arch = "aarch64")]
        {
            let sched_poll = iface.sched_poll();
            let wait_queue = sched_poll.polling_wait_queue();
            loop {
                aster_network::raise_send_softirq();
                aster_network::raise_receive_softirq();
                iface.poll();
                let _ = wait_queue.wait_until_or_timeout(|| None::<()>, &Duration::from_millis(2));
            }
        }

        // On other architectures, use IRQ-driven scheduling.
        #[cfg(not(target_arch = "aarch64"))]
        {
            let sched_poll = iface.sched_poll();
            let wait_queue = sched_poll.polling_wait_queue();

            loop {
                let next_poll_at_ms = if let Some(next_poll_at_ms) = sched_poll.next_poll_at_ms() {
                    next_poll_at_ms
                } else {
                    wait_queue.wait_until(|| sched_poll.next_poll_at_ms())
                };

                let now_as_ms = Jiffies::elapsed().as_duration().as_millis() as u64;

                if now_as_ms >= next_poll_at_ms {
                    iface.poll();
                    continue;
                }

                let duration = Duration::from_millis(next_poll_at_ms - now_as_ms);
                let _ = wait_queue.wait_until_or_timeout(
                    || (sched_poll.next_poll_at_ms()? < next_poll_at_ms).then_some(()),
                    &duration,
                );
            }
        }
    };

    ThreadOptions::new(task_fn)
        .sched_policy(SchedPolicy::Fair(Nice::MIN))
        .spawn();
}
