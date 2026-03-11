use std::env;
use smwe_rom::SmwRom;
use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

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
    let out = args.get(2).map(|s| s.as_str()).unwrap_or("ow_full.ppm");

    let raw = std::fs::read(path).unwrap();
    let rom_bytes = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };
    let smw = SmwRom::from_file(path).expect("load ROM");

    let pal = smw.gfx.color_palettes
        .get_submap_palette(0, OverworldState::PreSpecial).unwrap();
    let cgram: Vec<[u8; 3]> = (0..256usize).map(|i| {
        let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();
    let backdrop = pal.get_color_at(0, 8).map(abgr_to_rgb).unwrap_or([100, 160, 255]);

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    let total_w = 40usize;
    let total_h = 58usize;
    let tiles = decompress_rle2(&rom_bytes[tile_pc..], &rom_bytes[attr_pc..], total_w * total_h * 2);

    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        let page = chr >> 7;
        let offset = chr & 0x7F;
        smw.gfx.files.get(*OW_GFX_FILES.get(page)?)?.tiles.get(offset)
    };

    // Render the FULL tilemap as-is
    let img_w = total_w * 8;
    let img_h = total_h * 8;
    let mut pixels = vec![backdrop; img_w * img_h];

    for row in 0..total_h {
        for col in 0..total_w {
            let t = tiles[row * total_w + col];
            let chr = (t & 0x3FF) as usize;
            if chr == 0 { continue; }
            let pal_idx = ((t >> 10) & 7) as usize;
            let flip_x = (t >> 14) & 1 != 0;
            let flip_y = (t >> 15) & 1 != 0;
            let cgram_base = (4 + (pal_idx & 3)) * 16;

            if let Some(tile) = get_tile(chr) {
                for py in 0..8usize {
                    let spy = if flip_y { 7 - py } else { py };
                    for px in 0..8usize {
                        let spx = if flip_x { 7 - px } else { px };
                        let ci = tile.color_indices[spy * 8 + spx] as usize;
                        if ci != 0 {
                            pixels[(row * 8 + py) * img_w + (col * 8 + px)] = cgram[cgram_base + ci];
                        }
                    }
                }
            }
        }
    }

    // Draw a red line at row 27 to show main map boundary
    for col in 0..img_w {
        pixels[27 * 8 * img_w + col] = [255, 0, 0];
    }

    let header = format!("P6\n{} {}\n255\n", img_w, img_h);
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create(out).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("Written {} ({}x{})", out, img_w, img_h);
}
