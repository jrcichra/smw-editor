use smwe_rom::{
    SmwRom,
    graphics::palette::{ColorPalette, OverworldState},
    overworld::{OverworldMaps, OW_SUBMAP_COUNT, OW_LAYER2_BASE, OW_TILEMAP_BYTES},
    disassembler::RomDisassembly,
    snes_utils::{addr::AddrSnes, rom_slice::SnesSlice},
};

fn main() {
    // Test parse directly to see the actual error
    let path = "smw.smc";
    let raw = std::fs::read(path).expect("read ROM");
    // strip 0x200 SMC header if present
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    
    let mut disasm = match RomDisassembly::new(rom_bytes.into()) {
        Ok(d) => d,
        Err(e) => { eprintln!("disasm error: {e}"); return; }
    };
    
    println!("Testing overworld parse directly...");
    match OverworldMaps::parse(&mut disasm) {
        Ok(ow) => {
            println!("Parse OK!");
            for (sm, tm) in ow.layer2.iter().enumerate() {
                let nz = tm.tiles.iter().filter(|t| t.0 != 0).count();
                println!("  submap {} layer2: {} non-zero tiles", sm, nz);
            }
        }
        Err(e) => println!("Parse FAILED: {e:?}"),
    }
    
    // Also manually check the address
    let snes = AddrSnes(OW_LAYER2_BASE);
    let slice = SnesSlice::new(snes, OW_TILEMAP_BYTES);
    println!("\nManual slice check at {:#010x}, size {}:", OW_LAYER2_BASE, OW_TILEMAP_BYTES);
    match disasm.rom.slice_snes_lorom(slice) {
        Ok(bytes) => {
            println!("  got {} bytes: {:?}", bytes.len(), &bytes[..8.min(bytes.len())]);
        }
        Err(e) => println!("  slice error: {e:?}"),
    }
}
