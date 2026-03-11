use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::SmwRom;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let raw = std::fs::read("/home/justin/git/smw-editor/smw.smc").unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };
    let smw = SmwRom::from_file("/home/justin/git/smw-editor/smw.smc").unwrap();

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    let total = 40 * 80;
    let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total * 2);

    // YI scroll: (12288, 314) -> origin (16, 39)
    let origin_col = 16;
    let origin_row = 39;

    println!("Direct buffer lookup at YI origin (16, 39):");
    for row in 0..5 {
        print!("row {}: ", row);
        for col in 0..8 {
            let idx = (origin_row + row) * 40 + (origin_col + col) % 40;
            let tile = tiles.get(idx).map(|t| format!("{:04x}", t)).unwrap_or_else(|| "----".to_string());
            print!("{} ", tile);
        }
        println!();
    }

    println!("\nLibrary parsed tilemap (first 8x5):");
    if let Some(yi) = smw.overworld.layer2.get(1) {
        for row in 0..5 {
            print!("row {}: ", row);
            for col in 0..8 {
                let t = yi.get(col, row);
                print!("{:04x} ", t.0);
            }
            println!();
        }
    }
}
