use std::env;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // The GFX file table at $00A8C3 is 8 bytes per submap entry
    // But what VRAM address does each entry load to?
    // The SNES OW GFX loading routine is documented:
    // GFX files load to VRAM starting at $4000 (word address = $2000)
    // Each file takes 0x600 bytes (128 tiles × 3bpp × 24 bytes) of ROM
    // but expands to 0x800 bytes (128 tiles × 4bpp × 32 bytes) in VRAM
    // So: file[0] → VRAM $4000, file[1] → VRAM $4800, ...

    // Wait - let's look at the actual load routine at $00A8B7 (SMW overworld GFX loader)
    // The table is at $00A8C3 - just before that is the loading code

    let pc = lorom_pc(0x00A8B0);
    println!("Code near $00A8B0 (before GFX table at $A8C3):");
    println!("Hex dump:");
    for i in 0..80usize {
        if i % 16 == 0 { print!("\n  {:04X}: ", 0xA8B0 + i); }
        print!("{:02x} ", rom[pc + i]);
    }
    println!();

    // Also look at the GFX table more carefully - is it 4 entries per submap (not 8)?
    // 6 submaps × 4 entries = 24 bytes vs 6 × 8 = 48 bytes
    let table_pc = lorom_pc(0x00A8C3);
    println!("\nGFX table at $A8C3 (48 bytes = 6×8 or 12×4):");
    for i in 0..48usize {
        if i % 8 == 0 { print!("\n  [{}]: ", i/8); }
        print!("{:02x} ", rom[table_pc + i]);
    }
    println!();

    // The OW typically uses 4 GFX files loaded into VRAM at fixed positions
    // Files occupy VRAM $4000-$5FFF (low) and some in upper half
    // BG2 character base = VRAM $4000 in typical configuration
    // With 4bpp tiles: 128 tiles × 32 bytes = 4096 bytes = 0x1000 words per file
    // So file[0]→$4000, file[1]→$5000, file[2]→$6000, file[3]→$7000
    // That's only 4 files × 128 tiles = 512 tiles with base at $4000

    // Actually the standard SMW OW VRAM layout:
    // $0000-$1FFF: BG1 tilemap
    // $2000-$27FF: BG2 tilemap  
    // $4000-$7FFF: BG2 character data (4 files × 128 tiles × 32 bytes = 16384 bytes)
    // So only 4 GFX files (512 tiles), NOT 8!

    println!("\nConclusion: OW BG2 has 4 GFX files (512 tiles), pages 0-3 only");
    println!("CHR 0x000-0x07F = file[0], 0x080-0x0FF = file[1]");
    println!("CHR 0x100-0x17F = file[2], 0x180-0x1FF = file[3]");
    println!("CHR > 0x1FF = wraps (& 0x1FF)? or uses pages 4-7?");

    // What are the most-used CHR values?
    let table2_pc = lorom_pc(0x0C8000);
    let entries: Vec<u16> = (0..1024).map(|i| {
        let off = table2_pc + i * 2;
        u16::from_le_bytes([rom[off], rom[off+1]])
    }).collect();
    
    let mut chr_hist = std::collections::HashMap::new();
    for &e in &entries {
        let chr = e & 0x3FF;
        *chr_hist.entry(chr).or_insert(0u32) += 1;
    }
    let max_chr = chr_hist.keys().cloned().max().unwrap_or(0);
    let tiles_above_1ff = chr_hist.iter().filter(|(&k,_)| k > 0x1FF).map(|(_,&v)| v).sum::<u32>();
    let tiles_above_0ff = chr_hist.iter().filter(|(&k,_)| k > 0xFF).map(|(_,&v)| v).sum::<u32>();
    println!("\nTilemap stats:");
    println!("  Max CHR: {max_chr:#05x}");
    println!("  Tiles using CHR > 0x1FF: {tiles_above_1ff}/1024");
    println!("  Tiles using CHR > 0x0FF: {tiles_above_0ff}/1024");
}
