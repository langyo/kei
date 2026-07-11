// SPDX-License-Identifier: MPL-2.0

//! The logger implementation for Asterinas.
//!
//! This logger now has the most basic logging functionality, controls the output
//! based on the globally set log level. Different log levels will be represented
//! with different colors if enabling `log_color` feature.
//!
//! This logger guarantees _atomicity_ under concurrency: messages are always
//! printed in their entirety without being mixed with messages generated
//! concurrently on other cores.
//!
//! IRQs are disabled while printing. So do not print long log messages.
#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use component::{ComponentInitError, init_component};

// Set this crate's log prefix for `ostd::log`.
macro_rules! __log_prefix {
    () => {
        "logger: "
    };
}

mod aster_logger;
mod console;

pub use console::_print;

#[init_component]
fn init() -> Result<(), ComponentInitError> {
    aster_logger::init();
    Ok(())
}

/// Manual initialization for aarch64 where the component system is bypassed.
/// Call this early in the boot path to enable leveled logging with timestamps.
pub fn init_manual() {
    aster_logger::init();
    // Set the default log level to Info so boot messages are visible
    // without needing the ostd.log_level= kernel cmdline parameter.
    ostd::log::set_max_level(ostd::log::LevelFilter::Info);
}
