use std::{env, path::Path, sync::Arc};

use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

fn main() {
    let args: Vec<String> = env::args().collect();
    let level = args
        .iter()
        .find_map(|a| a.strip_prefix("--level="))
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x105);

    let raw = std::fs::read(Path::new("smw.smc")).expect("cannot read smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

    smwe_emu::emu::decompress_sublevel(&mut cpu, level);
    smwe_emu::emu::exec_sprites(&mut cpu);

    // Check sprite 7's palette property and OAM
    println!("Sprite 7 details:");
    println!("  Sprite ID ($9E+7): 0x{:02X}", cpu.mem.load_u8(0x9E + 7));
    println!("  State ($14C8+7): 0x{:02X}", cpu.mem.load_u8(0x14C8 + 7));
    println!("  Palette prop ($15F6+7): 0x{:02X}", cpu.mem.load_u8(0x15F6 + 7));
    println!("  OAM Index ($3360+7): 0x{:02X}", cpu.mem.load_u8(0x3360 + 7));

    // The OAM entry
    let oam_idx = 59; // The one with the purple sprite
    let x = cpu.mem.load_u8(0x300 + oam_idx * 4);
    let y = cpu.mem.load_u8(0x301 + oam_idx * 4);
    let tile = cpu.mem.load_u16(0x302 + oam_idx * 4);
    let attr = (tile >> 8) as u8;
    let pal_bits = (attr >> 1) & 0x3;

    println!("\nOAM entry 59:");
    println!("  X: 0x{:02X}", x);
    println!("  Y: 0x{:02X}", y);
    println!("  Tile word: 0x{:04X}", tile);
    println!("  Attr byte: 0x{:02X}", attr);
    println!("  Palette bits: {} (should be 1 for yellow)", pal_bits);
    println!("  Calculated palette: {}", ((tile >> 9) & 0x7) + 8);

    // Check all sprites' palette properties
    println!("\nAll sprite palette properties:");
    for spr in 0..12 {
        let id = cpu.mem.load_u8(0x9E + spr);
        let pal_prop = cpu.mem.load_u8(0x15F6 + spr);
        if id != 0 {
            println!("  Sprite {}: ID=0x{:02X} PalProp=0x{:02X}", spr, id, pal_prop);
        }
    }
}
