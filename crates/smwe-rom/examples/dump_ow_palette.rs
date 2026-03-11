use std::env;
use smwe_rom::SmwRom;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow_palette <rom.smc>");
    let rom = SmwRom::from_file(path).expect("load failed");

    let submap = 0usize;
    let pal = rom.gfx.color_palettes.get_submap_palette(submap, OverworldState::PreSpecial).expect("palette");

    println!("OW submap 0 palette rows 4-7 (layer2):");
    for row in 4..=7 {
        print!("  Row {:X}:", row);
        for col in 0..16 {
            let c = pal.get_color_at(row, col).unwrap_or(Abgr1555(0x8000));
            // Convert ABGR1555 to rough RGB
            let v = c.0;
            let r = ((v & 0x001F) << 3) as u8;
            let g = (((v >> 5) & 0x001F) << 3) as u8;
            let b = (((v >> 10) & 0x001F) << 3) as u8;
            let transp = (v & 0x8000) != 0;
            if transp {
                print!(" [TRANSP]");
            } else {
                print!(" #{:02X}{:02X}{:02X}", r, g, b);
            }
        }
        println!();
    }

    // Also build the flat CGRAM like world_editor.rs does and show rows 4-7
    let cgram: Vec<Abgr1555> = (0..256usize)
        .map(|i| pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555(0x8000)))
        .collect();

    println!("\nFlat CGRAM rows 4-7 (indices 64-127):");
    for row in 4..=7usize {
        print!("  Row {:X} (base={:3}):", row, row * 16);
        for col in 0..16 {
            let c = cgram[row * 16 + col];
            let v = c.0;
            let r = ((v & 0x001F) << 3) as u8;
            let g = (((v >> 5) & 0x001F) << 3) as u8;
            let b = (((v >> 10) & 0x001F) << 3) as u8;
            let transp = (v & 0x8000) != 0;
            if transp {
                print!(" [T]");
            } else {
                print!(" #{:02X}{:02X}{:02X}", r, g, b);
            }
        }
        println!();
    }
}
