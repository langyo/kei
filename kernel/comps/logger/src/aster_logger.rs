// SPDX-License-Identifier: MPL-2.0

use core::time::Duration;

use ostd::{
    log::{Level, Record},
    timer::Jiffies,
};

/// The logger used for Asterinas.
struct AsterLogger;

static LOGGER: AsterLogger = AsterLogger;

impl ostd::log::Log for AsterLogger {
    fn log(&self, record: &Record) {
        let timestamp = Jiffies::elapsed().as_duration();
        print_logs(record, &timestamp);
    }
}

#[cfg(feature = "log_color")]
fn print_logs(record: &Record, timestamp: &Duration) {
    use owo_colors::Style;

    let secs = timestamp.as_secs();
    let millis = timestamp.subsec_millis();

    let timestamp_style = Style::new().green();
    let record_style = Style::new().default_color();
    let level_style = match record.level() {
        Level::Error => Style::new().red(),
        Level::Warning => Style::new().bright_yellow(),
        Level::Info => Style::new().blue(),
        Level::Debug => Style::new().bright_green(),
        Level::Notice => Style::new().cyan(),
        Level::Emerg | Level::Alert | Level::Crit => Style::new().red().bold(),
    };

    let module = record.module_path();

    super::_print(format_args!(
        "{} {:<5} {:<24}: {}{}\n",
        timestamp_style.style(format_args!("[{:>6}.{:03}]", secs, millis)),
        level_style.style(record.level()),
        timestamp_style.style(module),
        record_style.style(record.prefix()),
        record_style.style(record.args())
    ));
}

#[cfg(not(feature = "log_color"))]
fn print_logs(record: &Record, timestamp: &Duration) {
    let secs = timestamp.as_secs();
    let millis = timestamp.subsec_millis();
    let module = record.module_path();

    super::_print(format_args!(
        "[{:>6}.{:03}] {:<5} {:<24}: {}{}\n",
        secs,
        millis,
        record.level(),
        module,
        record.prefix(),
        record.args()
    ));
}

pub(super) fn init() {
    // riscv64: do NOT inject the formatting logger. The LOGGER's log callback
    // uses format_args! + write_fmt which triggers a div-by-zero in
    // core::unicode::conversions (a rustc/core bug on riscv64). Without the
    // injected logger, log messages are silently dropped (early_println! still
    // works for boot diagnostics via raw serial).
    #[cfg(not(target_arch = "riscv64"))]
    ostd::log::inject_logger(&LOGGER);
}
