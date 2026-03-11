use std::env;
use smwe_rom::SmwRom;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use image::{ImageBuffer, Rgba};

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: render_ow_preview <rom.smc> [output.png]");
    let out_path = args.get(2).map(|s| s.as_str()).unwrap_or("ow_preview.png");
    let rom = SmwRom::from_file(path).expect("load failed");

    let submap = 0usize;

    // Build palette
    let pal = rom.gfx.color_palettes.get_submap_palette(submap, OverworldState::PreSpecial).expect("palette");
    let cgram: Vec<Abgr1555> = (0..256usize)
        .map(|i| pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555(0x8000)))
        .collect();

    const OW_GFX_FILES: [[usize; 8]; 6] = [
        [0x00, 0x01, 0x13, 0x02, 0x00, 0x01, 0x12, 0x03],
        [0x00, 0x01, 0x13, 0x05, 0x00, 0x01, 0x13, 0x04],
        [0x00, 0x01, 0x13, 0x06, 0x00, 0x01, 0x13, 0x09],
        [0x00, 0x01, 0x13, 0x04, 0x00, 0x01, 0x06, 0x11],
        [0x00, 0x01, 0x13, 0x20, 0x00, 0x01, 0x13, 0x0F],
        [0x00, 0x01, 0x13, 0x23, 0x00, 0x01, 0x0D, 0x14],
    ];

    let gfx_pages = OW_GFX_FILES[submap.min(5)];
    let layer2 = &rom.overworld.layer2[submap];

    let map_w = 32usize;
    let map_h = 27usize;
    let img_w = map_w * 8;
    let img_h = map_h * 8;

    let mut img = ImageBuffer::<Rgba<u8>, _>::from_pixel(img_w as u32, img_h as u32, Rgba([20, 20, 40, 255]));

    for row in 0..map_h {
        for col in 0..map_w {
            let entry = layer2.get(col, row);
            let chr = entry.tile_index() as usize;
            let page = chr >> 7;
            let offset = chr & 0x7F;
            let cgram_row = entry.palette() as usize;
            let flip_x = entry.flip_x();
            let flip_y = entry.flip_y();

            if page >= 8 { continue; }
            let file_idx = gfx_pages[page];
            let tile = match rom.gfx.files.get(file_idx).and_then(|f| f.tiles.get(offset)) {
                Some(t) => t,
                None => continue,
            };

            let base = cgram_row * 16;
            for py in 0..8usize {
                for px in 0..8usize {
                    let src_py = if flip_y { 7 - py } else { py };
                    let src_px = if flip_x { 7 - px } else { px };
                    let ci = tile.color_indices.get(src_py * 8 + src_px).copied().unwrap_or(0) as usize;
                    if ci == 0 { continue; }
                    let c = cgram.get(base + ci).copied().unwrap_or(Abgr1555(0x8000));
                    let v = c.0;
                    let r = ((v & 0x001F) << 3) as u8;
                    let g = (((v >> 5) & 0x001F) << 3) as u8;
                    let b = (((v >> 10) & 0x001F) << 3) as u8;
                    img.put_pixel((col * 8 + px) as u32, (row * 8 + py) as u32, Rgba([r, g, b, 255]));
                }
            }
        }
    }

    img.save(out_path).expect("save failed");
    println!("Saved {}", out_path);
}
