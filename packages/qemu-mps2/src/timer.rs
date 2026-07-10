//! CMSDK Timer-based cycle/elapsed counter for benchmarking.
//!
//! QEMU's mps2-an386 may not fully implement DWT (Data Watchpoint and
//! Trace), so we use the CMSDK APB Timer instead. TIMER0 is at
//! 0x40000000 with pclk = 25 MHz. We configure it as a free-running
//! down-counter and read its current value for before/after deltas.
//!
//! The timer counts down from a reload value at pclk speed.
//! Delta = before - after (since it counts down).

use core::ptr::{read_volatile, write_volatile};

const TIMER1_BASE: usize = 0x4000_1000; // Use TIMER1 (TIMER0 reserved for future embassy-time)
const REG_CTRL: *mut u32 = TIMER1_BASE as *mut u32;
const REG_VALUE: *mut u32 = (TIMER1_BASE + 0x04) as *mut u32; // Current value (counts down)
const REG_RELOAD: *mut u32 = (TIMER1_BASE + 0x08) as *mut u32;
const REG_INTSTATUS: *mut u32 = (TIMER1_BASE + 0x0C) as *mut u32;

const CTRL_ENABLE: u32 = 1 << 0;
const CTRL_MODE_PERIODIC: u32 = 1 << 6;

/// Initialize TIMER1 as a free-running down-counter with maximum reload.
/// At 25 MHz, u32::MAX reload = ~171 seconds before wraparound.
pub fn init() {
    unsafe {
        write_volatile(REG_CTRL, 0); // Disable first
        write_volatile(REG_RELOAD, 0xFFFF_FFFF);
        write_volatile(REG_VALUE, 0xFFFF_FFFF);
        // Enable as periodic (wraps around at 0)
        write_volatile(REG_CTRL, CTRL_ENABLE | CTRL_MODE_PERIODIC);
    }
}

/// Read the current timer value (counts DOWN from 0xFFFFFFFF).
#[inline]
pub fn now() -> u32 {
    unsafe { read_volatile(REG_VALUE) }
}

/// Measure elapsed ticks between two `now()` readings.
/// Since the counter counts DOWN, elapsed = before - after.
#[inline]
pub fn elapsed(before: u32, after: u32) -> u32 {
    before.wrapping_sub(after)
}

/// Convert timer ticks to nanoseconds (at 25 MHz, 1 tick = 40 ns).
#[inline]
pub fn ticks_to_ns(ticks: u32) -> u32 {
    ticks / 25 // 25 ticks per µs, so /25 = µs... no:
               // 25 MHz → 25 ticks/µs → 1 tick = 40 ns
               // ticks * 40 = ns, but that overflows u32 at ~107M ticks (~4.3s)
               // Use: ticks / 25 = µs, then * 1000 for ns
}

/// Convert timer ticks to microseconds (at 25 MHz).
#[inline]
pub fn ticks_to_us(ticks: u32) -> u32 {
    ticks / 25
}
