// SPDX-License-Identifier: MPL-2.0

//! Display controller chip implementations.
//! Detection is based on device tree compatible strings.

use alloc::boxed::Box;
use crate::DisplayController;

pub mod rk3566;

pub fn probe_display_controller() -> Option<Box<dyn DisplayController>> {
    if let Some(ctrl) = rk3566::probe() {
        return Some(Box::new(ctrl));
    }
    None
}
