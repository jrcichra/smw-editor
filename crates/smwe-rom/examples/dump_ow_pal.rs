use std::env;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow_pal <rom.smc>");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // LAYER2_PALETTE_INDIRECT1 at $00AD1E, 7 bytes
    let ind1_pc = lorom_pc(0x00AD1E);
    let ind1: Vec<u8> = rom[ind1_pc..ind1_pc+7].to_vec();
    println!("INDIRECT1 at $00AD1E: {:02x?}", ind1);

    // LAYER2_PALETTE_INDIRECT2 at $00ABDF, variable
    let ind2_base = lorom_pc(0x00ABDF);
    println!("INDIRECT2 at $00ABDF (first 20 bytes): {:02x?}", &rom[ind2_base..ind2_base+20]);

    // For each of the 7 indirect1 entries, compute the palette index
    println!("\nPalette index resolution:");
    for (i, &offset) in ind1.iter().enumerate() {
        let ptr_pc = ind2_base + 2 * offset as usize;
        let ptr16 = u16::from_le_bytes([rom[ptr_pc], rom[ptr_pc + 1]]);
        let idx = ptr16 / 0x38;
        println!("  indirect1[{i}] = {offset:#04x} -> ptr16 = {ptr16:#06x} -> palette_idx = {idx}");
    }

    // Now dump the actual layer2 palette data at $00B3D8
    // 4 sub-palettes × 7 colors × 2 bytes = 56 bytes per submap
    // 6 submaps total
    let pal_base = lorom_pc(0x00B3D8);
    println!("\nLayer2 palette data at $00B3D8:");
    for submap in 0..6usize {
        println!("  Submap {submap} (4 rows × 7 colors):");
        for row in 0..4usize {
            let base = pal_base + submap * 56 + row * 14;
            let colors: Vec<u16> = (0..7).map(|c| u16::from_le_bytes([rom[base+c*2], rom[base+c*2+1]])).collect();
            println!("    row {}: {:04x?}", row + 4, colors);
        }
    }

    // Also check what smwe-rom resolves as palette for submap 0
    let smw_rom = smwe_rom::SmwRom::from_file(path).expect("load");
    use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
    println!("\nsmwe-rom submap 0 palette (rows 4-7):");
    let pal = smw_rom.gfx.color_palettes.get_submap_palette(0, OverworldState::PreSpecial).unwrap();
    for row in 4..8usize {
        let colors: Vec<u16> = (0..8).map(|col| {
            pal.get_color_at(row, col).unwrap_or(smwe_render::color::Abgr1555::TRANSPARENT).0
        }).collect();
        println!("  row {row}: {:04x?}", colors);
    }
}
