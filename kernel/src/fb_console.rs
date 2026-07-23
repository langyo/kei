// SPDX-License-Identifier: MPL-2.0

//! Minimal framebuffer console for the aarch64 virtio-gpu display.
//!
//! Renders kernel boot log text directly into the virtio-gpu framebuffer
//! using an embedded 8x8 bitmap font, then flushes to the device. This runs
//! without the heap and without the component system, so it works at the
//! raw boot stage where the virtio-gpu driver initializes.

#![allow(unsafe_code)]
#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const CHAR_W: usize = 8;
const CHAR_H: usize = 8;
const COLS: usize = 80; // 640/8=80 cols
const ROWS: usize = 60; // 480/8=60 rows

static CURSOR_COL: AtomicUsize = AtomicUsize::new(0);
static CURSOR_ROW: AtomicUsize = AtomicUsize::new(0);

/// The current foreground color for text rendered by `write_byte_color`.
/// Starts as the One Half Dark default foreground; updated by SGR sequences.
static CURRENT_FG: AtomicUsize = AtomicUsize::new(FG_COLOR as usize);

/// Modern dark theme colors (One Half Dark from the kou project), XRGB8888.
const BG_COLOR: u32 = 0xFF282C34; // One Half Dark background
const FG_COLOR: u32 = 0xFFDCDFE4; // One Half Dark foreground (soft white)
const ACCENT_COLOR: u32 = 0xFF61AFEF; // One Half Dark blue accent for banner

/// One Half Dark 16-color ANSI palette, ported from kou's render.rs.
/// Index 0–7 = standard colors, 8–15 = bright variants.
#[rustfmt::skip]
const ANSI_PALETTE: [u32; 16] = [
    0xFF282C34, // 0  Black
    0xFFE06C75, // 1  Red
    0xFF98C379, // 2  Green
    0xFFE5C07B, // 3  Yellow
    0xFF61AFEF, // 4  Blue
    0xFFC678DD, // 5  Magenta
    0xFF56B6C2, // 6  Cyan
    0xFFDCDFE4, // 7  White
    0xFF5A6374, // 8  Bright Black (dim gray)
    0xFFE06C75, // 9  Bright Red
    0xFF98C379, // 10 Bright Green
    0xFFE5C07B, // 11 Bright Yellow
    0xFF61AFEF, // 12 Bright Blue
    0xFFC678DD, // 13 Bright Magenta
    0xFF56B6C2, // 14 Bright Cyan
    0xFFDCDFE4, // 15 Bright White
];

/// Clear the framebuffer, reset the cursor, and draw a title banner.
pub fn init() {
    clear();
    draw_banner();
}

fn clear() {
    if let Some((fb, w, h, _stride)) = crate::fb_gpu::framebuffer_info() {
        unsafe {
            let p = fb as *mut u32;
            let n = (w as usize) * (h as usize);
            for i in 0..n {
                core::ptr::write_volatile(p.add(i), BG_COLOR);
            }
        }
        crate::fb_gpu::flush_framebuffer();
    }
    CURSOR_COL.store(0, Ordering::Relaxed);
    CURSOR_ROW.store(0, Ordering::Relaxed);
}

fn draw_banner() {
    print_str("\x1b[34m kei kernel (aarch64) \x1b[0m\n");
    print_str("\x1b[36m framebuffer console \x1b[0m\n\n");
}

/// Public print: write a string with ANSI SGR color support, scroll if needed.
///
/// Parses minimal SGR escape sequences (`\x1b[Nm` and `\x1b[N;Mm`) to set the
/// foreground color from the One Half Dark ANSI palette. This lets kernel boot
/// log messages (which often use ANSI colors via the `log` crate) display in
/// full color on the framebuffer console.
pub fn print_str(s: &str) {
    print_str_ansi(s);
    // Note: flush is intentionally NOT called here to avoid flooding the
    // virtio-gpu command queue. The framebuffer is flushed periodically by
    // init() and by explicit flush calls. This prevents the QEMU error
    // "Guest says index N is available" caused by queue overflow.
}

/// Internal: parse ANSI escape sequences and render text with the current color.
/// Also handles DCS Sixel sequences for inline image rendering.
fn print_str_ansi(s: &str) {
    // State machine: ESC [ params... m (SGR) and ESC P q ... ESC \ (Sixel DCS)
    #[derive(PartialEq)]
    enum State {
        Normal,
        Escape,   // saw ESC (0x1b)
        Csi,      // saw ESC [
        CsiParam, // accumulating digits after ESC [
        DcsParam, // saw ESC P, waiting for 'q' command byte
        DcsData,  // accumulating sixel data until ESC \
        DcsEsc,   // saw ESC inside DCS, waiting for \ (ST)
    }

    let bytes = s.as_bytes();
    let mut i = 0;
    let mut state = State::Normal;
    let mut param_buf: u32 = 0;
    let mut has_param = false;
    // Sixel data accumulator (static mutable buffer for no_std boot console).
    // SAFETY: Only accessed from the single-threaded boot console path.
    static mut DCS_BUF: [u8; 4096] = [0; 4096];
    static mut DCS_LEN: usize = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match state {
            State::Normal => {
                if b == 0x1b {
                    state = State::Escape;
                } else {
                    write_byte_color(b, CURRENT_FG.load(Ordering::Relaxed) as u32);
                }
            }
            State::Escape => {
                match b {
                    b'[' => {
                        state = State::Csi;
                        param_buf = 0;
                        has_param = false;
                    }
                    b'P' => {
                        // DCS (Device Control String) — start Sixel accumulation
                        ostd::early_println!("[fb_console] DCS detected (ESC P)");
                        unsafe {
                            DCS_LEN = 0;
                        }
                        state = State::DcsParam;
                    }
                    0x1b => {
                        // Another ESC, stay in Escape
                    }
                    _ => {
                        state = State::Normal;
                        write_byte_color(b, CURRENT_FG.load(Ordering::Relaxed) as u32);
                    }
                }
            }
            State::DcsParam => {
                match b {
                    b'q' => {
                        // Sixel command byte found — start accumulating data
                        ostd::early_println!("[fb_console] Sixel 'q' command found");
                        state = State::DcsData;
                    }
                    0x1b => {
                        // ESC inside DCS params — might be ST
                        state = State::DcsEsc;
                    }
                    _ => {
                        // DCS parameters (digits, semicolons) — skip
                    }
                }
            }
            State::DcsData => {
                match b {
                    0x1b => {
                        state = State::DcsEsc;
                    }
                    _ => {
                        // Accumulate sixel data
                        unsafe {
                            let len = DCS_LEN;
                            if len < 4096 {
                                (*core::ptr::addr_of_mut!(DCS_BUF))[len] = b;
                                DCS_LEN = len + 1;
                            }
                        }
                    }
                }
            }
            State::DcsEsc => {
                if b == b'\\' {
                    // ST received — decode and render the Sixel image
                    let data: Vec<u8> = unsafe {
                        let len = DCS_LEN;
                        let buf = core::ptr::addr_of!(DCS_BUF) as *const u8;
                        core::slice::from_raw_parts(buf, len).to_vec()
                    };
                    ostd::early_println!(
                        "[fb_console] ST received, dcs_len={}, rendering sixel",
                        data.len()
                    );
                    render_sixel_inline(&data);
                    state = State::Normal;
                } else if b == 0x1b {
                    // Stay in DcsEsc
                } else {
                    // Not ST — treat as data
                    unsafe {
                        let len = DCS_LEN;
                        if len + 1 < 4096 {
                            (*core::ptr::addr_of_mut!(DCS_BUF))[len] = 0x1b;
                            (*core::ptr::addr_of_mut!(DCS_BUF))[len + 1] = b;
                            DCS_LEN = len + 2;
                        }
                    }
                    state = State::DcsData;
                }
            }
            State::Csi => {
                if b.is_ascii_digit() {
                    param_buf = param_buf
                        .saturating_mul(10)
                        .saturating_add((b - b'0') as u32);
                    has_param = true;
                    state = State::CsiParam;
                } else if b == b'm' {
                    // SGR with no param = reset (SGR 0).
                    if !has_param {
                        CURRENT_FG.store(FG_COLOR as usize, Ordering::Relaxed);
                    }
                    state = State::Normal;
                } else if b == b';' {
                    // Separator — skip (we only handle single-param SGR for fg).
                    state = State::CsiParam;
                } else {
                    // Unknown CSI final byte — swallow the sequence.
                    state = State::Normal;
                }
            }
            State::CsiParam => {
                if b.is_ascii_digit() {
                    param_buf = param_buf
                        .saturating_mul(10)
                        .saturating_add((b - b'0') as u32);
                    has_param = true;
                } else if b == b';' {
                    // Multi-param SGR — we only apply the first param for fg color.
                    // Fall through and keep parsing.
                } else if b == b'm' {
                    apply_sgr(param_buf);
                    param_buf = 0;
                    has_param = false;
                    state = State::Normal;
                } else {
                    // Unknown CSI final byte — swallow.
                    state = State::Normal;
                }
            }
        }
        i += 1;
    }
}

/// Apply a single SGR code to the current foreground color.
fn apply_sgr(code: u32) {
    let color = match code {
        0 => Some(FG_COLOR),          // Reset to default fg
        30 => Some(ANSI_PALETTE[0]),  // Black
        31 => Some(ANSI_PALETTE[1]),  // Red
        32 => Some(ANSI_PALETTE[2]),  // Green
        33 => Some(ANSI_PALETTE[3]),  // Yellow
        34 => Some(ANSI_PALETTE[4]),  // Blue
        35 => Some(ANSI_PALETTE[5]),  // Magenta
        36 => Some(ANSI_PALETTE[6]),  // Cyan
        37 => Some(ANSI_PALETTE[7]),  // White
        39 => Some(FG_COLOR),         // Default fg
        90 => Some(ANSI_PALETTE[8]),  // Bright Black
        91 => Some(ANSI_PALETTE[9]),  // Bright Red
        92 => Some(ANSI_PALETTE[10]), // Bright Green
        93 => Some(ANSI_PALETTE[11]), // Bright Yellow
        94 => Some(ANSI_PALETTE[12]), // Bright Blue
        95 => Some(ANSI_PALETTE[13]), // Bright Magenta
        96 => Some(ANSI_PALETTE[14]), // Bright Cyan
        97 => Some(ANSI_PALETTE[15]), // Bright White
        _ => None,                    // Unsupported SGR (bold, underline, etc.) — ignore
    };
    if let Some(c) = color {
        CURRENT_FG.store(c as usize, Ordering::Relaxed);
    }
}

/// Print a string with a specific foreground color (bypasses ANSI parsing).
pub fn print_str_color(s: &str, color: u32) {
    for &b in s.as_bytes() {
        write_byte_color(b, color);
    }
    crate::fb_gpu::flush_framebuffer();
}

pub fn println(s: &str) {
    print_str(s);
    print_str("\n");
}

fn write_byte(byte: u8) {
    write_byte_color(byte, CURRENT_FG.load(Ordering::Relaxed) as u32);
}

fn write_byte_color(byte: u8, color: u32) {
    match byte {
        b'\n' => {
            CURSOR_COL.store(0, Ordering::Relaxed);
            let r = CURSOR_ROW.fetch_add(1, Ordering::Relaxed) + 1;
            if r >= ROWS {
                scroll();
            }
        }
        b'\r' => {
            CURSOR_COL.store(0, Ordering::Relaxed);
        }
        0x20..=0x7e => {
            let col = CURSOR_COL.load(Ordering::Relaxed);
            let row = CURSOR_ROW.load(Ordering::Relaxed);
            if col < COLS && row < ROWS {
                draw_char_color(byte, col, row, color);
            }
            CURSOR_COL.store(col + 1, Ordering::Relaxed);
        }
        _ => {
            // Non-printable: draw a placeholder dot.
            write_byte_color(b'.', color);
        }
    }
}

fn scroll() {
    // Move every row up by one (row n+1 → row n), clear the last row.
    if let Some((fb, w, h, _stride)) = crate::fb_gpu::framebuffer_info() {
        let stride = w as usize;
        unsafe {
            let p = fb as *mut u32;
            for y in 0..(h as usize - CHAR_H) {
                for x in 0..stride {
                    let src = (y + CHAR_H) * stride + x;
                    let dst = y * stride + x;
                    core::ptr::write_volatile(p.add(dst), core::ptr::read_volatile(p.add(src)));
                }
            }
            // Clear the bottom CHAR_H rows.
            for y in (h as usize - CHAR_H)..(h as usize) {
                for x in 0..stride {
                    core::ptr::write_volatile(p.add(y * stride + x), BG_COLOR);
                }
            }
        }
    }
    CURSOR_ROW.store(ROWS - 1, Ordering::Relaxed);
}

fn draw_char(byte: u8, col: usize, row: usize) {
    draw_char_color(byte, col, row, FG_COLOR);
}

fn draw_char_color(byte: u8, col: usize, row: usize, fg: u32) {
    let glyph = font8x8_glyph(byte);
    if let Some((fb, w, _h, _stride)) = crate::fb_gpu::framebuffer_info() {
        let stride = w as usize;
        let x0 = col * CHAR_W;
        let y0 = row * CHAR_H;
        unsafe {
            let p = fb as *mut u32;
            for gy in 0..CHAR_H {
                let bits = glyph[gy];
                for gx in 0..CHAR_W {
                    let on = (bits >> gx) & 1 == 1;
                    let color = if on { fg } else { BG_COLOR };
                    let px = x0 + gx;
                    let py = y0 + gy;
                    if px < stride {
                        core::ptr::write_volatile(p.add(py * stride + px), color);
                    }
                }
            }
        }
    }
}

/// 8x8 bitmap font for printable ASCII (code points 0x20..=0x7e).
/// Each glyph is 8 bytes; bit `i` of byte `row` is column i (LSB = leftmost).
/// Derived from the public-domain font8x8 "legacy" BASIC subset.
fn font8x8_glyph(c: u8) -> [u8; 8] {
    // Only 0x20..=0x7e are meaningful; everything else is blank.
    const FONT: [[u8; 8]; 96] = include!("fb_console_font.rs");
    let idx = (c as usize).wrapping_sub(0x20);
    if idx < 96 { FONT[idx] } else { [0; 8] }
}

// ── Sixel inline image rendering ──────────────────────────────────────────

/// Renders a decoded Sixel image (row-major RGB pixels) directly onto the
/// framebuffer DMA buffer at the current cursor position.
fn render_sixel_inline(dcs_data: &[u8]) {
    // Decode the sixel data into RGB pixels
    let (width, height, pixels) = match decode_sixel(dcs_data) {
        Some(img) => img,
        None => {
            ostd::early_println!("[fb_console] sixel decode returned None");
            return;
        }
    };
    ostd::early_println!(
        "[fb_console] sixel decoded: {}x{}, first pixel={:#x}",
        width,
        height,
        pixels.get(0).copied().unwrap_or(0)
    );
    if width == 0 || height == 0 {
        return;
    }

    if let Some((fb, fb_w, fb_h, _stride)) = crate::fb_gpu::framebuffer_info() {
        let stride = fb_w as usize;
        let col = CURSOR_COL.load(Ordering::Relaxed) * CHAR_W;
        let row = CURSOR_ROW.load(Ordering::Relaxed) * CHAR_H;
        ostd::early_println!(
            "[fb_console] rendering at ({},{}) fb={}x{}",
            col,
            row,
            fb_w,
            fb_h
        );

        unsafe {
            let p = fb as *mut u32;
            for py in 0..height {
                let dst_y = row + py;
                if dst_y >= fb_h as usize {
                    break;
                }
                for px in 0..width {
                    let dst_x = col + px;
                    if dst_x >= stride {
                        break;
                    }
                    let pixel = pixels[py * width + px];
                    core::ptr::write_volatile(p.add(dst_y * stride + dst_x), pixel);
                }
            }
        }
        crate::fb_gpu::flush_framebuffer();
        ostd::early_println!("[fb_console] sixel rendered and flushed");
    } else {
        ostd::early_println!("[fb_console] WARNING: framebuffer_info() returned None");
    }

    // Advance cursor past the image
    let img_rows = (height + CHAR_H - 1) / CHAR_H;
    let new_row = CURSOR_ROW.fetch_add(img_rows, Ordering::Relaxed) + img_rows;
    if new_row >= ROWS {
        scroll();
    }
    CURSOR_COL.store(0, Ordering::Relaxed);
    crate::fb_gpu::flush_framebuffer();
}

/// Decodes Sixel data into (width, height, pixels) where pixels is a
/// row-major array of XRGB8888 u32 values.
fn decode_sixel(data: &[u8]) -> Option<(usize, usize, Vec<u32>)> {
    // VT-330 default palette (16 colors)
    // Mutable palette: starts with VT-330 defaults, updated by #N;Co;R;G;B.
    let mut palette: Vec<u32> = vec![
        0xFF000000, 0xFF3333CC, 0xFFCC3333, 0xFF33CC33, 0xFFCC33CC, 0xFF33CCCC, 0xFFCCCC33,
        0xFFCCCCCC, 0xFF333333, 0xFF3333FF, 0xFFFF3333, 0xFF33FF33, 0xFFFF33FF, 0xFF33FFFF,
        0xFFFFFF33, 0xFFFFFFFF,
    ];
    // Extend palette to 256 entries for safety.
    palette.resize(256, 0xFF000000);

    let mut current_color: usize = 0;
    let mut grid: Vec<Vec<u32>> = Vec::new();
    let mut grid_w: usize = 0;
    let mut cur_x: usize = 0;
    let mut cur_sixel_y: usize = 0;

    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        match c {
            b'#' => {
                // Color register: #N or #N;Co;R;G;B
                let (params, consumed) = parse_sixel_params(&data[i + 1..]);
                if params.len() >= 5 {
                    // Define color register: #N;Co;R;G;B (Co=2 means RGB)
                    let reg = params[0] as usize;
                    let r = params[2];
                    let g = params[3];
                    let b = params[4];
                    let color = 0xFF000000
                        | ((r * 255 / 100) as u32) << 16
                        | ((g * 255 / 100) as u32) << 8
                        | (b * 255 / 100) as u32;
                    if reg < palette.len() {
                        palette[reg] = color;
                    }
                    current_color = reg;
                } else if !params.is_empty() {
                    current_color = params[0] as usize;
                } else {
                    current_color = 0;
                }
                i += 1 + consumed;
            }
            b'!' => {
                // Repeat: !N<char>
                let (params, consumed) = parse_sixel_params(&data[i + 1..]);
                let count = if params.is_empty() {
                    1
                } else {
                    params[0] as usize
                };
                i += 1 + consumed;
                if i < data.len() && (0x3f..=0x7e).contains(&data[i]) {
                    let bits = (data[i] - 0x3f) as u32;
                    let color = palette[current_color.min(255)];
                    for _ in 0..count {
                        ensure_grid(&mut grid, &mut grid_w, cur_x + 1, (cur_sixel_y + 1) * 6);
                        write_sixel_col(&mut grid, grid_w, cur_x, cur_sixel_y, bits, color);
                        cur_x += 1;
                    }
                }
                i += 1;
            }
            b'$' => {
                cur_x = 0;
                i += 1;
            }
            b'-' => {
                cur_x = 0;
                cur_sixel_y += 1;
                i += 1;
            }
            0x3f..=0x7e => {
                let bits = (c - 0x3f) as u32;
                let color = palette[current_color as usize];
                ensure_grid(&mut grid, &mut grid_w, cur_x + 1, (cur_sixel_y + 1) * 6);
                write_sixel_col(&mut grid, grid_w, cur_x, cur_sixel_y, bits, color);
                cur_x += 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let height = grid.len();
    if grid_w == 0 || height == 0 {
        return None;
    }

    let mut pixels = Vec::with_capacity(grid_w * height);
    for row in &grid {
        for x in 0..grid_w {
            pixels.push(if x < row.len() { row[x] } else { 0xFF000000 });
        }
    }

    Some((grid_w, height, pixels))
}

fn ensure_grid(grid: &mut Vec<Vec<u32>>, grid_w: &mut usize, need_w: usize, need_h: usize) {
    if need_w > *grid_w {
        for row in grid.iter_mut() {
            row.resize(need_w, 0xFF000000);
        }
        *grid_w = need_w;
    }
    while grid.len() < need_h {
        grid.push(alloc::vec![0xFF000000; *grid_w]);
    }
}

fn write_sixel_col(
    grid: &mut [Vec<u32>],
    grid_w: usize,
    x: usize,
    sixel_y: usize,
    bits: u32,
    color: u32,
) {
    if x >= grid_w {
        return;
    }
    for bit in 0..6 {
        if (bits >> bit) & 1 == 1 {
            let py = sixel_y * 6 + bit;
            if py < grid.len() {
                grid[py][x] = color;
            }
        }
    }
}

fn parse_sixel_params(data: &[u8]) -> (Vec<u32>, usize) {
    let mut params = Vec::new();
    let mut current: u32 = 0;
    let mut has_digit = false;
    let mut consumed = 0;
    for &b in data {
        match b {
            b'0'..=b'9' => {
                current = current.saturating_mul(10).saturating_add((b - b'0') as u32);
                has_digit = true;
                consumed += 1;
            }
            b';' => {
                params.push(if has_digit { current } else { 0 });
                current = 0;
                has_digit = false;
                consumed += 1;
            }
            _ => break,
        }
    }
    if has_digit {
        params.push(current);
    }
    (params, consumed)
}
