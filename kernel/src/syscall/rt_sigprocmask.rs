// SPDX-License-Identifier: MPL-2.0

use ostd::mm::VmIo;

use super::SyscallReturn;
use crate::{
    prelude::*,
    process::{posix_thread::ContextPthreadAdminApi, signal::sig_mask::{SigMask, SigSet}},
};

pub fn sys_rt_sigprocmask(
    how: u32,
    set_ptr: Vaddr,
    oldset_ptr: Vaddr,
    sigset_size: usize,
    ctx: &Context,
) -> Result<SyscallReturn> {
    let mask_op = MaskOp::try_from(how)?;
    debug!(
        "mask op = {:?}, set_ptr = 0x{:x}, oldset_ptr = 0x{:x}, sigset_size = {}",
        mask_op, set_ptr, oldset_ptr, sigset_size
    );
    // Accept any sigset_size >= 8. Newer musl may pass 128 (sizeof(sigset_t)).
    // The actual mask is always 8 bytes (64 signals on aarch64).
    if sigset_size < 8 {
        return_errno_with_message!(Errno::EINVAL, "sigset size is less than 8");
    }
    ostd::early_println!(
        "[sigprocmask] how={} set={:#x} old={:#x} sz={}",
        how, set_ptr, oldset_ptr, sigset_size
    );
    do_rt_sigprocmask(mask_op, set_ptr, oldset_ptr, ctx)?;
    ostd::early_println!("[sigprocmask] returning ok");
    Ok(SyscallReturn::Return(0))
}

fn do_rt_sigprocmask(
    mask_op: MaskOp,
    set_ptr: Vaddr,
    oldset_ptr: Vaddr,
    ctx: &Context,
) -> Result<()> {
    let old_sig_mask_value = ctx.posix_thread.sig_mask();
    debug!("old sig mask value: 0x{:x}", old_sig_mask_value);
    if oldset_ptr != 0 {
        ctx.user_space()
            .write_val(oldset_ptr, &old_sig_mask_value)?;
    }

    if set_ptr != 0 {
        let read_mask = ctx.user_space().read_val::<SigMask>(set_ptr)?;
        let old_bits = u64::from(old_sig_mask_value);
        let new_bits = u64::from(read_mask);
        // SIGKILL(9) and SIGSTOP(19) cannot be blocked (POSIX/Linux guarantee).
        let kill_stop: u64 = (1u64 << 8) | (1u64 << 18);
        let effective_bits = match mask_op {
            MaskOp::Block => (old_bits | (new_bits & !kill_stop)),
            MaskOp::Unblock => old_bits & !new_bits,
            MaskOp::SetMask => new_bits & !kill_stop,
        };
        ctx.set_sig_mask(SigSet::from(effective_bits));
    }
    debug!("new set = {:x?}", ctx.posix_thread.sig_mask());

    Ok(())
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromInt)]
pub enum MaskOp {
    Block = 0,
    Unblock = 1,
    SetMask = 2,
}
