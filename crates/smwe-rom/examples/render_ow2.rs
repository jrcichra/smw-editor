use std::env;
use smwe_rom::SmwRom;
use smwe_rom::overworld::{OW_TILEMAP_COLS, OW_VISIBLE_ROWS};
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

const OW_GFX_FILES: [[usize; 8]; 6] = [
    [0x00, 0x01, 0x13, 0x02, 0x00, 0x01, 0x12, 0x03],
    [0x00, 0x01, 0x13, 0x05, 0x00, 0x01, 0x13, 0x04],
    [0x00, 0x01, 0x13, 0x06, 0x00, 0x01, 0x13, 0x09],
    [0x00, 0x01, 0x13, 0x04, 0x00, 0x01, 0x06, 0x11],
    [0x00, 0x01, 0x13, 0x20, 0x00, 0x01, 0x13, 0x0F],
    [0x00, 0x01, 0x13, 0x23, 0x00, 0x01, 0x0D, 0x14],
];

fn abgr_to_rgb(c: Abgr1555) -> [u8; 3] {
    [
        (((c.0 >> 0) & 0x1F) * 255 / 31) as u8,
        (((c.0 >> 5) & 0x1F) * 255 / 31) as u8,
        (((c.0 >> 10) & 0x1F) * 255 / 31) as u8,
    ]
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("ow_both.ppm");
    let rom = SmwRom::from_file(path).expect("load");

    let submap = 0;
    let gfx_pages = OW_GFX_FILES[submap];
    let pal = rom.gfx.color_palettes
        .get_submap_palette(submap, OverworldState::PreSpecial).unwrap();

    let cgram: Vec<[u8;3]> = (0..256usize).map(|i| {
        let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();

    // Get backdrop (CGRAM row 0 col 0) - use layer3 row 0 color as sky
    // For OW, the "background color" is typically in CGRAM[0][0] which is the
    // OW layer3 palette. SpecificOverworldColorPalette has layer3 at [0x0..=0x1, 0x8..=0xF]
    // Use row 0, col 8 as the sky/ocean backdrop
    let backdrop = pal.get_color_at(0, 8).map(abgr_to_rgb).unwrap_or([20, 20, 30]);
    println!("Backdrop color: {:?}", backdrop);

    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        let page = chr >> 7;
        let offset = chr & 0x7F;
        rom.gfx.files.get(*gfx_pages.get(page)?)?.tiles.get(offset)
    };

    let img_w = OW_TILEMAP_COLS * 8;
    let img_h = OW_VISIBLE_ROWS * 8;
    let mut pixels = vec![backdrop; img_w * img_h];

    let draw_layer = |pixels: &mut Vec<[u8;3]>, tilemap: &smwe_rom::overworld::OwTilemap,
                      cgram_row_offset: usize| {
        for row in 0..OW_VISIBLE_ROWS {
            for col in 0..OW_TILEMAP_COLS {
                let t = tilemap.get(col, row);
                let chr = t.tile_index() as usize;
                if chr == 0 { continue; }
                let cgram_base = (cgram_row_offset + (t.palette() as usize & 3)) * 16;
                if let Some(tile) = get_tile(chr) {
                    for py in 0..8usize {
                        let src_py = if t.flip_y() { 7 - py } else { py };
                        for px in 0..8usize {
                            let src_px = if t.flip_x() { 7 - px } else { px };
                            let ci = tile.color_indices[src_py * 8 + src_px] as usize;
                            if ci != 0 {
                                let idx = (row * 8 + py) * img_w + (col * 8 + px);
                                pixels[idx] = cgram[cgram_base + ci];
                            }
                        }
                    }
                }
            }
        }
    };

    // Layer 2 (background terrain) - sub-palettes 4-7 → CGRAM rows 4-7
    draw_layer(&mut pixels, &rom.overworld.layer2[submap], 4);

    // Layer 1 (paths/events) - sub-palettes 2-3 → CGRAM rows 2-3
    if rom.overworld.layer1.len() > submap {
        draw_layer(&mut pixels, &rom.overworld.layer1[submap], 2);
    }

    let header = format!("P6\n{img_w} {img_h}\n255\n");
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create(out).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("Written {out}");
}
