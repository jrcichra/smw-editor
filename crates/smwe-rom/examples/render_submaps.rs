use std::env;
use smwe_rom::SmwRom;
use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];
// Submap scroll origins (X pixel, Y pixel) from SNES scroll registers
// X mod 512 / 8 = tile col, Y / 8 = tile row
const SUBMAP_SCROLLS: &[(u32, u32, &str)] = &[
    (15876, 310,  "Main"),
    (12288, 314,  "YI"),
    (13313, 0,    "VanDome"),
    (13825, 343,  "Forest"),
    (14849, 388,  "ValBowser"),
    (1,     314,  "Special"),
    (22272, 0,    "StarWorld"),
];
const BUF_W: usize = 40;
const VIEW_W: usize = 32;
const VIEW_H: usize = 27;

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}
fn abgr_to_rgb(c: Abgr1555) -> [u8; 3] {
    [(((c.0)&0x1F)*255/31)as u8,(((c.0>>5)&0x1F)*255/31)as u8,(((c.0>>10)&0x1F)*255/31)as u8]
}

fn render_submap(
    tiles: &[u16], buf_w: usize, tile_col: usize, tile_row: usize,
    smw: &SmwRom, cgram: &[[u8;3]], backdrop: [u8;3],
    out: &str,
) {
    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        smw.gfx.files.get(*OW_GFX_FILES.get(chr>>7)?)?.tiles.get(chr&0x7F)
    };
    let img_w = VIEW_W * 8;
    let img_h = VIEW_H * 8;
    let mut pixels = vec![backdrop; img_w * img_h];
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            let buf_col = (tile_col + col) % buf_w;
            let buf_row = tile_row + row;
            if buf_row * buf_w + buf_col >= tiles.len() { continue; }
            let t = tiles[buf_row * buf_w + buf_col];
            let chr = (t & 0x3FF) as usize;
            if chr == 0 { continue; }
            let cg_base = (4 + ((t >> 10) & 3) as usize) * 16;
            let fx = (t >> 14) & 1 != 0;
            let fy = (t >> 15) & 1 != 0;
            if let Some(tile) = get_tile(chr) {
                for py in 0..8usize {
                    let spy = if fy { 7-py } else { py };
                    for px in 0..8usize {
                        let spx = if fx { 7-px } else { px };
                        let ci = tile.color_indices[spy*8+spx] as usize;
                        if ci != 0 {
                            pixels[(row*8+py)*img_w+(col*8+px)] = cgram[cg_base+ci];
                        }
                    }
                }
            }
        }
    }
    let hdr = format!("P6\n{} {}\n255\n", img_w, img_h);
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create(out).unwrap();
    f.write_all(hdr.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("  written {}", out);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };
    let smw = SmwRom::from_file(path).unwrap();

    // Use main map palette (submap 0)
    let pal = smw.gfx.color_palettes.get_submap_palette(0, OverworldState::PreSpecial).unwrap();
    let cgram: Vec<[u8;3]> = (0..256).map(|i| {
        let c = pal.get_color_at(i/16, i%16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();
    let backdrop = pal.get_color_at(0, 8).map(abgr_to_rgb).unwrap_or([100, 160, 255]);

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    let total = BUF_W * 64;
    let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total * 2);

    for (i, &(scroll_x, scroll_y, name)) in SUBMAP_SCROLLS.iter().enumerate() {
        // SNES BG2: 512-pixel wide virtual space → tile col = (scroll_x / 8) mod 64
        // Buffer is 40 wide, so col within buffer = col mod 40
        let tile_col = ((scroll_x / 8) as usize) % BUF_W;
        let tile_row = (scroll_y / 8) as usize;
        println!("Submap {i} ({name}): scroll=({scroll_x},{scroll_y}) → tile=({tile_col},{tile_row})");
        let out = format!("submap_{i}_{name}.ppm");
        render_submap(&tiles, BUF_W, tile_col, tile_row, &smw, &cgram, backdrop, &out);
    }
}
