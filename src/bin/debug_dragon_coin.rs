use std::{path::Path, sync::Arc};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_rom::level::sprite_layer::SpriteInstance;

fn main() {
    let raw = std::fs::read(Path::new("smw.smc")).expect("cannot read smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    // Get unique sprite IDs from level 0x105
    let smw = smwe_rom::SmwRom::from_file("smw.smc").expect("open rom");
    let level = &smw.levels[0x105];
    let mut ids: Vec<u8> = level.sprite_layer.sprites.iter()
        .map(|s| SpriteInstance::sprite_id(s))
        .collect();
    ids.sort();
    ids.dedup();
    println!("Unique sprite IDs in level 0x105: {:?}", ids.iter().map(|x| format!("0x{:02X}", x)).collect::<Vec<_>>());

    // For each ID, run exec_sprite_id and capture what OAM slot 0 gets
    println!("\nRunning exec_sprite_id for each ID:");
    println!("{:<8} {:<8} {:<8} {:<8} {:<8}", "ID", "tile_word", "tile_idx", "pal_raw", "16x16");
    for &id in &ids {
        let mut emu_rom = EmuRom::new(rom_bytes.clone());
        emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
        let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
        smwe_emu::emu::exec_sprite_id(&mut cpu, id);

        // Find the first OAM entry that's on-screen (y < 0xE0) and has a tile
        let mut found = false;
        for slot in 0..64usize {
            let y = cpu.mem.load_u8(0x301 + slot as u32 * 4);
            let tile = cpu.mem.load_u16(0x302 + slot as u32 * 4);
            let size = cpu.mem.load_u8(0x460 + slot as u32);
            if y < 0xE0 && tile != 0 {
                let pal_raw = (tile >> 9) & 7;
                let tile_idx = (tile & 0x1FF) as usize + 0x600;
                let vram_off = tile_idx * 32;
                let nz = if vram_off + 32 <= cpu.mem.vram.len() {
                    cpu.mem.vram[vram_off..vram_off+32].iter().filter(|&&b| b != 0).count()
                } else { 0 };
                println!("0x{:02X}     {:04X}     0x{:03X}    {}        {} (slot={} vram_nz={})",
                    id, tile, tile_idx, pal_raw, (size & 2) != 0, slot, nz);
                found = true;
                break;
            }
        }
        if !found {
            println!("0x{:02X}     (no OAM written)", id);
        }
    }

    // Also check Dragon Coin all animation frames
    println!("\n=== Dragon Coin animation frames (tile table $03DD6E) ===");
    let tile_tbl = 0x03usize * 0x8000 + (0xDD6Eusize - 0x8000);
    let attr_tbl = 0x03usize * 0x8000 + (0xDD73usize - 0x8000);
    for i in 0..5usize {
        let tile = rom_bytes[tile_tbl + i] as u16;
        let attr = rom_bytes[attr_tbl + i] as u16;
        let oam_word = tile | (attr << 8);
        let pal_raw = (oam_word >> 9) & 7;
        // check VRAM for this frame (need fresh CPU for correct VRAM)
        println!("  frame {}: tile=0x{:02X} attr=0x{:02X} oam={:04X} pal_raw={}",
            i, tile, attr, oam_word, pal_raw);
    }
}
