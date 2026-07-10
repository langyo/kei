# Board Support Package crates for kei.
#
# Each BSP provides device drivers for a specific SoC platform.
# BSPs are compiled as OSDK library crates and linked into the
# patched Asterinas kernel at build time.
#
# Adding a new BSP:
#   1. Copy the template: cp -r bsp/rk3566 bsp/mysoc
#   2. Implement the driver modules in src/
#   3. Add to Cargo.toml workspace members
#   4. Add board/ config directory
#   5. Add board to configs/mysoc.toml
