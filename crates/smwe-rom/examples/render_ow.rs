/// Renders submap 0 (Yoshi's Island) OW layer-2 to a PNG so we can visually inspect it.
use std::env;
use smwe_rom::SmwRom;
use smwe_rom::overworld::{OW_TILEMAP_COLS, OW_VISIBLE_ROWS};
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;

const OW_GFX_FILES: [[usize; 8]; 6] = [
    [0x00, 0x01, 0x13, 0x02, 0x00, 0x01, 0x12, 0x03],
    [0x00, 0x01, 0x13, 0x05, 0x00, 0x01, 0x13, 0x04],
    [0x00, 0x01, 0x13, 0x06, 0x00, 0x01, 0x13, 0x09],
    [0x00, 0x01, 0x13, 0x04, 0x00, 0x01, 0x06, 0x11],
    [0x00, 0x01, 0x13, 0x20, 0x00, 0x01, 0x13, 0x0F],
    [0x00, 0x01, 0x13, 0x23, 0x00, 0x01, 0x0D, 0x14],
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let rom_path = args.get(1).expect("Usage: render_ow <rom.smc> [out.ppm]");
    let out_path = args.get(2).map(|s| s.as_str()).unwrap_or("ow_submap0.ppm");
    let rom = SmwRom::from_file(rom_path).expect("load ROM");

    let submap = 0usize;
    let layer2 = &rom.overworld.layer2[submap];
    let gfx_pages = OW_GFX_FILES[submap];

    // Build CGRAM
    let pal = rom.gfx.color_palettes
        .get_submap_palette(submap, OverworldState::PreSpecial)
        .expect("palette");
    let cgram: Vec<[u8;3]> = (0..256usize).map(|i| {
        let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
        // ABGR1555: bits 4-0=R, 9-5=G, 14-10=B
        let r = (((c.0 >> 0) & 0x1F) * 255 / 31) as u8;
        let g = (((c.0 >> 5) & 0x1F) * 255 / 31) as u8;
        let b = (((c.0 >> 10) & 0x1F) * 255 / 31) as u8;
        [r, g, b]
    }).collect();

    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        let page = chr >> 7;
        let offset = chr & 0x7F;
        let file_idx = *gfx_pages.get(page)?;
        rom.gfx.files.get(file_idx)?.tiles.get(offset)
    };

    let img_w = OW_TILEMAP_COLS * 8;
    let img_h = OW_VISIBLE_ROWS * 8;
    let mut pixels = vec![[20u8, 20, 20]; img_w * img_h];

    for row in 0..OW_VISIBLE_ROWS {
        for col in 0..OW_TILEMAP_COLS {
            let t = layer2.get(col, row);
            let chr = t.tile_index() as usize;
            let pal_idx = t.palette() as usize;
            // OW layer2 sub-palette → CGRAM row 4..7
            // Sub-palettes 0-3 → rows 4-7; sub-palettes 4-7 → rows 4-7 (wrap)
            let cgram_base = (4 + (pal_idx & 3)) * 16;

            if let Some(tile) = get_tile(chr) {
                let flip_x = t.flip_x();
                let flip_y = t.flip_y();
                for py in 0..8usize {
                    for px in 0..8usize {
                        let src_py = if flip_y { 7 - py } else { py };
                        let src_px = if flip_x { 7 - px } else { px };
                        let ci = tile.color_indices.get(src_py * 8 + src_px).copied().unwrap_or(0) as usize;
                        if ci != 0 {
                            let rgb = cgram[cgram_base + ci];
                            let idx = (row * 8 + py) * img_w + (col * 8 + px);
                            pixels[idx] = rgb;
                        }
                    }
                }
            }
        }
    }

    // Write PPM
    let header = format!("P6\n{img_w} {img_h}\n255\n");
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    
    use std::io::Write;
    let mut f = std::fs::File::create(out_path).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("Wrote {out_path}");

    // Also print some debug info about the palette
    println!("\nCGRAM rows 4-7 (OW layer2 palettes):");
    for row in 4..8usize {
        print!("  row {row}:");
        for col in 0..8usize {
            let c = pal.get_color_at(row, col).unwrap_or(Abgr1555::TRANSPARENT);
            print!(" {:04x}", c.0);
        }
        println!();
    }

    println!("\nFirst 8 tiles of row 0:");
    for col in 0..8usize {
        let t = layer2.get(col, 0);
        let chr = t.tile_index() as usize;
        let page = chr >> 7;
        let gfx = if page < 8 { gfx_pages[page] } else { 0xFF };
        println!("  [{col}] chr={chr:#05x} page={page} gfx={gfx:#04x} pal={} flip={},{}",
            t.palette(), t.flip_x(), t.flip_y());
    }
}
