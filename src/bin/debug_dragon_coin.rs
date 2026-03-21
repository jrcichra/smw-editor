use std::{path::Path, sync::Arc};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

fn check_sprite(rom_bytes: &[u8], id: u8) {
    let mut emu_rom = EmuRom::new(rom_bytes.to_vec());
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
    smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);

    match smwe_emu::emu::sprite_oam_info(&mut cpu, id) {
        Some((tile, big)) => {
            let pal_raw = (tile >> 9) & 7;
            let tile_idx = (tile & 0x1FF) as usize + 0x600;
            println!("  0x{:02X}: tile_word={:04X} tile_idx=0x{:03X} pal_raw={} cgram_row={} 16x16={}",
                id, tile, tile_idx, pal_raw, pal_raw + 8, big);

            // Show all 4 sub-tile VRAM occupancy for 16x16 sprites
            if big {
                for (label, idx) in [("UL", tile_idx), ("UR", tile_idx+1), ("LL", tile_idx+16), ("LR", tile_idx+17)] {
                    let off = idx * 32;
                    let nz = if off + 32 <= cpu.mem.vram.len() {
                        cpu.mem.vram[off..off+32].iter().filter(|&&b| b != 0).count()
                    } else { 0 };
                    println!("    sub-tile {} (0x{:03X}): nonzero_bytes={}", label, idx, nz);
                }
            }

            // Show palette row colors
            for row_offset in 0..1u32 {
                let row = pal_raw as usize + 8 + row_offset as usize;
                print!("  CGRAM row {}: ", row);
                for col in 0..16usize {
                    let idx2 = (row * 16 + col) * 2;
                    let lo = cpu.mem.cgram[idx2] as u16;
                    let hi = cpu.mem.cgram[idx2+1] as u16;
                    let c = lo | (hi << 8);
                    let r = ((c & 0x1F) << 3) as u8;
                    let g = (((c >> 5) & 0x1F) << 3) as u8;
                    let b = (((c >> 10) & 0x1F) << 3) as u8;
                    print!("#{:02X}{:02X}{:02X} ", r, g, b);
                }
                println!();
            }
        }
        None => println!("  0x{:02X}: no OAM written", id),
    }
}

fn main() {
    let raw = std::fs::read(Path::new("smw.smc")).expect("smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    println!("=== Sprite OAM info ===");
    // Dragon Coin
    check_sprite(&rom_bytes, 0xA6);
    // Wiggler (0xBD from sprite list)
    check_sprite(&rom_bytes, 0xBD);

    // Also check what ALL OAM slots look like for Wiggler
    println!("\n=== All OAM for Wiggler (0xBD) ===");
    let mut emu_rom = EmuRom::new(rom_bytes.clone());
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
    smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
    smwe_emu::emu::exec_sprite_id(&mut cpu, 0xBD);
    for slot in 0..64usize {
        let y = cpu.mem.load_u8(0x301 + slot as u32 * 4);
        let tile = cpu.mem.load_u16(0x302 + slot as u32 * 4);
        let size = cpu.mem.load_u8(0x460 + slot as u32);
        if y < 0xE0 {
            let tile_idx = (tile & 0x1FF) as usize + 0x600;
            let off = tile_idx * 32;
            let nz = if off + 32 <= cpu.mem.vram.len() {
                cpu.mem.vram[off..off+32].iter().filter(|&&b| b != 0).count()
            } else { 0 };
            println!("  slot {:02}: y={:3} tile={:04X} tile_idx=0x{:03X} pal={} 16x16={} vram_nz={}",
                slot, y, tile, tile_idx, (tile>>9)&7, (size&2)!=0, nz);
        }
    }

    // Dragon Coin: check ALL OAM slots after exec_sprite_id(0xA6)
    println!("\n=== All OAM for Dragon Coin (0xA6) after exec_sprite_id ===");
    let mut emu_rom2 = EmuRom::new(rom_bytes.clone());
    emu_rom2.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu2 = Cpu::new(CheckedMem::new(Arc::new(emu_rom2)));
    smwe_emu::emu::decompress_sublevel(&mut cpu2, 0x105);
    smwe_emu::emu::exec_sprite_id(&mut cpu2, 0xA6);
    for slot in 0..64usize {
        let y = cpu2.mem.load_u8(0x301 + slot as u32 * 4);
        let tile = cpu2.mem.load_u16(0x302 + slot as u32 * 4);
        let size = cpu2.mem.load_u8(0x460 + slot as u32);
        if y < 0xE0 {
            println!("  slot {:02}: y={:3} tile={:04X} pal={} 16x16={}",
                slot, y, tile, (tile>>9)&7, (size&2)!=0);
        }
    }
    println!("(nothing = no OAM written by 0xA6)");

    // For Dragon Coin, hardcode known tile from ROM disassembly
    // We confirmed: tile=0xBC attr=0xB2 -> oam_word=0xB2BC, pal_raw=1, cgram_row=9
    // Let's check what CGRAM row 9 actually looks like after decompress
    println!("\n=== CGRAM row 9 after decompress_sublevel(0x105) ===");
    let mut emu_rom3 = EmuRom::new(rom_bytes);
    emu_rom3.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu3 = Cpu::new(CheckedMem::new(Arc::new(emu_rom3)));
    smwe_emu::emu::decompress_sublevel(&mut cpu3, 0x105);
    for row in 8..16usize {
        print!("  row {:2} SP{}: ", row, row-8);
        for col in 0..16usize {
            let idx = (row * 16 + col) * 2;
            let lo = cpu3.mem.cgram[idx] as u16;
            let hi = cpu3.mem.cgram[idx+1] as u16;
            let c = lo | (hi << 8);
            let r = ((c & 0x1F) << 3) as u8;
            let g = (((c >> 5) & 0x1F) << 3) as u8;
            let b = (((c >> 10) & 0x1F) << 3) as u8;
            print!("#{:02X}{:02X}{:02X} ", r, g, b);
        }
        println!();
    }
}
