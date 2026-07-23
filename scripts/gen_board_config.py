#!/usr/bin/env python3
"""Generate board_config.rs from TOML board config via tomllib (Python >= 3.11)."""
import tomllib, os

def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    cfg_path = os.path.join(root, "configs", "nanopi-r3s.toml")

    with open(cfg_path, "rb") as f:
        config = tomllib.load(f)

    code = "// Auto-generated from configs/nanopi-r3s.toml\n\n"

    # GPIO controllers
    ctrls = config.get("gpio", {}).get("controllers", {})
    if ctrls:
        code += "pub mod gpio {\n"
        for name, addr in sorted(ctrls.items()):
            code += f"    pub const {name}: usize = {addr};\n"
        code += "}\n\n"

    # GPIO registers
    regs = config.get("gpio", {}).get("registers", {})
    if regs:
        code += "pub mod gpio_reg {\n"
        for name, off in sorted(regs.items()):
            code += f"    pub const {name}: usize = {off};\n"
        code += "}\n\n"

    # LEDs
    leds = config.get("leds", [])
    if leds:
        code += """pub struct LedDef {
    pub name: &'static str,
    pub ctrl: &'static str,
    pub pin: u32,
    pub active_low: bool,
}

pub static LEDS: &[LedDef] = &[\n"""
        for led in leds:
            n = led.get("name", "?")
            c = led.get("gpio_controller", "GPIO0")
            p = led.get("gpio_pin", 0)
            al = "true" if led.get("active_low", False) else "false"
            code += f'    LedDef {{ name: "{n}", ctrl: "{c}", pin: {p}, active_low: {al} }},\n'
        code += "];\n\n"

    # Framebuffer
    fb = config.get("framebuffer", {})
    code += f"pub const FB_DEFAULT_WIDTH: u32 = {fb.get('default_width', 1920)};\n"
    code += f"pub const FB_DEFAULT_HEIGHT: u32 = {fb.get('default_height', 1080)};\n"
    code += f"pub const FB_DEFAULT_BPP: u32 = {fb.get('default_bpp', 32)};\n\n"

    # Serial
    ser = config.get("serial", {})
    code += f"pub const SERIAL_UART2_BASE: usize = {ser.get('uart2_base', '0xFE660000')};\n"
    code += f"pub const SERIAL_REG_SHIFT: u32 = {ser.get('reg_shift', 2)};\n"
    code += f"pub const SERIAL_IO_WIDTH: u32 = {ser.get('reg_io_width', 4)};\n\n"

    out = os.path.join(root, "packages", "ostd", "src", "arch", "aarch64", "boot", "board_config.rs")
    with open(out, "w") as f:
        f.write(code)
    print(f"Generated {out} ({len(code)} bytes)")

if __name__ == "__main__":
    main()
