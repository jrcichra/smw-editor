use std::env;
use smwe_rom::SmwRom;
use smwe_rom::overworld::{OW_TILEMAP_COLS, OW_VISIBLE_ROWS};

/// OW VRAM page layout: 8 pages × 128 tiles = CHR 0x000–0x3FF
/// Page N = GFX file OW_GFX_FILES[submap][N], tiles N*128 .. N*128+127
const OW_GFX_FILES: [[usize; 8]; 6] = [
    [0x00, 0x01, 0x13, 0x02, 0x00, 0x01, 0x12, 0x03], // Yoshi's Island
    [0x00, 0x01, 0x13, 0x05, 0x00, 0x01, 0x13, 0x04],
    [0x00, 0x01, 0x13, 0x06, 0x00, 0x01, 0x13, 0x09],
    [0x00, 0x01, 0x13, 0x04, 0x00, 0x01, 0x06, 0x11],
    [0x00, 0x01, 0x13, 0x20, 0x00, 0x01, 0x13, 0x0F],
    [0x00, 0x01, 0x13, 0x23, 0x00, 0x01, 0x0D, 0x14],
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: dump_ow_tiles <rom.smc>");
    let rom = SmwRom::from_file(path).expect("load failed");

    let submap = 0usize;
    let layer2 = &rom.overworld.layer2[submap];
    let gfx_pages = OW_GFX_FILES[submap];

    // Collect CHR usage histogram
    let mut chr_counts = std::collections::BTreeMap::new();
    for row in 0..OW_VISIBLE_ROWS {
        for col in 0..OW_TILEMAP_COLS {
            let t = layer2.get(col, row);
            *chr_counts.entry(t.tile_index()).or_insert(0u32) += 1;
        }
    }

    println!("CHR usage in submap 0 (Yoshi's Island), {} unique tiles:", chr_counts.len());
    for (chr, count) in &chr_counts {
        let page = (*chr as usize) >> 7;
        let offset = (*chr as usize) & 0x7F;
        let gfx_file = if page < 8 { gfx_pages[page] } else { 0xFF };
        // Does that tile exist?
        let exists = rom.gfx.files.get(gfx_file).map(|f| offset < f.tiles.len()).unwrap_or(false);
        println!("  CHR {chr:#05x}  page={page}  offset={offset:3}  gfx={gfx_file:02x}  exists={exists}  count={count}");
    }

    // Show first 8 tiles of submap 0 row 0 in detail
    println!("\nFirst row of submap 0:");
    for col in 0..OW_TILEMAP_COLS {
        let t = layer2.get(col, 0);
        let chr = t.tile_index() as usize;
        let page = chr >> 7;
        let offset = chr & 0x7F;
        let gfx_file = if page < 8 { gfx_pages[page] } else { 0xFF };
        println!("  [{col:2}] chr={chr:#05x} page={page} gfx={gfx_file:#04x} offset={offset} pal={} fx={} fy={}",
            t.palette(), t.flip_x(), t.flip_y());
    }
}
