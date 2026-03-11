use std::env;
use smwe_rom::compression::lc_rle2::decompress_rle2;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // Layer 2 tile numbers at $04A533, YXPCCCTT at $04C02B
    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);

    println!("Tile data at PC {tile_pc:#X}, attr data at PC {attr_pc:#X}");

    // The full overworld tilemap is 40 wide × ~32 tall (main map) + submaps
    // Total decompressed size: let's try 40*32*2 = 2560 bytes first
    // and increase until we get sensible data
    let tile_data = &rom[tile_pc..];
    let attr_data = &rom[attr_pc..];

    // Try various output sizes
    for total_tiles in [40*28, 40*32, 40*40, 40*56, 40*64] {
        let tiles = decompress_rle2(tile_data, attr_data, total_tiles * 2);
        let nonzero = tiles.iter().filter(|&&t| t != 0).count();
        let bytes = total_tiles * 2;
        println!("  Output {total_tiles} tiles ({bytes} bytes): {nonzero} non-zero");
    }

    // Use 40*32 as the main map (320 tiles per row, 32 rows visible)
    let total_tiles = 40 * 32;
    let tiles = decompress_rle2(tile_data, attr_data, total_tiles * 2);

    // Show first 4 rows (40 tiles each)
    println!("\nFirst 4 rows of decompressed data (40 wide):");
    for row in 0..4usize {
        print!("  [{row:2}]: ");
        for col in 0..40usize {
            let t = tiles[row * 40 + col];
            print!("{t:04x} ");
        }
        println!();
    }

    // Check: what does a 40-wide slice from offset (1,2) (submap origin for YI) look like?
    // According to wiki: on submaps subtract 2 from X and 1 from Y
    // So submap YI at display position (0,0) = stored at (0-2, 0-1) = invalid
    // Let's try different submap positions. Lunar Magic shows YI submap starting at tile ~(0,32)?

    // Let's try a much larger decompression and see the full picture
    let total_tiles2 = 40 * 64;
    let tiles2 = decompress_rle2(tile_data, attr_data, total_tiles2 * 2);
    let nonzero2 = tiles2.iter().filter(|&&t| t != 0).count();
    println!("\nWith 40x64={} tiles: {} non-zero", 40*64, nonzero2);

    // Find the first non-zero row
    for row in 0..64usize {
        let row_nonzero = (0..40).filter(|&c| tiles2[row*40+c] != 0).count();
        if row_nonzero > 5 {
            println!("  Row {row}: {row_nonzero} non-zero tiles");
        }
    }
}
