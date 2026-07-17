// SPDX-License-Identifier: MPL-2.0

//! Self-contained Sixel DCS image decoder and renderer.
//!
//! This module implements a minimal Sixel image protocol decoder for the
//! framebuffer console. Since the kei kernel cannot use external crates
//! like `icy_sixel` (which the kou project delegates to), this is a
//! from-scratch implementation of the Sixel VT-380 format.
//!
//! ## Sixel format overview
//!
//! A Sixel image is transmitted as a DCS (Device Control String) sequence:
//!   `ESC P q <data> ESC \`
//!
//! Within `<data>`, the following commands are recognized:
//! - `"P1;P2;P3;P4;P5;P6` — raster attributes (P3=width, P4=height in pixels)
//! - `#N` — select color register N
//! - `#N;Co;R;G;B` — define color register N (Co=2 means RGB, values 0–100)
//! - `!N<char>` — repeat the next sixel character N times
//! - `$` — carriage return (move to start of current sixel row)
//! - `-` — sixel newline (advance one sixel row = 6 pixels down)
//! - `0x3f..=0x7e` — data chars encoding 6 vertical pixels (bit 0 = topmost)

use alloc::vec::Vec;

use crate::pixel::Pixel;

/// Maximum number of color registers (VT-330 default).
const MAX_REGISTERS: usize = 256;

/// A decoded Sixel image as a flat pixel buffer.
pub(crate) struct SixelImage {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Pixel data in row-major order (top-to-bottom, left-to-right).
    pub pixels: Vec<Pixel>,
}

/// The default Sixel color palette (VT-330 16-color set).
fn default_palette() -> [Pixel; 16] {
    [
        Pixel {
            red: 0,
            green: 0,
            blue: 0,
        }, // 0: Black
        Pixel {
            red: 51,
            green: 51,
            blue: 204,
        }, // 1: Blue
        Pixel {
            red: 204,
            green: 51,
            blue: 51,
        }, // 2: Red
        Pixel {
            red: 51,
            green: 204,
            blue: 51,
        }, // 3: Green
        Pixel {
            red: 204,
            green: 51,
            blue: 204,
        }, // 4: Magenta
        Pixel {
            red: 51,
            green: 204,
            blue: 204,
        }, // 5: Cyan
        Pixel {
            red: 204,
            green: 204,
            blue: 51,
        }, // 6: Yellow
        Pixel {
            red: 204,
            green: 204,
            blue: 204,
        }, // 7: White (50%)
        Pixel {
            red: 51,
            green: 51,
            blue: 51,
        }, // 8: Gray (25%)
        Pixel {
            red: 51,
            green: 51,
            blue: 255,
        }, // 9: Bright Blue
        Pixel {
            red: 255,
            green: 51,
            blue: 51,
        }, // 10: Bright Red
        Pixel {
            red: 51,
            green: 255,
            blue: 51,
        }, // 11: Bright Green
        Pixel {
            red: 255,
            green: 51,
            blue: 255,
        }, // 12: Bright Magenta
        Pixel {
            red: 51,
            green: 255,
            blue: 255,
        }, // 13: Bright Cyan
        Pixel {
            red: 255,
            green: 255,
            blue: 51,
        }, // 14: Bright Yellow
        Pixel {
            red: 255,
            green: 255,
            blue: 255,
        }, // 15: White (100%)
    ]
}

/// Decodes a Sixel DCS data payload (the bytes between `DCS q` and `ST`)
/// into an RGB pixel buffer.
///
/// `dcs_data` should be the raw Sixel data WITHOUT the `ESC P` introducer,
/// the `q` command byte, or the `ESC \` terminator. Just the data payload.
pub(crate) fn decode(dcs_data: &[u8]) -> Option<SixelImage> {
    let mut palette = default_palette();

    let mut current_color: u32 = 0;
    let mut raster_w: Option<usize> = None;
    let mut raster_h: Option<usize> = None;

    // Grid: grid_pixels[y][x] = Pixel. Grows dynamically as data arrives.
    let mut grid_pixels: Vec<Vec<Pixel>> = Vec::new();
    let mut grid_width: usize = 0;
    let mut grid_height: usize = 0;
    let mut cur_x: usize = 0;
    let mut cur_sixel_y: usize = 0; // in units of 6 pixels (sixel rows)

    let mut i = 0;

    while i < dcs_data.len() {
        let c = dcs_data[i];

        match c {
            // Raster attributes: "P1;P2;P3;P4;P5;P6"
            b'"' => {
                let (params, consumed) = parse_params(&dcs_data[i + 1..]);
                if params.len() >= 4 {
                    raster_w = Some(params[2] as usize);
                    raster_h = Some(params[3] as usize);
                }
                i += 1 + consumed;
            }

            // Color register selection / definition: #N or #N;Co;R;G;B
            b'#' => {
                let (params, consumed) = parse_params(&dcs_data[i + 1..]);
                if params.is_empty() {
                    current_color = 0;
                } else if params.len() >= 5 {
                    // Color definition: #N;Co;R;G;B (Co=2 means RGB)
                    let reg = params[0] as usize;
                    let _color_type = params[1];
                    let r = params[2];
                    let g = params[3];
                    let b = params[4];
                    let pixel = Pixel {
                        red: (r * 255 / 100) as u8,
                        green: (g * 255 / 100) as u8,
                        blue: (b * 255 / 100) as u8,
                    };
                    if reg < MAX_REGISTERS {
                        palette[reg % 16] = pixel;
                        current_color = reg as u32;
                    }
                } else {
                    current_color = params[0];
                }
                i += 1 + consumed;
            }

            // Repeat: !N<char>
            b'!' => {
                let (params, consumed) = parse_params(&dcs_data[i + 1..]);
                let count = if params.is_empty() {
                    1
                } else {
                    params[0] as usize
                };
                i += 1 + consumed;

                // The next byte should be a sixel data char.
                if i < dcs_data.len() {
                    let sixel_char = dcs_data[i];
                    if (0x3f..=0x7e).contains(&sixel_char) {
                        let bits = (sixel_char - 0x3f) as u32;
                        let pixel = palette[(current_color as usize) % 16];

                        for _ in 0..count {
                            ensure_grid_size(
                                &mut grid_pixels,
                                &mut grid_width,
                                &mut grid_height,
                                cur_x + 1,
                                (cur_sixel_y + 1) * 6,
                            );
                            write_sixel_column(
                                &mut grid_pixels,
                                grid_width,
                                grid_height,
                                cur_x,
                                cur_sixel_y,
                                bits,
                                pixel,
                            );
                            cur_x += 1;
                        }
                    }
                    i += 1;
                }
            }

            // Carriage return: move to start of current sixel row
            b'$' => {
                cur_x = 0;
                i += 1;
            }

            // Newline: advance to next sixel row (6 pixels down)
            b'-' => {
                cur_x = 0;
                cur_sixel_y += 1;
                i += 1;
            }

            // Data characters: 0x3f..=0x7e (6 vertical pixels)
            0x3f..=0x7e => {
                let bits = (c - 0x3f) as u32;
                let pixel = palette[(current_color as usize) % 16];

                ensure_grid_size(
                    &mut grid_pixels,
                    &mut grid_width,
                    &mut grid_height,
                    cur_x + 1,
                    (cur_sixel_y + 1) * 6,
                );
                write_sixel_column(
                    &mut grid_pixels,
                    grid_width,
                    grid_height,
                    cur_x,
                    cur_sixel_y,
                    bits,
                    pixel,
                );
                cur_x += 1;
                i += 1;
            }

            // Skip digits/semicolons that are part of the current parameter block
            // (already consumed by parse_params in the command handlers above).
            // Also skip ESC and any other stray bytes.
            _ => {
                i += 1;
            }
        }
    }

    // Use raster dimensions if provided, otherwise use computed grid size.
    let width = raster_w.unwrap_or(grid_width);
    let height = raster_h.unwrap_or(grid_height);

    if width == 0 || height == 0 {
        return None;
    }

    // Flatten grid into a row-major pixel buffer.
    let mut flat_pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            if y < grid_height && x < grid_width {
                flat_pixels.push(grid_pixels[y][x]);
            } else {
                flat_pixels.push(Pixel {
                    red: 0,
                    green: 0,
                    blue: 0,
                });
            }
        }
    }

    Some(SixelImage {
        width,
        height,
        pixels: flat_pixels,
    })
}

/// Ensures the grid has at least `need_w` columns and `need_h` rows,
/// growing it as necessary (new cells default to black).
fn ensure_grid_size(
    grid: &mut Vec<Vec<Pixel>>,
    grid_width: &mut usize,
    grid_height: &mut usize,
    need_w: usize,
    need_h: usize,
) {
    if need_w > *grid_width {
        for row in grid.iter_mut() {
            row.resize(
                need_w,
                Pixel {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            );
        }
        *grid_width = need_w;
    }
    if need_h > *grid_height {
        for _ in *grid_height..need_h {
            grid.push(alloc::vec![Pixel { red: 0, green: 0, blue: 0 }; *grid_width]);
        }
        *grid_height = need_h;
    }
}

/// Writes a single sixel column: 6 vertical pixels at (x, sixel_y*6..sixel_y*6+6).
/// Only pixels whose corresponding bit in `bits` is 1 are written; the rest
/// remain unchanged (transparent/background).
fn write_sixel_column(
    grid: &mut [Vec<Pixel>],
    grid_width: usize,
    _grid_height: usize,
    x: usize,
    sixel_y: usize,
    bits: u32,
    color: Pixel,
) {
    if x >= grid_width {
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

/// Parses semicolon-separated numeric parameters from the start of a byte slice.
/// Returns (params, bytes_consumed). Stops at the first non-parameter byte.
fn parse_params(data: &[u8]) -> (Vec<u32>, usize) {
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
