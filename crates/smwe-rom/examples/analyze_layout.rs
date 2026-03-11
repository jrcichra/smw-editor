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

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);

    // Try 40×58 (main map 28 rows + 6 submaps × 5 rows each = nope)
    // Let's try width 40 with increasing heights and look at the raw data
    // to find boundaries between main map and submaps.
    //
    // Actually the speedruns wiki says the full overworld tilemap is one continuous
    // buffer. The main map is at (0..39, 0..27) and the submaps are tiled below.
    // Let's try 40×56 = 2240 tiles total.

    let total = 40 * 58;
    let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total * 2);

    // Print a "heatmap" of all rows - count distinct non-background tiles
    // Background tile is 0x1c75 (ocean) and 0xbaba (maybe the border)
    // Let's find the most common tile first
    let mut counts = std::collections::HashMap::new();
    for &t in &tiles { *counts.entry(t).or_insert(0u32) += 1; }
    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|a,b| b.1.cmp(a.1));
    println!("Top 5 most common tiles:");
    for (t, c) in sorted.iter().take(5) {
        println!("  {:04x}: {}", t, c);
    }
    let bg_tile = *sorted[0].0;
    let bg2_tile = *sorted[1].0;
    println!("Treating {:04x} and {:04x} as background", bg_tile, bg2_tile);

    println!("\nRow breakdown (non-bg tile count per row):");
    for row in 0..58usize {
        let interesting: Vec<usize> = (0..40).filter(|&c| {
            let t = tiles[row * 40 + c];
            t != bg_tile && t != bg2_tile && t != 0
        }).collect();
        if !interesting.is_empty() {
            println!("  row {:2}: {} non-bg tiles, cols {:?}...",
                row, interesting.len(),
                &interesting[..interesting.len().min(5)]);
        } else {
            println!("  row {:2}: (background only)", row);
        }
    }
}
