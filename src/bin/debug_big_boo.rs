use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use std::{env, path::Path, sync::Arc};

fn make_cpu(rom_bytes: Vec<u8>) -> Cpu {
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    Cpu::new(CheckedMem::new(Arc::new(emu_rom)))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let level = args
        .iter()
        .find_map(|a| a.strip_prefix("--level="))
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x105);

    let raw = std::fs::read(Path::new("smw.smc")).expect("smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    let mut cpu = make_cpu(rom_bytes);
    smwe_emu::emu::decompress_sublevel(&mut cpu, level);
    smwe_emu::emu::upload_sprite_tileset(&mut cpu, 7);
    smwe_emu::emu::exec_sprite_id(&mut cpu, 0x28);

    println!("Big Boo OAM:");
    for tile in smwe_emu::emu::sprite_oam_tiles(&mut cpu, 0x28) {
        println!(
            "  dx={:4} dy={:4} tile={:04X} pal={} 16x16={}",
            tile.dx,
            tile.dy,
            tile.tile_word,
            (tile.tile_word >> 9) & 7,
            tile.is_16x16
        );
    }
}
