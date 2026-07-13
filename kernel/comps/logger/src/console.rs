// SPDX-License-Identifier: MPL-2.0

//! `print` and `println` macros
//!
//! FIXME: It will print to all `virtio-console` devices, which is not a good choice.
//!

use alloc::{collections::btree_map::BTreeMap, fmt, string::String, sync::Arc};
use core::fmt::Write;

use aster_console::AnyConsoleDevice;
use ostd::sync::{LocalIrqDisabled, SpinLockGuard};

/// Prints the formatted arguments to the standard output.
pub fn _print(args: fmt::Arguments) {
    // riscv64: core::unicode::conversions triggers a div-by-zero panic during
    // format_args write_fmt (a rustc/core bug on riscv64 where a casemapping
    // table constant evaluates to 0). Always use early_print (raw serial,
    // no unicode lookup) on riscv64 to avoid this.
    #[cfg(target_arch = "riscv64")]
    {
        ostd::console::early_print(args);
        return;
    }

    // If the console component hasn't been initialized yet (e.g., during
    // early aarch64 boot), fall back to early_print (raw serial output).
    // This prevents panics when info!/println! are called before the
    // component system is set up.
    #[cfg(not(target_arch = "riscv64"))]
    let Some(component) = aster_console::component() else {
        ostd::console::early_print(args);
        return;
    };

    // We must call lock on the component's device table to prevent
    // interleaving and avoid clone-related deadbacks under low memory.
    #[cfg(not(target_arch = "riscv64"))]
    {
        let devices = component.console_device_table.lock();

        struct Printer<'a>(
            SpinLockGuard<'a, BTreeMap<String, Arc<dyn AnyConsoleDevice>>, LocalIrqDisabled>,
        );
        impl Write for Printer<'_> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                if self.0.is_empty() {
                    ostd::early_print!("{}", s);
                } else {
                    for console in self.0.values() {
                        console.send(s.as_bytes());
                    }
                }
                Ok(())
            }
        }

        Printer(devices).write_fmt(args).unwrap();
    }
}

/// Copied from Rust std: <https://github.com/rust-lang/rust/blob/master/library/std/src/macros.rs>
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::_print(format_args!($($arg)*));
    }};
}

/// Copied from Rust std: <https://github.com/rust-lang/rust/blob/master/library/std/src/macros.rs>
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::_print(::core::format_args_nl!($($arg)*));
    }};
}
