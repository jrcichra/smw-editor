/// Find submap positions by looking at where the SNES scrolls when entering each submap.
/// The submap scroll positions are stored at $04D89A (X scroll) and $04D8A1 (Y scroll),
/// 7 bytes each (one per submap: main=0, YI=1, VD=2, FoI=3, VoB=4, SW=5, StarW=6).
use std::env;
use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::SmwRom;
use smwe_rom::graphics::palette::{ColorPalette, OverworldState};
use smwe_render::color::Abgr1555;
use std::io::Write;

const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}
fn abgr_to_rgb(c: Abgr1555) -> [u8; 3] {
    [(((c.0)&0x1F)*255/31)as u8,(((c.0>>5)&0x1F)*255/31)as u8,(((c.0>>10)&0x1F)*255/31)as u8]
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("need rom");
    let raw = std::fs::read(path).unwrap();
    let rom = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };

    // Submap initial scroll positions (in pixels, divide by 8 for tile coords)
    // X scrolls at $04D89A, Y scrolls at $04D8A1 (7 bytes each)
    // Actually these are the camera positions; let's look at them
    let names = ["Main", "YI", "VanDome", "Forest", "ValBowser", "Special", "StarWorld"];
    
    // Try several known addresses for submap scroll init
    for &addr in &[0x04D89Au32, 0x04D8A1, 0x04DA1D, 0x048D90, 0x04D900] {
        let pc = lorom_pc(addr);
        print!("  ${:06X}: ", addr);
        for i in 0..7usize {
            print!("{:02x} ", rom[pc+i]);
        }
        println!();
    }

    // The OW camera uses $1F17 (X) and $1F19 (Y) pixel positions
    // Submap initial camera positions are at $04D89A (X, 7 entries) and $04D8A1 (Y, 7 entries)
    // These should be the top-left pixel of each submap's view
    // Dividing by 8 gives the tile column/row offset

    // Let's read the known camera init table
    // From SMW disassembly: InitialSubworldScrollX at $04D89A, InitialSubworldScrollY at $04D8A1
    let scroll_x_pc = lorom_pc(0x04D89A);
    let scroll_y_pc = lorom_pc(0x04D8A1);
    println!("\nSubmap scroll positions (pixels):");
    println!("{:<12} {:>8} {:>8} {:>8} {:>8}", "Submap", "X-px", "Y-px", "X-tile", "Y-tile");
    for i in 0..7usize {
        // These may be 16-bit values
        let x = u16::from_le_bytes([rom[scroll_x_pc + i*2], rom[scroll_x_pc + i*2 + 1]]) as usize;
        let y = u16::from_le_bytes([rom[scroll_y_pc + i*2], rom[scroll_y_pc + i*2 + 1]]) as usize;
        println!("  {:<12} {:8} {:8} {:8} {:8}", names[i], x, y, x/8, y/8);
    }

    // Now let's try to find the actual submap rectangle by rendering the full tilemap
    // and looking for the transition border tile (empty = 0 or specific divider)
    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    let w = 40usize;
    let h = 58usize;
    let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], w * h * 2);

    // Find rows that are all-same or near-zero (submap dividers)
    println!("\nRows with 0 non-uniform tiles (potential submap boundaries):");
    for row in 0..h {
        let first = tiles[row * w];
        let uniform = (0..w).all(|c| tiles[row * w + c] == first);
        let all_same = tiles[row * w];
        if uniform {
            println!("  row {:2}: all {:04x}", row, all_same);
        }
    }

    // Also render the full map with grid lines at row 27 and col 20
    let smw = SmwRom::from_file(path).unwrap();
    let pal = smw.gfx.color_palettes.get_submap_palette(0, OverworldState::PreSpecial).unwrap();
    let cgram: Vec<[u8;3]> = (0..256).map(|i|{
        let c = pal.get_color_at(i/16,i%16).unwrap_or(Abgr1555::TRANSPARENT);
        abgr_to_rgb(c)
    }).collect();
    let backdrop = pal.get_color_at(0,8).map(abgr_to_rgb).unwrap_or([100,160,255]);

    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        smw.gfx.files.get(*OW_GFX_FILES.get(chr>>7)?)?.tiles.get(chr&0x7F)
    };

    let img_w = w * 8;
    let img_h = h * 8;
    let mut pixels = vec![backdrop; img_w * img_h];
    for row in 0..h {
        for col in 0..w {
            let t = tiles[row*w+col];
            let chr = (t&0x3FF) as usize;
            if chr == 0 { continue; }
            let cg_base = (4+((t>>10)&3) as usize)*16;
            let fx = (t>>14)&1!=0; let fy = (t>>15)&1!=0;
            if let Some(tile) = get_tile(chr) {
                for py in 0..8usize {
                    let spy = if fy {7-py} else {py};
                    for px in 0..8usize {
                        let spx = if fx {7-px} else {px};
                        let ci = tile.color_indices[spy*8+spx] as usize;
                        if ci != 0 {
                            pixels[(row*8+py)*img_w+(col*8+px)] = cgram[cg_base+ci];
                        }
                    }
                }
            }
        }
    }
    // Red grid line at row 27 boundary
    for col in 0..img_w { pixels[27*8*img_w+col] = [255,0,0]; }
    // Blue grid at col 20 (half of 40)
    for row in 0..img_h { pixels[row*img_w+20*8] = [0,0,255]; }

    let hdr = format!("P6\n{} {}\n255\n", img_w, img_h);
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|rgb| rgb).collect();
    let mut f = std::fs::File::create("ow_analysis.ppm").unwrap();
    f.write_all(hdr.as_bytes()).unwrap();
    f.write_all(&bytes).unwrap();
    println!("\nWritten ow_analysis.ppm");
}
