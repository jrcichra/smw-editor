use std::env;

// LoROM SNES -> PC
fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow_gfxtable <rom.smc>");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // $00A8C3: OW GFX file table
    // According to smwcentral the table is at $00A8C3
    // It's 8 entries per submap (6 submaps normal + 6 special = 12 total states)
    // Let's just dump 24*8 = 192 bytes starting there
    let pc = lorom_pc(0x00A8C3);
    println!("Table at SNES $00A8C3 = PC {pc:#X}");
    println!("First 6 submaps (PreSpecial) * 8 pages each:");
    for submap in 0..6usize {
        let base = pc + submap * 8;
        let row: Vec<u8> = (0..8).map(|i| rom[base + i]).collect();
        println!("  submap {submap}: {:02x?}", row);
    }
    println!("\nNext 6 submaps (PostSpecial?) * 8 pages each:");
    for submap in 0..6usize {
        let base = pc + (submap + 6) * 8;
        let row: Vec<u8> = (0..8).map(|i| rom[base + i]).collect();
        println!("  submap {submap}: {:02x?}", row);
    }

    // Also check nearby, maybe it's indexed differently
    println!("\nRaw bytes at PC {pc:#X}:");
    for i in 0..96usize {
        if i % 8 == 0 { print!("\n  [{i:02}] "); }
        print!("{:02x} ", rom[pc + i]);
    }
    println!();
}
