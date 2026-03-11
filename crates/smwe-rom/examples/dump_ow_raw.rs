use std::env;

// LoROM: SNES $0C8000 → PC offset
fn lorom_to_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow_raw <rom.smc>");
    let raw = std::fs::read(path).unwrap();
    // Strip 0x200 SMC header
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // OW layer2 base: SNES $0C8000
    let snes_base: u32 = 0x0C8000;
    let pc = lorom_to_pc(snes_base);
    println!("OW layer2 SNES ${snes_base:06X} -> PC offset {pc:#X} (rom len {:#X})", rom.len());

    // Print first 32 bytes as u16 pairs
    if pc + 32 <= rom.len() {
        let bytes = &rom[pc..pc+32];
        print!("First 16 entries: ");
        for chunk in bytes.chunks(2) {
            print!("{:04x} ", u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        println!();
    } else {
        println!("Out of range!");
    }

    // Also check what smwe-rom is reading — maybe it's using a different header assumption
    // smwe-rom reads the ROM without stripping the header (it handles it internally)
    let raw_full = std::fs::read(path).unwrap();
    let pc_full = lorom_to_pc(snes_base);
    println!("\nWith full (header included) raw bytes (len {:#X}), PC = {pc_full:#X}", raw_full.len());
    if pc_full + 0x10 <= raw_full.len() {
        let bytes = &raw_full[pc_full..pc_full+0x10];
        print!("  bytes: ");
        for b in bytes { print!("{b:02x} "); }
        println!();
    }

    // And with 0x200 added to PC (accounting for SMC header)
    let pc_with_header = pc + 0x200;
    println!("\nWith header at PC+0x200 = {pc_with_header:#X}:");
    if pc_with_header + 0x20 <= raw_full.len() {
        let bytes = &raw_full[pc_with_header..pc_with_header+0x20];
        print!("  first 16 u16: ");
        for chunk in bytes.chunks(2) {
            print!("{:04x} ", u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        println!();
    }
}
