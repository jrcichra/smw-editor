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

    // Search for Yoshi coin (0x78) in sprite data
    println!("Searching for Yoshi coin (0x78) in level sprite data...");
    let mut found = false;
    for i in 0..64 {
        let addr = 0x7EC901 + i * 3;
        let b0 = cpu.mem.load_u8(addr);
        let b1 = cpu.mem.load_u8(addr + 1);
        let b2 = cpu.mem.load_u8(addr + 2);

        if b2 == 0xFF {
            break;
        }

        if b2 == 0x78 {
            let y = ((b0 >> 4) | ((b0 & 1) << 4)) as u8;
            let x = (b1 >> 4) as u8;
            let screen = ((b0 & 2) << 3) | (b1 & 0xF);
            println!("Found Yoshi coin at entry {}: X=0x{:02X} Y=0x{:02X} Screen={}", i, x, y, screen);
            found = true;
        }
    }

    if !found {
        println!("No Yoshi coin (0x78) found in level sprite data.");
        println!("\nAll sprite IDs in level:");
        for i in 0..64 {
            let addr = 0x7EC901 + i * 3;
            let b2 = cpu.mem.load_u8(addr + 2);
            if b2 == 0xFF {
                break;
            }
            print!("0x{:02X} ", b2);
        }
        println!();
    }
}
