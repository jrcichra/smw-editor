use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use std::{path::Path, sync::Arc};

fn make_cpu(rom_bytes: Vec<u8>) -> Cpu {
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    Cpu::new(CheckedMem::new(Arc::new(emu_rom)))
}

fn dump_oam(cpu: &mut Cpu, label: &str) {
    println!("  OAM [{}]:", label);
    for slot in 0..64u32 {
        let x = cpu.mem.load_u8(0x300 + slot * 4);
        let y = cpu.mem.load_u8(0x301 + slot * 4);
        let tile = cpu.mem.load_u16(0x302 + slot * 4);
        let size = cpu.mem.load_u8(0x460 + slot);
        if y < 0xE0 && tile != 0 {
            println!(
                "    slot {:02} x={:3} y={:3} tile={:04X} pal={} 16x16={}",
                slot,
                x,
                y,
                tile,
                (tile >> 9) & 7,
                (size & 2) != 0
            );
        }
    }
}

fn main() {
    let raw = std::fs::read(Path::new("smw.smc")).expect("smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    // ── Dragon Coin 0xA6 ──────────────────────────────────────────────────
    println!("=== Dragon Coin (0xA6) ===");
    {
        let mut cpu = make_cpu(rom_bytes.clone());
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
        smwe_emu::emu::exec_sprite_id(&mut cpu, 0xA6);
        dump_oam(&mut cpu, "exec_sprite_id x1");
        // Run more frames — dragon coin draws on frame 2+
        for _ in 0..4 {
            smwe_emu::emu::exec_sprites(&mut cpu);
        }
        dump_oam(&mut cpu, "after 4 more exec_sprites frames");
    }

    // ── Wiggler 0xBD ──────────────────────────────────────────────────────
    println!("\n=== Wiggler (0xBD) ALL OAM entries ===");
    {
        let mut cpu = make_cpu(rom_bytes.clone());
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
        smwe_emu::emu::exec_sprite_id(&mut cpu, 0xBD);
        dump_oam(&mut cpu, "exec_sprite_id(0xBD)");
    }

    // ── All IDs — show ALL OAM entries each produces ─────────────────────
    println!("\n=== All sprite IDs in level 0x105 ===");
    let smw = smwe_rom::SmwRom::from_file("smw.smc").expect("rom");
    let level = &smw.levels[0x105];
    let mut ids: Vec<u8> = level.sprite_layer.sprites.iter().map(|s| s.sprite_id()).collect();
    ids.sort();
    ids.dedup();

    for &id in &ids {
        let mut cpu = make_cpu(rom_bytes.clone());
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
        smwe_emu::emu::exec_sprite_id(&mut cpu, id);

        let mut entries = vec![];
        for slot in 0..64u32 {
            let y = cpu.mem.load_u8(0x301 + slot * 4);
            let tile = cpu.mem.load_u16(0x302 + slot * 4);
            let size = cpu.mem.load_u8(0x460 + slot);
            if y < 0xE0 && tile != 0 {
                let x = cpu.mem.load_u8(0x300 + slot * 4);
                let voff = ((tile & 0x1FF) as usize + 0x600) * 32;
                let nz = if voff + 32 <= cpu.mem.vram.len() {
                    cpu.mem.vram[voff..voff + 32].iter().filter(|&&b| b != 0).count()
                } else {
                    0
                };
                entries.push((slot, x, y, tile, size, nz));
            }
        }
        if entries.is_empty() {
            println!("  0x{:02X}: (no OAM)", id);
        } else {
            for (slot, x, y, tile, size, nz) in entries {
                println!(
                    "  0x{:02X} slot={:02} x={:3} y={:3} tile={:04X} pal={} 16x16={} vram_nz={}",
                    id,
                    slot,
                    x,
                    y,
                    tile,
                    (tile >> 9) & 7,
                    (size & 2) != 0,
                    nz
                );
            }
        }
    }

    // ── CGRAM rows after decompress_sublevel (clean state) ───────────────
    println!("\n=== CGRAM rows 8-15 (clean after decompress) ===");
    {
        let mut cpu = make_cpu(rom_bytes.clone());
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);

        // Check SpriteOBJAttribute for sprite 0xA6 (Dragon Coin)
        println!("  Sprite 0xA6 OBJ Attribute ($15F6+): {:02X}", cpu.mem.load_u8(0x15F6));

        // Check what palette the dragon coin should use
        let obj_attr = cpu.mem.load_u8(0x15F6);
        let pal = (obj_attr >> 1) & 0x07;
        println!("  Dragon coin palette from $15F6: {} (row {})", pal, 8 + pal);

        // Check SpritePalette setting
        println!("  SpritePalette ($192E): {:02X}", cpu.mem.load_u8(0x192E));

        // Check DynPaletteTable contents
        println!("  DynPaletteTable:");
        let count = cpu.mem.load_u8(0x0682);
        println!("    [00]: count={:02X}", count);
        if count != 0 {
            println!("    [01]: cgram={:02X}", cpu.mem.load_u8(0x0683));
            for i in 0..count.min(8) {
                println!("    [{:02X}]: {:02X}", i + 2, cpu.mem.load_u8(0x0684 + i as u32));
            }
        }

        // Check tweaker byte for sprite 0xA6 from ROM
        println!("  Sprite 0xA6 tweaker byte should be: 0x35");
        println!("  Expected palette: 5, Expected OBJ attr: 0x0A");

        // Check dragon coin tile color indices
        println!("  Dragon coin tile color indices:");
        let tile_base = 0x600 + 0xBC;
        for row in 0..8 {
            let base = (tile_base * 32) + (row * 4);
            let b0 = cpu.mem.vram[base];
            let b1 = cpu.mem.vram[base + 1];
            let b2 = cpu.mem.vram[base + 16];
            let b3 = cpu.mem.vram[base + 17];

            let mut max_c = 0;
            for bit in (0..8).rev() {
                let c =
                    ((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3);
                if c > max_c {
                    max_c = c;
                }
            }
            if max_c > 7 {
                println!("    Row {} uses color {}", row, max_c);
            }
        }

        for row in 8..16usize {
            print!("  SP{} (row {:2}): ", row - 8, row);
            for col in 0..16usize {
                let idx = (row * 16 + col) * 2;
                let c = cpu.mem.cgram[idx] as u16 | ((cpu.mem.cgram[idx + 1] as u16) << 8);
                let r = ((c & 0x1F) << 3) as u8;
                let g = (((c >> 5) & 0x1F) << 3) as u8;
                let b = (((c >> 10) & 0x1F) << 3) as u8;
                print!("#{:02X}{:02X}{:02X} ", r, g, b);
            }
            println!();
        }
    }

    // ── DynPaletteTable contents ─────────────────────────────────────────
    println!("\n=== DynPaletteTable at $0682 (checking 256 bytes) ===");
    {
        let mut cpu = make_cpu(rom_bytes.clone());
        smwe_emu::emu::decompress_sublevel(&mut cpu, 0x105);
        let mut found = 0;
        for i in 0..256usize {
            let addr = 0x0682 + i;
            let val = cpu.mem.load_u8(addr as u32);
            if val != 0 {
                println!("  [{:03X}]: {:02X} -> CGRAM ${:02X}", i, val, 0x82 + i);
                found += 1;
                if found > 30 {
                    println!("  ... (more entries)");
                    break;
                }
            }
        }
        if found == 0 {
            println!("  (all zeros - DynPaletteTable is empty!)");
        }
    }
}
