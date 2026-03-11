use std::env;
use smwe_rom::SmwRom;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow <rom.smc>");
    let rom = SmwRom::from_file(path).expect("load failed");

    println!("GFX files: {}", rom.gfx.files.len());
    for (i, f) in rom.gfx.files.iter().enumerate() {
        println!("  [{}] {} tiles", i, f.tiles.len());
    }
}
