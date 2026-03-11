use std::env;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    let pc = lorom_pc(0x0C8000);

    // The OW tilemap is 0x800 bytes = 1024 u16 entries = 32*32 tiles
    // But what if it's structured as 2 separate 32x16 screens side by side?
    // SNES BG tilemaps can be arranged in various ways.

    // Check: are there runs of the same tile? That would indicate large terrain areas.
    let entries: Vec<u16> = (0..1024).map(|i| {
        let off = pc + i * 2;
        u16::from_le_bytes([rom[off], rom[off+1]])
    }).collect();

    // Count runs
    println!("First 64 entries (first 2 rows of 32):");
    for row in 0..4usize {
        print!("  [{:2}]: ", row);
        for col in 0..32usize {
            print!("{:04x} ", entries[row*32+col]);
        }
        println!();
    }

    // Check for repeating patterns
    println!("\nMost common tiles:");
    let mut counts = std::collections::HashMap::new();
    for &e in &entries { *counts.entry(e).or_insert(0u32) += 1; }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a,b| b.1.cmp(&a.1));
    for (tile, count) in sorted.iter().take(10) {
        let chr = tile & 0x3FF;
        let pal = (tile >> 10) & 7;
        println!("  {:04x} (chr={chr:#05x} pal={pal}): {count} times", tile);
    }

    // Check if maybe this is a 16-bit CHR index (using all 16 bits somehow)
    // Or if the CHR field is only 9 bits (0x1FF max)
    let max_chr = entries.iter().map(|e| e & 0x3FF).max().unwrap();
    let min_pal = entries.iter().map(|e| (e >> 10) & 7).min().unwrap();
    let max_pal = entries.iter().map(|e| (e >> 10) & 7).max().unwrap();
    println!("\nCHR range: 0..={max_chr:#05x}");
    println!("PAL range: {min_pal}..={max_pal}");

    // Check: on the SNES the OW uses a 512-tile VRAM configuration
    // and the BG tilemap CHR field might only be 9 bits (0-511)
    // with bit 9 used for something else
    // Let's see what bit 9 contains
    let bit9_set = entries.iter().filter(|&&e| (e >> 9) & 1 != 0).count();
    let bit8_set = entries.iter().filter(|&&e| (e >> 8) & 1 != 0).count();
    println!("Tiles with bit9 set: {bit9_set}/1024");
    println!("Tiles with bit8 set: {bit8_set}/1024");
}
