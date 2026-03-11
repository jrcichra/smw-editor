/// Render all 7 submaps using the correct approach from render_ow_correct
/// but via the smwe-rom OverworldMaps parsed data
use std::env;
use smwe_rom::SmwRom;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

fn abgr_to_rgb(c: Abgr1555) -> [u8; 3] {
    [
        (((c.0 >> 0) & 0x1F) * 255 / 31) as u8,
        (((c.0 >> 5) & 0x1F) * 255 / 31) as u8,
        (((c.0 >> 10) & 0x1F) * 255 / 31) as u8,
    ]
}

fn main() {
    let path = env::args().nth(1).expect("need rom path");
    let smw = SmwRom::from_file(&path).expect("load ROM");

    for submap in 0..smw.overworld.layer2.len() {
        let sm_pal = submap.min(5);
        let pal = smw.gfx.color_palettes
            .get_submap_palette(sm_pal, OverworldState::PreSpecial).unwrap();
        let cgram: Vec<[u8; 3]> = (0..256usize).map(|i| {
            let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
            abgr_to_rgb(c)
        }).collect();

        let backdrop = pal.get_color_at(0, 8).map(abgr_to_rgb).unwrap_or([20, 20, 60]);
        let layer2 = &smw.overworld.layer2[submap];

        let w = 32usize;
        let h = 27usize;
        let img_w = w * 8;
        let img_h = h * 8;
        let mut pixels = vec![backdrop; img_w * img_h];

        for row in 0..h {
            for col in 0..w {
                let entry = layer2.get(col, row);
                let chr = entry.tile_index() as usize;
                if chr == 0 { continue; }
                let pal_idx = entry.palette() as usize;
                let flip_x = entry.flip_x();
                let flip_y = entry.flip_y();
                let page = chr >> 7;
                let offset = chr & 0x7F;
                let cgram_base = (4 + (pal_idx & 3)) * 16;

                if page >= 4 { continue; }
                let file_idx = OW_GFX_FILES[page];
                if let Some(tile) = smw.gfx.files.get(file_idx).and_then(|f| f.tiles.get(offset)) {
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

        let out = format!("ow_sm{}.ppm", submap);
        let header = format!("P6\n{} {}\n255\n", img_w, img_h);
        let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
        let mut f = std::fs::File::create(&out).unwrap();
        f.write_all(header.as_bytes()).unwrap();
        f.write_all(&bytes).unwrap();
        println!("Written {}", out);
    }
}
