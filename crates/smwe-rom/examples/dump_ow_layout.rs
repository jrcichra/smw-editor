use std::env;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // Layer2 submap 0 at $0C8000
    let pc = lorom_pc(0x0C8000);
    println!("OW layer2 submap 0 raw bytes at PC {pc:#X} (first 128 bytes as u16s):");
    
    // First 4 rows × 32 cols = 128 u16 entries
    for row in 0..4usize {
        print!("  row {row:2}: ");
        for col in 0..32usize {
            let off = pc + (row * 32 + col) * 2;
            let v = u16::from_le_bytes([rom[off], rom[off+1]]);
            print!("{v:04x} ");
        }
        println!();
    }

    // Check if maybe tiles are stored in a different order
    // SNES BG tilemap: normally stored as (row * 32 + col) but maybe it's 64 cols wide?
    // OW tilemap might be 64 cols wide with only 32 visible
    println!("\nIf stride=64 (64-wide tilemap), first 4 rows:");
    for row in 0..4usize {
        print!("  row {row:2}: ");
        for col in 0..32usize {
            let off = pc + (row * 64 + col) * 2;
            let v = u16::from_le_bytes([rom[off], rom[off+1]]);
            print!("{v:04x} ");
        }
        println!();
    }
}
