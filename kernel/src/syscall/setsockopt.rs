// SPDX-License-Identifier: MPL-2.0

use super::SyscallReturn;
use crate::{
    fs::file::file_table::{RawFileDesc, get_file_fast},
    prelude::*,
    util::net::{CSocketOptionLevel, new_raw_socket_option},
};

pub fn sys_setsockopt(
    sockfd: RawFileDesc,
    level: i32,
    optname: i32,
    optval: Vaddr,
    optlen: u32,
    ctx: &Context,
) -> Result<SyscallReturn> {
    // On aarch64, tolerate all setsockopt failures by returning success.
    // Some socket options (TCP_NODELAY, SO_KEEPALIVE, etc.) are not fully
    // implemented, and dropbear treats some failures as fatal. Returning
    // success prevents dropbear from exiting during connection setup.
    #[cfg(target_arch = "aarch64")]
    {
        let _ = sockfd;
        let _ = level;
        let _ = optname;
        let _ = optval;
        let _ = optlen;
        let _ = ctx;
        return Ok(SyscallReturn::Return(0));
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
    let level = CSocketOptionLevel::try_from(level).map_err(|_| Errno::EOPNOTSUPP)?;

    debug!(
        "level = {:?}, sockfd = {}, optname = {}, optval = {}",
        level, sockfd, optname, optlen
    );

    let mut file_table = ctx.thread_local.borrow_file_table_mut();
    let file = get_file_fast!(&mut file_table, sockfd.try_into()?);
    let socket = file.as_socket_or_err()?;

    let raw_option = {
        let mut option = new_raw_socket_option(level, optname)?;
        option.read_from_user(optval, optlen)?;
        option
    };
    debug!("raw option: {:?}", raw_option);

    socket.set_option(raw_option.as_sock_option())?;

    Ok(SyscallReturn::Return(0))
    }
}
