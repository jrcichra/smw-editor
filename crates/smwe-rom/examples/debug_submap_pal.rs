/// Debug submap 1 (Yoshi's Island) tile palette distribution
use std::env;
use smwe_rom::SmwRom;
use smwe_rom::overworld::{OW_TILEMAP_COLS, OW_VISIBLE_ROWS};

fn main() {
    let path = env::args().nth(1).expect("need rom");
    let smw = SmwRom::from_file(&path).expect("load");

    for submap in 0..4 {
        let layer = &smw.overworld.layer2[submap];
        let mut pal_counts = [0u32; 8];
        let mut page_counts = [0u32; 8];
        for row in 0..OW_VISIBLE_ROWS {
            for col in 0..OW_TILEMAP_COLS {
                let t = layer.get(col, row);
                if t.tile_index() != 0 {
                    pal_counts[t.palette() as usize] += 1;
                    page_counts[(t.tile_index() as usize) >> 7] += 1;
                }
            }
        }
        println!("Submap {}: pal_dist={:?} page_dist={:?}", submap, pal_counts, page_counts);
        // Show first row
        print!("  Row0: ");
        for col in 0..8 {
            let t = layer.get(col, 0);
            print!("({:#05x},p={}) ", t.tile_index(), t.palette());
        }
        println!();
    }
}
