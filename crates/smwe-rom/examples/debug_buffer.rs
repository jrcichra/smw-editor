use smwe_rom::compression::lc_rle2::decompress_rle2;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let raw = std::fs::read("/home/justin/git/smw-editor/smw.smc").unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);

    // Try different buffer sizes
    for height in [64, 80, 96, 128] {
        let total = 40 * height;
        let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total * 2);
        println!("Height {}: decompressed {} tiles", height, tiles.len());
    }

    // Check what's at YI position for height=80
    let height = 80;
    let total = 40 * height;
    let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total * 2);

    let origin_col = 16;
    let origin_row = 39;

    println!("\nWith height=80, origin=({},{}):", origin_col, origin_row);
    for row in [0, 1, 2, 24, 25, 26, 39, 40, 41] {
        let buffer_row = origin_row + row;
        let idx = buffer_row * 40 + origin_col;
        let valid = if idx < tiles.len() { "valid" } else { "OUT" };
        println!("  row {}: buffer_row={}, idx={} - {}", row, buffer_row, idx, valid);
    }
}
