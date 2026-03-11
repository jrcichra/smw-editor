use std::env;
use smwe_rom::SmwRom;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

fn abgr_to_rgb(c: Abgr1555) -> [u8; 3] {
    let r = (((c.0 >> 0) & 0x1F) * 255 / 31) as u8;
    let g = (((c.0 >> 5) & 0x1F) * 255 / 31) as u8;
    let b = (((c.0 >> 10) & 0x1F) * 255 / 31) as u8;
    [r, g, b]
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("Usage: render_gfx <rom.smc> <file_num> [out.ppm]");
    let file_num: usize = args.get(2).expect("need file_num").parse().unwrap();
    let out_path = args.get(3).map(|s| s.as_str()).unwrap_or("gfx_sheet.ppm");

    let rom = SmwRom::from_file(path).expect("load ROM");

    let pal = rom.gfx.color_palettes
        .get_submap_palette(0, OverworldState::PreSpecial)
        .expect("palette");
    let cgram: Vec<[u8;3]> = (0..256usize).map(|i| {
        let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();

    let gfx_file = rom.gfx.files.get(file_num).expect("file not found");
    let n = gfx_file.tiles.len();
    let cols = 16usize;
    let rows = (n + cols - 1) / cols;
    let img_w = cols * 8;
    let img_h = rows * 8;

    let mut pixels = vec![[20u8, 20, 20]; img_w * img_h];

    // Use sub-palette 0 of OW (cgram row 4)
    let cgram_base = 4 * 16;

    for (tidx, tile) in gfx_file.tiles.iter().enumerate() {
        let tc = tidx % cols;
        let tr = tidx / cols;
        for py in 0..8usize {
            for px in 0..8usize {
                let ci = tile.color_indices[py * 8 + px] as usize;
                let rgb = if ci == 0 {
                    if (tc + px / 4 + tr + py / 4) % 2 == 0 { [40u8, 40, 40] } else { [25, 25, 25] }
                } else {
                    cgram[cgram_base + ci]
                };
                pixels[(tr * 8 + py) * img_w + (tc * 8 + px)] = rgb;
            }
        }
    }

    let header = format!("P6\n{img_w} {img_h}\n255\n");
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create(out_path).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("GFX file {file_num:#04x}: {n} tiles -> {out_path}");
}
