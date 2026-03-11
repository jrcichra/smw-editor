use std::env;
use smwe_rom::SmwRom;

fn main() {
    let path = env::args().nth(1).expect("need rom path");
    let rom = SmwRom::from_file(&path).expect("load failed");
    
    println!("Total GFX files: {}", rom.gfx.files.len());
    
    let files_to_check: &[(usize, &str)] = &[
        (0x00, "0x00"), (0x01, "0x01"), (0x13, "0x13"), (0x02, "0x02"),
        (0x1C, "0x1C"), (0x1D, "0x1D"), (0x08, "0x08"), (0x1E, "0x1E"),
    ];
    
    for (fi, name) in files_to_check {
        if let Some(gfx) = rom.gfx.files.get(*fi) {
            let n = gfx.tiles.len();
            if let Some(tile) = gfx.tiles.get(117) {
                let nonzero: Vec<u8> = tile.color_indices.iter().copied().filter(|&c| c != 0).collect();
                println!("GFX[{}] ({} tiles) tile[117] nonzero={} values={:?}", 
                    name, n, nonzero.len(), &nonzero[..nonzero.len().min(8)]);
            } else {
                println!("GFX[{}] ({} tiles) - no tile 117", name, n);
            }
        } else {
            println!("GFX[{}] - FILE NOT FOUND", name);
        }
    }
    
    if let (Some(f0), Some(f1c)) = (rom.gfx.files.get(0x00), rom.gfx.files.get(0x1C)) {
        let same = f0.tiles.len() == f1c.tiles.len() && 
            f0.tiles.iter().zip(f1c.tiles.iter())
                .all(|(a,b)| a.color_indices == b.color_indices);
        println!("\nGFX 0x00 tiles: {}, GFX 0x1C tiles: {}", f0.tiles.len(), f1c.tiles.len());
        println!("GFX 0x00 == GFX 0x1C: {}", same);
    }
    if let (Some(f1), Some(f1d)) = (rom.gfx.files.get(0x01), rom.gfx.files.get(0x1D)) {
        let same = f1.tiles.len() == f1d.tiles.len() && 
            f1.tiles.iter().zip(f1d.tiles.iter())
                .all(|(a,b)| a.color_indices == b.color_indices);
        println!("GFX 0x01 tiles: {}, GFX 0x1D tiles: {}", f1.tiles.len(), f1d.tiles.len());
        println!("GFX 0x01 == GFX 0x1D: {}", same);
    }
    if let (Some(f13), Some(f08)) = (rom.gfx.files.get(0x13), rom.gfx.files.get(0x08)) {
        let same = f13.tiles.len() == f08.tiles.len() && 
            f13.tiles.iter().zip(f08.tiles.iter())
                .all(|(a,b)| a.color_indices == b.color_indices);
        println!("GFX 0x13 tiles: {}, GFX 0x08 tiles: {}", f13.tiles.len(), f08.tiles.len());
        println!("GFX 0x13 == GFX 0x08: {}", same);
    }
    if let (Some(f02), Some(f1e)) = (rom.gfx.files.get(0x02), rom.gfx.files.get(0x1E)) {
        let same = f02.tiles.len() == f1e.tiles.len() && 
            f02.tiles.iter().zip(f1e.tiles.iter())
                .all(|(a,b)| a.color_indices == b.color_indices);
        println!("GFX 0x02 tiles: {}, GFX 0x1E tiles: {}", f02.tiles.len(), f1e.tiles.len());
        println!("GFX 0x02 == GFX 0x1E: {}", same);
    }
}
