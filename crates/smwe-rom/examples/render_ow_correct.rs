/// Quick render using correct GFX files (GFX1C, GFX1D, GFX08, GFX1E)
/// and the correct tilemap source ($04A533/$04C02B, LC_RLE2, stride 40).
use std::env;
use smwe_rom::SmwRom;
use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

// Correct OW GFX file indices for all submaps
const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

// Tilemap is 40 tiles wide, 28 visible rows per submap
const OW_WIDTH: usize = 40;
const OW_HEIGHT: usize = 27;

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
    let submap_arg: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(0);
    let out = args.get(3).map(|s| s.as_str()).unwrap_or("ow_correct.ppm");

    let raw = std::fs::read(path).unwrap();
    let rom_bytes = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };
    let smw = SmwRom::from_file(path).expect("load ROM");

    // --- Palette ---
    let pal = smw.gfx.color_palettes
        .get_submap_palette(submap_arg, OverworldState::PreSpecial).unwrap();
    let cgram: Vec<[u8; 3]> = (0..256usize).map(|i| {
        let c = pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();

    // Use layer3 row 0 col 8 as backdrop (sky/ocean color)
    let backdrop = pal.get_color_at(0, 8).map(abgr_to_rgb).unwrap_or([100, 160, 255]);
    println!("Backdrop: {:?}", backdrop);

    // --- Decompress tilemap ---
    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    // Full overworld is 40 wide × 58 rows (main map 28 + submaps below/beside)
    // Let's decompress 40×64 and find where each submap starts
    let total_tiles = OW_WIDTH * 64;
    let tiles = decompress_rle2(&rom_bytes[tile_pc..], &rom_bytes[attr_pc..], total_tiles * 2);
    println!("Decompressed {} tiles, {} non-zero", total_tiles,
        tiles.iter().filter(|&&t| t != 0 && t != 0xBABA).count());

    // Find which rows have content (to locate the submap)
    println!("Row occupancy (non-zero, non-baba tiles):");
    for row in 0..64usize {
        let count = (0..OW_WIDTH).filter(|&c| {
            let t = tiles[row * OW_WIDTH + c];
            t != 0 && t != 0xBABA
        }).count();
        if count > 3 {
            println!("  row {:2}: {} tiles", row, count);
        }
    }

    // Submaps are arranged in the buffer. Main map = rows 0-27, submaps below.
    // According to the wiki: on submaps subtract 2 from X and 1 from Y
    // So submap display position (0,0) = buffer position (X-2, Y-1)
    // Let's try to render from the correct submap Y offset.
    // Main map = rows 0-27. Submaps start at row 28? Let's check.

    // For now, render starting at row = submap_arg * 28 as a first guess
    let row_offset = submap_arg * OW_HEIGHT;
    println!("Rendering submap {} at row offset {}", submap_arg, row_offset);

    // --- GFX tiles ---
    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        let page = chr >> 7;          // which of 4 GFX files (0-3)
        let offset = chr & 0x7F;      // tile within that file
        smw.gfx.files.get(*OW_GFX_FILES.get(page)?)?.tiles.get(offset)
    };

    // Print first-row tile info
    println!("First row tiles (submap {} at row_offset {}):", submap_arg, row_offset);
    for col in 0..10usize {
        let idx = (row_offset) * OW_WIDTH + col;
        let t = if idx < tiles.len() { tiles[idx] } else { 0 };
        let chr = (t & 0x3FF) as usize;
        let pal_idx = ((t >> 10) & 7) as usize;
        println!("  [{},{}] t={:04x} chr={:#05x} page={} offset={} pal={}", 
            0, col, t, chr, chr>>7, chr&0x7F, pal_idx);
    }

    // --- Render ---
    let img_w = OW_WIDTH * 8;
    let img_h = OW_HEIGHT * 8;
    let mut pixels = vec![backdrop; img_w * img_h];

    for row in 0..OW_HEIGHT {
        for col in 0..OW_WIDTH {
            let map_idx = (row_offset + row) * OW_WIDTH + col;
            if map_idx >= tiles.len() { continue; }
            let t = tiles[map_idx];
            let chr = (t & 0x3FF) as usize;
            if chr == 0 { continue; }
            let pal_idx = ((t >> 10) & 7) as usize;
            let flip_x = (t >> 14) & 1 != 0;
            let flip_y = (t >> 15) & 1 != 0;
            // Layer 2 sub-palettes 0-7, CGRAM rows 4-7 (pal 0-3) or wrap
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

    let header = format!("P6\n{} {}\n255\n", img_w, img_h);
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create(out).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("Written {}", out);
}
