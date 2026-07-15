# Board Support Package crates for kei

#

# Each BSP provides device drivers for a specific SoC platform

# BSPs are compiled as OSDK library crates and linked into the

# patched Asterinas kernel at build time

#

# Adding a new BSP

# 1. Copy the template: cp -r bsp/rk3566 bsp/mysoc

# 2. Implement the driver modules in src/

# 3. Add to Cargo.toml workspace members

# 4. Add board/ config directory

# 5. Add board to configs/mysoc.toml

#

# ──────────────────────────────────────────────────────────────────

# BSP completion matrix (last reviewed 2026-07-14 by kei-echo)

# ──────────────────────────────────────────────────────────────────

#

# | BSP         | SoC / Board          | Status     | Drivers implemented                              | OSDK buildable |

# |-------------|----------------------|------------|--------------------------------------------------|----------------|

# | bcm2711     | Raspberry Pi 4 / CM4 | skeleton   | none (`pub fn init() {}` placeholder + guard)   | NO (compile_error!) |

# | jh7110      | StarFive VisionFive2 | skeleton   | none (`pub fn init() {}` placeholder + guard)   | NO (compile_error!) |

# | rk3566      | NanoPi R3S           | partial    | gpio, i2c, spi, uart (driver source only)        | NO (ostd dep gated, see note below) |

#

# Definitions

# skeleton  — only `lib.rs` with `pub fn init() {}`; will trigger `compile_error!` if linked

# partial   — driver source code present but the crate is not linkable without manual wiring

#

# Why the compile_error! on the two skeleton BSPs

# We want to fail loudly if anyone enables bcm2711/jh7110 in a build before drivers are

# implemented, rather than silently producing a kernel that boots with no input/network

# drivers and no clear error. Remove the `compile_error!` after adding the first real driver

#

# rk3566 build status

# The crate source compiles standalone, but linking into the kei kernel requires a

# path dep on the workspace `ostd` crate. That dep is intentionally commented out in

# `bsp/rk3566/Cargo.toml` until the patched-ostd → upstream-ostd merge stabilises

# (see `docs/en/guides/upstream-sync.md`). When uncommented, use

# `ostd = { path = "../../ostd", default-features = false }` and gate `#[cfg(...)]`

# on `feature = "rk3566"` from the kernel side
