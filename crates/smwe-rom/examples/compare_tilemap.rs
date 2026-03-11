/// Compare what smwe-rom's overworld parser gives vs raw decompressed bytes
use std::env;
use smwe_rom::SmwRom;
use smwe_rom::compression::lc_rle2::decompress_rle2;
use smwe_rom::overworld::{OW_TILEMAP_COLS, OW_VISIBLE_ROWS};

fn lorom_pc(snes: u32) -> usize {
    (((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize
}

fn main() {
    let path = env::args().nth(1).expect("need rom path");
    let raw = std::fs::read(&path).unwrap();
    let rom_bytes = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw[..] };
    let smw = SmwRom::from_file(&path).expect("load ROM");

    let tile_pc = lorom_pc(0x04A533);
    let attr_pc = lorom_pc(0x04C02B);
    let tiles_raw = decompress_rle2(&rom_bytes[tile_pc..], &rom_bytes[attr_pc..], 40 * 64 * 2);

    println!("Raw decompressed tiles: {}", tiles_raw.len());

    for submap in 0..3 {
        let row_offset = submap * 27;
        println!("\nSubmap {} (row_offset={}) - First 10 tiles of row 0:", submap, row_offset);
        for col in 0..10usize {
            let raw_idx = row_offset * 40 + col;
            let raw_word = if raw_idx < tiles_raw.len() { tiles_raw[raw_idx] } else { 0 };
            let raw_chr = raw_word & 0x3FF;
            let raw_pal = (raw_word >> 10) & 7;

            let parsed = smw.overworld.layer2.get(submap).map(|tm| tm.get(col, 0));
            let p_chr = parsed.map(|t| t.tile_index()).unwrap_or(0);
            let p_pal = parsed.map(|t| t.palette()).unwrap_or(0);

            println!("  col={} raw={:04x} (chr={:#05x} pal={}) parsed(chr={:#05x} pal={}) MATCH={}",
                col, raw_word, raw_chr, raw_pal, p_chr, p_pal,
                raw_chr == p_chr && raw_pal as u8 == p_pal);
        }
    }
}
